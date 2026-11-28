//! Native `storage_*`: private versioned snapshots of an owned context and
//! replacing or merging restores through the hidden storage page.
use super::super::http::cookies_for_write;
use super::*;
use crate::browser::preparation::CHROME_VERSION;
use crate::browser::storage::{self, ApplyMode, Options, native};
use std::path::Path;

/// Each step of a restore, so a failure reports what completed, what may
/// have changed and what was never sent.
struct Progress {
    mode: ApplyMode,
    /// Pinned initial mode at context creation: nothing to clear first.
    initial: bool,
    cache: &'static str,
    cookies: &'static str,
    origins: Vec<(String, &'static str)>,
    dispatched: bool,
}
impl Progress {
    fn value(&self) -> Value {
        json!({"mode":self.mode,"cacheCleared":self.cache,"cookies":self.cookies,
            "origins":self.origins.iter().map(|(origin,status)|json!({"origin":origin,"status":status})).collect::<Vec<_>>()})
    }
}

/// A failed restore reports its progress; once anything was sent, the
/// outcome is unknown and nothing is replayed.
fn restore_error(
    error: Error,
    progress: &Progress,
    context: Reference,
    snapshot: Option<Reference>,
) -> Error {
    let unknown = progress.dispatched;
    let mut result = if unknown {
        Error::caused("OUTCOME_UNKNOWN", error)
    } else {
        error
    };
    let mut partial = progress.value();
    partial["context"] = json!(context);
    partial["snapshot"] = json!(snapshot);
    partial["initial"] = json!(progress.initial);
    partial["stateMayHaveChanged"] = json!(unknown);
    partial["replayed"] = json!(false);
    partial["nextAction"] = json!("inspect_owned_context_or_close_it");
    result.partial = Some(partial);
    result
}

/// A restore that completed but left the hidden page open reports its
/// result, so callers keep the context it changed.
fn cleanup_pending(cause: Error, mut result: Value) -> Error {
    let mut error = Error::caused("CLEANUP_PENDING", cause);
    result["cleanupPending"] = json!(true);
    result["nextAction"] = json!("retry_storage_operation_or_browser_disconnect");
    error.partial = Some(result);
    error
}
/// Creation state's cleanup outcome; a failure keeps the new context.
fn initial_cleanup(
    progress: &Progress,
    context: Reference,
    cleanup: Result<(), Error>,
) -> Result<(), Error> {
    let Err(cause) = cleanup else {
        return Ok(());
    };
    let mut result = progress.value();
    result["context"] = json!(context);
    result["initial"] = json!(true);
    Err(cleanup_pending(cause, result))
}

impl Worker {
    /// Pinned `newContext({storageState})` on a fresh context, before its
    /// first page: native or converted Playwright state, cookies then origins,
    /// with nothing cleared first.
    /// Loads and validates creation state before the context exists, so an
    /// invalid file refuses without allocating anything.
    pub(in super::super) async fn native_load_initial_storage(
        &mut self,
        id: Uuid,
        input: Option<crate::browser::http::StorageStateInput>,
        deadline: Instant,
    ) -> Result<Option<Value>, Error> {
        let Some(input) = input else {
            return Ok(None);
        };
        let state = self
            .await_http_preparation(
                id,
                crate::browser::http::storage_state_load(input, deadline),
                deadline,
            )
            .await?;
        let state = native::register(state, CHROME_VERSION)?.0;
        storage::refuse_creation_credentials(&state)?;
        Ok(Some(state))
    }
    pub(in super::super) async fn native_initial_storage(
        &mut self,
        id: Uuid,
        context: Reference,
        state: Value,
        deadline: Instant,
    ) -> Result<(), Error> {
        let (_, cleanup) = self
            .native_restore(
                id,
                context,
                &state,
                ApplyMode::Replace,
                true,
                None,
                deadline,
            )
            .await?;
        cleanup
    }
    /// One validated restore through the hidden page: cookies, then each
    /// target origin, with progress, page cleanup and visited-origin records.
    /// `replace` targets the visited and snapshot origins, `merge` the
    /// snapshot's; `initial` (creation state) clears nothing first. Returns
    /// the progress and the page cleanup result; creation state's cleanup
    /// failure already carries the new context.
    #[expect(
        clippy::too_many_arguments,
        reason = "one restore's state, mode and error identity"
    )]
    async fn native_restore(
        &mut self,
        id: Uuid,
        context: Reference,
        state: &Value,
        mode: ApplyMode,
        initial: bool,
        snapshot: Option<Reference>,
        deadline: Instant,
    ) -> Result<(Progress, Result<(), Error>), Error> {
        let projection = native::playwright(state)?;
        let cookies = cookies_for_write(projection["cookies"].clone())?;
        let native_context = self
            .index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        self.native_storage_ready(deadline).await?;
        self.drain()?;
        let named = native::origins(state);
        let mut targets = if mode == ApplyMode::Replace {
            self.http_state
                .origins
                .plan(context)
                .map_err(Error::from)?
                .origins
        } else {
            Vec::new()
        };
        for origin in &named {
            if !targets.contains(origin) {
                targets.push(origin.clone());
            }
        }
        let mut progress = Progress {
            mode,
            initial,
            cache: if mode == ApplyMode::Replace && !initial {
                "pending"
            } else {
                "unchanged"
            },
            cookies: "pending",
            origins: targets
                .into_iter()
                .map(|origin| (origin, "pending"))
                .collect(),
            dispatched: false,
        };
        self.invalidate_context_inputs(context);
        self.http_state.capturing = true;
        let applied = self
            .native_apply_steps(
                id,
                context,
                &native_context,
                &projection,
                cookies,
                &mut progress,
                deadline,
            )
            .await;
        let mut cleanup = self.cleanup_storage_page(deadline).await;
        if initial {
            cleanup = initial_cleanup(&progress, context, cleanup);
        }
        self.http_state.capturing = false;
        self.invalidate_context_inputs(context);
        applied.map_err(|error| restore_error(error, &progress, context, snapshot))?;
        match mode {
            ApplyMode::Replace => self.http_state.origins.replace(context, &named),
            ApplyMode::Merge => self.http_state.origins.extend(context, &named),
        }
        Ok((progress, cleanup))
    }
    /// One origin of the hidden storage page: navigate, wait for that
    /// origin's document, then evaluate in a fresh utility world. Writes use
    /// `call`, so an interrupted write is an unknown outcome.
    pub(super) async fn storage_origin_evaluate(
        &mut self,
        id: Uuid,
        session: &str,
        origin: &str,
        expression: String,
        write: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.require_storage_page()?;
        let navigated = self
            .call(
                id,
                Some(session),
                "Page.navigate",
                json!({"url":origin}),
                deadline,
            )
            .await?;
        if navigated.get("errorText").is_some() {
            return Err("HTTP_STORAGE_PAGE_NAVIGATION".into());
        }
        let frame = navigated["frameId"]
            .as_str()
            .ok_or("BACKEND_PROTOCOL")?
            .to_owned();
        loop {
            self.require_storage_page()?;
            let tree = self
                .call_read(id, Some(session), "Page.getFrameTree", json!({}), deadline)
                .await?;
            if tree["frameTree"]["frame"]["id"] == frame
                && tree["frameTree"]["frame"]["url"]
                    .as_str()
                    .and_then(|url| origins::origin(url).ok().flatten())
                    .as_deref()
                    == Some(origin)
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err("CONDITION_TIMEOUT".into());
            }
            self.observation_tick(deadline.min(Instant::now() + Duration::from_millis(25)))
                .await?;
        }
        let world = self.call(id,Some(session),"Page.createIsolatedWorld",json!({"frameId":frame,"worldName":"canonic.testrun.http-state.v1","grantUniveralAccess":false}),deadline).await?;
        let numeric = world["executionContextId"]
            .as_i64()
            .ok_or("BACKEND_PROTOCOL")?;
        let params = json!({"expression":expression,"contextId":numeric,"awaitPromise":true,"returnByValue":true});
        let result = if write {
            self.call(id, Some(session), "Runtime.evaluate", params, deadline)
                .await?
        } else {
            self.call_read(id, Some(session), "Runtime.evaluate", params, deadline)
                .await?
        };
        self.require_storage_page()?;
        if result.get("exceptionDetails").is_some() {
            return Err("STORAGE_SCRIPT_FAILED".into());
        }
        Ok(result["result"]["value"].clone())
    }

    /// Storage work owns the hidden page alone. A page an earlier operation
    /// could not close is closed first; its retained evidence stays readable.
    async fn native_storage_ready(&mut self, deadline: Instant) -> Result<(), Error> {
        if self.http_state.capturing {
            return Err("HTTP_STORAGE_STATE_PENDING".into());
        }
        if self.http_state.collection.is_some() {
            self.cleanup_storage_page(deadline).await?;
        }
        Ok(())
    }
    fn native_retain_snapshot(&mut self, state: Value) -> Result<Value, Error> {
        native::validate(&state)?;
        let connection = self
            .shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .connection;
        let format = native::format(&state);
        self.snapshots
            .retain(connection, self.journal.directory(), state, format)
    }
    pub(in super::super) fn native_storage_register(
        &mut self,
        path: String,
    ) -> Result<Value, Error> {
        self.snapshots.capacity()?;
        let bytes = storage::read_private(Path::new(&path))?;
        let state =
            serde_json::from_slice(&bytes).map_err(|_| Error::from("INVALID_STORAGE_STATE"))?;
        let (state, converted) = native::register(state, CHROME_VERSION)?;
        let source = if converted {
            storage::PLAYWRIGHT_FORMAT
        } else {
            native::format(&state)
        };
        let mut result = self.native_retain_snapshot(state)?;
        result["converted"] = json!(converted);
        result["sourceFormat"] = json!(source);
        Ok(result)
    }
    pub(in super::super) async fn native_storage_export(
        &mut self,
        id: Uuid,
        context: Reference,
        options: Options,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.snapshots.capacity()?;
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        self.native_storage_ready(deadline).await?;
        self.drain()?;
        self.http_state.origins.plan(context).map_err(Error::from)?;
        self.http_state.capturing = true;
        let collection = crate::browser::http::StorageStateOptions {
            indexed_db: options.indexed_db,
            opfs: options.opfs,
            path: None,
        };
        let collected = self
            .collect_http_state(id, context, Uuid::new_v4(), &collection, deadline)
            .await;
        let cleanup = self.cleanup_storage_page(deadline).await;
        self.http_state.capturing = false;
        let mut collected = collected?;
        if options.credentials {
            // Page events keep the registry current without another dispatch.
            collected["credentials"] = json!(self.context_credential_records(context));
        }
        let state = native::snapshot(&collected, &options, CHROME_VERSION)?;
        let copy = options.path.is_some().then(|| state.clone());
        let mut result = self.native_retain_snapshot(state)?;
        result["context"] = json!(context);
        result["included"] = json!({"cookies":true,"localStorage":true,"indexedDB":options.indexed_db,"opfs":options.opfs,"sessionStorage":false,"virtualCredentials":options.credentials});
        if cleanup.is_err() {
            let mut error = Error::from("CLEANUP_PENDING");
            error.partial = Some(
                json!({"snapshot":result["snapshot"],"stateRetained":true,"cleanupPending":true,"nextAction":"retry_storage_operation_or_browser_disconnect","replayed":false}),
            );
            return Err(error);
        }
        match copy {
            Some(state) => storage::copy_export(result, options.path, &state, deadline).await,
            None => Ok(result),
        }
    }
    fn invalidate_context_inputs(&mut self, context: Reference) {
        for (page, parent) in self.index.list(Kind::Page) {
            if parent == Some(context) {
                self.inputs.invalidate(page);
            }
        }
    }
    pub(in super::super) async fn native_storage_apply(
        &mut self,
        id: Uuid,
        context: Reference,
        snapshot: Reference,
        mode: ApplyMode,
        install_authenticator: bool,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let state = self
            .snapshots
            .0
            .get(&snapshot)
            .ok_or("STALE_REFERENCE")?
            .state
            .clone();
        // Complete prevalidation before anything is dispatched.
        native::validate(&state)?;
        storage::require_installation(&state, install_authenticator)?;
        let restored = match state.get("credentials") {
            Some(credentials) => crate::browser::credentials::validate_all(credentials)?,
            None => Vec::new(),
        };
        self.check_restore_capacity(context, &restored, mode == ApplyMode::Replace)?;
        let (progress, cleanup) = self
            .native_restore(id, context, &state, mode, false, Some(snapshot), deadline)
            .await?;
        let mut result = progress.value();
        // Credentials follow the storage steps, which have dispatched.
        let count = restored.len();
        let credentials = self
            .restore_credentials(id, context, restored, mode == ApplyMode::Replace, deadline)
            .await
            .map_err(|cause| {
                let mut error = Error::caused("OUTCOME_UNKNOWN", cause);
                let mut partial = result.clone();
                partial["context"] = json!(context);
                partial["snapshot"] = json!(snapshot);
                partial["credentials"] = json!({"status":"unknown","count":count});
                partial["stateMayHaveChanged"] = json!(true);
                partial["replayed"] = json!(false);
                partial["nextAction"] = json!("inspect_owned_context_or_close_it");
                error.partial = Some(partial);
                error
            })?;
        result["credentials"] = json!({"status":credentials,"count":count});
        result["context"] = json!(context);
        result["snapshot"] = json!(snapshot);
        result["dispatchCompleted"] = json!(true);
        result["effectsObserved"] = json!(false);
        result["restoration"] = json!("close_owned_context_or_apply_explicit_snapshot");
        match cleanup {
            Ok(()) => Ok(result),
            Err(cause) => Err(cleanup_pending(cause, result)),
        }
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "one restore's prevalidated inputs and its progress record"
    )]
    async fn native_apply_steps(
        &mut self,
        id: Uuid,
        context: Reference,
        native_context: &str,
        projection: &Value,
        cookies: Value,
        progress: &mut Progress,
        deadline: Instant,
    ) -> Result<(), Error> {
        let replace = progress.mode == ApplyMode::Replace;
        let clears = replace && !progress.initial;
        let session = if clears || !progress.origins.is_empty() {
            Some(
                self.storage_page(id, context, Uuid::new_v4(), deadline)
                    .await?
                    .1,
            )
        } else {
            None
        };
        if clears {
            let session = session.as_deref().expect("Replacing storage page");
            progress.dispatched = true;
            progress.cache = "unknown";
            self.call(
                id,
                Some(session),
                "Network.clearBrowserCache",
                json!({}),
                deadline,
            )
            .await?;
            progress.cache = "cleared";
            progress.cookies = "unknown";
            self.call(
                id,
                None,
                "Storage.clearCookies",
                json!({"browserContextId":native_context}),
                deadline,
            )
            .await?;
            progress.cookies = "cleared";
        }
        if cookies
            .as_array()
            .is_some_and(|cookies| !cookies.is_empty())
        {
            progress.dispatched = true;
            progress.cookies = "unknown";
            self.call(
                id,
                None,
                "Storage.setCookies",
                json!({"browserContextId":native_context,"cookies":cookies}),
                deadline,
            )
            .await?;
        }
        progress.cookies = if replace { "replaced" } else { "merged" };
        for index in 0..progress.origins.len() {
            let session = session.as_deref().expect("Origin storage page");
            let origin = progress.origins[index].0.clone();
            let expression = if replace {
                native::restore_expression(projection, &origin)
            } else {
                native::merge_expression(projection, &origin)
            };
            progress.dispatched = true;
            progress.origins[index].1 = "unknown";
            self.storage_origin_evaluate(id, session, &origin, expression, true, deadline)
                .await?;
            progress.origins[index].1 = if !replace {
                "merged"
            } else if projection["origins"]
                .as_array()
                .is_some_and(|origins| origins.iter().any(|item| item["origin"] == origin))
            {
                "restored"
            } else {
                "cleared"
            };
        }
        Ok(())
    }
    pub(in super::super) fn native_storage_forget(
        &mut self,
        reference: Reference,
    ) -> Result<Value, Error> {
        self.snapshots.forget(reference)
    }
}
