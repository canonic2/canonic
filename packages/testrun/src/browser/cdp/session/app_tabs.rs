//! Exact Chrome tab-to-page witnesses for profile-owned apps and extensions.
use super::*;

pub(super) struct Probe {
    tab: String,
    session: Option<String>,
    page: Option<Value>,
    ambiguous: bool,
}

impl Probe {
    pub(super) fn observe(&mut self, event: &Event) {
        if event.method != "Target.attachedToTarget" {
            return;
        }
        let info = &event.params["targetInfo"];
        if event.session_id.is_none() && info["type"] == "tab" && info["targetId"] == self.tab {
            self.session = event.params["sessionId"].as_str().map(str::to_owned);
        }
        if info["type"] == "page" && self.session.is_some() && self.session == event.session_id {
            let previous = self.page.replace(info.clone());
            self.ambiguous |= previous.is_some_and(|page| page["targetId"] != info["targetId"]);
        }
    }
}

impl Worker {
    pub(super) fn page_profile_apps(
        &self,
        name: &str,
        cursor: Option<&str>,
        limit: Option<u64>,
        items: Vec<(Uuid, Value)>,
    ) -> Result<Value, Error> {
        let connection = self
            .shared
            .state
            .lock()
            .expect("Chrome lifecycle")
            .connection;
        let page = crate::pagination::collect(
            name,
            &connection,
            &canonic_mcp_kit::page::arguments(cursor, limit),
            items
                .into_iter()
                .map(|(id, item)| (id.as_bytes().to_vec(), item)),
            Ok,
            "browser_send",
        )
        .map_err(|error| {
            let code = match error.code.as_str() {
                "invalid_cursor" => "invalid_cursor",
                "invalid_arguments" => "invalid_arguments",
                "item_too_large" => "item_too_large",
                _ => "INTERNAL",
            };
            Error::caused(code, error)
        })?;
        Ok(json!({(name):page.items,"nextCursor":page.next_cursor}))
    }

    pub(super) async fn app_targets(&mut self, deadline: Instant) -> Result<Vec<Value>, Error> {
        let mut value = self
            .responsive_call(
                None,
                "Target.getTargets",
                json!({"filter":[{"type":"tab"},{"type":"page"},{"exclude":true}]}),
                Lane::Ordinary,
                deadline,
            )
            .await?;
        match value["targetInfos"].take() {
            Value::Array(targets) if targets.len() <= 256 => Ok(targets),
            _ => Err("APP_TARGET_INVENTORY_UNAVAILABLE".into()),
        }
    }

    pub(super) async fn app_tab_page(
        &mut self,
        tab: &str,
        deadline: Instant,
    ) -> Result<Option<Value>, Error> {
        // Root target inventory may omit parentId. Ask Chrome for the page
        // attached specifically below this tab. The attachment has no debugger
        // pause and never becomes a second owner of the public page.
        self.app_tab_probe = Some(Probe {
            tab: tab.into(),
            session: None,
            page: None,
            ambiguous: false,
        });
        let result = self.probe_app_tab(tab, deadline).await;
        let probe = self.app_tab_probe.take().expect("Active app tab probe");
        if let Some(session) = probe.session {
            self.cleanup_call(
                None,
                "Target.detachFromTarget",
                json!({"sessionId":session}),
                Instant::now() + Duration::from_secs(2),
            )
            .await?;
        }
        result?;
        if probe.ambiguous {
            return Err("APP_TARGET_AMBIGUOUS".into());
        }
        Ok(probe.page)
    }

    async fn probe_app_tab(&mut self, tab: &str, deadline: Instant) -> Result<(), Error> {
        let reply = self
            .responsive_call(
                None,
                "Target.attachToTarget",
                json!({"targetId":tab,"flatten":true}),
                Lane::Ordinary,
                deadline,
            )
            .await?;
        let session = reply["sessionId"].as_str().ok_or("BACKEND_PROTOCOL")?;
        self.app_tab_probe
            .as_mut()
            .expect("Active app tab probe")
            .session = Some(session.into());
        self.responsive_call(
            Some(session),
            "Target.setAutoAttach",
            json!({"autoAttach":true,"waitForDebuggerOnStart":false,"flatten":true,"filter":[{"type":"page"},{"exclude":true}]}),
            Lane::Ordinary,
            deadline,
        )
        .await?;
        Ok(())
    }

    pub(super) async fn app_tab_for_page(
        &mut self,
        id: Uuid,
        page: Reference,
        deadline: Instant,
    ) -> Result<String, Error> {
        let page_target = self
            .index
            .native(page, Kind::Page)
            .map_err(resource_error)?;
        let context = self
            .index
            .retained_page_context(page)
            .ok_or("INVALID_SCOPE")?;
        let context = self
            .index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        let candidates = self.app_targets(deadline).await?;
        for candidate in candidates {
            if candidate["type"] != "tab" || candidate["browserContextId"] != context {
                continue;
            }
            self.controller_current(id, true)?;
            let tab = candidate["targetId"].as_str().ok_or("BACKEND_PROTOCOL")?;
            if self.app_tab_page(tab, deadline).await?.is_some_and(|info| {
                info["targetId"] == page_target && info["browserContextId"] == context
            }) {
                self.index
                    .native(page, Kind::Page)
                    .map_err(resource_error)?;
                return Ok(tab.into());
            }
        }
        Err("EXTENSION_TAB_UNAVAILABLE".into())
    }
}
