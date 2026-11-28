//! Pinned driver binding registrations and opt-in argument disclosure.
use super::*;
use crate::browser::{bindings, evaluation};
impl Host {
    pub(super) async fn binding_operation(
        &self,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.connection()?.event_barrier(deadline).await?;
        if let Request::BindingRead { callback } = request {
            return self.binding_read(callback, deadline).await;
        }
        let scope = match &request {
            Request::BindingAdd { scope, .. }
            | Request::BindingList { scope }
            | Request::BindingRemove { scope, .. } => *scope,
            _ => return Err("INVALID_SCOPE".into()),
        };
        let resource = {
            let state = self.state.lock().expect("Browser owner");
            let r = state.resource(scope, None)?.clone();
            if !matches!(r.kind, Kind::Page | Kind::Context) {
                return Err("INVALID_SCOPE".into());
            }
            self.bindings.lock().expect("Bindings").retain(|scope| {
                state
                    .resources
                    .get(&scope.id)
                    .is_some_and(|r| r.reference == scope)
            });
            r
        };
        match request {
            Request::BindingList { .. } => Ok(self.bindings.lock().expect("Bindings").list(scope)),
            Request::BindingAdd { name, .. } => {
                self.binding_add(scope, &resource.wire, name, deadline)
                    .await
            }
            Request::BindingRemove { binding_id, .. } => {
                {
                    let state = self.state.lock().expect("Browser owner");
                    self.binding_calls
                        .lock()
                        .expect("Binding calls")
                        .retain(|r, _| {
                            state
                                .resources
                                .get(&r.id)
                                .is_some_and(|resource| resource.reference == *r)
                        });
                }
                if self
                    .binding_calls
                    .lock()
                    .expect("Binding calls")
                    .values()
                    .any(|id| *id == binding_id)
                {
                    return Err("BINDING_CALLS_PENDING".into());
                }
                let registration = self
                    .bindings
                    .lock()
                    .expect("Bindings")
                    .get(scope, binding_id)?;
                let wire = registration.backend.ok_or("BINDING_PENDING")?;
                let connection = self.connection()?;
                self.bindings
                    .lock()
                    .expect("Bindings")
                    .start_remove(scope, binding_id)?;
                let result = connection
                    .retained(&wire, "dispose", json!({}), deadline, false)
                    .await;
                if let Err(error) = result {
                    let error = Error::from(error);
                    if error.code != "OUTCOME_UNKNOWN" {
                        self.bindings
                            .lock()
                            .expect("Bindings")
                            .retry_remove(binding_id);
                    }
                    return Err(error);
                }
                self.bindings.lock().expect("Bindings").forget(binding_id);
                Ok(json!({"scope":scope,"bindingId":binding_id,"removed":true}))
            }
            _ => Err("INVALID_SCOPE".into()),
        }
    }
    async fn binding_read(&self, callback: Reference, deadline: Instant) -> Result<Value, Error> {
        if self
            .callbacks
            .lock()
            .expect("Callbacks")
            .get(&callback)
            .is_none_or(|expires| *expires <= Instant::now())
        {
            return Err("CALLBACK_EXPIRED".into());
        }
        let resource = self
            .state
            .lock()
            .expect("Browser owner")
            .resource(callback, Some(Kind::Binding))?
            .clone();
        let id = self
            .binding_calls
            .lock()
            .expect("Binding calls")
            .get(&callback)
            .copied()
            .ok_or("UNKNOWN_BINDING")?;
        let init = self
            .connection()?
            .initializer(&resource.wire)
            .await?
            .ok_or("STALE_REFERENCE")?;
        if serde_json::to_vec(&init["args"])
            .map_err(|_| Error::from("BACKEND_PROTOCOL"))?
            .len()
            > bindings::MAX_ARGUMENT_BYTES
        {
            return Err("BINDING_ARGUMENT_BOUNDS".into());
        }
        let args = init["args"].as_array().ok_or("BACKEND_PROTOCOL")?;
        if args.len() > 64 {
            return Err("BINDING_ARGUMENT_BOUNDS".into());
        }
        let arguments = args
            .iter()
            .map(evaluation::result)
            .collect::<Result<Vec<_>, _>>()?;
        self.connection()?.event_barrier(deadline).await?;
        let frame = {
            let state = self.state.lock().expect("Browser owner");
            state.resource(callback, Some(Kind::Binding))?;
            let frame = state
                .ancestor(callback, Kind::Frame)
                .ok_or("INVALID_SCOPE")?;
            state.resource(frame, Some(Kind::Frame))?;
            frame
        };
        Ok(
            json!({"callback":callback,"bindingId":id,"frame":frame,"arguments":arguments,"private":true}),
        )
    }
    async fn binding_add(
        &self,
        scope: Reference,
        wire: &crate::browser::objects::ObjectRef,
        name: String,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let id = self
            .bindings
            .lock()
            .expect("Bindings")
            .begin(scope, name.clone())?;
        let result: Result<_, Error> = async {
            let reply = self
                .connection()?
                .retained(
                    wire,
                    "exposeBinding",
                    json!({"name":name,"noGlobal":false}),
                    deadline,
                    false,
                )
                .await?
                .ok_or("MISSING_RESULT")?;
            Ok(self
                .connection()?
                .resolve(
                    reply["disposable"]["guid"]
                        .as_str()
                        .ok_or("BACKEND_PROTOCOL")?,
                    "Disposable",
                )
                .await?)
        }
        .await;
        match result {
            Ok(wire) => {
                self.bindings.lock().expect("Bindings").complete(id, wire)?;
                Ok(json!({"scope":scope,"bindingId":id,"registered":true}))
            }
            Err(mut error) => {
                let uncertain = matches!(
                    error.code,
                    "OUTCOME_UNKNOWN" | "BACKEND_PROTOCOL" | "MISSING_RESULT"
                );
                if !uncertain {
                    self.bindings.lock().expect("Bindings").forget(id);
                }
                error.partial =
                    Some(json!({"scope":scope,"bindingId":id,"registrationUncertain":uncertain}));
                Err(error)
            }
        }
    }
}
