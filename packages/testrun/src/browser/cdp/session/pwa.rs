//! Profile-scoped PWA admission, exact launch targets and retained removal.
use super::*;
use crate::browser::pwa::{Action, in_scope, standard_url};
use tokio::task::JoinHandle;
use url::Url;

#[derive(Default)]
pub(super) struct Apps {
    items: BTreeMap<Uuid, App>,
    pub(super) cleanup_unverified: Vec<Uuid>,
    pub(super) cleanup_journal_failure: Option<&'static str>,
}

struct App {
    manifest: Url,
    scope: Url,
    tabs: BTreeSet<String>,
    pending: Option<Pending>,
    outcome_unknown: bool,
}

enum Mutation {
    Install,
    Launch { existing_tabs: BTreeSet<String> },
    Uninstall,
}

struct Pending {
    operation: Uuid,
    action: Mutation,
    response: JoinHandle<NativeReply>,
}

impl Apps {
    #[cfg(test)]
    pub(super) fn mark_outcome_unknown(&mut self, app_id: Uuid) {
        self.items.get_mut(&app_id).unwrap().outcome_unknown = true;
    }

    pub(super) fn owns_pending(&self, operation: Uuid) -> bool {
        self.items.values().any(|app| {
            app.pending
                .as_ref()
                .is_some_and(|task| task.operation == operation)
        })
    }

    pub(super) async fn record_unverified_cleanup(&mut self, journal: &Journal) {
        if self.items.is_empty() {
            return;
        }
        let apps: Vec<_> = self
            .items
            .iter()
            .map(|(id, app)| json!({"appId":id,"manifestId":app.manifest.as_str()}))
            .collect();
        self.cleanup_journal_failure = journal
            .record(json!({"stage":"pwa_cleanup_unverified","apps":apps}))
            .await
            .err();
        self.cleanup_unverified.extend(self.items.keys().copied());
        self.items.clear();
    }
}

fn cleanup_pending(app_id: Uuid, code: &'static str) -> Error {
    let mut error = Error::from(code);
    error.partial = Some(json!({"appId":app_id,"replayed":false,"cleanupPending":true}));
    error
}

impl Worker {
    pub(super) async fn pwa_operation(
        &mut self,
        id: Uuid,
        action: Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if !self.managed_apps {
            return Err("NATIVE_PWA_OWNED_PROFILE_REQUIRED".into());
        }
        match action {
            Action::Install { page } => self.pwa_install(id, page, deadline).await,
            Action::List { cursor, limit } => {
                self.pwa_list(id, cursor.as_deref(), limit, deadline).await
            }
            Action::Launch { app_id, url } => self.pwa_launch(id, app_id, url, deadline).await,
            Action::Uninstall { app_id } => {
                self.remove_pwa(Some(id), app_id, deadline).await?;
                Ok(json!({"appId":app_id,"removed":true}))
            }
        }
    }

    async fn pwa_manifest(
        &mut self,
        id: Uuid,
        page: Reference,
        deadline: Instant,
    ) -> Result<(Url, Url, Url), Error> {
        let (_, world) = self.index.execution_world(page).map_err(resource_error)?;
        let target = self
            .index
            .native(page, Kind::Page)
            .map_err(resource_error)?;
        let session = self
            .index
            .route(world, Some(Kind::World))
            .map_err(resource_error)?
            .session_id;
        let page_info = self
            .call_read(
                id,
                None,
                "Target.getTargetInfo",
                json!({"targetId":target}),
                deadline,
            )
            .await?;
        let install_url = standard_url(
            page_info["targetInfo"]["url"]
                .as_str()
                .ok_or("PWA_URL_INVALID")?,
        )?;
        let result = self
            .call_read(
                id,
                Some(&session),
                "Page.getAppManifest",
                json!({}),
                deadline,
            )
            .await?;
        self.index
            .route(world, Some(Kind::World))
            .map_err(resource_error)?;
        let manifest = &result["manifest"];
        if result["url"].as_str().is_none_or(str::is_empty)
            || !manifest.is_object()
            || result["errors"]
                .as_array()
                .is_some_and(|errors| errors.iter().any(|error| error["critical"] == 1))
        {
            return Err("PWA_MANIFEST_UNAVAILABLE".into());
        }
        if ["fileHandlers", "protocolHandlers", "scopeExtensions"]
            .iter()
            .any(|key| {
                manifest[*key]
                    .as_array()
                    .is_some_and(|items| !items.is_empty())
            })
        {
            return Err("PWA_HOST_INTEGRATION_UNSUPPORTED".into());
        }
        let start = standard_url(
            manifest["startUrl"]
                .as_str()
                .ok_or("PWA_MANIFEST_UNAVAILABLE")?,
        )?;
        let identity = manifest["id"]
            .as_str()
            .map_or(Ok(start.clone()), standard_url)?;
        let scope = match manifest["scope"].as_str() {
            Some(scope) => standard_url(scope)?,
            None => start.join(".").map_err(|_| "PWA_MANIFEST_UNAVAILABLE")?,
        };
        if identity.origin() != install_url.origin()
            || !in_scope(&start, &scope)
            || !in_scope(&install_url, &scope)
        {
            return Err("PWA_SCOPE_MISMATCH".into());
        }
        Ok((identity, scope, install_url))
    }

    async fn pwa_present(&mut self, manifest: &Url, deadline: Instant) -> Result<bool, Error> {
        use super::super::{Error as Native, wire::RemoteError};
        let response = self
            .responsive_call_raw(
                None,
                "PWA.getOsAppState",
                json!({"manifestId":manifest.as_str()}),
                Lane::Ordinary,
                deadline,
                false,
            )
            .await?;
        match response {
            Ok(_) => Ok(true),
            Err(Native::Remote(RemoteError::Command {
                code: -32602,
                message,
                ..
            })) if message.strip_prefix("Unknown web-app manifest id ")
                == Some(manifest.as_str()) =>
            {
                Ok(false)
            }
            Err(error) => Err(protocol_error(error)),
        }
    }

    async fn pwa_install(
        &mut self,
        id: Uuid,
        page: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self.pwas.items.len() >= 8 {
            return Err("PWA_CAPACITY".into());
        }
        let (manifest, scope, install_url) = self.pwa_manifest(id, page, deadline).await?;
        if self.pwas.items.values().any(|app| app.manifest == manifest)
            || self.pwa_present(&manifest, deadline).await?
        {
            return Err("PWA_ALREADY_INSTALLED_OR_PENDING".into());
        }
        let app_id = Uuid::new_v4();
        let params =
            json!({"manifestId":manifest.as_str(),"installUrlOrBundleUrl":install_url.as_str()});
        self.pwas.items.insert(
            app_id,
            App {
                manifest,
                scope,
                tabs: BTreeSet::new(),
                pending: None,
                outcome_unknown: false,
            },
        );
        if let Err(error) = self
            .start_pwa_call(
                Some(id),
                app_id,
                Mutation::Install,
                "PWA.install",
                params,
                deadline,
            )
            .await
        {
            self.pwas.items.remove(&app_id);
            return Err(error);
        }
        self.settle_pwa_call(Some(id), app_id, deadline).await?;
        Ok(json!({"appId":app_id,"installed":true}))
    }

    async fn pwa_list(
        &mut self,
        id: Uuid,
        cursor: Option<&str>,
        limit: Option<u64>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let mut items = Vec::new();
        let ids: Vec<_> = self.pwas.items.keys().copied().collect();
        for app_id in ids {
            self.controller_current(id, true)?;
            if self.pwas.items[&app_id]
                .pending
                .as_ref()
                .is_some_and(|pending| pending.response.is_finished())
                && let Err(error) = self.settle_pwa_call(Some(id), app_id, deadline).await
                && !matches!(
                    error.code,
                    "OUTCOME_UNKNOWN" | "PWA_OUTCOME_UNRESOLVED" | "BROWSER_METHOD_FAILED"
                )
            {
                return Err(error);
            }
            let Some(app) = self.pwas.items.get(&app_id) else {
                continue;
            };
            let manifest = app.manifest.clone();
            let pending = app.pending.is_some();
            let uncertain = app.outcome_unknown;
            let installed = if pending || uncertain {
                None
            } else {
                Some(self.pwa_present(&manifest, deadline).await?)
            };
            let launched_tabs = self.pwas.items[&app_id].tabs.len();
            items.push((app_id, json!({"appId":app_id,"manifestId":manifest.as_str(),"installed":installed,"pending":pending,"outcomeUnknown":uncertain,"launchedTabs":launched_tabs})));
        }
        self.page_profile_apps("apps", cursor, limit, items)
    }

    async fn start_pwa_call(
        &mut self,
        id: Option<Uuid>,
        app_id: Uuid,
        action: Mutation,
        method: &'static str,
        params: Value,
        deadline: Instant,
    ) -> Result<(), Error> {
        if deadline <= Instant::now() {
            return Err("NOT_DISPATCHED".into());
        }
        if let Some(id) = id {
            self.observation_ready(id, deadline)?;
            self.journal.record(json!({"operationId":id,"stage":"dispatch_admitted","method":method,"appId":app_id})).await?;
            self.observation_ready(id, deadline)?;
        }
        let connection = self.connection.clone().ok_or("BACKEND_LOST")?;
        let response = tokio::spawn(async move {
            connection
                .call(
                    None,
                    method,
                    params,
                    Lane::Ordinary,
                    Instant::now() + Duration::from_secs(30),
                )
                .await
        });
        self.pwas
            .items
            .get_mut(&app_id)
            .expect("Retained PWA owner")
            .pending = Some(Pending {
            operation: id.unwrap_or_default(),
            action,
            response,
        });
        Ok(())
    }

    async fn settle_pwa_call(
        &mut self,
        id: Option<Uuid>,
        app_id: Uuid,
        deadline: Instant,
    ) -> Result<Option<String>, Error> {
        let app = self
            .pwas
            .items
            .get_mut(&app_id)
            .ok_or("PWA_REFERENCE_UNKNOWN")?;
        if app.outcome_unknown {
            return Err(cleanup_pending(app_id, "PWA_OUTCOME_UNRESOLVED"));
        }
        let Some(mut pending) = app.pending.take() else {
            return Ok(None);
        };
        let result = self
            .wait_retained_reply(id, &mut pending.response, deadline)
            .await;
        let app = self
            .pwas
            .items
            .get_mut(&app_id)
            .expect("Retained PWA owner");
        let Some(result) = result else {
            app.pending = Some(pending);
            return Err(cleanup_pending(app_id, "OUTCOME_UNKNOWN"));
        };
        let value = match result {
            Ok(Ok(reply)) => reply,
            Ok(Err(
                error @ super::super::Error::Remote(super::super::RemoteError::Command { .. }),
            )) => {
                if matches!(pending.action, Mutation::Install) {
                    self.pwas.items.remove(&app_id);
                }
                return Err(protocol_error(error));
            }
            _ => {
                app.outcome_unknown = true;
                return Err(cleanup_pending(app_id, "PWA_OUTCOME_UNRESOLVED"));
            }
        };
        if let Mutation::Launch { existing_tabs } = pending.action {
            let tab = value["targetId"]
                .as_str()
                .filter(|id| !id.is_empty() && id.len() <= 1024)
                .ok_or("PWA_TARGET_UNAVAILABLE")?;
            if existing_tabs.contains(tab) && !app.tabs.contains(tab) {
                return Err("PWA_TARGET_NOT_OWNED".into());
            }
            app.tabs.insert(tab.into());
            return Ok(Some(tab.into()));
        }
        Ok(None)
    }

    async fn pwa_launch(
        &mut self,
        id: Uuid,
        app_id: Uuid,
        url: Option<String>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let app = self
            .pwas
            .items
            .get(&app_id)
            .ok_or("PWA_REFERENCE_UNKNOWN")?;
        if app.pending.is_some() || app.outcome_unknown {
            return Err(cleanup_pending(app_id, "PWA_ACTION_PENDING"));
        }
        if app.tabs.len() >= 16 {
            return Err("PWA_TAB_CAPACITY".into());
        }
        let mut params = json!({"manifestId":app.manifest.as_str()});
        if let Some(url) = url {
            let url = standard_url(&url)?;
            if !in_scope(&url, &app.scope) {
                return Err("PWA_SCOPE_MISMATCH".into());
            }
            params["url"] = json!(url.as_str());
        }
        let targets = self.app_targets(deadline).await?;
        let existing_tabs = targets
            .iter()
            .filter(|target| target["type"] == "tab")
            .filter_map(|target| target["targetId"].as_str().map(str::to_owned))
            .collect();
        self.start_pwa_call(
            Some(id),
            app_id,
            Mutation::Launch { existing_tabs },
            "PWA.launch",
            params,
            deadline,
        )
        .await?;
        let tab = self
            .settle_pwa_call(Some(id), app_id, deadline)
            .await?
            .ok_or("PWA_TARGET_UNAVAILABLE")?;
        let page = self.pwa_page(id, &tab, deadline).await?;
        Ok(json!({"appId":app_id,"page":page}))
    }

    async fn pwa_page(
        &mut self,
        id: Uuid,
        tab: &str,
        deadline: Instant,
    ) -> Result<Reference, Error> {
        let (target, info) = loop {
            if let Some(info) = self.app_tab_page(tab, deadline).await? {
                break (
                    info["targetId"]
                        .as_str()
                        .ok_or("PWA_TARGET_UNAVAILABLE")?
                        .to_owned(),
                    info,
                );
            }
            if Instant::now() + Duration::from_millis(25) >= deadline {
                return Err("PWA_TARGET_UNAVAILABLE".into());
            }
            self.controller_current(id, true)?;
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        let context_id = info["browserContextId"]
            .as_str()
            .ok_or("PWA_TARGET_UNAVAILABLE")?;
        if let Some(context) = self.persistent_context {
            if self
                .index
                .native(context, Kind::Context)
                .map_err(resource_error)?
                != context_id
            {
                return Err("PWA_TARGET_NOT_OWNED".into());
            }
        } else {
            let context = self
                .index
                .register_context(context_id)
                .map_err(resource_error)?;
            self.context_layout.entry(context).or_default();
            self.persistent_context = Some(context);
        }
        self.index
            .apply(Event {
                method: "Target.targetInfoChanged".into(),
                session_id: None,
                params: json!({"targetInfo":info}),
            })
            .map_err(resource_error)?;
        self.reconcile(id, deadline).await?;
        let page = self
            .index
            .target_reference(&target)
            .ok_or("PWA_TARGET_UNAVAILABLE")?;
        if self.index.route(page, Some(Kind::Page)).is_err() {
            let attached = self
                .call(
                    id,
                    None,
                    "Target.attachToTarget",
                    json!({"targetId":target,"flatten":true}),
                    deadline,
                )
                .await?;
            let session = attached["sessionId"]
                .as_str()
                .ok_or("PWA_TARGET_UNAVAILABLE")?;
            self.index
                .register_attachment(page, session)
                .map_err(resource_error)?;
            self.setup(id, page, Kind::Page, session, deadline).await?;
        }
        Ok(page)
    }

    async fn remove_pwa(
        &mut self,
        id: Option<Uuid>,
        app_id: Uuid,
        deadline: Instant,
    ) -> Result<(), Error> {
        if let Err(error) = self.settle_pwa_call(id, app_id, deadline).await {
            if error.code == "BROWSER_METHOD_FAILED" && !self.pwas.items.contains_key(&app_id) {
                // The pending installation was definitively rejected.
                return Ok(());
            }
            return Err(error);
        }
        let manifest = self.pwas.items[&app_id].manifest.clone();
        if self.pwa_present(&manifest, deadline).await? {
            self.start_pwa_call(
                id,
                app_id,
                Mutation::Uninstall,
                "PWA.uninstall",
                json!({"manifestId":manifest.as_str()}),
                deadline,
            )
            .await?;
            self.settle_pwa_call(id, app_id, deadline).await?;
        }
        let tabs = self.pwas.items[&app_id].tabs.clone();
        let mut close_requested = BTreeSet::new();
        loop {
            let targets = self.app_targets(deadline).await?;
            let open: BTreeSet<_> = targets
                .iter()
                .filter(|target| target["type"] == "tab")
                .filter_map(|target| target["targetId"].as_str())
                .filter(|id| tabs.contains(*id))
                .collect();
            if open.is_empty() {
                break;
            }
            for tab in open {
                if close_requested.insert(tab.to_owned()) {
                    self.responsive_call(
                        None,
                        "Target.closeTarget",
                        json!({"targetId":tab}),
                        Lane::Ordinary,
                        deadline,
                    )
                    .await?;
                }
            }
            if Instant::now() + Duration::from_millis(25) >= deadline {
                return Err(cleanup_pending(app_id, "PWA_CLEANUP_PENDING"));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        if self.pwa_present(&manifest, deadline).await? {
            return Err(cleanup_pending(app_id, "PWA_CLEANUP_PENDING"));
        }
        self.pwas.items.remove(&app_id);
        Ok(())
    }

    pub(super) async fn cleanup_pwas(&mut self, deadline: Instant) -> Result<(), Error> {
        let pending: Vec<_> = self.pwas.items.keys().copied().collect();
        let mut failure = None;
        for app in pending {
            if let Err(error) = self.remove_pwa(None, app, deadline).await {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }
}
