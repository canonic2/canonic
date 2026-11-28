//! Scoped page reading, actions, navigation and driver-owned condition waits.
use super::*;
use crate::browser::{
    lifecycle::Resource,
    objects::ObjectRef,
    page::{Action, Condition, Locator, Navigation, Read},
    values,
};

pub(super) const INITIALIZATION: &str = concat!(
    include_str!("page/native-timing.js"),
    "\n",
    include_str!("page/messages.js")
);
pub(super) const PAINT: &str = "async () => { const timing=globalThis[Symbol.for('canonic.testrun.native-timing.v1')]; if (!timing) throw new Error('Native paint scheduling unavailable'); await new timing.Promise(timing.raf); await new timing.Promise(timing.raf); return true; }";
pub(super) struct ActionOptions {
    pub operation: Option<Uuid>,
    pub force: bool,
    pub postcondition: Option<Condition>,
}
pub(super) const GEOMETRY: &str = include_str!("../reading/geometry.js");

impl Host {
    pub(super) async fn page_resize(
        &self,
        page: Reference,
        viewport: contract::Viewport,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let scope = self.page_scope(page, None, deadline).await?;
        self.inputs.lock().expect("Browser input").invalidate(page);
        self.suspend_previews(page);
        self.connection()?
            .retained(
                &self.wire(page, Kind::Page)?,
                "setViewportSize",
                json!({"viewportSize":viewport}),
                deadline,
                false,
            )
            .await?;
        let geometry = self
            .evaluate(&scope.wire, GEOMETRY, Value::Null, deadline)
            .await?;
        let current = self.frame_current(&scope.wire, deadline).await?;
        if current != scope.reference {
            return Err(Error {
                partial: Some(json!({"dispatchCompleted":true})),
                ..Error::from("DOCUMENT_CHANGED")
            });
        }
        let preview_recovery = match self.restart_previews(page, deadline).await {
            Ok(()) => json!({"succeeded":true}),
            Err(error) => json!({"succeeded":false,"code":error.code}),
        };
        Ok(
            json!({"page":page,"frame":current,"geometry":geometry,"dispatchCompleted":true,"previewRecovery":preview_recovery}),
        )
    }
    pub(super) async fn page_scope(
        &self,
        page: Reference,
        frame: Option<Reference>,
        deadline: Instant,
    ) -> Result<Resource, Error> {
        self.connection()?.event_barrier(deadline).await?;
        let state = self.state.lock().expect("Browser owner");
        state.resource(page, Some(Kind::Page))?;
        let resource = if let Some(frame) = frame {
            state.resource(frame, Some(Kind::Frame))?.clone()
        } else {
            state
                .resources
                .values()
                .find(|r| r.kind == Kind::Frame && r.parent == Some(page))
                .cloned()
                .ok_or("FRAME_UNAVAILABLE")?
        };
        if state.ancestor(resource.reference, Kind::Page) != Some(page) {
            return Err("FOREIGN_FRAME".into());
        }
        Ok(resource)
    }
    pub(super) async fn evaluate(
        &self,
        wire: &ObjectRef,
        expression: &str,
        arg: Value,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.evaluate_probe(wire, expression, arg, deadline, false)
            .await
    }
    pub(super) async fn evaluate_internal(
        &self,
        wire: &ObjectRef,
        expression: &str,
        arg: Value,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.evaluate_probe(wire, expression, arg, deadline, true)
            .await
    }
    async fn evaluate_probe(
        &self,
        wire: &ObjectRef,
        expression: &str,
        arg: Value,
        deadline: Instant,
        internal: bool,
    ) -> Result<Value, Error> {
        let connection = self.connection()?;
        let params =
            json!({"expression":expression,"isFunction":true,"arg":values::argument(&arg)?});
        let reply = if internal {
            connection
                .retained_internal(wire, "evaluateExpression", params, deadline, false)
                .await
        } else {
            connection
                .retained(wire, "evaluateExpression", params, deadline, false)
                .await
        }?
        .ok_or("MISSING_RESULT")?;
        values::result(&reply["value"]).map_err(Error::from)
    }
    pub(super) async fn frame_current(
        &self,
        wire: &ObjectRef,
        deadline: Instant,
    ) -> Result<Reference, Error> {
        self.connection()?
            .event_barrier(deadline.max(Instant::now() + Duration::from_secs(5)))
            .await?;
        let state = self.state.lock().expect("Browser owner");
        let current = state.by_wire(wire).ok_or("STALE_REFERENCE")?;
        state.resource(current.reference, Some(Kind::Frame))?;
        Ok(current.reference)
    }
    pub(super) async fn page_read(
        &self,
        page: Reference,
        frame: Option<Reference>,
        locator: Option<Locator>,
        read: Read,
        max_bytes: usize,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let started = std::time::Instant::now();
        let input_epoch = self.input_epoch.load(Ordering::SeqCst);
        let scope = self.page_scope(page, frame, deadline).await?;
        let connection = self.connection()?;
        let selector = locator
            .as_ref()
            .map(|locator| locator.scoped_selector(scope.reference))
            .transpose()?
            .unwrap_or_else(|| "css=body".into());
        let mut value = match read {
            Read::Geometry => {
                self.evaluate_internal(&scope.wire, GEOMETRY, Value::Null, deadline)
                    .await?
            }
            Read::Count => connection
                .retained_internal(
                    &scope.wire,
                    "queryCount",
                    json!({"selector":selector}),
                    deadline,
                    false,
                )
                .await?
                .ok_or("MISSING_RESULT")?,
            _ => {
                let expression = if matches!(read, Read::Aria) {
                    crate::browser::reading::aria_reader(
                        self.state.lock().expect("Browser owner").engine.name(),
                    )
                } else if matches!(read, Read::Snapshot) {
                    format!(
                        "(element,options)=>({})(element,{{...options,retain:({}).retain}})",
                        crate::browser::reading::DOM,
                        crate::browser::reading::REFERENCES
                    )
                } else {
                    crate::browser::reading::DOM.into()
                };
                let result=connection.retained_internal(&scope.wire,"evalOnSelector",json!({"selector":selector,"strict":true,"expression":expression,"isFunction":true,"arg":values::argument(&json!({"read":read,"maxBytes":max_bytes,"frame":scope.reference}))?}),deadline,false).await?.ok_or("MISSING_RESULT")?;
                let text = values::result(&result["value"])?;
                serde_json::from_str(text.as_str().ok_or("NON_JSON_RESULT")?)
                    .map_err(|_| Error::from("NON_JSON_RESULT"))?
            }
        };
        if serde_json::to_vec(&value)
            .map_err(|_| Error::from("READ_BOUNDS"))?
            .len()
            > max_bytes
        {
            value = json!({"value":null,"truncated":true,"narrowingRequired":true});
        }
        let value = crate::browser::reading::with_narrowing_hint(value);
        let current = self.frame_current(&scope.wire, deadline).await?;
        if current != scope.reference {
            return Err("DOCUMENT_CHANGED".into());
        }
        let observation = if matches!(read, Read::Geometry) && frame.is_none() {
            Some(self.inputs.lock().expect("Browser input").observe(
                page,
                current,
                value.clone(),
                started,
                input_epoch,
            ))
        } else {
            None
        };
        Ok(
            json!({"page":page,"frame":current,"source":"playwright-frame","read":read,"observedAtUnixMs":now_ms(),"result":value,"inputObservation":observation,"inputCoordinateSpace":"css-viewport","inputObservationTtlMs":10000}),
        )
    }
    #[expect(
        clippy::too_many_lines,
        reason = "A compound locator action keeps its preflight, physical hold reservation and postcondition evidence in one owned transaction"
    )]
    pub(super) async fn page_action(
        &self,
        page: Reference,
        frame: Option<Reference>,
        locator: Locator,
        action: Action,
        options: ActionOptions,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let ActionOptions {
            operation,
            force,
            postcondition,
        } = options;
        let annotated = action;
        let action = match &annotated {
            Action::AtSource { action, .. } => action.as_ref(),
            action => action,
        };
        if self
            .inputs
            .lock()
            .expect("Browser input")
            .held
            .get(&page)
            .is_some_and(|held| !held.empty() || held.inflight != 0)
        {
            return Err("INPUT_STATE_CONFLICT".into());
        }
        let scope = self.page_scope(page, frame, deadline).await?;
        let selector = locator.scoped_selector(scope.reference)?;
        let context = self
            .state
            .lock()
            .expect("Browser owner")
            .ancestor(page, Kind::Context);
        let source = match &annotated {
            Action::AtSource { source, .. } => Some(source),
            _ => None,
        };
        let caller_first = context.is_some_and(|context| {
            self.automation_debuggers
                .lock()
                .unwrap()
                .caller_owns_actionability(context, source)
        });
        // Collected calls and confirmed debugger requests expose their real caller.
        // The same strict, non-forced SDK action owns readiness. For pending
        // controls, collection determines pause ordering around that wait. Physical
        // reservation and unknown-release quarantine remain owned below.
        if !force
            && !caller_first
            && matches!(
                action,
                Action::Click {}
                    | Action::DoubleClick {}
                    | Action::Hover {}
                    | Action::Check { .. }
                    | Action::Drag { .. }
                    | Action::Fill { .. }
                    | Action::Clear {}
            )
        {
            self.wait_actionable(operation, &scope, &locator, action, deadline)
                .await?;
        }
        let mut params = json!({"selector":selector,"strict":true});
        let method = match &action {
            Action::AtSource { .. } => return Err("CALL_SOURCE_NESTING".into()),
            Action::Click {} => "click",
            Action::DoubleClick {} => "dblclick",
            Action::Hover {} => "hover",
            Action::Fill { value } => {
                params["value"] = json!(value);
                "fill"
            }
            Action::Clear {} => {
                params["value"] = json!("");
                "fill"
            }
            Action::Focus {} => "focus",
            Action::Blur {} => {
                params["expression"] = json!("element => element.blur()");
                params["isFunction"] = json!(true);
                params["arg"] = values::argument(&Value::Null)?;
                "evalOnSelector"
            }
            Action::Check { checked } => {
                if *checked {
                    "check"
                } else {
                    "uncheck"
                }
            }
            Action::Select { values } => {
                params["options"] = json!(
                    values
                        .iter()
                        .map(|v| json!({"value":v}))
                        .collect::<Vec<_>>()
                );
                "selectOption"
            }
            Action::Press { key } => {
                params["key"] = json!(key);
                "press"
            }
            Action::Type { text } => {
                params["text"] = json!(text);
                "type"
            }
            Action::Drag { target } => {
                params = json!({"source":selector,"target":target.scoped_selector(scope.reference)?,"strict":true});
                "dragAndDrop"
            }
            Action::Dispatch { event } => {
                params["type"] = json!(event);
                params["eventInit"] = values::argument(&json!({}))?;
                "dispatchEvent"
            }
        };
        if matches!(
            method,
            "click"
                | "dblclick"
                | "hover"
                | "fill"
                | "check"
                | "uncheck"
                | "selectOption"
                | "dragAndDrop"
        ) {
            params["force"] = json!(force);
        }
        if let Some(id) = operation {
            self.admissible(id, deadline)?;
        }
        self.compound_input(page, &scope.wire, &annotated, method, params, deadline)
            .await?;
        let observed = if let Some(condition) = postcondition {
            match self
                .wait_condition(operation, page, frame, Some(locator), condition, deadline)
                .await
            {
                Ok(observed) => Some(observed),
                Err(mut error) => {
                    error.partial = Some(
                        json!({"page":page,"dispatchCompleted":true,"postconditionSatisfied":false,"mode":if force{"forced"}else if matches!(action,Action::Dispatch{..}){"scripted"}else{"normal"}}),
                    );
                    return Err(error);
                }
            }
        } else {
            None
        };
        let current = match self.frame_current(&scope.wire, deadline).await {
            Ok(current) => Some(current),
            Err(error) if error.code == "STALE_REFERENCE" => None,
            Err(mut error) => {
                error.partial = Some(
                    json!({"page":page,"dispatchCompleted":true,"potentiallyMutating":true,"source":"playwright-action"}),
                );
                return Err(error);
            }
        };
        Ok(
            json!({"page":page,"frame":current,"mode":if matches!(action,Action::Dispatch{..}){"scripted"}else if force{"forced"}else{"normal"},"source":"playwright-action","dispatchCompleted":true,"postcondition":observed}),
        )
    }
    pub(super) async fn page_wait(
        &self,
        page: Reference,
        frame: Option<Reference>,
        locator: Option<Locator>,
        condition: Condition,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.wait_condition(None, page, frame, locator, condition, deadline)
            .await
    }
    pub(super) async fn navigate(
        &self,
        page: Reference,
        navigation: Navigation,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let scope = self.page_scope(page, None, deadline).await?;
        let page_wire = self.wire(page, Kind::Page)?;
        let (wire, method, params) = match &navigation {
            Navigation::Goto { url, wait_until } => (
                &scope.wire,
                "goto",
                json!({"url":url,"waitUntil":wait_until}),
            ),
            Navigation::SetContent { html, wait_until } => (
                &scope.wire,
                "setContent",
                json!({"html":html,"waitUntil":wait_until}),
            ),
            Navigation::Reload { wait_until } => {
                (&page_wire, "reload", json!({"waitUntil":wait_until}))
            }
            Navigation::Back { wait_until } => {
                (&page_wire, "goBack", json!({"waitUntil":wait_until}))
            }
            Navigation::Forward { wait_until } => {
                (&page_wire, "goForward", json!({"waitUntil":wait_until}))
            }
        };
        let connection = self.connection()?;
        self.suspend_previews(page);
        if matches!(navigation, Navigation::SetContent { .. }) {
            self.inputs
                .lock()
                .expect("Browser input")
                .document_changed(page);
            self.invalidate_document(&scope.wire).await;
        }
        let result = connection
            .retained(wire, method, params, deadline, false)
            .await?;
        let response = if let Some(guid) =
            result.as_ref().and_then(|r| r["response"]["guid"].as_str())
        {
            let wire = connection.resolve(guid, "Response").await?;
            let init = connection
                .initializer(&wire)
                .await?
                .ok_or("MISSING_INITIALIZER")?;
            Some(json!({"url":init["url"],"status":init["status"],"statusText":init["statusText"]}))
        } else {
            None
        };
        let geometry = self
            .evaluate(&scope.wire, GEOMETRY, Value::Null, deadline)
            .await?;
        let current = self.frame_current(&scope.wire, deadline).await?;
        let preview_recovery = match self.restart_previews(page, deadline).await {
            Ok(()) => json!({"succeeded":true}),
            Err(error) => json!({"succeeded":false,"code":error.code}),
        };
        Ok(
            json!({"page":page,"frame":current,"url":geometry["url"],"response":response,"documentReplaced":current!=scope.reference,"source":"playwright-navigation","previewRecovery":preview_recovery}),
        )
    }
    pub(super) async fn frame_enter(
        &self,
        page: Reference,
        frame: Option<Reference>,
        locator: Locator,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let scope = self.page_scope(page, frame, deadline).await?;
        let element = self.element(&scope, &locator, deadline).await?;
        let result = self
            .connection()?
            .retained(&element, "contentFrame", json!({}), deadline, false)
            .await;
        self.dispose_element(&element).await?;
        let reply = result?.ok_or("MISSING_RESULT")?;
        let wire = self
            .connection()?
            .resolve(
                reply["frame"]["guid"].as_str().ok_or("NOT_A_FRAME")?,
                "Frame",
            )
            .await?;
        let reference = self.frame_current(&wire, deadline).await?;
        if self
            .state
            .lock()
            .expect("Browser owner")
            .ancestor(reference, Kind::Page)
            != Some(page)
        {
            return Err("FOREIGN_FRAME".into());
        }
        Ok(json!({"frame":reference,"page":page}))
    }
    pub(super) async fn element(
        &self,
        scope: &Resource,
        locator: &Locator,
        deadline: Instant,
    ) -> Result<ObjectRef, Error> {
        let connection = self.connection()?;
        let result = connection
            .retained(
                &scope.wire,
                "querySelector",
                json!({"selector":locator.scoped_selector(scope.reference)?,"strict":true}),
                deadline,
                false,
            )
            .await?;
        super::workflows::channel(&connection, result, "element", "ElementHandle").await
    }
    pub(super) async fn dispose_element(&self, element: &ObjectRef) -> Result<(), Error> {
        self.connection()?
            .retained(
                element,
                "dispose",
                json!({}),
                Instant::now() + Duration::from_secs(30),
                true,
            )
            .await?;
        Ok(())
    }
    pub(super) async fn invalidate_document(&self, wire: &ObjectRef) {
        let handles = {
            let mut state = self.state.lock().expect("Browser owner");
            let handles = state
                .subtree(
                    state
                        .by_wire(wire)
                        .map(|r| r.reference)
                        .unwrap_or(Reference {
                            connection_id: Uuid::nil(),
                            id: Uuid::nil(),
                            generation: Uuid::nil(),
                        }),
                )
                .into_iter()
                .filter(|r| r.kind == Kind::Handle)
                .collect::<Vec<_>>();
            state.navigate(wire);
            handles
        };
        for handle in handles {
            if self.dispose_element(&handle.wire).await.is_err() {
                self.set_failure("HANDLE_CLEANUP_PENDING");
            }
        }
    }
}
pub(super) fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
