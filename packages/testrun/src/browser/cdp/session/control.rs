//! Chrome composition of the shared exclusive authority and authenticated viewer input channel.
use super::*;
use crate::control::{Authority, Credential, Owner};
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) struct ControlState {
    authority: Arc<Authority>,
    page: Mutex<Option<Reference>>,
    credentials: Mutex<BTreeMap<Uuid, Credential>>,
    applied: Mutex<Credential>,
    outcome: Mutex<Value>,
    input_generation: AtomicU64,
    input_commands: Mutex<BTreeMap<Uuid, u64>>,
    channel: Mutex<Option<Arc<crate::control::channel::Channel>>>,
}
impl Default for ControlState {
    fn default() -> Self {
        let authority = Arc::new(Authority::new(Uuid::new_v4().to_string(), true));
        let applied = authority.credential(std::time::Instant::now());
        Self {
            authority,
            page: Mutex::new(None),
            credentials: Mutex::default(),
            applied: Mutex::new(applied),
            outcome: Mutex::new(json!({"state":"idle"})),
            input_generation: AtomicU64::new(0),
            input_commands: Mutex::default(),
            channel: Mutex::new(None),
        }
    }
}
impl ControlState {
    pub fn snapshot(&self) -> Value {
        let mut status =
            crate::managed::clean_result(self.authority.status(std::time::Instant::now()));
        status["page"] = json!(*self.page.lock().expect("Viewer page"));
        status["coordinateSpace"] = json!("css-viewport");
        status["handoff"] = self.outcome.lock().expect("Control outcome").clone();
        if let Some(channel) = self.channel.lock().expect("Viewer channel").as_ref()
            && status["owner"] == "viewer"
            && channel.metadata()["controlVersion"] == status["controlVersion"]
        {
            status["channel"] = channel.metadata();
        }
        status
    }
    pub fn active(&self) -> bool {
        self.authority.credential(std::time::Instant::now())
            != *self.applied.lock().expect("Applied control")
    }
    pub fn needs_poll(&self) -> bool {
        self.active() || self.authority.status(std::time::Instant::now())["owner"] == "viewer"
    }
    pub fn reconcile(&self, pages: &[Reference]) {
        if self
            .page
            .lock()
            .expect("Viewer page")
            .is_some_and(|page| !pages.contains(&page))
        {
            let credential = self.authority.credential(std::time::Instant::now());
            self.authority.disconnect(&credential);
        }
    }
    pub fn finish(&self, id: Uuid) {
        self.credentials
            .lock()
            .expect("Input credentials")
            .remove(&id);
        self.input_commands
            .lock()
            .expect("Input generations")
            .remove(&id);
    }
    pub fn fence_inputs(&self) {
        self.input_generation.fetch_add(1, Ordering::SeqCst);
    }
    pub fn input_current(&self, id: Uuid) -> Result<(), Error> {
        if self
            .input_commands
            .lock()
            .expect("Input generations")
            .get(&id)
            .is_some_and(|generation| *generation != self.input_generation.load(Ordering::SeqCst))
        {
            return Err("INPUT_RELEASE_REQUESTED".into());
        }
        Ok(())
    }
}
fn contains_input(request: &Request) -> bool {
    match request {
        Request::PageInput { .. } | Request::PageAction { .. } => true,
        Request::Batch { requests } => requests.iter().any(contains_input),
        _ => false,
    }
}
pub(super) struct ControlCommand {
    expected: Credential,
    desired: Option<Owner>,
    page: Option<Reference>,
    reply: tokio::sync::oneshot::Sender<Result<Value, Error>>,
    handler: crate::control::channel::Handler,
}
fn control_error(error: crate::error::Error) -> Error {
    let code = match error.code.as_str() {
        "CONTROL_HANDOFF_PENDING" => "CONTROL_HANDOFF_PENDING",
        "CONTROL_NOT_OWNED" => "CONTROL_NOT_OWNED",
        "STALE_CONTROL_LEASE" => "STALE_CONTROL_LEASE",
        _ => "CONTROL_HANDOFF_FAILED",
    };
    Error::caused(code, error)
}

impl Session {
    pub(super) fn admit_native_control(
        &self,
        id: Uuid,
        request: &Request,
        viewer: Option<&Credential>,
    ) -> Result<(), Error> {
        if !crate::browser::session::control::fenced(request) {
            return Ok(());
        }
        let control = &self.0.shared.control;
        if viewer.is_none()
            && control.authority.status(std::time::Instant::now())["owner"] != "agent"
        {
            return Err("CONTROL_NOT_OWNED".into());
        }
        let credential = viewer
            .cloned()
            .unwrap_or_else(|| control.authority.credential(std::time::Instant::now()));
        let credential = control
            .authority
            .admit(&credential.id, std::time::Instant::now())
            .map_err(control_error)?;
        control
            .credentials
            .lock()
            .expect("Input credentials")
            .insert(id, credential);
        if contains_input(request) {
            control
                .input_commands
                .lock()
                .expect("Input generations")
                .insert(id, control.input_generation.load(Ordering::SeqCst));
        }
        Ok(())
    }
    pub(super) async fn native_change_control(
        &self,
        name: &str,
        args: Value,
    ) -> Result<Value, Error> {
        if self.0.shared.state.lock().expect("Chrome state").phase != Phase::Ready {
            return Err("SESSION_NOT_READY".into());
        }
        let desired = if name == "browser_control_release" {
            None
        } else {
            Some(crate::control::owner(&args).map_err(control_error)?)
        };
        let page: Option<Reference> = args
            .get("page")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| Error::from("INVALID_SCOPE"))?;
        if matches!(desired, Some(Owner::Viewer(_))) != page.is_some() {
            return Err("INVALID_SCOPE".into());
        }
        if let Some(page) = page {
            let resources = self.0.shared.resources.lock().expect("Chrome resources");
            if !resources.as_array().is_some_and(|resources| {
                resources.iter().any(|resource| {
                    resource["kind"] == "page" && resource["reference"] == json!(page)
                })
            }) {
                return Err("STALE_REFERENCE".into());
            }
        }
        let expected = self
            .0
            .shared
            .control
            .authority
            .credential(std::time::Instant::now());
        if args["expectedVersion"].as_u64() != Some(expected.version) {
            return Err("STALE_CONTROL_LEASE".into());
        }
        let slot = self
            .0
            .controls
            .try_reserve()
            .map_err(|_| Error::from("CONTROL_HANDOFF_PENDING"))?;
        self.0
            .shared
            .control
            .authority
            .plan(&expected, desired.clone(), std::time::Instant::now())
            .map_err(control_error)?;
        let handler: crate::control::channel::Handler = {
            let weak = Arc::downgrade(&self.0);
            Arc::new(move |input| {
                let weak = weak.clone();
                Box::pin(async move {
                    let client = weak.upgrade().ok_or_else(|| {
                        crate::error::Error::new("SESSION_CLOSING", "Browser session ended")
                    })?;
                    Session(client).native_viewer_input(input).await
                })
            })
        };
        let (reply, receive) = tokio::sync::oneshot::channel();
        *self
            .0
            .shared
            .control
            .outcome
            .lock()
            .expect("Control outcome") = json!({"state":"running","expectedVersion":expected.version,"requestedOwner":args["owner"],"page":page});
        slot.send(ControlCommand {
            expected,
            desired,
            page,
            reply,
            handler,
        });
        receive
            .await
            .map_err(|_| Error::from("CONTROL_HANDOFF_FAILED"))?
    }
    async fn native_viewer_input(&self, input: Value) -> crate::error::Result<Value> {
        let control = &self.0.shared.control;
        let credential = control.authority.credential(std::time::Instant::now());
        if input["viewerLeaseId"] != credential.id || input["viewerVersion"] != credential.version {
            return Err(crate::error::Error::new(
                "STALE_CONTROL_LEASE",
                "Viewer lease changed",
            ));
        }
        let page: Reference = serde_json::from_value(input["page"].clone())
            .map_err(|_| crate::error::Error::invalid("Use a complete page reference"))?;
        if *control.page.lock().expect("Viewer page") != Some(page) {
            return Err(crate::error::Error::new(
                "INVALID_SCOPE",
                "Viewer belongs to another page",
            ));
        }
        let id = input["commandId"]
            .as_str()
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| crate::error::Error::invalid("Use a UUID commandId"))?;
        let (_, envelope) = crate::browser::api::translate(
            "act",
            json!({"targetId":format!("browser:{}",page.connection_id),"actions":[{"action":"page_input","page":page,"observation":input["observation"],"actions":input["actions"]}]}),
        )?;
        let request: Request =
            serde_json::from_value(envelope["request"]["requests"][0].clone())
                .map_err(|_| crate::error::Error::invalid("Invalid browser input"))?;
        self.enqueue_as(id, Some(request), 30000, Some(&credential))
            .map_err(|error| crate::error::Error::new(error.code, "Viewer input refused"))?;
        let operation = self
            .wait(id, Instant::now() + Duration::from_secs(31))
            .await
            .map_err(|error| crate::error::Error::new(error.code, "Viewer status unavailable"))?;
        Ok(json!({"operation":operation}))
    }
}
impl Worker {
    pub(super) fn controller_current(&self, id: Uuid, queued: bool) -> Result<(), Error> {
        let credential = self
            .shared
            .control
            .credentials
            .lock()
            .expect("Input credentials")
            .get(&id)
            .cloned();
        if let Some(credential) = credential {
            if queued {
                self.shared
                    .control
                    .authority
                    .admit(&credential.id, std::time::Instant::now())
                    .map_err(control_error)?;
            } else if self
                .shared
                .control
                .authority
                .credential(std::time::Instant::now())
                != credential
            {
                return Err("STALE_CONTROL_LEASE".into());
            }
        }
        Ok(())
    }
    pub(super) async fn apply_native_control(&mut self, command: ControlCommand) {
        let result = async {
            if self
                .shared
                .control
                .authority
                .credential(std::time::Instant::now())
                != command.expected
            {
                return Err("STALE_CONTROL_LEASE".into());
            }
            if let Some(page) = command.page {
                self.index
                    .route(page, Some(Kind::Page))
                    .map_err(resource_error)?;
            }
            self.release_inputs().await?;
            self.inputs.observations.clear();
            self.media
                .observations
                .lock()
                .expect("Media observations")
                .observations
                .clear();
            self.shared
                .control
                .authority
                .apply(
                    &command.expected,
                    command.desired.clone(),
                    std::time::Instant::now(),
                )
                .map_err(control_error)?;
            *self.shared.control.page.lock().expect("Viewer page") = command.page;
            *self.shared.control.applied.lock().expect("Applied control") = self
                .shared
                .control
                .authority
                .credential(std::time::Instant::now());
            {
                let old = {
                    self.shared
                        .control
                        .channel
                        .lock()
                        .expect("Viewer channel")
                        .take()
                };
                if let Some(old) = old {
                    old.close().await;
                }
                if matches!(command.desired, Some(Owner::Viewer(_))) {
                    let credential = self
                        .shared
                        .control
                        .authority
                        .credential(std::time::Instant::now());
                    let channel = crate::control::channel::Channel::start_browser(
                        self.shared.control.authority.clone(),
                        credential.clone(),
                        command.handler,
                    )
                    .await
                    .map_err(|error| {
                        self.shared.control.authority.disconnect(&credential);
                        control_error(error)
                    })?;
                    *self.shared.control.channel.lock().expect("Viewer channel") = Some(channel);
                }
            }
            Ok(self.shared.control.snapshot())
        }
        .await;
        {
            let mut outcome = self.shared.control.outcome.lock().expect("Control outcome");
            if outcome["state"] == "running"
                && outcome["expectedVersion"] == command.expected.version
            {
                outcome["state"] = json!(if result.is_ok() { "complete" } else { "failed" });
                outcome["code"] = result
                    .as_ref()
                    .err()
                    .map_or(Value::Null, |error: &Error| json!(error.code));
            }
        }
        let _ = command.reply.send(result);
        self.shared.changed.notify_waiters();
    }
    pub(super) async fn expire_control(&mut self) -> Result<(), Error> {
        if !self.shared.control.active() {
            return Ok(());
        }
        self.release_inputs().await?;
        self.inputs.observations.clear();
        self.media
            .observations
            .lock()
            .expect("Media observations")
            .observations
            .clear();
        *self.shared.control.applied.lock().expect("Applied control") = self
            .shared
            .control
            .authority
            .credential(std::time::Instant::now());
        *self.shared.control.page.lock().expect("Viewer page") = None;
        {
            let old = {
                self.shared
                    .control
                    .channel
                    .lock()
                    .expect("Viewer channel")
                    .take()
            };
            if let Some(old) = old {
                old.close().await;
            }
        }
        Ok(())
    }
    pub(super) async fn close_native_control(&mut self) {
        self.shared.control.authority.close();
        let _ = self.expire_control().await;
    }
}
pub(super) async fn next(
    queue: &mut Option<mpsc::Receiver<ControlCommand>>,
) -> Option<ControlCommand> {
    match queue {
        Some(queue) => queue.recv().await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_fences_only_previously_admitted_input_generations() {
        let control = ControlState::default();
        let old = Uuid::new_v4();
        let next = Uuid::new_v4();
        control.input_commands.lock().unwrap().insert(old, 0);
        assert!(control.input_current(old).is_ok());
        control.fence_inputs();
        assert_eq!(
            control.input_current(old).unwrap_err().code,
            "INPUT_RELEASE_REQUESTED"
        );
        control.input_commands.lock().unwrap().insert(next, 1);
        assert!(control.input_current(next).is_ok());
        assert!(
            control.input_current(Uuid::new_v4()).is_ok(),
            "Unrelated reads retain admission"
        );
        control.finish(old);
        assert!(control.input_commands.lock().unwrap().get(&old).is_none());
    }
    #[tokio::test]
    async fn native_handoff_fences_queued_credentials_before_changing_owner() {
        let (mut worker, _peer, _root) = super::super::tests::fixture().await;
        let id = Uuid::new_v4();
        let credential = worker
            .shared
            .control
            .authority
            .credential(std::time::Instant::now());
        worker
            .shared
            .control
            .credentials
            .lock()
            .unwrap()
            .insert(id, credential.clone());
        worker
            .shared
            .control
            .authority
            .plan(
                &credential,
                Some(Owner::Viewer("Acme".into())),
                std::time::Instant::now(),
            )
            .unwrap();
        assert_eq!(
            worker.controller_current(id, true).unwrap_err().code,
            "CONTROL_HANDOFF_PENDING"
        );
        assert!(
            worker.controller_current(id, false).is_ok(),
            "Already dispatched work keeps its receipt"
        );
        worker
            .shared
            .control
            .authority
            .apply(
                &credential,
                Some(Owner::Viewer("Acme".into())),
                std::time::Instant::now(),
            )
            .unwrap();
        assert_eq!(
            worker.controller_current(id, false).unwrap_err().code,
            "STALE_CONTROL_LEASE"
        );
        worker
            .close(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn stale_native_handoff_cannot_release_the_current_owners_holds() {
        let (mut worker, _peer, _root) = super::super::tests::fixture().await;
        let expected = worker
            .shared
            .control
            .authority
            .credential(std::time::Instant::now());
        worker
            .shared
            .control
            .authority
            .plan(&expected, Some(Owner::Agent), std::time::Instant::now())
            .unwrap();
        worker
            .shared
            .control
            .authority
            .apply(&expected, Some(Owner::Agent), std::time::Instant::now())
            .unwrap();
        let page = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        worker
            .inputs
            .held
            .entry(page)
            .or_default()
            .keys
            .insert("ShiftLeft".into(), "Shift".into());
        let (reply, receive) = tokio::sync::oneshot::channel();
        worker
            .apply_native_control(ControlCommand {
                expected,
                desired: None,
                page: None,
                reply,
                handler: Arc::new(|_| Box::pin(async { Ok(Value::Null) })),
            })
            .await;
        assert_eq!(
            receive.await.unwrap().unwrap_err().code,
            "STALE_CONTROL_LEASE"
        );
        assert_eq!(worker.inputs.held[&page].keys.len(), 1);
        worker.inputs.held.clear();
        worker
            .close(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
    }
}
