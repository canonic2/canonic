//! Native scoped bindings with document-fenced callbacks and independent replies.
use super::*;
use crate::browser::{bindings, evaluation, values};
mod registration;
#[derive(Clone, Default)]
struct Installation {
    script: Option<String>,
    registered: bool,
    pending: bool,
}
#[derive(Clone)]
struct Native {
    scope: Reference,
    context: Reference,
    kind: Kind,
    name: String,
    raw: String,
    targets: BTreeMap<String, Installation>,
    initializing: bool,
    closing: bool,
}
#[derive(Clone)]
struct Call {
    binding: Uuid,
    scope: Reference,
    world: Reference,
    raw: String,
    ticket: i64,
    args: Vec<evaluation::EvaluationValue>,
    expires: Instant,
    responding: bool,
    retry: Instant,
}
#[derive(Default)]
pub(super) struct Bindings {
    registrations: bindings::Registrations<()>,
    items: BTreeMap<Uuid, Native>,
    calls: BTreeMap<Reference, Call>,
}
impl Worker {
    pub(super) fn fence_bindings(&mut self) {
        self.evaluation
            .bindings
            .calls
            .retain(|_, call| self.index.retained(call.world, Kind::World));
        self.evaluation.bindings.registrations.retain(|scope| {
            self.index.retained(scope, Kind::Page) || self.index.retained(scope, Kind::Context)
        });
        self.evaluation.bindings.items.retain(|_, item| {
            item.targets
                .retain(|session, _| self.index.retained_attachment(session));
            self.index.retained(item.scope, item.kind)
        });
    }
    pub(super) fn binding_snapshot(&self) -> Vec<Value> {
        self.evaluation.bindings.calls.iter().map(|(reference,c)|json!({"reference":reference,"kind":"binding","page":c.scope,"bindingId":c.binding,"argumentCount":c.args.len(),"argumentsIncluded":false,"expiresInMs":c.expires.saturating_duration_since(Instant::now()).as_millis()})).collect()
    }
    pub(super) fn binding_count(&self) -> usize {
        self.evaluation.bindings.calls.len()
    }
    pub(super) fn binding_event(&mut self, event: &Event) -> Result<(), Error> {
        if event.method != "Runtime.bindingCalled" {
            return Ok(());
        }
        let Some(session) = event.session_id.as_deref() else {
            return Ok(());
        };
        let Some((id, scope, context, kind, raw)) = self
            .evaluation
            .bindings
            .items
            .iter()
            .find(|(_, item)| {
                item.raw == event.params["name"]
                    && item
                        .targets
                        .get(session)
                        .is_some_and(|target| target.registered)
            })
            .map(|(id, item)| (*id, item.scope, item.context, item.kind, item.raw.clone()))
        else {
            return Ok(());
        };
        if self.index.native(scope, kind).is_err() {
            return Ok(());
        }
        let Some((owner, Some(page), _)) = self.index.diagnostic_scope(session) else {
            return Ok(());
        };
        if owner != context || (kind == Kind::Page && page != scope) {
            return Ok(());
        }
        let Some(numeric) = event.params["executionContextId"].as_i64() else {
            return Ok(());
        };
        // Application code can call the raw bridge, and a queued call can
        // arrive after its document disappears. Neither is transport loss.
        let Ok(world) = self.index.binding_world(page, session, numeric) else {
            return Ok(());
        };
        let Some(payload) = event.params["payload"].as_str() else {
            return Ok(());
        };
        if payload.len() > bindings::MAX_ARGUMENT_BYTES {
            return Ok(());
        }
        let Ok(payload) = serde_json::from_str::<Value>(payload) else {
            return Ok(());
        };
        let Some(ticket) = payload["ticket"]
            .as_i64()
            .filter(|n| *n > 0 && *n <= 9007199254740991)
        else {
            return Ok(());
        };
        if self
            .evaluation
            .bindings
            .calls
            .values()
            .any(|c| c.world == world && c.raw == raw && c.ticket == ticket)
        {
            return Ok(());
        }
        if self.binding_count() + self.interactions.dialogs.len() + self.interactions.routes.len()
            >= 64
        {
            return Err("CALLBACK_CAPACITY".into());
        }
        let Some(args) = payload["args"].as_array().filter(|a| a.len() <= 64) else {
            return Ok(());
        };
        let Ok(args) = args
            .iter()
            .map(evaluation::result)
            .collect::<Result<Vec<_>, _>>()
        else {
            return Ok(());
        };
        let reference = Reference {
            connection_id: scope.connection_id,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        self.evaluation.bindings.calls.insert(
            reference,
            Call {
                binding: id,
                scope: page,
                world,
                raw,
                ticket,
                args,
                expires: Instant::now() + Duration::from_secs(30),
                responding: false,
                retry: Instant::now(),
            },
        );
        Ok(())
    }
    pub(super) async fn binding_operation(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.drain()?;
        if let Request::BindingRead { callback } = request {
            if callback.connection_id != self.shared.state.lock().expect("Chrome owner").connection
            {
                return Err("FOREIGN_REFERENCE".into());
            }
            let call = self
                .evaluation
                .bindings
                .calls
                .get(&callback)
                .ok_or("STALE_REFERENCE")?;
            let source = self
                .index
                .route(call.world, Some(Kind::World))
                .map_err(resource_error)?;
            let frame = source.frame.ok_or("INVALID_SCOPE")?;
            if call.expires <= Instant::now() {
                return Err("CALLBACK_EXPIRED".into());
            }
            return Ok(
                json!({"callback":callback,"bindingId":call.binding,"frame":frame,"arguments":call.args,"private":true}),
            );
        }
        self.binding_registration(id, request, deadline).await
    }
    async fn binding_bridge(
        &mut self,
        world: Reference,
        function: &str,
        arguments: Vec<Value>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let route = self
            .index
            .route(world, Some(Kind::World))
            .map_err(resource_error)?;
        let reply=self.responsive_call(Some(&route.session_id),"Runtime.callFunctionOn",json!({"functionDeclaration":function,"arguments":arguments,"uniqueContextId":route.execution_unique_id,"returnByValue":true,"awaitPromise":false}),Lane::Ordinary,deadline).await?;
        if reply.get("exceptionDetails").is_some() {
            return Err("BINDING_BRIDGE_FAILED".into());
        }
        Ok(reply["result"]["value"].clone())
    }
    pub(super) async fn resolve_binding(
        &mut self,
        reference: Reference,
        response: contract::CallbackResponse,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if reference.connection_id != self.shared.state.lock().expect("Chrome owner").connection {
            return Err("FOREIGN_REFERENCE".into());
        }
        let call = self
            .evaluation
            .bindings
            .calls
            .get(&reference)
            .cloned()
            .ok_or("STALE_REFERENCE")?;
        if call.responding {
            return Err("CALLBACK_ALREADY_RESOLVING".into());
        }
        if call.expires <= Instant::now() {
            return Err("CALLBACK_EXPIRED".into());
        }
        let (value, reject) = match response {
            contract::CallbackResponse::BindingResolve { value } => {
                (values::argument(&value)?["value"].clone(), json!(false))
            }
            contract::CallbackResponse::BindingReject {} => (Value::Null, json!(true)),
            contract::CallbackResponse::BindingResolveValue { value } => (
                bindings::value_argument(&value)?["value"].clone(),
                json!(false),
            ),
            contract::CallbackResponse::BindingRejectValue { value } => (
                bindings::value_argument(&value)?["value"].clone(),
                json!("value"),
            ),
            _ => return Err("CALLBACK_RESPONSE_MISMATCH".into()),
        };
        self.evaluation
            .bindings
            .calls
            .get_mut(&reference)
            .ok_or("STALE_REFERENCE")?
            .responding = true;
        let result = self
            .binding_bridge(
                call.world,
                bindings::SETTLE,
                vec![
                    json!({"value":call.raw}),
                    json!({"value":call.ticket}),
                    json!({"value":value}),
                    json!({"value":reject}),
                ],
                deadline,
            )
            .await;
        match result {
            Ok(value) => {
                self.evaluation.bindings.calls.remove(&reference);
                if value != true {
                    return Err("STALE_REFERENCE".into());
                }
                Ok(json!({"callback":reference,"resolved":true,"replayed":false}))
            }
            Err(error) => {
                if error.code != "OUTCOME_UNKNOWN"
                    && let Some(call) = self.evaluation.bindings.calls.get_mut(&reference)
                {
                    call.responding = false;
                }
                Err(error)
            }
        }
    }
    pub(super) async fn expire_bindings(&mut self) {
        self.evaluation
            .bindings
            .calls
            .retain(|_, call| self.index.retained(call.world, Kind::World));
        let expired = self
            .evaluation
            .bindings
            .calls
            .iter()
            .filter(|(_, c)| {
                !c.responding && c.expires <= Instant::now() && c.retry <= Instant::now()
            })
            .map(|(id, c)| (*id, c.clone()))
            .collect::<Vec<_>>();
        for (id, call) in expired {
            let Some(c) = self.evaluation.bindings.calls.get_mut(&id) else {
                continue;
            };
            if c.responding {
                continue;
            }
            c.responding = true;
            let result = self
                .binding_bridge(
                    call.world,
                    bindings::SETTLE,
                    vec![
                        json!({"value":call.raw}),
                        json!({"value":call.ticket}),
                        json!({"value":null}),
                        json!({"value":true}),
                    ],
                    Instant::now() + Duration::from_secs(2),
                )
                .await;
            match result {
                Ok(_) => {
                    self.evaluation.bindings.calls.remove(&id);
                }
                Err(error) => {
                    if error.code == "OUTCOME_UNKNOWN" {
                        self.lose("CALLBACK_OUTCOME_UNKNOWN");
                        break;
                    }
                    if let Some(c) = self.evaluation.bindings.calls.get_mut(&id) {
                        c.responding = false;
                        c.retry = Instant::now() + Duration::from_secs(2);
                    }
                }
            }
        }
    }
}
