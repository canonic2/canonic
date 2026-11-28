//! Cancellable read-only condition observations with ancestor-frame visibility.
use super::*;
use crate::browser::{
    lifecycle::Resource,
    page::{Condition, Locator},
};

impl Host {
    pub(super) async fn wait_actionable(
        &self,
        id: Option<Uuid>,
        scope: &Resource,
        locator: &Locator,
        action: &crate::browser::page::Action,
        deadline: Instant,
    ) -> Result<(), Error> {
        let selector = locator.scoped_selector(scope.reference)?;
        loop {
            self.condition_ready(id, deadline)?;
            let visible = self
                .condition_frame_visible(scope.reference, deadline)
                .await?;
            let result = self
                .condition_query(
                    id,
                    scope,
                    json!({"selector":selector,"actionability":action,"frameVisible":visible}),
                    deadline,
                )
                .await?;
            if self.frame_current(&scope.wire, deadline).await? != scope.reference {
                return Err("DOCUMENT_CHANGED".into());
            }
            if result["satisfied"] == true {
                return Ok(());
            }
            tokio::select! {()=self.changed.notified()=>{},()=tokio::time::sleep_until(deadline.min(Instant::now()+Duration::from_millis(25)))=>{}}
        }
    }
    pub(super) async fn wait_condition(
        &self,
        id: Option<Uuid>,
        page: Reference,
        frame: Option<Reference>,
        locator: Option<Locator>,
        condition: Condition,
        deadline: Instant,
    ) -> Result<Value, Error> {
        // The wait's own read-only driver calls carry its deadline as the SDK
        // timeout. Reaching it undispatched, or the SDK replying TimeoutError
        // to it, is the wait timing out; neither changed anything.
        let mut progress = crate::browser::reading::LocatorProgress::default();
        self.wait_condition_loop(
            id,
            page,
            frame,
            locator.as_ref(),
            &condition,
            deadline,
            &mut progress,
        )
        .await
        .map_err(|mut error| {
            let sdk_timeout = error.failure_evidence().is_some_and(|evidence| {
                evidence["source"] == "sdk-reply" && evidence["kind"] == "timeout"
            });
            if sdk_timeout || (error.code == "NOT_DISPATCHED" && Instant::now() >= deadline) {
                error.code = "CONDITION_TIMEOUT";
            }
            progress.timeout(error)
        })
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "Observe one scoped condition and retain its locator match evidence"
    )]
    async fn wait_condition_loop(
        &self,
        id: Option<Uuid>,
        page: Reference,
        frame: Option<Reference>,
        locator: Option<&Locator>,
        condition: &Condition,
        deadline: Instant,
        progress: &mut crate::browser::reading::LocatorProgress,
    ) -> Result<Value, Error> {
        loop {
            self.condition_ready(id, deadline)?;
            let scope = self.page_scope(page, frame, deadline).await?;
            let selector = locator
                .map(|locator| locator.scoped_selector(scope.reference))
                .transpose()?
                .unwrap_or_else(|| "css=body".into());
            let visible = if matches!(condition, Condition::Visible {} | Condition::Hidden {}) {
                self.condition_frame_visible(scope.reference, deadline)
                    .await?
            } else {
                true
            };
            let mut observed = self
                .condition_query(
                    id,
                    &scope,
                    json!({"selector":selector,"condition":condition,"frameVisible":visible}),
                    deadline,
                )
                .await?;
            // The driver's frame `loadStates` carries its network-idle witness.
            if condition.needs_network_idle() {
                let states = self.connection()?.initializer(&scope.wire).await?;
                observed["networkIdle"] = json!(states.is_some_and(|states| {
                    states["loadStates"]
                        .as_array()
                        .is_some_and(|states| states.iter().any(|state| state == "networkidle"))
                }));
            }
            let current = self.frame_current(&scope.wire, deadline).await?;
            if current != scope.reference {
                if frame.is_some() {
                    return Err("DOCUMENT_CHANGED".into());
                }
                continue;
            }
            if locator.is_some() {
                progress.observe(&observed);
            }
            let value = condition.settle(observed);
            if value["satisfied"] == true {
                return Ok(
                    json!({"page":page,"frame":current,"satisfied":true,"source":"playwright-condition","observedAtUnixMs":page::now_ms()}),
                );
            }
            self.condition_ready(id, deadline)?;
            tokio::select! {()=self.changed.notified()=>{},()=tokio::time::sleep_until(deadline.min(Instant::now()+Duration::from_millis(25)))=>{}}
        }
    }
    fn condition_ready(&self, id: Option<Uuid>, deadline: Instant) -> Result<(), Error> {
        if let Some(id) = id {
            self.admissible(id, deadline).map_err(|mut error| {
                if error.code == "NOT_DISPATCHED" {
                    error.code = "CONDITION_TIMEOUT";
                }
                error
            })?;
        } else if Instant::now() >= deadline {
            return Err("CONDITION_TIMEOUT".into());
        }
        if self.state.lock().expect("Browser owner").phase != Phase::Ready {
            return Err("SESSION_NOT_READY".into());
        }
        Ok(())
    }
    async fn condition_query(
        &self,
        id: Option<Uuid>,
        scope: &Resource,
        options: Value,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let page = self
            .state
            .lock()
            .expect("Browser owner")
            .ancestor(scope.reference, Kind::Page)
            .ok_or("STALE_REFERENCE")?;
        let engine = self.state.lock().expect("Browser owner").engine;
        let expression = crate::browser::reading::query_function(engine.name());
        let call = self.evaluate_internal(&scope.wire, &expression, options, deadline);
        tokio::pin!(call);
        let result = loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            self.condition_ready(id, deadline)?;
            if self.state.lock().expect("Browser owner").retired.get(&page) == Some(&Kind::Page) {
                return Err("STALE_REFERENCE".into());
            }
            tokio::select! {
                result=&mut call=>break result?,
                ()=&mut changed=>{},
                ()=tokio::time::sleep_until(deadline)=>return Err("CONDITION_TIMEOUT".into()),
            }
        };
        if let Some(code) = result["error"].as_str() {
            return Err(match code {
                "STALE_REFERENCE" => "STALE_REFERENCE",
                "AMBIGUOUS_LOCATOR" => "AMBIGUOUS_LOCATOR",
                "ELEMENT_NOT_FOUND" => "ELEMENT_NOT_FOUND",
                "INVALID_SELECTOR" => "INVALID_SELECTOR",
                _ => "PAGE_EXECUTION_FAILED",
            }
            .into());
        }
        Ok(result)
    }
    async fn condition_frame_visible(
        &self,
        frame: Reference,
        deadline: Instant,
    ) -> Result<bool, Error> {
        let connection = self.connection()?;
        let mut current = frame;
        for _ in 0..32 {
            let parent = {
                let state = self.state.lock().expect("Browser owner");
                let resource = state.resource(current, Some(Kind::Frame))?;
                let parent = resource.parent.ok_or("INVALID_SCOPE")?;
                if state.resource(parent, None)?.kind == Kind::Page {
                    return Ok(true);
                }
                parent
            };
            let wire = self.wire(current, Kind::Frame)?;
            let reply = connection
                .retained_internal(&wire, "frameElement", json!({}), deadline, false)
                .await?;
            let element =
                super::workflows::channel(&connection, reply, "element", "ElementHandle").await?;
            let result = connection
                .retained_internal(&element, "isVisible", json!({}), deadline, false)
                .await;
            // Preserve the ordinary cleanup deadline and priority lane.
            let released = connection
                .retained_internal(
                    &element,
                    "dispose",
                    json!({}),
                    Instant::now() + Duration::from_secs(30),
                    true,
                )
                .await;
            let result = result?.ok_or("MISSING_RESULT")?;
            released?;
            if self.frame_current(&wire, deadline).await? != current {
                return Err("DOCUMENT_CHANGED".into());
            }
            if result["value"] == false {
                return Ok(false);
            }
            current = parent;
        }
        Err("FRAME_DEPTH_BOUNDS".into())
    }
}
