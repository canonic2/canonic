//! Document-fenced WebMCP discovery and retained asynchronous invocation outcomes.
use super::*;
use crate::browser::webmcp_debug::{Action, PageAction};

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Flavor {
    Debugging,
    Page,
}

impl Flavor {
    fn from_tool(tool: &Value) -> Self {
        if tool["annotations"]["debugging"] == true {
            Self::Debugging
        } else {
            Self::Page
        }
    }

    fn operation(self) -> &'static str {
        match self {
            Self::Debugging => "web_mcp_debug",
            Self::Page => "web_mcp",
        }
    }

    fn source(self) -> &'static str {
        match self {
            Self::Debugging => "native-WebMCP-debugging-annotation",
            Self::Page => "native-WebMCP-page-tools",
        }
    }

    fn issue_tool_ref(self, tool: &mut Value) {
        if self == Self::Page {
            tool["toolRef"] = json!(Uuid::new_v4());
        }
    }
}

enum Selector {
    Name(String),
    Registration(Uuid),
}

enum Command {
    Enable,
    Status,
    Invoke(Selector, Value),
    Read(Uuid),
    Cancel(Uuid),
    Disable,
}

struct Invocation {
    native: Option<String>,
    state: &'static str,
    result: Option<Value>,
    pending: Option<PendingAck>,
}

struct PendingAck {
    operation: Uuid,
    response: tokio::task::JoinHandle<NativeReply>,
}
struct Document {
    world: Reference,
    frame: String,
    session: String,
    ready: bool,
    enabling: bool,
    lost: bool,
    tools: BTreeMap<String, Value>,
    invocations: BTreeMap<Uuid, Invocation>,
    early: BTreeMap<String, Value>,
}
struct CachedTools {
    world: Reference,
    tools: BTreeMap<String, Value>,
}
#[derive(Default)]
pub(super) struct DebugTools {
    documents: BTreeMap<(Reference, Flavor), Document>,
    releases: BTreeSet<(String, String)>,
    objects_lost: BTreeSet<String>,
    registrations: BTreeMap<(Flavor, String, String), CachedTools>,
    cache_lost: BTreeSet<(Flavor, String)>,
}

fn settle(invocation: &mut Invocation, result: Value) {
    if invocation.result.is_some() {
        return;
    }
    invocation.state = match result["status"].as_str() {
        Some("Completed") => "completed",
        Some("Canceled") => "canceled",
        Some("ResultBounds") => "result_bounds",
        _ => "error",
    };
    invocation.result = Some(result);
}

fn invocation_error(mut error: Error, scope: Reference, invocation_id: Uuid) -> Error {
    error.partial = Some(
        json!({"scope":scope,"invocationId":invocation_id,"potentiallyMutating":true,"replayed":false,"cleanupPending":true}),
    );
    error
}

fn acknowledge(
    document: &mut Document,
    scope: Reference,
    invocation_id: Uuid,
    reply: Result<NativeReply, tokio::task::JoinError>,
) -> Result<Value, Error> {
    let invocation = document
        .invocations
        .get_mut(&invocation_id)
        .expect("Owned invocation");
    let result = reply
        .map_err(|_| Error::from("OUTCOME_UNKNOWN"))
        .and_then(|reply| reply.map_err(protocol_error))
        .and_then(|reply| {
            reply["invocationId"]
                .as_str()
                .filter(|id| !id.is_empty() && id.len() <= 256)
                .map(str::to_owned)
                .ok_or_else(|| Error::from("WEB_MCP_PROTOCOL"))
        });
    let mut early = std::mem::take(&mut document.early);
    match result {
        Ok(native) => {
            if invocation.state != "document_lost" {
                if invocation.state == "awaiting_ack" {
                    invocation.state = if document.lost {
                        "observation_lost"
                    } else {
                        "running"
                    };
                }
                if let Some(result) = early.remove(&native) {
                    settle(invocation, result);
                }
                invocation.native = Some(native);
            }
            Ok(
                json!({"scope":scope,"invocationId":invocation_id,"state":invocation.state,"potentiallyMutating":true,"dispatchCompleted":true}),
            )
        }
        Err(error) => {
            if invocation.state != "document_lost" {
                if matches!(error.code, "BROWSER_METHOD_FAILED" | "NOT_DISPATCHED") {
                    invocation.state = "rejected";
                    invocation.result = Some(json!({"status":"Rejected","code":error.code}));
                } else {
                    invocation.state = "acknowledgement_unknown";
                }
            }
            Err(invocation_error(error, scope, invocation_id))
        }
    }
}

impl DebugTools {
    pub(super) fn owns_pending(&self, operation: Uuid) -> bool {
        self.documents.values().any(|document| {
            document.invocations.values().any(|invocation| {
                invocation
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.operation == operation)
            })
        })
    }

    fn fence_lost_caches(&mut self, session: &str) {
        for flavor in [Flavor::Debugging, Flavor::Page] {
            if !self.cache_lost.contains(&(flavor, session.into())) {
                continue;
            }
            self.registrations
                .retain(|(kind, owner, _), _| *kind != flavor || owner != session);
            for (_, document) in self
                .documents
                .iter_mut()
                .filter(|((_, kind), d)| *kind == flavor && d.session == session)
            {
                document.lost = true;
                document.tools.clear();
            }
        }
    }

    pub(super) fn gap(&mut self) {
        self.registrations.clear();
        for document in self.documents.values_mut() {
            // Exception events can allocate inspector objects without a group.
            // After source loss only the actual session teardown proves release.
            self.objects_lost.insert(document.session.clone());
            document.lost = true;
            document.tools.clear();
            document.early.clear();
            for invocation in document.invocations.values_mut() {
                if invocation.result.is_none() {
                    invocation.state = "observation_lost";
                }
            }
        }
    }
}
impl Worker {
    async fn poll_webmcp_acks(&mut self) {
        for ((scope, _), document) in &mut self.webmcp_debug.documents {
            let ready: Vec<_> = document
                .invocations
                .iter()
                .filter_map(|(id, invocation)| {
                    invocation
                        .pending
                        .as_ref()
                        .filter(|pending| pending.response.is_finished())
                        .map(|_| *id)
                })
                .collect();
            for invocation_id in ready {
                let pending = document
                    .invocations
                    .get_mut(&invocation_id)
                    .unwrap()
                    .pending
                    .take()
                    .unwrap();
                let _ = acknowledge(document, *scope, invocation_id, pending.response.await);
            }
        }
    }

    pub(super) fn fence_webmcp_debug(&mut self) {
        self.webmcp_debug
            .registrations
            .retain(|_, cached| self.index.route(cached.world, Some(Kind::World)).is_ok());
        for document in self.webmcp_debug.documents.values_mut() {
            if self.index.route(document.world, Some(Kind::World)).is_err() {
                document.lost = true;
                document.tools.clear();
                document.early.clear();
                for invocation in document.invocations.values_mut() {
                    invocation.state = "document_lost";
                    invocation.result = None;
                }
            }
        }
    }
    fn cache_webmcp_tools(&mut self, event: &Event, session: &str) {
        if !matches!(
            event.method.as_str(),
            "WebMCP.toolsAdded" | "WebMCP.toolsRemoved"
        ) || !self
            .webmcp_debug
            .documents
            .values()
            .any(|document| document.session == session)
        {
            return;
        }
        for tool in event.params["tools"].as_array().into_iter().flatten() {
            let (Some(frame), Some(name)) = (tool["frameId"].as_str(), tool["name"].as_str())
            else {
                continue;
            };
            let flavor = Flavor::from_tool(tool);
            let added = event.method == "WebMCP.toolsAdded";
            for kind in [Flavor::Debugging, Flavor::Page] {
                if (!added || kind != flavor)
                    && let Some(cached) = self.webmcp_debug.registrations.get_mut(&(
                        kind,
                        session.into(),
                        frame.into(),
                    ))
                {
                    cached.tools.remove(name);
                }
            }
            let owner = (flavor, session.to_owned());
            if !added || self.webmcp_debug.cache_lost.contains(&owner) {
                continue;
            }
            let key = (flavor, session.to_owned(), frame.to_owned());
            let Some(world) =
                self.index
                    .list(Kind::Frame)
                    .into_iter()
                    .find_map(|(reference, _)| {
                        if self.index.native(reference, Kind::Frame).ok().as_deref() != Some(frame)
                        {
                            return None;
                        }
                        let (_, world) = self.index.execution_world(reference).ok()?;
                        (self.index.route(world, Some(Kind::World)).ok()?.session_id == session)
                            .then_some(world)
                    })
            else {
                continue;
            };
            let definition = json!({"name":name,"description":tool["description"],"inputSchema":tool["inputSchema"],"annotations":tool["annotations"]});
            if name.is_empty()
                || name.len() > 256
                || tool["description"]
                    .as_str()
                    .is_none_or(|text| text.len() > 4096)
                || crate::browser::schema::encoded_len(&tool["inputSchema"], 8192).is_err()
                || crate::browser::schema::encoded_len(&definition, 16384).is_err()
                || self
                    .webmcp_debug
                    .registrations
                    .keys()
                    .filter(|(kind, _, _)| *kind == flavor)
                    .count()
                    >= 64
                    && !self.webmcp_debug.registrations.contains_key(&key)
            {
                self.webmcp_debug.cache_lost.insert(owner);
                continue;
            }
            let cached = self
                .webmcp_debug
                .registrations
                .entry(key)
                .or_insert_with(|| CachedTools {
                    world,
                    tools: BTreeMap::new(),
                });
            if cached.world != world {
                cached.world = world;
                cached.tools.clear();
            }
            if cached.tools.len() >= 32 && !cached.tools.contains_key(name) {
                self.webmcp_debug.cache_lost.insert(owner);
                continue;
            }
            cached.tools.insert(name.into(), definition);
            let total = self
                .webmcp_debug
                .registrations
                .iter()
                .filter_map(|((kind, _, _), cached)| (*kind == flavor).then_some(cached))
                .flat_map(|cache| cache.tools.values())
                .map(|tool| crate::browser::schema::encoded_len(tool, 16384).unwrap_or(16385))
                .sum::<usize>();
            if total > 1048576 {
                self.webmcp_debug.cache_lost.insert(owner);
            }
        }
        self.webmcp_debug.fence_lost_caches(session);
    }
    #[expect(
        clippy::too_many_lines,
        reason = "One event reducer preserves document and invocation identity before retaining bounded metadata or output"
    )]
    pub(super) fn webmcp_debug_event(&mut self, event: &Event) {
        let Some(session) = event.session_id.as_deref() else {
            return;
        };
        self.cache_webmcp_tools(event, session);
        if event.method == "WebMCP.toolResponded"
            && self
                .webmcp_debug
                .documents
                .values()
                .any(|d| d.session == session)
            && let Some(object) = event.params["exception"]["objectId"]
                .as_str()
                .filter(|object| object.len() <= 1024)
        {
            if self.webmcp_debug.releases.len() < 128 {
                self.webmcp_debug
                    .releases
                    .insert((session.into(), object.into()));
            } else {
                self.webmcp_debug.gap();
            }
        }
        for ((_, flavor), document) in self
            .webmcp_debug
            .documents
            .iter_mut()
            .filter(|(_, d)| d.session == session)
        {
            if document.lost && event.method != "WebMCP.toolResponded" {
                continue;
            }
            match event.method.as_str() {
                "WebMCP.toolsAdded" => {
                    let Some(tools) = event.params["tools"].as_array() else {
                        continue;
                    };
                    for tool in tools {
                        if tool["frameId"] != document.frame {
                            continue;
                        }
                        let Some(name) = tool["name"]
                            .as_str()
                            .filter(|name| !name.is_empty() && name.len() <= 256)
                        else {
                            continue;
                        };
                        if Flavor::from_tool(tool) != *flavor {
                            document.tools.remove(name);
                            continue;
                        }
                        if document.tools.len() >= 32 && !document.tools.contains_key(name)
                            || tool["description"]
                                .as_str()
                                .is_none_or(|text| text.len() > 4096)
                            || crate::browser::schema::encoded_len(&tool["inputSchema"], 8192)
                                .is_err()
                        {
                            document.lost = true;
                            document.tools.clear();
                            break;
                        }
                        // Registration stacks and backend DOM IDs are not part of discovery.
                        let mut metadata = json!({"name":name,"description":tool["description"],"inputSchema":tool["inputSchema"],"annotations":tool["annotations"]});
                        flavor.issue_tool_ref(&mut metadata);
                        document.tools.insert(name.into(), metadata);
                    }
                }
                "WebMCP.toolsRemoved" => {
                    for tool in event.params["tools"].as_array().into_iter().flatten() {
                        if tool["frameId"] == document.frame
                            && let Some(name) = tool["name"].as_str()
                        {
                            document.tools.remove(name);
                        }
                    }
                }
                "WebMCP.toolResponded" => {
                    let Some(native) = event.params["invocationId"]
                        .as_str()
                        .filter(|id| id.len() <= 256)
                    else {
                        continue;
                    };
                    let status = event.params["status"].as_str().unwrap_or("Unknown");
                    let result = if document.lost {
                        json!({"status":status,"outputUnavailable":"observation_lost"})
                    } else {
                        json!({"status":status,"output":event.params["output"],"errorText":event.params["errorText"],"exception":event.params["exception"]["description"]})
                    };
                    let result = if crate::browser::schema::encoded_len(&result, 65536).is_err() {
                        json!({"status":"ResultBounds"})
                    } else {
                        result
                    };
                    if let Some(invocation) = document
                        .invocations
                        .values_mut()
                        .find(|i| i.native.as_deref() == Some(native))
                    {
                        settle(invocation, result);
                    } else if document
                        .invocations
                        .values()
                        .any(|i| i.native.is_none() && i.state == "awaiting_ack")
                    {
                        // Correlate only with the command's returned ID. Another
                        // client may invoke the same tool while our ACK is pending.
                        if document.early.len() < 16 {
                            document.early.insert(native.into(), result);
                        } else {
                            document.lost = true;
                            document.early.clear();
                            document.tools.clear();
                        }
                    }
                }
                _ => {}
            }
        }
    }
    pub(super) async fn release_webmcp_objects(&mut self) {
        let releases = std::mem::take(&mut self.webmcp_debug.releases);
        let deadline = Instant::now() + Duration::from_secs(2);
        for (session, object) in releases {
            if self.index.diagnostic_scope(&session).is_none() {
                continue;
            }
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
                self.webmcp_debug.releases.insert((session, object));
            }
        }
    }
    fn webmcp_status(&self, key: (Reference, Flavor)) -> Result<Value, Error> {
        let document = self
            .webmcp_debug
            .documents
            .get(&key)
            .ok_or("WEB_MCP_NOT_ENABLED")?;
        Ok(
            json!({"scope":key.0,"ready":document.ready,"observationLost":document.lost,"sessionTeardownRequired":self.webmcp_debug.objects_lost.contains(&document.session),"tools":document.tools.values().collect::<Vec<_>>(),"invocations":document.invocations.iter().map(|(id,i)|json!({"invocationId":id,"state":i.state})).collect::<Vec<_>>(),"source":key.1.source(),"untrustedApplicationContent":true}),
        )
    }
    pub(super) async fn webmcp_debug_operation(
        &mut self,
        id: Uuid,
        scope: Reference,
        action: Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        action.validate()?;
        let command = match action {
            Action::Enable {} => Command::Enable,
            Action::Status {} => Command::Status,
            Action::Invoke { name, parameters } => {
                Command::Invoke(Selector::Name(name), parameters)
            }
            Action::Read { invocation_id } => Command::Read(invocation_id),
            Action::Cancel { invocation_id } => Command::Cancel(invocation_id),
            Action::Disable {} => Command::Disable,
        };
        self.webmcp_operation(id, (scope, Flavor::Debugging), command, deadline)
            .await
    }

    pub(super) async fn webmcp_page_operation(
        &mut self,
        id: Uuid,
        scope: Reference,
        action: PageAction,
        deadline: Instant,
    ) -> Result<Value, Error> {
        action.validate()?;
        let command = match action {
            PageAction::Enable {} => Command::Enable,
            PageAction::Status {} => Command::Status,
            PageAction::Invoke {
                tool_ref,
                parameters,
            } => Command::Invoke(Selector::Registration(tool_ref), parameters),
            PageAction::Read { invocation_id } => Command::Read(invocation_id),
            PageAction::Cancel { invocation_id } => Command::Cancel(invocation_id),
            PageAction::Disable {} => Command::Disable,
        };
        self.webmcp_operation(id, (scope, Flavor::Page), command, deadline)
            .await
    }

    #[expect(
        clippy::too_many_lines,
        reason = "The typed lifecycle router retains subscription and invocation ownership across native acknowledgements"
    )]
    async fn webmcp_operation(
        &mut self,
        id: Uuid,
        key: (Reference, Flavor),
        command: Command,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let (scope, flavor) = key;
        self.observation_ready(id, deadline)?;
        self.drain()?;
        self.fence_webmcp_debug();
        self.poll_webmcp_acks().await;
        match command {
            Command::Enable => {
                if self.webmcp_debug.documents.contains_key(&key) {
                    return self.webmcp_status(key);
                }
                if self
                    .webmcp_debug
                    .documents
                    .keys()
                    .filter(|(_, kind)| *kind == flavor)
                    .count()
                    >= 4
                {
                    return Err("WEB_MCP_CAPACITY".into());
                }
                let (frame, world) = self.index.execution_world(scope).map_err(resource_error)?;
                if self
                    .webmcp_debug
                    .documents
                    .iter()
                    .any(|((_, kind), document)| *kind == flavor && document.world == world)
                {
                    return Err("WEB_MCP_SCOPE_CONFLICT".into());
                }
                let native = self
                    .index
                    .native(frame, Kind::Frame)
                    .map_err(resource_error)?;
                let session = self
                    .index
                    .route(world, Some(Kind::World))
                    .map_err(resource_error)?
                    .session_id;
                let mut subscribed = false;
                for document in self
                    .webmcp_debug
                    .documents
                    .values()
                    .filter(|d| d.session == session)
                {
                    if document.enabling {
                        return Err("WEB_MCP_SUBSCRIPTION_PENDING".into());
                    }
                    subscribed |= document.ready;
                }
                let mut tools = self
                    .webmcp_debug
                    .registrations
                    .get(&(flavor, session.clone(), native.clone()))
                    .filter(|cache| cache.world == world)
                    .map(|cache| cache.tools.clone())
                    .unwrap_or_default();
                for tool in tools.values_mut() {
                    flavor.issue_tool_ref(tool);
                }
                let lost = self
                    .webmcp_debug
                    .cache_lost
                    .contains(&(flavor, session.clone()));
                // Native subscription belongs to the session. Later views use
                // the document cache without replaying root registrations.
                self.webmcp_debug.documents.insert(
                    key,
                    Document {
                        world,
                        frame: native,
                        session: session.clone(),
                        ready: subscribed,
                        enabling: !subscribed,
                        lost,
                        tools,
                        invocations: BTreeMap::new(),
                        early: BTreeMap::new(),
                    },
                );
                if subscribed {
                    return self.webmcp_status(key);
                }
                let result = self
                    .call(id, Some(&session), "WebMCP.enable", json!({}), deadline)
                    .await;
                if let Some(document) = self.webmcp_debug.documents.get_mut(&key) {
                    document.enabling = result
                        .as_ref()
                        .err()
                        .is_some_and(|error| error.code == "OUTCOME_UNKNOWN");
                }
                if let Err(mut error) = result {
                    error.partial = Some(
                        json!({"scope":scope,"cleanupPending":true,"nextRequest":{"operation":flavor.operation(),"scope":scope,"action":{"kind":"disable"}}}),
                    );
                    return Err(error);
                }
                let document = self
                    .webmcp_debug
                    .documents
                    .get_mut(&key)
                    .ok_or("WEB_MCP_NOT_ENABLED")?;
                document.ready = true;
                self.webmcp_status(key)
            }
            Command::Status => self.webmcp_status(key),
            Command::Read(invocation_id) => {
                let document = self
                    .webmcp_debug
                    .documents
                    .get(&key)
                    .ok_or("WEB_MCP_NOT_ENABLED")?;
                let invocation = document
                    .invocations
                    .get(&invocation_id)
                    .ok_or("UNKNOWN_WEB_MCP_INVOCATION")?;
                Ok(
                    json!({"scope":scope,"invocationId":invocation_id,"state":invocation.state,"result":invocation.result,"potentiallyMutating":true,"untrustedApplicationContent":true}),
                )
            }
            Command::Invoke(selector, parameters) => {
                let document = self
                    .webmcp_debug
                    .documents
                    .get_mut(&key)
                    .ok_or("WEB_MCP_NOT_ENABLED")?;
                if document.lost || !document.ready {
                    return Err("WEB_MCP_OBSERVATION_UNAVAILABLE".into());
                }
                let name = match selector {
                    Selector::Name(name) if document.tools.contains_key(&name) => name,
                    Selector::Name(_) => return Err("UNKNOWN_WEB_MCP_DEBUG_TOOL".into()),
                    Selector::Registration(reference) => {
                        let reference = json!(reference);
                        document
                            .tools
                            .iter()
                            .find_map(|(name, tool)| {
                                (tool["toolRef"] == reference).then(|| name.clone())
                            })
                            .ok_or("WEB_MCP_TOOL_STALE")?
                    }
                };
                if document.invocations.len() >= 16 {
                    return Err("WEB_MCP_INVOCATION_CAPACITY".into());
                }
                let invocation_id = Uuid::new_v4();
                let (session, frame) = (document.session.clone(), document.frame.clone());
                if self.webmcp_debug.documents.values().any(|document| {
                    document.session == session
                        && document
                            .invocations
                            .values()
                            .any(|invocation| invocation.pending.is_some())
                }) {
                    return Err("WEB_MCP_ACK_PENDING".into());
                }
                self.journal.record(json!({"operationId":id,"stage":"dispatch_admitted","method":"WebMCP.invokeTool"})).await?;
                self.observation_ready(id, deadline)?;
                let connection = self.connection.clone().ok_or("BACKEND_LOST")?;
                let mut response = tokio::spawn(async move {
                    connection
                        .call(
                            Some(&session),
                            "WebMCP.invokeTool",
                            json!({"frameId":frame,"toolName":name,"input":parameters}),
                            Lane::Ordinary,
                            Instant::now() + Duration::from_secs(30),
                        )
                        .await
                });
                self.webmcp_debug
                    .documents
                    .get_mut(&key)
                    .expect("Owned document")
                    .invocations
                    .insert(
                        invocation_id,
                        Invocation {
                            native: None,
                            state: "awaiting_ack",
                            result: None,
                            pending: None,
                        },
                    );
                let reply = self
                    .wait_retained_reply(Some(id), &mut response, deadline)
                    .await;
                if reply.is_some() {
                    self.drain()?;
                }
                let document = self
                    .webmcp_debug
                    .documents
                    .get_mut(&key)
                    .expect("Owned document");
                if let Some(reply) = reply {
                    acknowledge(document, scope, invocation_id, reply)
                } else {
                    document
                        .invocations
                        .get_mut(&invocation_id)
                        .expect("Owned invocation")
                        .pending = Some(PendingAck {
                        operation: id,
                        response,
                    });
                    Err(invocation_error(
                        "OUTCOME_UNKNOWN".into(),
                        scope,
                        invocation_id,
                    ))
                }
            }
            Command::Cancel(invocation_id) => {
                let document = self
                    .webmcp_debug
                    .documents
                    .get(&key)
                    .ok_or("WEB_MCP_NOT_ENABLED")?;
                let invocation = document
                    .invocations
                    .get(&invocation_id)
                    .ok_or("UNKNOWN_WEB_MCP_INVOCATION")?;
                if invocation.result.is_some() {
                    return Ok(json!({"invocationId":invocation_id,"settled":true}));
                }
                let native = invocation
                    .native
                    .clone()
                    .ok_or("WEB_MCP_INVOCATION_OUTCOME_UNKNOWN")?;
                let session = document.session.clone();
                self.cleanup_call(
                    Some(&session),
                    "WebMCP.cancelInvocation",
                    json!({"invocationId":native}),
                    deadline,
                )
                .await?;
                Ok(
                    json!({"invocationId":invocation_id,"cancelRequested":true,"settled":self.webmcp_debug.documents[&key].invocations[&invocation_id].result.is_some()}),
                )
            }
            Command::Disable => {
                let Some(document) = self.webmcp_debug.documents.get(&key) else {
                    return Ok(json!({"scope":scope,"disabled":true}));
                };
                if document.enabling && self.index.route(document.world, Some(Kind::World)).is_ok()
                {
                    return Err("WEB_MCP_SUBSCRIPTION_PENDING".into());
                }
                if document.invocations.values().any(|i| {
                    i.result.is_none() && !matches!(i.state, "result_bounds" | "document_lost")
                }) {
                    return Err("WEB_MCP_INVOCATIONS_PENDING".into());
                }
                let session = document.session.clone();
                if self.webmcp_debug.objects_lost.contains(&session)
                    && self.index.diagnostic_scope(&session).is_some()
                {
                    let mut error = Error::from("WEB_MCP_SESSION_TEARDOWN_REQUIRED");
                    error.partial = Some(
                        json!({"scope":scope,"cleanupPending":true,"nextAction":"close owning page or connection"}),
                    );
                    return Err(error);
                }
                if !self
                    .webmcp_debug
                    .documents
                    .iter()
                    .any(|(other, d)| *other != key && d.session == session)
                    && self.index.diagnostic_scope(&session).is_some()
                {
                    self.cleanup_call(Some(&session), "WebMCP.disable", json!({}), deadline)
                        .await?;
                }
                self.webmcp_debug.documents.remove(&key);
                if !self
                    .webmcp_debug
                    .documents
                    .values()
                    .any(|document| document.session == session)
                {
                    self.webmcp_debug
                        .registrations
                        .retain(|(_, owner, _), _| *owner != session);
                    self.webmcp_debug
                        .cache_lost
                        .retain(|(_, owner)| *owner != session);
                    self.webmcp_debug.objects_lost.remove(&session);
                }
                Ok(json!({"scope":scope,"disabled":true}))
            }
        }
    }
}
