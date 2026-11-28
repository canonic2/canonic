//! Native console objects stay in their actual worlds until explicit release.
use super::*;
use crate::browser::diagnostic_arguments::{self as arguments, Group, Groups, Phase};

#[derive(Clone)]
pub(super) struct Payload {
    session: String,
    world: Reference,
    scope: Reference,
    values: Vec<Value>,
}
#[derive(Default)]
pub(super) struct Arguments {
    groups: Groups<Payload>,
    releases: BTreeSet<(String, String)>,
    pub(super) releasing_groups: BTreeSet<String>,
    releasing_objects: BTreeSet<(String, String)>,
    pub(super) active_groups: BTreeSet<String>,
    active_objects: BTreeSet<(String, String)>,
}
impl Worker {
    fn diagnostic_argument_list(&self, context: Reference) -> Value {
        let mut value = self.argument_groups.groups.list(context);
        let reads = self
            .evaluation
            .groups
            .pending(context, super::execution_groups::Purpose::DiagnosticRead);
        value["pendingReadCleanup"] = json!(reads);
        value["maxPendingReadOwners"] = json!(32);
        value["cleanupPending"] = json!(value["cleanupPending"] == true || reads > 0);
        value
    }
    pub(super) fn argument_source_gap(&mut self) {
        self.argument_groups.releasing_groups.extend(
            self.index
                .targets()
                .into_iter()
                .filter_map(|(_, _, _, session)| session),
        );
        for group in self.argument_groups.groups.items.values_mut() {
            group.phase = Phase::Expired;
        }
    }
    pub(super) fn fence_diagnostic_arguments(&mut self) {
        let live = self
            .index
            .list(Kind::Context)
            .into_iter()
            .map(|(reference, _)| reference)
            .collect();
        self.argument_groups.groups.retain_contexts(&live);
        self.argument_groups
            .groups
            .items
            .retain(|_, group| self.index.session_attached(&group.payload.session));
        self.argument_groups
            .releasing_groups
            .retain(|session| self.index.session_attached(session));
        self.argument_groups
            .releases
            .retain(|(session, _)| self.index.session_attached(session));
        self.argument_groups
            .releasing_objects
            .retain(|(session, _)| self.index.session_attached(session));
        self.fence_execution_groups();
        for group in self.argument_groups.groups.items.values_mut() {
            if group.phase == Phase::Live
                && self
                    .index
                    .route(group.payload.world, Some(Kind::World))
                    .is_err()
            {
                group.phase = Phase::Expired;
                self.diagnostics
                    .discard
                    .insert(group.payload.session.clone());
                for value in &group.payload.values {
                    if let Some(object) = value["objectId"].as_str() {
                        self.argument_groups
                            .releases
                            .insert((group.payload.session.clone(), object.into()));
                    }
                }
            }
        }
    }
    pub(super) fn notice_argument_objects(&mut self, event: &Event) {
        let Some(session) = event.session_id.as_deref() else {
            return;
        };
        let objects: Vec<&Value> = match event.method.as_str() {
            "Runtime.consoleAPICalled" => event.params["args"]
                .as_array()
                .into_iter()
                .flatten()
                .collect(),
            "Runtime.exceptionThrown" => vec![&event.params["exceptionDetails"]["exception"]],
            _ => return,
        };
        for object in objects {
            let Some(object) = object["objectId"].as_str() else {
                continue;
            };
            if object.len() > 1024 || self.argument_groups.releases.len() >= 1024 {
                for group in self
                    .argument_groups
                    .groups
                    .items
                    .values_mut()
                    .filter(|group| group.payload.session == session)
                {
                    group.phase = Phase::Expired;
                }
                self.argument_groups
                    .releases
                    .retain(|(owner, _)| owner != session);
                self.argument_groups.releasing_groups.insert(session.into());
                self.diagnostics.discard.insert(session.into());
                return;
            }
            self.argument_groups
                .releases
                .insert((session.into(), object.into()));
        }
    }
    pub(super) fn capture_console_arguments(
        &mut self,
        context: Reference,
        page: Option<Reference>,
        worker: Option<Reference>,
        session: &str,
        params: &Value,
    ) -> Value {
        if !self.argument_groups.groups.enabled(context) {
            return Value::Null;
        }
        let capture = (|| -> Result<Value, &'static str> {
            if self.argument_groups.releasing_groups.contains(session) {
                return Err("CONSOLE_CLEANUP_PENDING");
            }
            self.argument_groups.groups.capacity()?;
            let numeric = params["executionContextId"]
                .as_i64()
                .ok_or("ARGUMENT_WORLD_UNAVAILABLE")?;
            let (scope, world) = self
                .index
                .diagnostic_world(session, numeric)
                .map_err(|_| "ARGUMENT_WORLD_UNAVAILABLE")?;
            let source = params["args"].as_array().ok_or("ARGUMENTS_UNAVAILABLE")?;
            if source.is_empty() {
                return Ok(json!({"retained":false,"count":0}));
            }
            let mut values = Vec::new();
            for value in source.iter().take(arguments::MAX_ARGUMENTS) {
                let call = if let Some(object) = value["objectId"].as_str() {
                    if self
                        .argument_groups
                        .releasing_objects
                        .contains(&(session.into(), object.into()))
                        || self
                            .argument_groups
                            .active_objects
                            .contains(&(session.into(), object.into()))
                    {
                        return Err("CONSOLE_CLEANUP_PENDING");
                    }
                    json!({"objectId":object})
                } else if let Some(value) = value.get("unserializableValue") {
                    json!({"unserializableValue":value})
                } else if let Some(value) = value.get("value") {
                    json!({"value":value})
                } else {
                    json!({})
                };
                crate::browser::schema::encoded_len(&call, 32768)
                    .map_err(|_| "DIAGNOSTIC_ARGUMENT_BOUNDS")?;
                values.push(call);
            }
            let count = values.len();
            let truncated = source.len() > count;
            let id = self.argument_groups.groups.insert(Group {
                context,
                page,
                worker,
                count,
                truncated,
                phase: Phase::Live,
                payload: Payload {
                    session: session.into(),
                    world,
                    scope,
                    values,
                },
            })?;
            Ok(
                json!({"groupId":id,"count":count,"totalCount":source.len(),"truncated":truncated,"retained":true}),
            )
        })();
        capture.unwrap_or_else(|reason| json!({"retained":false,"unavailable":reason}))
    }
    pub(super) fn argument_objects_retained(&self, session: &str) -> bool {
        self.argument_groups.groups.items.values().any(|group| {
            group.phase == Phase::Live
                && group.payload.session == session
                && group
                    .payload
                    .values
                    .iter()
                    .any(|value| value["objectId"].is_string())
        })
    }
    fn argument_object_owned(&self, session: &str, object: &str) -> bool {
        self.argument_groups.groups.items.values().any(|group| {
            group.phase == Phase::Live
                && group.payload.session == session
                && group
                    .payload
                    .values
                    .iter()
                    .any(|value| value["objectId"] == object)
        })
    }
    pub(super) async fn release_unretained_arguments(&mut self, deadline: Instant) {
        self.fence_diagnostic_arguments();
        self.release_execution_groups(deadline).await;
        let releases = std::mem::take(&mut self.argument_groups.releases);
        for (session, object) in releases {
            if !self.argument_objects_retained(&session)
                || self.argument_object_owned(&session, &object)
            {
                continue;
            }
            if !self
                .argument_groups
                .active_objects
                .insert((session.clone(), object.clone()))
            {
                self.argument_groups.releases.insert((session, object));
                continue;
            }
            let active = (session.clone(), object.clone());
            self.argument_groups
                .releasing_objects
                .insert((session.clone(), object.clone()));
            if self
                .cleanup_call(
                    Some(&session),
                    "Runtime.releaseObject",
                    json!({"objectId":object}),
                    deadline,
                )
                .await
                .is_err()
            {
                self.argument_groups.releases.insert((session, object));
            } else {
                self.argument_groups
                    .releasing_objects
                    .remove(&(session, object));
            }
            self.argument_groups.active_objects.remove(&active);
        }
        let retired = self
            .argument_groups
            .groups
            .items
            .iter()
            .filter(|(_, group)| {
                group.phase != Phase::Live
                    && self.argument_objects_retained(&group.payload.session)
                    && group.payload.values.iter().all(|value| {
                        value["objectId"].as_str().is_none_or(|object| {
                            self.argument_object_owned(&group.payload.session, object)
                                || !self
                                    .argument_groups
                                    .releases
                                    .contains(&(group.payload.session.clone(), object.into()))
                        })
                    })
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in retired {
            self.argument_groups.groups.items.remove(&id);
        }
    }
    pub(super) fn argument_console_group_released(&mut self, session: &str) {
        self.argument_groups
            .groups
            .items
            .retain(|_, group| group.payload.session != session || group.phase == Phase::Live);
        self.argument_groups
            .releases
            .retain(|(owner, _)| owner != session);
        self.argument_groups
            .releasing_objects
            .retain(|(owner, _)| owner != session);
        self.argument_groups.releasing_groups.remove(session);
    }
    pub(super) async fn diagnostic_arguments_operation(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.observation_ready(id, deadline)?;
        self.drain()?;
        self.fence_diagnostic_arguments();
        let context = match &request {
            Request::DiagnosticsArgumentsSet { context, .. }
            | Request::DiagnosticsArgumentsList { context }
            | Request::DiagnosticsArgumentsRead { context, .. }
            | Request::DiagnosticsArgumentsRemove { context, .. } => *context,
            _ => return Err("INVALID_SCOPE".into()),
        };
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        match request {
            Request::DiagnosticsArgumentsSet { enabled, .. } => {
                if enabled && !self.diagnostics.history.active(context) {
                    return Err("DIAGNOSTICS_INACTIVE".into());
                }
                self.argument_groups.groups.configure(context, enabled);
                for group in self
                    .argument_groups
                    .groups
                    .items
                    .values()
                    .filter(|group| group.context == context && group.phase != Phase::Live)
                {
                    self.diagnostics
                        .discard
                        .insert(group.payload.session.clone());
                }
                self.discard_diagnostic_objects().await;
                Ok(self.diagnostic_argument_list(context))
            }
            Request::DiagnosticsArgumentsList { .. } => Ok(self.diagnostic_argument_list(context)),
            Request::DiagnosticsArgumentsRemove { group_id, .. } => {
                self.argument_groups.groups.removing(context, group_id)?;
                let payload = self.argument_groups.groups.items[&group_id].payload.clone();
                for value in payload.values {
                    if let Some(object) = value["objectId"].as_str() {
                        self.argument_groups
                            .releases
                            .insert((payload.session.clone(), object.into()));
                    }
                }
                self.diagnostics.discard.insert(payload.session);
                self.discard_diagnostic_objects().await;
                if self.argument_groups.groups.items.contains_key(&group_id) {
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
        &mut self,
        id: Uuid,
        context: Reference,
        group_id: Uuid,
        query: arguments::Read,
        deadline: Instant,
    ) -> Result<Value, Error> {
        query.validate()?;
        let payload = self
            .argument_groups
            .groups
            .get(context, group_id)?
            .payload
            .clone();
        let value = payload
            .values
            .get(query.index)
            .ok_or("DIAGNOSTIC_ARGUMENT_INDEX")?
            .clone();
        let route = self
            .index
            .route(payload.world, Some(Kind::World))
            .map_err(resource_error)?;
        let expression = format!(
            "function(query,value){{const codec={};return ({})(codec)(value,query);}}",
            arguments::CODEC,
            arguments::READER
        );
        let unique = route
            .execution_unique_id
            .ok_or("ARGUMENT_WORLD_UNAVAILABLE")?;
        let read_group = self.evaluation.groups.begin(
            context,
            payload.session.clone(),
            super::execution_groups::Purpose::DiagnosticRead,
        )?;
        let reply=self.execution_group_call(id,&read_group,"Runtime.callFunctionOn",json!({"functionDeclaration":expression,"uniqueContextId":unique,"arguments":[{"value":query},value],"returnByValue":true,"awaitPromise":false,"objectGroup":read_group.1}),deadline).await;
        let dispatch_completed = reply.is_ok()
            || reply
                .as_ref()
                .err()
                .is_some_and(|error| error.code == "BROWSER_METHOD_FAILED");
        self.release_unretained_arguments(deadline.min(Instant::now() + Duration::from_secs(2)))
            .await;
        let result = (|| {
            let reply = reply?;
            self.fence_diagnostic_arguments();
            self.argument_groups.groups.get(context, group_id)?;
            if reply.get("exceptionDetails").is_some() {
                return Err(Error::from("DIAGNOSTIC_ARGUMENT_READ_FAILED"));
            }
            let value = crate::browser::evaluation::result(&reply["result"]["value"])?;
            let result = json!({"context":context,"groupId":group_id,"index":query.index,"scope":payload.scope,"value":value,"dispatchCompleted":true,"potentiallyMutating":true});
            crate::browser::schema::encoded_len(&result, 65536)
                .map_err(|_| Error::from("DIAGNOSTIC_ARGUMENT_RESULT_BOUNDS"))?;
            Ok(result)
        })();
        result.map_err(|mut error:Error|{error.partial=Some(json!({"context":context,"groupId":group_id,"dispatchCompleted":dispatch_completed,"potentiallyMutating":true,"replayed":false}));error})
    }
}
