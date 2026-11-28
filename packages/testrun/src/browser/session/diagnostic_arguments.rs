//! Private driver console handles use conservative document fences and witnessed disposal.
use super::*;
use crate::browser::{
    diagnostic_arguments::{self as arguments, Group, Groups, Phase},
    objects::ObjectRef,
};

#[derive(Clone)]
struct Payload {
    values: Vec<ObjectRef>,
    fences: Vec<Reference>,
    owner: ObjectRef,
}
#[derive(Default)]
pub(super) struct Arguments {
    groups: Groups<Payload>,
}

impl Host {
    pub(super) fn diagnostic_argument_owned(&self, wire: &ObjectRef) -> bool {
        self.argument_groups
            .lock()
            .expect("Diagnostic argument owner")
            .groups
            .items
            .values()
            .any(|group| group.payload.values.contains(wire))
    }
    pub(super) fn fence_diagnostic_arguments(&self) {
        let state = self.state.lock().expect("Browser owner");
        let live = state
            .resources
            .values()
            .filter(|resource| resource.kind == Kind::Context)
            .map(|resource| resource.reference)
            .collect();
        let mut arguments = self
            .argument_groups
            .lock()
            .expect("Diagnostic argument owner");
        arguments.groups.retain_contexts(&live);
        for group in arguments.groups.items.values_mut() {
            if state.resource(group.context, Some(Kind::Context)).is_err()
                || group
                    .page
                    .is_some_and(|page| state.resource(page, Some(Kind::Page)).is_err())
                || group
                    .worker
                    .is_some_and(|worker| state.resource(worker, Some(Kind::Worker)).is_err())
                || group
                    .payload
                    .fences
                    .iter()
                    .any(|reference| state.resource(*reference, None).is_err())
            {
                group.phase = Phase::Expired;
            }
        }
    }
    pub(super) async fn capture_console_arguments(
        &self,
        context: Reference,
        page: Option<Reference>,
        worker: Option<Reference>,
        params: &Value,
    ) -> Value {
        if !self
            .argument_groups
            .lock()
            .expect("Diagnostic argument owner")
            .groups
            .enabled(context)
        {
            return Value::Null;
        }
        let capture=async {
            self.argument_groups.lock().expect("Diagnostic argument owner").groups.capacity()?;
            let (owner,fences)={
                let state=self.state.lock().expect("Browser owner");
                state.resource(context,Some(Kind::Context))?;
                if params["worker"]["guid"].is_string()&&worker.is_none() {return Err("ARGUMENT_WORLD_UNAVAILABLE");}
                let owner=worker.or(page).ok_or("ARGUMENT_WORLD_UNAVAILABLE")?;
                let resource=state.resource(owner,None)?;
                if state.ancestor(owner,Kind::Context)!=Some(context) {return Err("FOREIGN_REFERENCE");}
                let fences=if worker.is_some(){vec![owner]}else{
                    state.resources.values().filter(|resource|resource.kind==Kind::Frame&&state.ancestor(resource.reference,Kind::Page)==page).map(|resource|resource.reference).collect::<Vec<_>>()
                };
                if fences.is_empty(){return Err("ARGUMENT_WORLD_UNAVAILABLE");}
                (resource.wire.clone(),fences)
            };
            let source=params["args"].as_array().ok_or("ARGUMENTS_UNAVAILABLE")?;
            if source.is_empty(){return Ok(json!({"retained":false,"count":0}));}
            let connection=self.connection().map_err(|_|"BACKEND_LOST")?;
            let mut values=Vec::new();
            for value in source.iter().take(arguments::MAX_ARGUMENTS) {
                let guid=value["guid"].as_str().ok_or("BACKEND_PROTOCOL")?;
                let wire=match connection.resolve(guid,"JSHandle").await {
                    Ok(wire)=>wire,
                    Err(_)=>connection.resolve(guid,"ElementHandle").await.map_err(|_|"ARGUMENTS_UNAVAILABLE")?,
                };
                if !connection.owned_by(&wire,&owner).await.map_err(|_|"ARGUMENTS_UNAVAILABLE")? {return Err("FOREIGN_REFERENCE");}
                if self.state.lock().expect("Browser owner").by_wire(&wire).is_some() {return Err("PUBLIC_HANDLE_ALIAS");}
                let arguments=self.argument_groups.lock().expect("Diagnostic argument owner");
                if arguments.groups.items.values().any(|group|group.phase!=Phase::Live&&group.payload.values.contains(&wire)) {return Err("CONSOLE_CLEANUP_PENDING");}
                values.push(wire);
            }
            let count=values.len();let truncated=source.len()>count;
            let state=self.state.lock().expect("Browser owner");
            state.resource(context,Some(Kind::Context))?;
            for fence in &fences {state.resource(*fence,None)?;}
            let id=self.argument_groups.lock().expect("Diagnostic argument owner").groups.insert(Group {context,page,worker,count,truncated,phase:Phase::Live,payload:Payload{values,fences,owner}})?;
            Ok::<Value,&'static str>(json!({"groupId":id,"count":count,"totalCount":source.len(),"truncated":truncated,"retained":true,"frameIdentityUnavailable":worker.is_none()}))
        }.await;
        capture.unwrap_or_else(|reason| json!({"retained":false,"unavailable":reason}))
    }
    pub(super) async fn cleanup_diagnostic_arguments(
        &self,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.fence_diagnostic_arguments();
        let ids = self
            .argument_groups
            .lock()
            .expect("Diagnostic argument owner")
            .groups
            .items
            .iter()
            .filter(|(_, group)| group.phase != Phase::Live)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(());
        }
        let connection = self.connection()?;
        for id in ids {
            let payload = self
                .argument_groups
                .lock()
                .expect("Diagnostic argument owner")
                .groups
                .items
                .get(&id)
                .map(|group| group.payload.clone());
            let Some(payload) = payload else {
                continue;
            };
            let mut pending = false;
            for wire in payload.values {
                let borrowed = self
                    .state
                    .lock()
                    .expect("Browser owner")
                    .by_wire(&wire)
                    .is_some()
                    || self
                        .argument_groups
                        .lock()
                        .expect("Diagnostic argument owner")
                        .groups
                        .items
                        .iter()
                        .any(|(other, group)| {
                            *other != id
                                && group.phase == Phase::Live
                                && group.payload.values.contains(&wire)
                        });
                if borrowed {
                    continue;
                }
                match connection.initializer(&wire).await {
                    Ok(None)
                    | Err(crate::browser::connection::Error::Protocol(
                        crate::browser::dispatcher::Error::Object(
                            crate::browser::objects::Error::Stale
                            | crate::browser::objects::Error::Unknown,
                        ),
                    )) => continue,
                    Err(_) => {
                        pending = true;
                        continue;
                    }
                    Ok(Some(_)) => {}
                }
                if connection
                    .retained(&wire, "dispose", json!({}), deadline, true)
                    .await
                    .is_err()
                {
                    pending = true;
                }
            }
            if !pending {
                self.argument_groups
                    .lock()
                    .expect("Diagnostic argument owner")
                    .groups
                    .items
                    .remove(&id);
            }
        }
        Ok(())
    }
    pub(super) async fn prune_disposed_diagnostic_arguments(&self) {
        self.fence_diagnostic_arguments();
        let expired = self
            .argument_groups
            .lock()
            .expect("Diagnostic argument owner")
            .groups
            .items
            .iter()
            .filter(|(_, group)| group.phase != Phase::Live)
            .map(|(id, group)| (*id, group.payload.values.clone()))
            .collect::<Vec<_>>();
        if expired.is_empty() {
            return;
        }
        let Ok(connection) = self.connection() else {
            return;
        };
        for (id, values) in expired {
            let mut alive = false;
            for wire in values {
                match connection.initializer(&wire).await {
                    Ok(None)
                    | Err(crate::browser::connection::Error::Protocol(
                        crate::browser::dispatcher::Error::Object(
                            crate::browser::objects::Error::Stale
                            | crate::browser::objects::Error::Unknown,
                        ),
                    )) => {}
                    _ => {
                        alive = true;
                        break;
                    }
                }
            }
            if !alive {
                self.argument_groups
                    .lock()
                    .expect("Diagnostic argument owner")
                    .groups
                    .items
                    .remove(&id);
            }
        }
    }
    pub(super) async fn diagnostic_arguments_operation(
        &self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.connection()?.event_barrier(deadline).await?;
        self.admissible(id, deadline)?;
        self.fence_diagnostic_arguments();
        let context = match &request {
            Request::DiagnosticsArgumentsSet { context, .. }
            | Request::DiagnosticsArgumentsList { context }
            | Request::DiagnosticsArgumentsRead { context, .. }
            | Request::DiagnosticsArgumentsRemove { context, .. } => *context,
            _ => return Err("INVALID_SCOPE".into()),
        };
        self.state
            .lock()
            .expect("Browser owner")
            .resource(context, Some(Kind::Context))?;
        match request {
            Request::DiagnosticsArgumentsSet { enabled, .. } => {
                if enabled
                    && !self
                        .diagnostics
                        .lock()
                        .expect("Diagnostics")
                        .active(context)
                {
                    return Err("DIAGNOSTICS_INACTIVE".into());
                }
                self.argument_groups
                    .lock()
                    .expect("Diagnostic argument owner")
                    .groups
                    .configure(context, enabled);
                self.cleanup_diagnostic_arguments(
                    deadline.min(Instant::now() + Duration::from_secs(2)),
                )
                .await?;
                Ok(self
                    .argument_groups
                    .lock()
                    .expect("Diagnostic argument owner")
                    .groups
                    .list(context))
            }
            Request::DiagnosticsArgumentsList { .. } => Ok(self
                .argument_groups
                .lock()
                .expect("Diagnostic argument owner")
                .groups
                .list(context)),
            Request::DiagnosticsArgumentsRemove { group_id, .. } => {
                self.argument_groups
                    .lock()
                    .expect("Diagnostic argument owner")
                    .groups
                    .removing(context, group_id)?;
                self.cleanup_diagnostic_arguments(
                    deadline.min(Instant::now() + Duration::from_secs(2)),
                )
                .await?;
                if self
                    .argument_groups
                    .lock()
                    .expect("Diagnostic argument owner")
                    .groups
                    .items
                    .contains_key(&group_id)
                {
                    let mut error = Error::from("CLEANUP_PENDING");
                    error.partial =
                        Some(json!({"context":context,"groupId":group_id,"cleanupPending":true}));
                    return Err(error);
                }
                Ok(json!({"groupId":group_id,"removed":true}))
            }
            Request::DiagnosticsArgumentsRead {
                group_id, query, ..
            } => {
                self.read_diagnostic_argument(id, context, group_id, query, deadline)
                    .await
            }
            _ => Err("INVALID_SCOPE".into()),
        }
    }
    async fn read_diagnostic_argument(
        &self,
        id: Uuid,
        context: Reference,
        group_id: Uuid,
        query: arguments::Read,
        deadline: Instant,
    ) -> Result<Value, Error> {
        query.validate()?;
        let payload = self
            .argument_groups
            .lock()
            .expect("Diagnostic argument owner")
            .groups
            .get(context, group_id)?
            .payload
            .clone();
        let wire = payload
            .values
            .get(query.index)
            .ok_or("DIAGNOSTIC_ARGUMENT_INDEX")?;
        if !self.connection()?.owned_by(wire, &payload.owner).await? {
            return Err("DIAGNOSTIC_ARGUMENTS_EXPIRED".into());
        }
        let arg = crate::browser::evaluation::argument(
            &crate::browser::evaluation::EvaluationValue::Json {
                value: json!(query),
            },
            |_| Err("FOREIGN_HANDLE"),
        )?;
        let expression = format!(
            "(value,query)=>{{const codec={};return ({})(codec)(value,query);}}",
            arguments::CODEC,
            arguments::READER
        );
        self.admissible(id, deadline)?;
        let reply = self
            .connection()?
            .retained(
                wire,
                "evaluateExpression",
                json!({"expression":expression,"isFunction":true,"arg":arg}),
                deadline,
                false,
            )
            .await
            .map_err(Error::from);
        let dispatch_completed = reply.is_ok()
            || reply
                .as_ref()
                .err()
                .is_some_and(|error| error.code == "BROWSER_METHOD_FAILED");
        let result=async {
            let reply=reply?.ok_or("MISSING_RESULT")?;
            self.connection()?.event_barrier(deadline).await?;
            self.fence_diagnostic_arguments();
            self.argument_groups.lock().expect("Diagnostic argument owner").groups.get(context,group_id)?;
            if !self.connection()?.owned_by(wire,&payload.owner).await? {return Err(Error::from("DIAGNOSTIC_ARGUMENTS_EXPIRED"));}
            let encoded=crate::browser::values::result(&reply["value"])?;
            let value=crate::browser::evaluation::result(&encoded)?;
            let result=json!({"context":context,"groupId":group_id,"index":query.index,"value":value,"frame":null,"frameIdentityUnavailable":true,"lifetimeFence":"owned-page-frame-generations-or-worker","dispatchCompleted":true,"potentiallyMutating":true});
            crate::browser::schema::encoded_len(&result,65536).map_err(|_|Error::from("DIAGNOSTIC_ARGUMENT_RESULT_BOUNDS"))?;
            Ok(result)
        }.await;
        result.map_err(|mut error:Error|{error.partial=Some(json!({"context":context,"groupId":group_id,"dispatchCompleted":dispatch_completed,"potentiallyMutating":true,"replayed":false}));error})
    }
}
