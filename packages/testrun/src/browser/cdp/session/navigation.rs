//! Native navigation commitment and lifecycle witnesses, never inferred from a reply.
use super::*;
use crate::browser::page::{Load, Navigation};

#[derive(Default)]
pub(super) struct NavigationEvents {
    milestones: BTreeMap<(String, String), (String, BTreeSet<String>)>,
    same_document: BTreeMap<(String, String), u64>,
    responses: BTreeMap<(String, String), (u64, Value)>,
    sequence: u64,
    pub(super) gaps: u64,
}
impl NavigationEvents {
    /// Whether the frame's current loader reported `networkIdle`, or `None`
    /// before any lifecycle event for it.
    pub(super) fn network_idle(&self, session: &str, frame: &str) -> Option<bool> {
        self.milestones
            .get(&(session.to_owned(), frame.to_owned()))
            .map(|(_, states)| states.contains("networkIdle"))
    }
    pub(super) fn observe(&mut self, index: &Index, event: &Event) {
        let Some(session) = event.session_id.as_deref() else {
            return;
        };
        let Some(frame) = event.params["frameId"].as_str() else {
            return;
        };
        if index.event_frame(session, frame).is_none() {
            return;
        }
        let key = (session.to_owned(), frame.to_owned());
        if event.method == "Network.responseReceived" && event.params["type"] == "Document" {
            let Some(loader) = event.params["loaderId"].as_str() else {
                return;
            };
            if self.responses.len() >= contract::MAX_RESOURCES
                && let Some(oldest) = self
                    .responses
                    .iter()
                    .min_by_key(|(_, value)| value.0)
                    .map(|(key, _)| key.clone())
            {
                self.responses.remove(&oldest);
            }
            self.sequence = self.sequence.saturating_add(1);
            let response = &event.params["response"];
            self.responses.insert((session.into(),loader.into()),(self.sequence,json!({"url":response["url"],"status":response["status"],"statusText":response["statusText"]})));
        }
        if event.method == "Page.lifecycleEvent" {
            let Some(loader) = event.params["loaderId"].as_str() else {
                return;
            };
            let Some(name) = event.params["name"].as_str() else {
                return;
            };
            if self.milestones.len() >= contract::MAX_RESOURCES
                && !self.milestones.contains_key(&key)
            {
                return;
            }
            let entry = self.milestones.entry(key).or_default();
            if entry.0 != loader {
                *entry = (loader.into(), BTreeSet::new());
            }
            if entry.1.len() < 32 {
                entry.1.insert(name.into());
            }
        } else if event.method == "Page.navigatedWithinDocument" {
            if self.same_document.len() >= contract::MAX_RESOURCES
                && !self.same_document.contains_key(&key)
            {
                return;
            }
            let sequence = self.same_document.entry(key).or_default();
            *sequence = sequence.saturating_add(1);
        }
    }
}

impl Worker {
    pub(super) async fn navigate(
        &mut self,
        id: Uuid,
        page: Reference,
        navigation: Navigation,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.navigate_with_reload_cache(id, page, navigation, None, deadline)
            .await
    }
    pub(super) async fn navigate_with_reload_cache(
        &mut self,
        id: Uuid,
        page: Reference,
        navigation: Navigation,
        ignore_cache: Option<bool>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let scope = self.page_scope(id, page, None, deadline).await?;
        self.call(
            id,
            Some(&scope.session),
            "Page.setLifecycleEventsEnabled",
            json!({"enabled":true}),
            deadline,
        )
        .await?;
        self.call(
            id,
            Some(&scope.session),
            "Network.enable",
            json!({"maxTotalBufferSize":1048576,"maxResourceBufferSize":262144,"maxPostDataSize":self.capture_post_data_budget(&scope.session)}),
            deadline,
        )
        .await?;
        let response_barrier = self.navigation.sequence;
        let gaps = self.navigation.gaps;
        let native = self
            .index
            .native(scope.frame, Kind::Frame)
            .map_err(resource_error)?;
        let key = (scope.session.clone(), native.clone());
        self.navigation.milestones.remove(&key);
        let same_document = self
            .navigation
            .same_document
            .get(&key)
            .copied()
            .unwrap_or_default();
        let (method, mut params, load) = self
            .navigation_command(id, &scope, &navigation, deadline)
            .await?;
        let Some(method) = method else {
            return Ok(json!({"page":page,"frame":scope.frame,"navigated":false,"response":null}));
        };
        if method == "Page.reload"
            && let Some(ignore_cache) = ignore_cache
        {
            params["ignoreCache"] = json!(ignore_cache);
        }
        // Fence the old tree before dispatch. New child-frame events can be
        // delivered before setDocumentContent replies and must remain owned.
        if matches!(navigation, Navigation::SetContent { .. }) {
            self.index
                .content_replaced(scope.frame)
                .map_err(resource_error)?;
        }
        let result = self
            .call(id, Some(&scope.session), method, params, deadline)
            .await?;
        if result["isDownload"] == true {
            return Err("NAVIGATION_DOWNLOAD".into());
        }
        if result["errorText"]
            .as_str()
            .is_some_and(|text| !text.is_empty())
        {
            return Err("NAVIGATION_FAILED".into());
        }
        let loader = result["loaderId"].as_str().map(str::to_owned);
        self.wait_navigation(
            id,
            page,
            &scope,
            &key,
            (load, loader, same_document),
            deadline,
        )
        .await?;
        self.navigation_result(id, page, scope.frame, response_barrier, gaps, deadline)
            .await
    }

    async fn navigation_result(
        &mut self,
        id: Uuid,
        page: Reference,
        previous: Reference,
        response_barrier: u64,
        gaps: u64,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let mut attempts = 0;
        let (current, geometry) = loop {
            attempts += 1;
            let current = self.page_scope(id, page, None, deadline).await?;
            let geometry = self
                .page_expression(
                    id,
                    &current,
                    format!("({})()", include_str!("../../reading/geometry.js")),
                    deadline,
                )
                .await;
            match geometry {
                Err(error)
                    if error.code == "STALE_REFERENCE"
                        && attempts < 4
                        && self
                            .index
                            .page_frame(page, None)
                            .is_ok_and(|frame| frame != current.frame) => {}
                result => break (current, result?),
            }
        };
        let response = self
            .index
            .loader(current.frame)
            .map_err(resource_error)?
            .and_then(|loader| {
                self.navigation
                    .responses
                    .get(&(current.session.clone(), loader))
            })
            .filter(|(sequence, _)| *sequence > response_barrier)
            .map(|(_, response)| response.clone());
        Ok(
            json!({"page":page,"frame":current.frame,"url":geometry["url"],"response":response,"responseMetadataComplete":gaps==self.navigation.gaps,
            "documentReplaced":previous!=current.frame,"source":"chrome-cdp-navigation","previewRecovery":{"succeeded":true}}),
        )
    }

    async fn navigation_command(
        &mut self,
        id: Uuid,
        scope: &page::Scope,
        navigation: &Navigation,
        deadline: Instant,
    ) -> Result<(Option<&'static str>, Value, Load), Error> {
        Ok(match navigation {
            Navigation::Goto { url, wait_until } => {
                (Some("Page.navigate"), json!({"url":url}), *wait_until)
            }
            Navigation::SetContent { html, wait_until } => {
                self.page_expression(
                    id,
                    scope,
                    "globalThis.__testrunMessages?.reset(); null".into(),
                    deadline,
                )
                .await?;
                (
                    Some("Page.setDocumentContent"),
                    json!({"frameId":self.index.native(scope.frame, Kind::Frame).map_err(resource_error)?,"html":html}),
                    *wait_until,
                )
            }
            Navigation::Reload { wait_until } => (Some("Page.reload"), json!({}), *wait_until),
            Navigation::Back { wait_until } | Navigation::Forward { wait_until } => {
                let history = self
                    .call(
                        id,
                        Some(&scope.session),
                        "Page.getNavigationHistory",
                        json!({}),
                        deadline,
                    )
                    .await?;
                let index = history["currentIndex"].as_i64().ok_or("BACKEND_PROTOCOL")?
                    + if matches!(navigation, Navigation::Back { .. }) {
                        -1
                    } else {
                        1
                    };
                let entry = usize::try_from(index)
                    .ok()
                    .and_then(|index| history["entries"].get(index));
                match entry {
                    Some(entry) => (
                        Some("Page.navigateToHistoryEntry"),
                        json!({"entryId":entry["id"]}),
                        *wait_until,
                    ),
                    None => (None, Value::Null, *wait_until),
                }
            }
        })
    }

    async fn wait_navigation(
        &mut self,
        id: Uuid,
        page: Reference,
        scope: &page::Scope,
        key: &(String, String),
        expected: (Load, Option<String>, u64),
        deadline: Instant,
    ) -> Result<(), Error> {
        let (load, loader, same_document) = expected;
        loop {
            self.drain()?;
            let frame = self.index.page_frame(page, None).map_err(resource_error)?;
            if self
                .navigation
                .same_document
                .get(key)
                .copied()
                .unwrap_or_default()
                > same_document
            {
                return Ok(());
            }
            if matches!(load, Load::Commit) && frame != scope.frame {
                return Ok(());
            }
            let milestone = match load {
                Load::Commit => "init",
                Load::Domcontentloaded => "DOMContentLoaded",
                Load::Load => "load",
                Load::Networkidle => "networkIdle",
            };
            // Client redirects can replace the committed document before its
            // load event. Follow the current owned main frame and loader rather
            // than waiting forever on the first navigation reply's loader.
            let route = self
                .index
                .route(frame, Some(Kind::Frame))
                .map_err(resource_error)?;
            let current_key = (
                route.session_id,
                self.index
                    .native(frame, Kind::Frame)
                    .map_err(resource_error)?,
            );
            let current_loader = if frame != scope.frame {
                self.index.loader(frame).map_err(resource_error)?
            } else {
                loader.clone()
            };
            if self
                .navigation
                .milestones
                .get(&current_key)
                .is_some_and(|(current, states)| {
                    current_loader
                        .as_ref()
                        .is_none_or(|loader| loader == current)
                        && states.contains(milestone)
                })
            {
                return Ok(());
            }
            self.navigation_event(id, deadline).await?;
        }
    }

    async fn navigation_event(&mut self, id: Uuid, deadline: Instant) -> Result<(), Error> {
        let shared = self.shared.clone();
        if shared.state.lock().expect("Chrome lifecycle").phase == Phase::Closing {
            return Err("OUTCOME_UNKNOWN".into());
        }
        if shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .operations
            .get(&id)
            .is_some_and(|o| o.cancel_requested)
        {
            return Err("OUTCOME_UNKNOWN".into());
        }
        if Instant::now() >= deadline {
            return Err("OUTCOME_UNKNOWN".into());
        }
        self.observation_tick(deadline).await
    }
}
