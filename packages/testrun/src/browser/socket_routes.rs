//! Managed WebSocket helpers composed from owned bindings and init scripts.
use super::{
    contract::{Reference, Request},
    session::Error,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::time::Instant;
use uuid::Uuid;
mod automaton;
mod matcher;
pub use matcher::Matcher;
pub(crate) use matcher::websocket_base;

/// Ordered socket-only batches can release a held binding without borrowing
/// response capacity or admitting unrelated work ahead of ordinary input.
pub(crate) fn independent_batch(request: &Request) -> bool {
    matches!(request, Request::Batch { requests }
        if !requests.is_empty() && requests.iter().all(|request| matches!(request,
            Request::WebSocketRouteSend { .. }
                | Request::WebSocketRouteHandlers { .. }
                | Request::WebSocketRouteClose { .. })))
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Destination {
    Page,
    Server,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Message {
    pub data: String,
    #[serde(default)]
    pub is_base64: bool,
}
#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HandlerEvent {
    PageMessage,
    ServerMessage,
    PageClose,
    ServerClose,
}
/// Caller-owned handler identities; null removes only the selected handler.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Handlers(BTreeMap<HandlerEvent, Option<Uuid>>);
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloseOptions {
    #[serde(
        default,
        deserialize_with = "present_close_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub code: Option<u16>,
    #[serde(
        default,
        deserialize_with = "present_close_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub reason: Option<String>,
}
fn present_close_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
pub(crate) fn validate_close(
    socket: &str,
    to: Destination,
    options: &CloseOptions,
) -> Result<(), &'static str> {
    validate_socket(socket)?;
    if options
        .reason
        .as_ref()
        .is_some_and(|reason| reason.len() > 123)
    {
        return Err("WEBSOCKET_CLOSE_INVALID");
    }
    if let Some(code) = options.code {
        let valid = code == 1000
            || (3000..=4999).contains(&code)
            || matches!(to, Destination::Page)
                && (1001..=1014).contains(&code)
                && ![1004, 1005, 1006].contains(&code);
        if !valid {
            return Err("WEBSOCKET_CLOSE_INVALID");
        }
    }
    Ok(())
}
impl Handlers {
    fn schema() -> Value {
        let listener = json!({"anyOf":[{"type":"string","format":"uuid"},{"type":"null"}]});
        json!({"type":"object","minProperties":1,"maxProperties":4,"properties":{"pageMessage":listener,"serverMessage":listener,"pageClose":listener,"serverClose":listener},"additionalProperties":false})
    }
}
fn validate_socket(socket: &str) -> Result<(), &'static str> {
    if socket.len() != 64 || !socket.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("WEBSOCKET_SOCKET_INVALID");
    }
    Ok(())
}
pub(crate) fn validate_handlers(socket: &str, handlers: &Handlers) -> Result<(), &'static str> {
    validate_socket(socket)?;
    if handlers.0.is_empty() {
        return Err("WEBSOCKET_HANDLERS_INVALID");
    }
    Ok(())
}
/// New routes forward like the pinned SDK unless the caller selects command mode.
pub(crate) const fn sdk_defaults() -> bool {
    true
}
/// Decoded bytes of one routed message.
pub(crate) const MAX_MESSAGE_BYTES: usize = 512 * 1024;
/// JSON-encoded text a routed message may occupy, so a message and its command
/// wrapper stay within the shared 1 MiB evaluation and driver limits.
const MAX_ENCODED_MESSAGE_BYTES: usize = 768 * 1024;
pub(crate) fn validate_send(socket: &str, message: &Message) -> Result<(), &'static str> {
    validate_socket(socket)?;
    if message.is_base64 {
        if message.data.len() > MAX_MESSAGE_BYTES.div_ceil(3) * 4 {
            return Err("WEBSOCKET_MESSAGE_BOUNDS");
        }
        let bytes = STANDARD
            .decode(&message.data)
            .map_err(|_| "WEBSOCKET_MESSAGE_INVALID")?;
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err("WEBSOCKET_MESSAGE_BOUNDS");
        }
    } else if message.data.len() > MAX_MESSAGE_BYTES
        || serde_json::to_string(&message.data).map_or(usize::MAX, |encoded| encoded.len())
            > MAX_ENCODED_MESSAGE_BYTES
    {
        return Err("WEBSOCKET_MESSAGE_BOUNDS");
    }
    Ok(())
}

const MOCK: &str = include_str!("reading/websocket.js");
const GUARD: &str = include_str!("reading/websocket-guard.js");
const ROUTING: &str = include_str!("reading/websocket-routing.js");
const REGEX: &str = include_str!("reading/websocket-regex.js");
#[derive(Clone)]
struct Route {
    scope: Reference,
    page: bool,
    order: u32,
    sdk_defaults: bool,
    binding: Option<Uuid>,
    script: Option<Uuid>,
    ready: bool,
    removing: bool,
}
#[derive(Default)]
pub(crate) struct Catalog {
    items: BTreeMap<Uuid, Route>,
    next_order: u32,
}
pub(crate) type Shared = Arc<Mutex<Catalog>>;
pub(crate) trait Backend: Send {
    fn scope_is_page(&self, scope: Reference) -> Result<bool, Error>;
    fn binding(
        &mut self,
        request: Request,
        deadline: Instant,
    ) -> impl std::future::Future<Output = Result<Value, Error>> + Send;
    fn script(
        &mut self,
        request: Request,
        deadline: Instant,
    ) -> impl std::future::Future<Output = Result<Value, Error>> + Send;
    fn documents(
        &mut self,
        scope: Reference,
        source: String,
        deadline: Instant,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send;
    fn send(
        &mut self,
        scope: Reference,
        frame: Reference,
        source: String,
        deadline: Instant,
    ) -> futures_util::future::BoxFuture<'_, Result<Value, Error>>;
}
pub(crate) fn validate_url(url: &str) -> Result<(), &'static str> {
    let http = if let Some(rest) = url.strip_prefix("wss://") {
        format!("https://{rest}")
    } else if let Some(rest) = url.strip_prefix("ws://") {
        format!("http://{rest}")
    } else {
        return Err("WEBSOCKET_URL_INVALID");
    };
    super::routing::validate_url(&http).map_err(|_| "WEBSOCKET_URL_INVALID")
}
fn key(scope: Reference) -> String {
    format!("canonic:websocket:{}", scope.connection_id)
}
fn source(
    scope: Reference,
    id: Uuid,
    pattern: &Value,
    page: bool,
    order: u32,
    sdk_defaults: bool,
) -> String {
    let binding = format!("canonicSocket_{}", id.simple());
    format!(
        "(()=>{{const key=Symbol.for({key});let owner=globalThis[key];if(!owner){{if(globalThis.__pwWebSocketDispatch)throw Error('WEBSOCKET_HELPER_CONFLICT');const install=emit=>{{const previous=globalThis.__pwWebSocketBinding;globalThis.__pwWebSocketBinding=emit;try{{const module={{exports:{{}}}};{MOCK}\nmodule.exports.inject()(globalThis);}}finally{{if(previous===undefined)delete globalThis.__pwWebSocketBinding;else globalThis.__pwWebSocketBinding=previous;}}return {{Mock:globalThis.WebSocket,dispatch:globalThis.__pwWebSocketDispatch}};}};owner=({ROUTING})(install,{GUARD},{REGEX});Object.defineProperty(globalThis,key,{{value:owner,configurable:true}});globalThis.WebSocket=owner.WebSocket;}}owner.add({id},{url},globalThis[{binding}],{page},{order},{sdk_defaults});return true;}})()",
        key = json!(key(scope)),
        id = json!(id),
        url = pattern,
        binding = json!(binding)
    )
}
fn close_source(scope: Reference, id: Uuid) -> String {
    format!(
        "(()=>{{const owner=globalThis[Symbol.for({})];if(owner)owner.remove({});return true;}})()",
        json!(key(scope)),
        json!(id)
    )
}
fn partial_id(result: &Result<Value, Error>, field: &str) -> Option<Uuid> {
    let value = match result {
        Ok(value) => value,
        Err(error) => error.partial.as_ref()?,
    };
    serde_json::from_value(value[field].clone()).ok()
}
fn script_add(scope: Reference, page: bool, source: String) -> Request {
    if page {
        Request::PageInitScriptAdd {
            page: scope,
            source,
        }
    } else {
        Request::ContextInitScriptAdd {
            context: scope,
            source,
        }
    }
}
fn script_remove(route: &Route, id: Uuid) -> Request {
    if route.page {
        Request::PageInitScriptRemove {
            page: route.scope,
            script_id: id,
        }
    } else {
        Request::ContextInitScriptRemove {
            context: route.scope,
            script_id: id,
        }
    }
}
impl Catalog {
    fn snapshot(&self, scope: Reference) -> Value {
        let mut routes = self
            .items
            .iter()
            .filter(|(_, route)| route.scope == scope)
            .collect::<Vec<_>>();
        routes.sort_unstable_by_key(|(_, route)| std::cmp::Reverse(route.order));
        json!({"scope":scope,"routes":routes.into_iter().map(|(id,r)|json!({"routeId":id,"bindingId":r.binding,"scriptId":r.script,"sdkDefaults":r.sdk_defaults,"state":if r.removing{"removing"}else if r.ready{"ready"}else{"setup_pending"}})).collect::<Vec<_>>(),"order":"newest-first","payloadsIncluded":false})
    }
}
#[expect(
    clippy::too_many_lines,
    reason = "one match over every socket-route request"
)]
pub(crate) async fn execute(
    backend: &mut impl Backend,
    catalog: Shared,
    request: Request,
    deadline: Instant,
) -> Result<Value, Error> {
    let scope = match &request {
        Request::WebSocketRouteAdd { scope, .. }
        | Request::WebSocketRouteAddMatcher { scope, .. }
        | Request::WebSocketRouteList { scope }
        | Request::WebSocketRouteSend { scope, .. }
        | Request::WebSocketRouteHandlers { scope, .. }
        | Request::WebSocketRouteClose { scope, .. }
        | Request::WebSocketRouteRemove { scope, .. } => *scope,
        _ => return Err("INVALID_SCOPE".into()),
    };
    let page = backend.scope_is_page(scope)?;
    catalog
        .lock()
        .expect("Socket routes")
        .items
        .retain(|_, r| backend.scope_is_page(r.scope).is_ok());
    if matches!(request, Request::WebSocketRouteList { .. }) {
        return Ok(catalog.lock().expect("Socket routes").snapshot(scope));
    }
    if matches!(
        request,
        Request::WebSocketRouteSend { .. }
            | Request::WebSocketRouteHandlers { .. }
            | Request::WebSocketRouteClose { .. }
    ) {
        return command(backend, catalog, request, deadline).await;
    }
    let id = match request {
        Request::WebSocketRouteAdd { .. } | Request::WebSocketRouteAddMatcher { .. } => {
            let (matcher, sdk_defaults) = match request {
                Request::WebSocketRouteAdd {
                    url, sdk_defaults, ..
                } => (Matcher::Exact { url }, sdk_defaults),
                Request::WebSocketRouteAddMatcher {
                    matcher,
                    sdk_defaults,
                    ..
                } => (matcher, sdk_defaults),
                _ => unreachable!(),
            };
            let pattern = matcher.prepare()?;
            let id = Uuid::new_v4();
            {
                let mut catalog = catalog.lock().expect("Socket routes");
                if catalog.items.len() >= 32 {
                    return Err("WEBSOCKET_ROUTE_CAPACITY".into());
                }
                if catalog
                    .items
                    .values()
                    .any(|r| r.scope == scope && (!r.ready || r.removing))
                {
                    return Err("WEBSOCKET_ROUTE_NOT_READY".into());
                }
                let order = catalog
                    .next_order
                    .checked_add(1)
                    .ok_or("WEBSOCKET_ROUTE_CAPACITY")?;
                catalog.next_order = order;
                catalog.items.insert(
                    id,
                    Route {
                        scope,
                        page,
                        order,
                        sdk_defaults,
                        binding: None,
                        script: None,
                        ready: false,
                        removing: false,
                    },
                );
            }
            let result = add(backend, catalog.clone(), scope, id, pattern, page, deadline).await;
            return result.map_err(|mut error| {
                let detail = error.partial.take();
                error.partial =
                    Some(json!({"scope":scope,"routeId":id,"cleanupPending":true,"detail":detail}));
                error
            });
        }
        Request::WebSocketRouteRemove { route_id, .. } => route_id,
        _ => return Err("INVALID_SCOPE".into()),
    };
    let route = {
        let mut catalog = catalog.lock().expect("Socket routes");
        let route = catalog
            .items
            .get_mut(&id)
            .filter(|r| r.scope == scope)
            .ok_or("UNKNOWN_WEBSOCKET_ROUTE")?;
        route.removing = true;
        route.clone()
    };
    if let Some(script) = route.script {
        if let Err(error) = backend
            .script(script_remove(&route, script), deadline)
            .await
            && error.code != "UNKNOWN_INIT_SCRIPT"
        {
            return Err(error);
        }
        catalog
            .lock()
            .expect("Socket routes")
            .items
            .get_mut(&id)
            .ok_or("UNKNOWN_WEBSOCKET_ROUTE")?
            .script = None;
    }
    backend
        .documents(scope, close_source(scope, id), deadline)
        .await?;
    if let Some(binding_id) = route.binding
        && let Err(error) = backend
            .binding(Request::BindingRemove { scope, binding_id }, deadline)
            .await
        && error.code != "UNKNOWN_BINDING"
    {
        return Err(error);
    }
    catalog.lock().expect("Socket routes").items.remove(&id);
    Ok(json!({"scope":scope,"routeId":id,"removed":true}))
}
pub(crate) async fn command(
    backend: &mut impl Backend,
    catalog: Shared,
    request: Request,
    deadline: Instant,
) -> Result<Value, Error> {
    match request {
        request @ Request::WebSocketRouteHandlers { .. } => {
            handlers(backend, catalog, request, deadline).await
        }
        request @ Request::WebSocketRouteClose { .. } => {
            close_socket(backend, catalog, request, deadline).await
        }
        request => send(backend, catalog, request, deadline).await,
    }
}
async fn close_socket(
    backend: &mut impl Backend,
    catalog: Shared,
    request: Request,
    deadline: Instant,
) -> Result<Value, Error> {
    let Request::WebSocketRouteClose {
        scope,
        frame,
        route_id,
        socket_id,
        to,
        options,
    } = request
    else {
        return Err("INVALID_SCOPE".into());
    };
    backend.scope_is_page(scope)?;
    validate_close(&socket_id, to, &options)?;
    {
        let catalog = catalog.lock().expect("Socket routes");
        let route = catalog
            .items
            .get(&route_id)
            .filter(|route| route.scope == scope)
            .ok_or("UNKNOWN_WEBSOCKET_ROUTE")?;
        if !route.ready || route.removing {
            return Err("WEBSOCKET_ROUTE_NOT_READY".into());
        }
    }
    let source = format!(
        "(()=>{{const owner=globalThis[Symbol.for({})];return owner?.closeSocket?owner.closeSocket({},{},{},{}):'WEBSOCKET_SOCKET_EXPIRED';}})()",
        json!(key(scope)),
        json!(route_id),
        json!(socket_id),
        json!(to),
        json!(options)
    );
    let result = backend.send(scope, frame, source, deadline).await?;
    if result["value"]["value"] != "close-requested" {
        let code = if result["value"]["value"] == "WEBSOCKET_SOCKET_EXPIRED" {
            "WEBSOCKET_SOCKET_EXPIRED"
        } else {
            "WEBSOCKET_CLOSE_PROTOCOL"
        };
        let mut error = Error::from(code);
        error.partial = Some(
            json!({"scope":scope,"frame":frame,"dispatchCompleted":true,"potentiallyMutating":true,"replayed":false}),
        );
        return Err(error);
    }
    Ok(
        json!({"scope":scope,"frame":frame,"routeId":route_id,"socketId":socket_id,"to":to,"closeRequested":true,"dispatchCompleted":true,"potentiallyMutating":true,"replayed":false}),
    )
}
async fn handlers(
    backend: &mut impl Backend,
    catalog: Shared,
    request: Request,
    deadline: Instant,
) -> Result<Value, Error> {
    let Request::WebSocketRouteHandlers {
        scope,
        frame,
        route_id,
        socket_id,
        handlers,
    } = request
    else {
        return Err("INVALID_SCOPE".into());
    };
    backend.scope_is_page(scope)?;
    validate_handlers(&socket_id, &handlers)?;
    {
        let catalog = catalog.lock().expect("Socket routes");
        let route = catalog
            .items
            .get(&route_id)
            .filter(|route| route.scope == scope)
            .ok_or("UNKNOWN_WEBSOCKET_ROUTE")?;
        if !route.ready || route.removing {
            return Err("WEBSOCKET_ROUTE_NOT_READY".into());
        }
        if !route.sdk_defaults {
            return Err("WEBSOCKET_SDK_MODE_REQUIRED".into());
        }
    }
    let source = format!(
        "(()=>{{const owner=globalThis[Symbol.for({})];return owner?.handlers?owner.handlers({},{},{}):'WEBSOCKET_SOCKET_EXPIRED';}})()",
        json!(key(scope)),
        json!(route_id),
        json!(socket_id),
        json!(handlers)
    );
    let result = backend.send(scope, frame, source, deadline).await?;
    if result["value"]["value"] != "configured" {
        let code = match result["value"]["value"].as_str() {
            Some("WEBSOCKET_SOCKET_EXPIRED") => "WEBSOCKET_SOCKET_EXPIRED",
            Some("WEBSOCKET_SDK_MODE_REQUIRED") => "WEBSOCKET_SDK_MODE_REQUIRED",
            _ => "WEBSOCKET_HANDLER_PROTOCOL",
        };
        let mut error = Error::from(code);
        error.partial = Some(
            json!({"scope":scope,"frame":frame,"dispatchCompleted":true,"potentiallyMutating":true,"replayed":false}),
        );
        return Err(error);
    }
    Ok(
        json!({"scope":scope,"frame":frame,"routeId":route_id,"socketId":socket_id,"configured":true,"handlerChanges":handlers.0.len(),"dispatchCompleted":true,"potentiallyMutating":true,"replayed":false}),
    )
}
async fn send(
    backend: &mut impl Backend,
    catalog: Shared,
    request: Request,
    deadline: Instant,
) -> Result<Value, Error> {
    let Request::WebSocketRouteSend {
        scope,
        frame,
        route_id,
        socket_id,
        to,
        message,
    } = request
    else {
        return Err("INVALID_SCOPE".into());
    };
    backend.scope_is_page(scope)?;
    validate_send(&socket_id, &message)?;
    {
        let catalog = catalog.lock().expect("Socket routes");
        let route = catalog
            .items
            .get(&route_id)
            .filter(|route| route.scope == scope)
            .ok_or("UNKNOWN_WEBSOCKET_ROUTE")?;
        if !route.ready || route.removing {
            return Err("WEBSOCKET_ROUTE_NOT_READY".into());
        }
    }
    let source = format!(
        "(()=>{{const owner=globalThis[Symbol.for({})];return owner?.send?owner.send({},{},{},{}):'WEBSOCKET_SOCKET_EXPIRED';}})()",
        json!(key(scope)),
        json!(route_id),
        json!(socket_id),
        json!(to),
        json!(message)
    );
    let value = backend.send(scope, frame, source, deadline).await?;
    if value["value"]["value"] != "sent" {
        let code = match value["value"]["value"].as_str() {
            Some("WEBSOCKET_SOCKET_EXPIRED") => "WEBSOCKET_SOCKET_EXPIRED",
            Some("WEBSOCKET_SOCKET_CLOSED") => "WEBSOCKET_SOCKET_CLOSED",
            Some("WEBSOCKET_SERVER_NOT_CONNECTED") => "WEBSOCKET_SERVER_NOT_CONNECTED",
            _ => "WEBSOCKET_SEND_PROTOCOL",
        };
        let mut error = Error::from(code);
        error.partial = Some(
            json!({"scope":scope,"frame":frame,"dispatchCompleted":true,"potentiallyMutating":true}),
        );
        return Err(error);
    }
    Ok(
        json!({"scope":scope,"frame":frame,"routeId":route_id,"socketId":socket_id,"to":to,"dispatchCompleted":true,"potentiallyMutating":true,"payloadIncluded":false,"replayed":false}),
    )
}
async fn add(
    backend: &mut impl Backend,
    catalog: Shared,
    scope: Reference,
    id: Uuid,
    url: Arc<Value>,
    page: bool,
    deadline: Instant,
) -> Result<Value, Error> {
    let binding = backend
        .binding(
            Request::BindingAdd {
                scope,
                name: format!("canonicSocket_{}", id.simple()),
            },
            deadline,
        )
        .await;
    let binding_id = partial_id(&binding, "bindingId");
    catalog
        .lock()
        .expect("Socket routes")
        .items
        .get_mut(&id)
        .ok_or("UNKNOWN_WEBSOCKET_ROUTE")?
        .binding = binding_id;
    binding?;
    let (order, sdk_defaults) = {
        let catalog = catalog.lock().expect("Socket routes");
        let route = catalog.items.get(&id).ok_or("UNKNOWN_WEBSOCKET_ROUTE")?;
        (route.order, route.sdk_defaults)
    };
    let source = source(scope, id, url.as_ref(), page, order, sdk_defaults);
    let script = backend
        .script(script_add(scope, page, source.clone()), deadline)
        .await;
    catalog
        .lock()
        .expect("Socket routes")
        .items
        .get_mut(&id)
        .ok_or("UNKNOWN_WEBSOCKET_ROUTE")?
        .script = partial_id(&script, "scriptId");
    script?;
    backend.documents(scope, source, deadline).await?;
    catalog
        .lock()
        .expect("Socket routes")
        .items
        .get_mut(&id)
        .ok_or("UNKNOWN_WEBSOCKET_ROUTE")?
        .ready = true;
    Ok(
        json!({"scope":scope,"routeId":id,"bindingId":binding_id,"registered":true,"callbackKind":"binding","maxMessageBytes":MAX_MESSAGE_BYTES,"newSocketsOnly":true,"sdkDefaults":sdk_defaults}),
    )
}
// Append endpoint controls at the contract's extension point so existing public
// schema branch coordinates retain their inventory and acceptance identities.
pub(crate) fn control_schemas(reference: &Value) -> Vec<Value> {
    vec![
        json!({"type":"object","properties":{"operation":{"const":"web_socket_route_close"},"scope":reference,"frame":reference,"routeId":{"type":"string","format":"uuid"},"socketId":{"type":"string","minLength":64,"maxLength":64},"to":{"enum":["page","server"]},"options":{"type":"object","properties":{"code":{"type":"integer","minimum":1000,"maximum":4999},"reason":{"type":"string","maxLength":123}},"additionalProperties":false,"default":{}}},"required":["operation","scope","frame","routeId","socketId","to"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"web_socket_route_handlers"},"scope":reference,"frame":reference,"routeId":{"type":"string","format":"uuid"},"socketId":{"type":"string","minLength":64,"maxLength":64},"handlers":Handlers::schema()},"required":["operation","scope","frame","routeId","socketId","handlers"],"additionalProperties":false}),
    ]
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    vec![
        json!({"type":"object","properties":{"operation":{"const":"web_socket_route_send"},"scope":reference,"frame":reference,"routeId":{"type":"string","format":"uuid"},"socketId":{"type":"string","minLength":64,"maxLength":64},"to":{"enum":["page","server"]},"message":{"type":"object","properties":{"data":{"type":"string","maxLength":MAX_MESSAGE_BYTES.div_ceil(3) * 4,"description":"Text or base64 of at most 512 KiB decoded"},"isBase64":{"type":"boolean","default":false}},"required":["data"],"additionalProperties":false}},"required":["operation","scope","frame","routeId","socketId","to","message"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"web_socket_route_add"},"scope":reference,"url":{"type":"string","maxLength":2048},"sdkDefaults":{"type":"boolean","default":true,"description":"SDK default forwarding, automatic mock opening and independently owned message/close handlers; false selects command mode."}},"required":["operation","scope","url"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"web_socket_route_add_matcher"},"scope":reference,"matcher":Matcher::schema(),"sdkDefaults":{"type":"boolean","default":true}},"required":["operation","scope","matcher"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"web_socket_route_list"},"scope":reference},"required":["operation","scope"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"web_socket_route_remove"},"scope":reference,"routeId":{"type":"string","format":"uuid"}},"required":["operation","scope","routeId"],"additionalProperties":false}),
    ]
}
pub(crate) fn capabilities(available: bool) -> Value {
    let mut value = json!({"available":available,"scopes":["page","context"],"operations":["web_socket_route_add","web_socket_route_add_matcher","web_socket_route_list","web_socket_route_send","web_socket_route_close","web_socket_route_handlers","web_socket_route_remove"],"matching":["exact-ws-url-including-query","glob","regex","url_pattern"],"regexDialect":"ECMAScript-host-compiled-DFA-or-page-native-RegExp","maxRegexBytes":2048,"maxRegexStates":512,"maxRegexAutomatonBytes":16384,"maxRegexUrlBytes":8192,"globDialect":"pinned-Playwright-ASCII-WS(S)","maxRoutes":32,"handlerOrder":"newest-first","selection":"first-matching-page-then-context","newSocketsOnly":true,"workers":false,"maxMessageBytes":MAX_MESSAGE_BYTES,"maxEncodedMessageBytes":MAX_ENCODED_MESSAGE_BYTES,"maxPendingCallbacks":64,"maxPendingPerSocket":32,"maxSocketsPerRealmLifetime":256,"callbackKind":"binding","payloadRead":"binding_read","reply":"binding_resolve-command-array","expiryAction":"close-both-sides","precedence":"matching-page-before-context","unsolicitedSend":available,"sendScope":"exact-live-frame-route-and-private-socket-id","messageForwarding":"SDK-defaults-or-explicit-command-mode","automaticCloseForwarding":available,"closeForwarding":"SDK-defaults-or-explicit-command-mode","documentGone":"sockets-expire-without-close-callbacks","protocolOffers":"private-construction-snapshot","maxProtocols":16,"maxProtocolBytes":128,"mockProtocolSelection":"first-offered","relayProtocolSelection":"server-negotiated","closePageCodes":[[1000,1003],[1007,1014],[3000,4999]],"closeServerCodes":[[1000,1000],[3000,4999]]});
    value["sdkDefaults"] = json!(available);
    value["handlerUpdates"] = json!(available);
    value["handlerIdentity"] = json!("caller-UUID-snapshotted-at-event-arrival");
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake {
        calls: Vec<&'static str>,
        fail_script: bool,
        send_reply: Option<&'static str>,
    }
    impl Backend for Fake {
        fn send(
            &mut self,
            _: Reference,
            _: Reference,
            _: String,
            _: Instant,
        ) -> futures_util::future::BoxFuture<'_, Result<Value, Error>> {
            Box::pin(async move {
                self.calls.push("send");
                Ok(json!({"value":{"value":self.send_reply.unwrap_or("sent")}}))
            })
        }
        fn scope_is_page(&self, _: Reference) -> Result<bool, Error> {
            Ok(true)
        }
        async fn binding(&mut self, request: Request, _: Instant) -> Result<Value, Error> {
            match request {
                Request::BindingAdd { .. } => {
                    self.calls.push("binding_add");
                    Ok(json!({"bindingId":Uuid::new_v4()}))
                }
                _ => {
                    self.calls.push("binding_remove");
                    Ok(json!({}))
                }
            }
        }
        async fn script(&mut self, request: Request, _: Instant) -> Result<Value, Error> {
            match request {
                Request::PageInitScriptAdd { source, .. } => {
                    self.calls.push("script_add");
                    assert!(super::super::scripts::validate(&source).is_ok());
                    if self.fail_script {
                        let mut error = Error::from("OUTCOME_UNKNOWN");
                        error.partial = Some(json!({"scriptId":Uuid::new_v4()}));
                        Err(error)
                    } else {
                        Ok(json!({"scriptId":Uuid::new_v4()}))
                    }
                }
                _ => {
                    self.calls.push("script_remove");
                    Ok(json!({}))
                }
            }
        }
        async fn documents(
            &mut self,
            _: Reference,
            source: String,
            _: Instant,
        ) -> Result<(), Error> {
            self.calls.push(if source.contains("owner.remove(") {
                "close"
            } else {
                "install"
            });
            Ok(())
        }
    }
    fn scope() -> Reference {
        Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }
    }
    #[test]
    fn sdk_handler_updates_preserve_null_removal_and_reject_unknown_events_before_dispatch() {
        let handlers: Handlers =
            serde_json::from_value(json!({"pageMessage":Uuid::nil(),"serverClose":null})).unwrap();
        assert_eq!(
            serde_json::to_value(&handlers).unwrap(),
            json!({"pageMessage":Uuid::nil(),"serverClose":null})
        );
        assert!(validate_handlers(&"a".repeat(64), &handlers).is_ok());
        let empty: Handlers = serde_json::from_value(json!({})).unwrap();
        assert_eq!(
            validate_handlers(&"a".repeat(64), &empty),
            Err("WEBSOCKET_HANDLERS_INVALID")
        );
        assert_eq!(
            validate_handlers("invalid", &handlers),
            Err("WEBSOCKET_SOCKET_INVALID")
        );
        for value in [
            json!({"pageMessage":true}),
            json!({"pageMessage":"Acme unknown function"}),
            json!({"unknownEvent":null}),
        ] {
            assert!(serde_json::from_value::<Handlers>(value).is_err());
        }
    }
    #[tokio::test]
    async fn sdk_handler_updates_require_the_exact_ready_sdk_route_before_backend_dispatch() {
        let mut backend = Fake {
            calls: vec![],
            fail_script: false,
            send_reply: Some("configured"),
        };
        let catalog = Shared::default();
        let owner = scope();
        let frame = scope();
        let route_id = Uuid::new_v4();
        let request = |owner| Request::WebSocketRouteHandlers {
            scope: owner,
            frame,
            route_id,
            socket_id: "a".repeat(64),
            handlers: serde_json::from_value(json!({"pageMessage":Uuid::nil()})).unwrap(),
        };
        let error = sdk_route_error(&mut backend, &catalog, request(owner)).await;
        assert_eq!(error, "UNKNOWN_WEBSOCKET_ROUTE");
        catalog.lock().unwrap().items.insert(
            route_id,
            Route {
                scope: owner,
                page: true,
                order: 1,
                sdk_defaults: false,
                binding: None,
                script: None,
                ready: false,
                removing: false,
            },
        );
        let error = sdk_route_error(&mut backend, &catalog, request(owner)).await;
        assert_eq!(error, "WEBSOCKET_ROUTE_NOT_READY");
        catalog
            .lock()
            .unwrap()
            .items
            .get_mut(&route_id)
            .unwrap()
            .ready = true;
        let error = sdk_route_error(&mut backend, &catalog, request(owner)).await;
        assert_eq!(error, "WEBSOCKET_SDK_MODE_REQUIRED");
        catalog
            .lock()
            .unwrap()
            .items
            .get_mut(&route_id)
            .unwrap()
            .sdk_defaults = true;
        let error = sdk_route_error(&mut backend, &catalog, request(scope())).await;
        assert_eq!(error, "UNKNOWN_WEBSOCKET_ROUTE");
        assert!(backend.calls.is_empty());
        let configured = execute(
            &mut backend,
            catalog.clone(),
            request(owner),
            Instant::now(),
        )
        .await
        .unwrap();
        assert_eq!(configured["configured"], true);
        assert_eq!(configured["handlerChanges"], 1);
        assert_eq!(backend.calls, vec!["send"]);
        assert!(!configured.to_string().contains(&Uuid::nil().to_string()));
        catalog
            .lock()
            .unwrap()
            .items
            .get_mut(&route_id)
            .unwrap()
            .removing = true;
        let error = sdk_route_error(&mut backend, &catalog, request(owner)).await;
        assert_eq!(error, "WEBSOCKET_ROUTE_NOT_READY");
        assert_eq!(backend.calls, vec!["send"]);
    }
    async fn sdk_route_error(
        backend: &mut Fake,
        catalog: &Shared,
        request: Request,
    ) -> &'static str {
        execute(backend, catalog.clone(), request, Instant::now())
            .await
            .unwrap_err()
            .code
    }
    #[tokio::test]
    async fn sdk_handler_updates_keep_expired_realm_refusals_and_unknown_reply_ownership() {
        let mut backend = Fake {
            calls: vec![],
            fail_script: false,
            send_reply: None,
        };
        let catalog = Shared::default();
        let owner = scope();
        let frame = scope();
        let added = execute(
            &mut backend,
            catalog.clone(),
            Request::WebSocketRouteAdd {
                scope: owner,
                url: "ws://example.com/Acme".into(),
                sdk_defaults: true,
            },
            Instant::now(),
        )
        .await
        .unwrap();
        let route_id = serde_json::from_value(added["routeId"].clone()).unwrap();
        assert_eq!(added["sdkDefaults"], true);
        assert_eq!(
            catalog.lock().unwrap().snapshot(owner)["routes"][0]["sdkDefaults"],
            true
        );
        for (reply, expected) in [
            ("WEBSOCKET_SOCKET_EXPIRED", "WEBSOCKET_SOCKET_EXPIRED"),
            ("WEBSOCKET_SDK_MODE_REQUIRED", "WEBSOCKET_SDK_MODE_REQUIRED"),
            ("unexpected", "WEBSOCKET_HANDLER_PROTOCOL"),
        ] {
            backend.send_reply = Some(reply);
            let request = Request::WebSocketRouteHandlers {
                scope: owner,
                frame,
                route_id,
                socket_id: "a".repeat(64),
                handlers: serde_json::from_value(json!({"pageMessage":null})).unwrap(),
            };
            let error = execute(&mut backend, catalog.clone(), request, Instant::now())
                .await
                .unwrap_err();
            assert_eq!(error.code, expected);
            assert_eq!(error.partial.as_ref().unwrap()["dispatchCompleted"], true);
            assert_eq!(error.partial.unwrap()["replayed"], false);
        }
        assert_eq!(catalog.lock().unwrap().items.len(), 1);
    }
    #[test]
    fn sdk_defaults_are_the_default_and_registered_source_preserves_command_mode() {
        let owner = scope();
        for sdk in [None, Some(false), Some(true)] {
            let mut value = json!({"operation":"web_socket_route_add","scope":owner,"url":"wss://example.com/Acme"});
            if let Some(sdk) = sdk {
                value["sdkDefaults"] = json!(sdk);
            }
            let Request::WebSocketRouteAdd { sdk_defaults, .. } =
                serde_json::from_value(value).unwrap()
            else {
                panic!("Wrong request");
            };
            assert_eq!(sdk_defaults, sdk.unwrap_or(true));
            let script = source(
                owner,
                Uuid::new_v4(),
                &json!("wss://example.com/Acme"),
                true,
                1,
                sdk_defaults,
            );
            assert!(script.contains(&format!(",true,1,{sdk_defaults});return true;")));
        }
    }
    #[test]
    fn sdk_socket_close_preserves_omission_and_validates_role_codes_and_utf8_before_dispatch() {
        let socket = "a".repeat(64);
        let omitted: CloseOptions = serde_json::from_value(json!({})).unwrap();
        assert_eq!(serde_json::to_value(&omitted).unwrap(), json!({}));
        assert!(validate_close(&socket, Destination::Server, &omitted).is_ok());
        let empty: CloseOptions = serde_json::from_value(json!({"reason":""})).unwrap();
        assert_eq!(serde_json::to_value(&empty).unwrap(), json!({"reason":""}));
        for value in [
            json!({"code":null}),
            json!({"reason":null}),
            json!({"type":"connect"}),
        ] {
            assert!(serde_json::from_value::<CloseOptions>(value).is_err());
        }
        for (code, page, server) in [
            (1000, true, true),
            (1001, true, false),
            (1004, false, false),
            (1006, false, false),
            (1014, true, false),
            (3000, true, true),
            (4999, true, true),
            (5000, false, false),
        ] {
            let options = CloseOptions {
                code: Some(code),
                reason: None,
            };
            assert_eq!(
                validate_close(&socket, Destination::Page, &options).is_ok(),
                page
            );
            assert_eq!(
                validate_close(&socket, Destination::Server, &options).is_ok(),
                server
            );
        }
        let mut options = CloseOptions {
            code: Some(3001),
            reason: Some("é".repeat(62)),
        };
        assert_eq!(
            validate_close(&socket, Destination::Page, &options),
            Err("WEBSOCKET_CLOSE_INVALID")
        );
        options.reason = Some("é".repeat(61));
        assert!(validate_close(&socket, Destination::Page, &options).is_ok());
    }
    #[tokio::test]
    async fn sdk_socket_close_requires_an_owned_ready_route_and_reports_request_not_handshake_completion()
     {
        let mut backend = Fake {
            calls: vec![],
            fail_script: false,
            send_reply: Some("close-requested"),
        };
        let catalog = Shared::default();
        let owner = scope();
        let frame = scope();
        let route_id = Uuid::new_v4();
        let request = |scope| Request::WebSocketRouteClose {
            scope,
            frame,
            route_id,
            socket_id: "a".repeat(64),
            to: Destination::Server,
            options: CloseOptions::default(),
        };
        catalog.lock().unwrap().items.insert(
            route_id,
            Route {
                scope: owner,
                page: true,
                order: 1,
                sdk_defaults: false,
                binding: None,
                script: None,
                ready: false,
                removing: false,
            },
        );
        let error = execute(
            &mut backend,
            catalog.clone(),
            request(owner),
            Instant::now(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "WEBSOCKET_ROUTE_NOT_READY");
        catalog
            .lock()
            .unwrap()
            .items
            .get_mut(&route_id)
            .unwrap()
            .ready = true;
        let error = execute(
            &mut backend,
            catalog.clone(),
            request(scope()),
            Instant::now(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "UNKNOWN_WEBSOCKET_ROUTE");
        assert!(backend.calls.is_empty());
        let receipt = execute(
            &mut backend,
            catalog.clone(),
            request(owner),
            Instant::now(),
        )
        .await
        .unwrap();
        assert_eq!(receipt["closeRequested"], true);
        assert_eq!(receipt["replayed"], false);
        assert!(receipt.get("closed").is_none());
        assert_eq!(backend.calls, vec!["send"]);
        backend.send_reply = Some("WEBSOCKET_SOCKET_EXPIRED");
        let error = execute(
            &mut backend,
            catalog.clone(),
            request(owner),
            Instant::now(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "WEBSOCKET_SOCKET_EXPIRED");
        assert_eq!(error.partial.unwrap()["dispatchCompleted"], true);
        catalog
            .lock()
            .unwrap()
            .items
            .get_mut(&route_id)
            .unwrap()
            .removing = true;
        let calls = backend.calls.len();
        let error = execute(&mut backend, catalog, request(owner), Instant::now())
            .await
            .unwrap_err();
        assert_eq!(error.code, "WEBSOCKET_ROUTE_NOT_READY");
        assert_eq!(backend.calls.len(), calls);
    }
    #[test]
    fn unsolicited_payloads_bound_decoded_bytes_before_backend_dispatch() {
        let socket = "a".repeat(64);
        assert!(
            validate_send(
                &socket,
                &Message {
                    data: STANDARD.encode([0, 255]),
                    is_base64: true
                }
            )
            .is_ok()
        );
        // 512 KiB decoded is accepted in either encoding; one byte more is not.
        for (data, is_base64, accepted) in [
            ("é".repeat(MAX_MESSAGE_BYTES / 2), false, true),
            ("é".repeat(MAX_MESSAGE_BYTES / 2 + 1), false, false),
            (STANDARD.encode(vec![0; MAX_MESSAGE_BYTES]), true, true),
            (STANDARD.encode(vec![0; MAX_MESSAGE_BYTES + 1]), true, false),
            // Escaping would exceed the shared transport limit.
            ("\u{1}".repeat(MAX_MESSAGE_BYTES / 4), false, false),
        ] {
            assert_eq!(
                validate_send(&socket, &Message { data, is_base64 }).is_ok(),
                accepted,
                "{is_base64}/{accepted}"
            );
        }
        assert_eq!(
            validate_send(
                "invalid",
                &Message {
                    data: String::new(),
                    is_base64: false
                }
            ),
            Err("WEBSOCKET_SOCKET_INVALID")
        );
    }
    #[tokio::test]
    async fn unsolicited_sends_require_the_exact_ready_route_before_backend_dispatch() {
        let mut backend = Fake {
            calls: vec![],
            fail_script: false,
            send_reply: None,
        };
        let catalog = Shared::default();
        let owner = scope();
        let frame = scope();
        let id = Uuid::new_v4();
        catalog.lock().unwrap().items.insert(
            id,
            Route {
                scope: owner,
                page: true,
                order: 1,
                sdk_defaults: false,
                binding: None,
                script: None,
                ready: false,
                removing: false,
            },
        );
        let request = || Request::WebSocketRouteSend {
            scope: owner,
            frame,
            route_id: id,
            socket_id: "a".repeat(64),
            to: Destination::Page,
            message: Message {
                data: "Acme private push".into(),
                is_base64: false,
            },
        };
        assert_eq!(
            execute(&mut backend, catalog.clone(), request(), Instant::now())
                .await
                .unwrap_err()
                .code,
            "WEBSOCKET_ROUTE_NOT_READY"
        );
        assert!(backend.calls.is_empty());
        catalog.lock().unwrap().items.get_mut(&id).unwrap().ready = true;
        let sent = execute(&mut backend, catalog.clone(), request(), Instant::now())
            .await
            .unwrap();
        assert!(!sent.to_string().contains("Acme private push"));
        assert_eq!(backend.calls, vec!["send"]);
        catalog.lock().unwrap().items.get_mut(&id).unwrap().removing = true;
        assert_eq!(
            execute(&mut backend, catalog.clone(), request(), Instant::now())
                .await
                .unwrap_err()
                .code,
            "WEBSOCKET_ROUTE_NOT_READY"
        );
        assert_eq!(backend.calls, vec!["send"]);
        catalog.lock().unwrap().items.clear();
        assert_eq!(
            execute(&mut backend, catalog, request(), Instant::now())
                .await
                .unwrap_err()
                .code,
            "UNKNOWN_WEBSOCKET_ROUTE"
        );
        assert_eq!(backend.calls, vec!["send"]);
    }
    #[tokio::test]
    async fn unsolicited_server_sends_preserve_endpoint_refusal_codes_without_payloads() {
        let mut backend = Fake {
            calls: vec![],
            fail_script: false,
            send_reply: None,
        };
        let catalog = Shared::default();
        let owner = scope();
        let frame = scope();
        let added = execute(
            &mut backend,
            catalog.clone(),
            Request::WebSocketRouteAdd {
                scope: owner,
                url: "ws://example.com/socket".into(),
                sdk_defaults: false,
            },
            Instant::now(),
        )
        .await
        .unwrap();
        let route_id = serde_json::from_value(added["routeId"].clone()).unwrap();
        for code in ["WEBSOCKET_SERVER_NOT_CONNECTED", "WEBSOCKET_SOCKET_CLOSED"] {
            backend.send_reply = Some(code);
            let error = execute(
                &mut backend,
                catalog.clone(),
                Request::WebSocketRouteSend {
                    scope: owner,
                    frame,
                    route_id,
                    socket_id: "a".repeat(64),
                    to: Destination::Server,
                    message: Message {
                        data: "Acme private message".into(),
                        is_base64: false,
                    },
                },
                Instant::now(),
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, code);
            let partial = error.partial.unwrap();
            assert_eq!(partial["dispatchCompleted"], true);
            assert!(!partial.to_string().contains("Acme private message"));
        }
        assert_eq!(
            backend.calls.iter().filter(|call| **call == "send").count(),
            2
        );
        assert_eq!(catalog.lock().unwrap().items.len(), 1);
    }
    #[tokio::test]
    async fn managed_socket_routes_remove_future_scripts_before_live_sockets_and_binding() {
        let mut backend = Fake {
            calls: vec![],
            fail_script: false,
            send_reply: None,
        };
        let catalog = Shared::default();
        let scope = scope();
        let deadline = Instant::now();
        let value = execute(
            &mut backend,
            catalog.clone(),
            Request::WebSocketRouteAdd {
                scope,
                url: "wss://example.com/socket".into(),
                sdk_defaults: false,
            },
            deadline,
        )
        .await
        .unwrap();
        let id = serde_json::from_value(value["routeId"].clone()).unwrap();
        let sibling = execute(
            &mut backend,
            catalog.clone(),
            Request::WebSocketRouteAdd {
                scope,
                url: "wss://example.com/other".into(),
                sdk_defaults: false,
            },
            deadline,
        )
        .await
        .unwrap();
        let listed = catalog.lock().unwrap().snapshot(scope);
        assert_eq!(listed["routes"][0]["routeId"], sibling["routeId"]);
        assert_eq!(listed["routes"][1]["routeId"], value["routeId"]);
        backend.calls.clear();
        execute(
            &mut backend,
            catalog.clone(),
            Request::WebSocketRouteRemove {
                scope,
                route_id: id,
            },
            deadline,
        )
        .await
        .unwrap();
        assert_eq!(
            backend.calls,
            vec!["script_remove", "close", "binding_remove"]
        );
        let listed = catalog.lock().unwrap().snapshot(scope);
        assert_eq!(listed["routes"].as_array().unwrap().len(), 1);
        assert_eq!(listed["routes"][0]["routeId"], sibling["routeId"]);
    }
    #[tokio::test]
    async fn regex_refusal_precedes_owner_allocation_and_valid_programs_fit_script_bounds() {
        let mut backend = Fake {
            calls: vec![],
            fail_script: false,
            send_reply: None,
        };
        let catalog = Shared::default();
        let scope = scope();
        for pattern in ["(", "a".repeat(2049).as_str()] {
            let matcher =
                serde_json::from_value(json!({"kind":"regex","pattern":pattern})).unwrap();
            execute(
                &mut backend,
                catalog.clone(),
                Request::WebSocketRouteAddMatcher {
                    scope,
                    matcher,
                    sdk_defaults: false,
                },
                Instant::now(),
            )
            .await
            .unwrap_err();
            assert!(backend.calls.is_empty());
            assert!(catalog.lock().unwrap().items.is_empty());
        }
        let matcher = serde_json::from_value(
            json!({"kind":"regex","pattern":r"^wss?://[^/]+/Acme[0-9]+$","flags":"i"}),
        )
        .unwrap();
        execute(
            &mut backend,
            catalog.clone(),
            Request::WebSocketRouteAddMatcher {
                scope,
                matcher,
                sdk_defaults: false,
            },
            Instant::now(),
        )
        .await
        .unwrap();
        assert_eq!(backend.calls, vec!["binding_add", "script_add", "install"]);
        // A lookbehind registers for the page's native RegExp.
        let matcher = serde_json::from_value(
            json!({"kind":"regex","pattern":r"(?<=/)Acme(?=\?)","flags":"i"}),
        )
        .unwrap();
        execute(
            &mut backend,
            catalog.clone(),
            Request::WebSocketRouteAddMatcher {
                scope,
                matcher,
                sdk_defaults: true,
            },
            Instant::now(),
        )
        .await
        .unwrap();
        assert_eq!(backend.calls.len(), 6);
        assert!(
            !catalog
                .lock()
                .unwrap()
                .snapshot(scope)
                .to_string()
                .contains("Acme")
        );
    }
    #[tokio::test]
    async fn sibling_capacity_and_registration_order_exhaustion_refuse_before_backend_work() {
        let mut backend = Fake {
            calls: vec![],
            fail_script: false,
            send_reply: None,
        };
        let catalog = Shared::default();
        let scope = scope();
        let request = || Request::WebSocketRouteAdd {
            scope,
            url: "ws://example.com/socket".into(),
            sdk_defaults: false,
        };
        for _ in 0..32 {
            execute(&mut backend, catalog.clone(), request(), Instant::now())
                .await
                .unwrap();
        }
        let calls = backend.calls.len();
        let error = execute(&mut backend, catalog.clone(), request(), Instant::now())
            .await
            .unwrap_err();
        assert_eq!(error.code, "WEBSOCKET_ROUTE_CAPACITY");
        assert_eq!(backend.calls.len(), calls);
        assert_eq!(catalog.lock().unwrap().items.len(), 32);
        {
            let mut catalog = catalog.lock().unwrap();
            catalog.items.clear();
            catalog.next_order = u32::MAX;
        }
        let error = execute(&mut backend, catalog.clone(), request(), Instant::now())
            .await
            .unwrap_err();
        assert_eq!(error.code, "WEBSOCKET_ROUTE_CAPACITY");
        assert_eq!(backend.calls.len(), calls);
        assert!(catalog.lock().unwrap().items.is_empty());
    }
    #[tokio::test]
    async fn uncertain_socket_script_registration_keeps_both_original_owners() {
        let mut backend = Fake {
            calls: vec![],
            fail_script: true,
            send_reply: None,
        };
        let catalog = Shared::default();
        let scope = scope();
        let deadline = Instant::now();
        let error = execute(
            &mut backend,
            catalog.clone(),
            Request::WebSocketRouteAdd {
                scope,
                url: "ws://example.com/socket".into(),
                sdk_defaults: false,
            },
            deadline,
        )
        .await
        .unwrap_err();
        let id = serde_json::from_value(error.partial.unwrap()["routeId"].clone()).unwrap();
        let listed = execute(
            &mut backend,
            catalog.clone(),
            Request::WebSocketRouteList { scope },
            deadline,
        )
        .await
        .unwrap();
        assert_eq!(listed["routes"][0]["state"], "setup_pending");
        assert!(listed["routes"][0]["bindingId"].is_string());
        assert!(listed["routes"][0]["scriptId"].is_string());
        let calls = backend.calls.clone();
        let blocked = execute(
            &mut backend,
            catalog.clone(),
            Request::WebSocketRouteAdd {
                scope,
                url: "ws://example.com/sibling".into(),
                sdk_defaults: false,
            },
            deadline,
        )
        .await
        .unwrap_err();
        assert_eq!(blocked.code, "WEBSOCKET_ROUTE_NOT_READY");
        assert_eq!(backend.calls, calls);
        assert_eq!(catalog.lock().unwrap().items.len(), 1);
        execute(
            &mut backend,
            catalog,
            Request::WebSocketRouteRemove {
                scope,
                route_id: id,
            },
            deadline,
        )
        .await
        .unwrap();
        assert_eq!(
            backend.calls,
            vec![
                "binding_add",
                "script_add",
                "script_remove",
                "close",
                "binding_remove"
            ]
        );
    }
}
