//! Bounded diagnostic history, private request identities and explicit cursor loss.

use super::{contract::Reference, objects::ObjectRef};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
mod paging;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use uuid::Uuid;
mod correlation;
mod ignores;
mod projection;
pub use correlation::Correlation;
pub use ignores::IgnoreRule;
mod sockets;
pub(crate) mod stacks;

pub(crate) const MAX_RECORDS: usize = 8192;
pub(crate) const MAX_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_REQUESTS: usize = 4096;
pub(crate) const SUBSCRIPTIONS: &[&str] = &[
    "console",
    "request",
    "response",
    "requestFinished",
    "requestFailed",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    Console,
    PageError,
    Request,
    Response,
    RequestFinished,
    RequestFailed,
    WebSocket,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventCondition {
    pub event: DiagnosticKind,
    #[serde(default)]
    pub page: Option<Reference>,
    #[serde(default)]
    pub request_id: Option<Uuid>,
    #[serde(default)]
    pub web_socket_id: Option<Uuid>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub status: Option<u16>,
    #[serde(default)]
    pub url: Option<String>,
}

impl EventCondition {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.method.as_ref().is_some_and(|method| {
            method.is_empty()
                || method.len() > 32
                || !method
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
        }) || self.status.is_some_and(|status| {
            !(100..=599).contains(&status) || self.event != DiagnosticKind::Response
        }) || self.url.as_ref().is_some_and(|value| {
            value.len() > 2048
                || !(value.starts_with("http://")
                    || value.starts_with("https://")
                    || (self.event == DiagnosticKind::WebSocket
                        && (value.starts_with("ws://") || value.starts_with("wss://"))))
                || url(value) != *value
        }) {
            return Err("INVALID_EVENT_CONDITION");
        }
        if self.web_socket_id.is_some() && self.event != DiagnosticKind::WebSocket {
            return Err("INVALID_EVENT_CONDITION");
        }
        if self.event == DiagnosticKind::WebSocket
            && (self.method.is_some() || self.status.is_some() || self.request_id.is_some())
        {
            return Err("INVALID_EVENT_CONDITION");
        }
        if matches!(
            self.event,
            DiagnosticKind::Console | DiagnosticKind::PageError
        ) && (self.method.is_some()
            || self.status.is_some()
            || self.request_id.is_some()
            || self.url.is_some())
        {
            return Err("INVALID_EVENT_CONDITION");
        }
        Ok(())
    }
    pub(crate) fn matches(&self, event: &Value) -> bool {
        event["kind"] == json!(self.event)
            && self.page.is_none_or(|page| event["page"] == json!(page))
            && self
                .request_id
                .is_none_or(|id| event["data"]["requestId"] == json!(id))
            && self
                .web_socket_id
                .is_none_or(|id| event["data"]["webSocketId"] == json!(id))
            && self
                .method
                .as_ref()
                .is_none_or(|method| event["data"]["method"] == *method)
            && self
                .status
                .is_none_or(|status| event["data"]["status"] == status)
            && self
                .url
                .as_ref()
                .is_none_or(|url| event["data"]["url"] == *url)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Query {
    #[serde(default)]
    pub through: Option<u64>,
    #[serde(default)]
    pub latest: bool,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub status: Option<u16>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub sequence: Option<u64>,
    #[serde(skip)]
    pub after: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default)]
    pub page: Option<Reference>,
    #[serde(default)]
    pub worker: Option<Reference>,
    #[serde(default)]
    pub levels: Vec<String>,
    #[serde(default)]
    pub resource_types: Vec<String>,
    #[serde(default)]
    pub kind: Option<DiagnosticKind>,
    #[serde(default)]
    pub request_id: Option<Uuid>,
    #[serde(default)]
    pub web_socket_id: Option<Uuid>,
    #[serde(default)]
    pub include_text: bool,
    #[serde(default)]
    pub include_stack: bool,
    #[serde(default)]
    pub correlation: Option<Correlation>,
    #[serde(default)]
    pub ignore_rules: Vec<IgnoreRule>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}
fn default_limit() -> usize {
    50
}
impl Default for Query {
    fn default() -> Self {
        Self {
            through: None,
            latest: false,
            method: None,
            status: None,
            url: None,
            sequence: None,
            after: None,
            cursor: None,
            page: None,
            worker: None,
            levels: Vec::new(),
            resource_types: Vec::new(),
            kind: None,
            request_id: None,
            web_socket_id: None,
            include_text: false,
            include_stack: false,
            correlation: None,
            ignore_rules: Vec::new(),
            limit: default_limit(),
        }
    }
}
impl Query {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.ignore_rules.len() > 4 {
            return Err("DIAGNOSTIC_IGNORE_BOUNDS");
        }
        for rule in &self.ignore_rules {
            rule.validate()?;
        }
        if !(1..=100).contains(&self.limit)
            || self
                .through
                .is_some_and(|cursor| cursor > super::activity::MAX_CURSOR)
            || (self.sequence.is_some() && (self.latest || self.through.is_some()))
            || self
                .after
                .zip(self.through)
                .is_some_and(|(after, through)| after > through)
            || self
                .status
                .is_some_and(|status| !(100..=599).contains(&status))
            || self.method.as_ref().is_some_and(|method| {
                method.is_empty()
                    || method.len() > 32
                    || !method.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
                    })
            })
            || self.url.as_ref().is_some_and(|value| {
                value.len() > 2048
                    || !["http://", "https://", "ws://", "wss://"]
                        .iter()
                        .any(|prefix| value.starts_with(*prefix))
                    || url(value) != *value
            })
            || self
                .after
                .is_some_and(|cursor| cursor > super::activity::MAX_CURSOR)
            || self
                .sequence
                .is_some_and(|sequence| sequence == 0 || sequence > super::activity::MAX_CURSOR)
            || (self.sequence.is_some() && (self.after.is_some() || self.cursor.is_some()))
            || self.levels.len() > 32
            || self.resource_types.len() > 32
            || self.correlation.as_ref().is_some_and(|options| {
                !(1..=60000).contains(&options.window_ms) || !(1..=4).contains(&options.limit)
            })
            || self.levels.iter().chain(&self.resource_types).any(|value| {
                value.is_empty()
                    || value.len() > 64
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            })
        {
            return Err("DIAGNOSTIC_QUERY_BOUNDS");
        }
        Ok(())
    }
    fn matches(&self, context: Reference, event: &Value) -> bool {
        let selected = |values: &[String], value: &Value| {
            values.is_empty()
                || value.as_str().is_some_and(|value| {
                    values
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(value))
                })
        };
        event["context"] == json!(context)
            && self
                .method
                .as_ref()
                .is_none_or(|method| event["data"]["method"].as_str() == Some(method.as_str()))
            && self
                .status
                .is_none_or(|status| event["data"]["status"].as_f64() == Some(f64::from(status)))
            && self.url.as_ref().is_none_or(|url| {
                event["data"]["url"]
                    .as_str()
                    .or_else(|| event["data"]["location"]["url"].as_str())
                    == Some(url.as_str())
            })
            && self.page.is_none_or(|page| event["page"] == json!(page))
            && self
                .worker
                .is_none_or(|worker| event["data"]["worker"] == json!(worker))
            && self.kind.is_none_or(|kind| event["kind"] == json!(kind))
            && self
                .request_id
                .is_none_or(|id| event["data"]["requestId"] == json!(id))
            && self
                .web_socket_id
                .is_none_or(|id| event["data"]["webSocketId"] == json!(id))
            && selected(&self.levels, &event["data"]["level"])
            && selected(&self.resource_types, &event["data"]["resourceType"])
    }
}

#[derive(Default)]
pub(crate) struct Diagnostics {
    sockets: sockets::Sockets,
    active: BTreeSet<Reference>,
    pending: BTreeSet<Reference>,
    events: VecDeque<Value>,
    bytes: usize,
    sequence: u64,
    evicted_through: u64,
    dropped: u64,
    requests: HashMap<ObjectRef, (Uuid, bool)>,
    request_ids: HashMap<Uuid, ObjectRef>,
    request_contexts: HashMap<ObjectRef, Reference>,
    request_states: HashMap<ObjectRef, RequestState>,
    request_order: VecDeque<ObjectRef>,
    forgotten_requests: u64,
    forgotten_sockets: u64,
    /// Redirect predecessor and successor request IDs, oldest first.
    redirects: VecDeque<(Uuid, Uuid)>,
    /// Contexts with trusted sign-in entry in progress, and how many.
    withholding: BTreeMap<Reference, usize>,
}

#[derive(Default)]
struct RequestState {
    response: bool,
    complete: bool,
    end: Option<f64>,
    /// Seen while its context withheld request bodies.
    body_withheld: bool,
}

impl Diagnostics {
    /// An upstream gap has no reliable position among queued records. Fence all
    /// earlier cursors rather than allowing a wait to claim complete history.
    pub(crate) fn source_gap(&mut self, dropped: u64) {
        self.sequence += 1;
        self.evicted_through = self.sequence;
        self.dropped = self.dropped.saturating_add(dropped);
        self.events.clear();
        self.bytes = 0;
    }
    pub(crate) fn forget_requests(&mut self, count: usize) {
        self.forgotten_requests = self.forgotten_requests.saturating_add(count as u64);
    }
    pub(crate) fn forget_sockets(&mut self, count: usize) {
        self.forgotten_sockets = self.forgotten_sockets.saturating_add(count as u64);
    }
    pub fn active(&self, context: Reference) -> bool {
        self.active.contains(&context)
    }
    pub(crate) fn capturing(&self) -> bool {
        !self.active.is_empty()
    }
    pub fn enable(&mut self, context: Reference, enabled: bool) {
        self.pending.remove(&context);
        if enabled {
            self.active.insert(context);
        } else {
            self.active.remove(&context);
            self.sockets.close_context(context);
        }
    }
    pub fn begin_subscription(&mut self, context: Reference, enabled: bool) {
        self.pending.insert(context);
        if enabled {
            self.active.insert(context);
        }
    }
    pub fn close(&mut self) {
        self.active.clear();
        self.sockets.clear();
        self.pending.clear();
        self.requests.clear();
        self.request_ids.clear();
        self.request_contexts.clear();
        self.request_states.clear();
        self.request_order.clear();
        self.redirects.clear();
    }
    pub fn forget_object(&mut self, object: &ObjectRef) {
        self.sockets.forget(object);
        if let Some((id, _)) = self.requests.remove(object) {
            self.request_ids.remove(&id);
        }
        self.request_contexts.remove(object);
        self.request_states.remove(object);
        self.request_order.retain(|entry| entry != object);
    }
    pub fn request(&mut self, wire: ObjectRef, initial: bool) -> (Uuid, bool) {
        if let Some((id, missing)) = self.requests.get_mut(&wire) {
            if initial {
                *missing = false;
            }
            return (*id, *missing);
        }
        if self.request_order.len() == MAX_REQUESTS {
            let oldest = self.request_order.pop_front().expect("Full request index");
            if let Some((id, _)) = self.requests.remove(&oldest) {
                self.request_ids.remove(&id);
            }
            self.request_contexts.remove(&oldest);
            self.request_states.remove(&oldest);
            self.forgotten_requests += 1;
        }
        let id = Uuid::new_v4();
        self.request_order.push_back(wire.clone());
        let missing = !initial;
        self.requests.insert(wire, (id, missing));
        self.request_ids.insert(
            id,
            self.request_order.back().expect("Indexed request").clone(),
        );
        (id, missing)
    }
    pub(crate) fn scope_request(
        &mut self,
        wire: &ObjectRef,
        context: Reference,
        response: bool,
        complete: bool,
    ) {
        let withheld = self.withholds_bodies(context);
        self.request_contexts.insert(wire.clone(), context);
        let entry = self.request_states.entry(wire.clone()).or_default();
        entry.response |= response;
        entry.complete |= complete;
        entry.body_withheld |= withheld;
    }
    /// Starts or ends one trusted entry in `context`. Request bodies seen
    /// meanwhile stay withheld, as they can carry the entered values.
    pub(crate) fn withhold_bodies(&mut self, context: Reference, active: bool) {
        if active {
            *self.withholding.entry(context).or_default() += 1;
        } else if let Some(count) = self.withholding.get_mut(&context) {
            *count -= 1;
            if *count == 0 {
                self.withholding.remove(&context);
            }
        }
    }
    pub(crate) fn withholds_bodies(&self, context: Reference) -> bool {
        self.withholding.contains_key(&context)
    }
    pub(crate) fn request_body_withheld(&self, wire: &ObjectRef) -> bool {
        self.request_states
            .get(wire)
            .is_some_and(|state| state.body_withheld)
    }
    pub(crate) fn request_end(&self, wire: &ObjectRef) -> Option<f64> {
        self.request_states.get(wire).and_then(|state| state.end)
    }
    pub(crate) fn finish_timing(&mut self, wire: &ObjectRef, value: &Value) {
        if let Some(state) = self.request_states.get_mut(wire) {
            state.end = super::network::response::number(value);
        }
    }
    pub(crate) fn resolve_request(
        &self,
        context: Reference,
        id: Uuid,
    ) -> Result<(ObjectRef, bool, bool), &'static str> {
        let wire = self.request_ids.get(&id).ok_or("NETWORK_REQUEST_EXPIRED")?;
        if self.request_contexts.get(wire) != Some(&context) {
            return Err("NETWORK_REQUEST_EXPIRED");
        }
        let state = self.request_states.get(wire);
        Ok((
            wire.clone(),
            state.is_some_and(|s| s.response),
            state.is_some_and(|s| s.complete),
        ))
    }
    pub fn known_request(&self, wire: &ObjectRef) -> Option<Uuid> {
        self.requests.get(wire).map(|(id, _)| *id)
    }
    /// SDK `redirectedTo()`: remember `successor` as `predecessor`'s redirect,
    /// bounded like the request index.
    pub(crate) fn redirected(&mut self, predecessor: Uuid, successor: Uuid) {
        if self.redirects.len() == MAX_REQUESTS {
            self.redirects.pop_front();
        }
        self.redirects.push_back((predecessor, successor));
    }
    pub(crate) fn redirected_to(&self, predecessor: Uuid) -> Option<Uuid> {
        self.redirects
            .iter()
            .rev()
            .find(|(from, _)| *from == predecessor)
            .map(|(_, to)| *to)
    }
    pub fn push(
        &mut self,
        context: Reference,
        page: Option<Reference>,
        kind: DiagnosticKind,
        data: Value,
        observed_at: u128,
    ) {
        self.sequence += 1;
        let event = json!({"sequence":self.sequence,"observedAtUnixMs":observed_at,"context":context,"page":page,"kind":kind,"data":data});
        let bytes = serde_json::to_vec(&event).expect("Diagnostic JSON").len();
        while self.events.len() >= MAX_RECORDS || self.bytes + bytes > MAX_BYTES {
            let Some(oldest) = self.events.pop_front() else {
                break;
            };
            self.bytes -= serde_json::to_vec(&oldest).expect("Diagnostic JSON").len();
            self.evicted_through = oldest["sequence"].as_u64().expect("Sequence");
            self.dropped += 1;
        }
        if bytes > MAX_BYTES {
            self.evicted_through = self.sequence;
            self.dropped += 1;
            return;
        }
        self.bytes += bytes;
        self.events.push_back(event);
    }
    fn latest_matching(
        &self,
        context: Reference,
        query: &Query,
        after: u64,
        through: u64,
    ) -> Option<u64> {
        if !query.latest {
            return None;
        }
        self.events.iter().rev().find_map(|event| {
            let sequence = event["sequence"].as_u64()?;
            (sequence > after && sequence <= through && query.matches(context, event))
                .then_some(sequence)
        })
    }

    fn project_event(&self, context: Reference, event: &Value, query: &Query) -> Value {
        let mut event = event.clone();
        ignores::annotate(&mut event, context, &query.ignore_rules);
        if let Some(options) = &query.correlation
            && matches!(event["kind"].as_str(), Some("console" | "page_error"))
        {
            event["data"]["requestCorrelation"] =
                self.correlate(&event, options, (query.after, query.through));
        }
        projection::read(event, query)
    }

    pub fn read(&self, context: Reference, query: &Query) -> Result<Value, &'static str> {
        query.validate()?;
        let after = query
            .sequence
            .map(|sequence| sequence - 1)
            .or(query.after)
            .unwrap_or(self.evicted_through);
        if after < self.evicted_through {
            return Err("DIAGNOSTIC_CURSOR_EXPIRED");
        }
        if query
            .sequence
            .is_some_and(|sequence| sequence > self.sequence)
        {
            return Err("DIAGNOSTIC_EVENT_NOT_FOUND");
        }
        if after > self.sequence {
            return Err("INVALID_DIAGNOSTIC_CURSOR");
        }
        let through = query.sequence.or(query.through).unwrap_or(self.sequence);
        if through > self.sequence {
            return Err("INVALID_DIAGNOSTIC_CURSOR");
        }
        if through < self.evicted_through {
            return Err("DIAGNOSTIC_CURSOR_EXPIRED");
        }
        let latest = self.latest_matching(context, query, after, through);
        let mut events = Vec::new();
        let mut bytes = 0;
        let mut cursor = after;
        let mut more = false;
        for event in &self.events {
            let sequence = event["sequence"].as_u64().expect("Sequence");
            if sequence <= after {
                continue;
            }
            if sequence > through {
                break;
            }
            if query.latest && Some(sequence) != latest {
                cursor = sequence;
                continue;
            }
            if query.sequence.is_some_and(|selected| sequence > selected) {
                break;
            }
            if !query.matches(context, event) {
                cursor = sequence;
                continue;
            }
            if events.len() == query.limit {
                more = true;
                break;
            }
            let event = self.project_event(context, event, query);
            let size = serde_json::to_vec(&event).expect("Diagnostic JSON").len();
            if bytes + size > 7000 {
                more = true;
                break;
            }
            cursor = sequence;
            bytes += size;
            events.push(event);
        }
        if query.sequence.is_some() && events.is_empty() {
            return Err("DIAGNOSTIC_EVENT_NOT_FOUND");
        }
        if query.latest {
            cursor = through;
        }
        Ok(
            json!({"context":context,"events":events,"nextCursor":cursor,"hasMore":more,"oldestCursor":self.evicted_through,"latestCursor":self.sequence,"observedRange":{"after":after,"through":through},"latestOnly":query.latest,"dropped":self.dropped,"forgottenRequestIdentities":self.forgotten_requests,"forgottenWebSocketIdentities":self.forgotten_sockets,"active":self.active(context),"subscriptionPending":self.pending.contains(&context),"textIncluded":query.include_text,"bodiesIncluded":false,"headersIncluded":false,"correlation":if query.correlation.is_some(){"source-identity-or-unproven-candidates"}else{"exact-network-identity-only"}}),
        )
    }
    pub fn summary(&self) -> Value {
        json!({"activeContexts":self.active,"pendingContexts":self.pending,"records":self.events.len(),"bytes":self.bytes,"dropped":self.dropped,"oldestCursor":self.evicted_through,"latestCursor":self.sequence,"forgottenRequestIdentities":self.forgotten_requests,"forgottenWebSocketIdentities":self.forgotten_sockets})
    }
}

pub(crate) fn text(value: &str) -> (String, bool) {
    let mut end = value.len().min(2048);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].into(), end < value.len())
}

/// Omit credentials, the entire query and fragment, and non-network payload URLs.
pub(crate) fn url(value: &str) -> String {
    let value = value.split(['?', '#']).next().unwrap_or("");
    let Some((scheme, remainder)) = value.split_once("://") else {
        return if value == "about:blank" {
            value.into()
        } else {
            "[non-network URL]".into()
        };
    };
    if !matches!(scheme, "http" | "https" | "ws" | "wss") || value.chars().any(char::is_control) {
        return "[non-network URL]".into();
    }
    let (authority, path) = remainder.split_once('/').unwrap_or((remainder, ""));
    let authority = authority.rsplit('@').next().unwrap_or("");
    text(&format!("{scheme}://{authority}/{path}")).0
}

pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let object = |properties, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let mut result = Vec::new();
    for operation in ["diagnostics_start", "diagnostics_stop"] {
        result.push(object(
            json!({"operation":{"const":operation},"context":reference}),
            &["operation", "context"],
        ));
    }
    result.push(object(json!({"operation":{"const":"diagnostics_read"},"context":reference,"query":object(json!({"after":{"type":["integer","null"],"minimum":0,"maximum":9007199254740991u64},"page":{"anyOf":[reference,{"type":"null"}]},"kind":{"enum":["console","page_error","request","response","request_finished","request_failed","web_socket",null]},"requestId":{"type":["string","null"],"format":"uuid"},"webSocketId":{"type":["string","null"],"format":"uuid"},"includeText":{"type":"boolean","default":false},"includeStack":{"type":"boolean","default":false,"description":"Include bounded private engine stacks; stack text and function names can contain application secrets"},"limit":{"type":"integer","minimum":1,"maximum":100,"default":50}}), &[])}), &["operation","context"]));
    let condition = object(
        json!({"event":{"enum":["console","page_error","request","response","request_finished","request_failed","web_socket"]},"page":reference,"requestId":{"type":"string","format":"uuid"},"webSocketId":{"type":"string","format":"uuid"},"method":{"type":"string","minLength":1,"maxLength":32},"status":{"type":"integer","minimum":100,"maximum":599},"url":{"type":"string","maxLength":2048,"description":"Exact redacted HTTP(S) URL as diagnostics_read reports it; no query, fragment or credentials"}}),
        &["event"],
    );
    result.push(object(json!({"operation":{"const":"diagnostics_wait"},"context":reference,"after":{"type":"integer","minimum":0,"maximum":9007199254740991u64,"description":"Capture latestCursor before the action that should cause the event"},"conditions":{"type":"array","minItems":1,"maxItems":8,"items":condition}}), &["operation","context","after","conditions"]));
    let read = &mut result
        .iter_mut()
        .find(|schema| schema["properties"]["operation"]["const"] == "diagnostics_read")
        .expect("Diagnostic read schema")["properties"]["query"]["properties"];
    read["sequence"] = json!({"type":["integer","null"],"minimum":1,"maximum":9007199254740991u64,"description":"Read exactly this retained event sequence; separate from after, through and latest selection"});
    read["through"] = json!({"type":["integer","null"],"minimum":0,"maximum":9007199254740991u64,"description":"Inclusive observed window end; omit for the current cursor"});
    read.as_object_mut()
        .expect("diagnostic properties")
        .remove("after");
    read.as_object_mut().expect("diagnostic properties").extend(
        crate::pagination::properties(canonic_mcp_kit::page::Limits::STANDARD)
            .as_object()
            .expect("page properties")
            .clone(),
    );
    read["latest"] = json!({"type":"boolean","default":false,"description":"Return at most the last matching event in the observed window; nextCursor advances to its end"});
    read["method"] = json!({"type":["string","null"],"minLength":1,"maxLength":32});
    read["status"] = json!({"type":["integer","null"],"minimum":100,"maximum":599});
    read["url"] = json!({"type":["string","null"],"maxLength":2048,"description":"Exact redacted network/location URL as reported; no credentials, query or fragment"});
    read["worker"] = json!({"anyOf":[reference,{"type":"null"}]});
    let selectors = json!({"type":"array","maxItems":32,"default":[],"items":{"type":"string","minLength":1,"maxLength":64,"description":"ASCII letters, digits, underscores and hyphens"},"description":"Case-insensitive selection of reported metadata values; empty selects all"});
    read["levels"] = selectors.clone();
    read["ignoreRules"] = ignores::schema(reference);
    read["correlation"] = json!({"anyOf":[{"type":"null"},{"type":"object","properties":{"windowMs":{"type":"integer","minimum":1,"maximum":60000,"default":5000},"limit":{"type":"integer","minimum":1,"maximum":4,"default":4}},"additionalProperties":false}]});
    read["resourceTypes"] = selectors;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_pages_bind_filters_and_traverse_to_null() {
        let context = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let mut history = Diagnostics::default();
        for _ in 0..3 {
            history.push(
                context,
                None,
                DiagnosticKind::Response,
                json!({"method":"POST","status":409,"url":"https://example.com/api"}),
                0,
            );
        }
        let mut query = Query {
            limit: 1,
            ..Default::default()
        };
        let first = history.page(context, &query).unwrap();
        assert_eq!(first["events"].as_array().unwrap().len(), 1);
        query.cursor = first["nextCursor"].as_str().map(str::to_owned);
        query.method = Some("GET".into());
        assert_eq!(history.page(context, &query), Err("invalid_cursor"));
        query.method = None;
        query.limit = 100;
        let last = history.page(context, &query).unwrap();
        assert_eq!(last["events"].as_array().unwrap().len(), 2);
        assert!(last["nextCursor"].is_null());
    }
    #[test]
    fn latest_mutation_filters_stay_inside_the_observed_window_and_advance_to_its_end() {
        let context = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let page = Reference {
            id: Uuid::new_v4(),
            ..context
        };
        let other = Reference {
            id: Uuid::new_v4(),
            ..context
        };
        let first = Uuid::new_v4();
        let last = Uuid::new_v4();
        let future = Uuid::new_v4();
        let mut history = Diagnostics::default();
        for (page, method, id) in [
            (page, "POST", first),
            (other, "POST", Uuid::new_v4()),
            (page, "GET", Uuid::new_v4()),
            (page, "POST", last),
        ] {
            history.push(context,Some(page),DiagnosticKind::Response,json!({"requestId":id,"method":method,"status":409.0,"url":"https://example.com/api"}),0);
        }
        let through = history.sequence;
        history.push(context,Some(page),DiagnosticKind::Response,json!({"requestId":future,"method":"POST","status":409,"url":"https://example.com/api"}),0);
        let mut query = Query {
            after: Some(0),
            through: Some(through),
            latest: true,
            page: Some(page),
            kind: Some(DiagnosticKind::Response),
            method: Some("POST".into()),
            status: Some(409),
            url: Some("https://example.com/api".into()),
            ..Default::default()
        };
        let read = history.read(context, &query).unwrap();
        assert_eq!(read["events"].as_array().unwrap().len(), 1);
        assert_eq!(read["events"][0]["data"]["requestId"], json!(last));
        assert_eq!(read["nextCursor"], through);
        assert_eq!(read["hasMore"], false);
        query.through = None;
        assert_eq!(
            history.read(context, &query).unwrap()["events"][0]["data"]["requestId"],
            json!(future)
        );
        query.through = Some(through);
        query.after = Some(through);
        let empty = history.read(context, &query).unwrap();
        assert!(empty["events"].as_array().unwrap().is_empty());
        assert_eq!(empty["nextCursor"], through);
        query.through = Some(history.sequence + 1);
        assert_eq!(
            history.read(context, &query).unwrap_err(),
            "INVALID_DIAGNOSTIC_CURSOR"
        );
        query.through = None;
        query.after = None;
        query.sequence = Some(1);
        assert_eq!(
            history.read(context, &query).unwrap_err(),
            "DIAGNOSTIC_QUERY_BOUNDS"
        );
        query.sequence = None;
        query.url = Some("https://example.com/api?token=Acme-private-token".into());
        assert_eq!(
            history.read(context, &query).unwrap_err(),
            "DIAGNOSTIC_QUERY_BOUNDS"
        );
    }
    #[test]
    fn typed_conditions_refuse_private_url_matching_and_keep_exact_event_identity() {
        let id = Uuid::new_v4();
        let condition = EventCondition {
            event: DiagnosticKind::Response,
            page: None,
            request_id: Some(id),
            web_socket_id: None,
            method: Some("POST".into()),
            status: Some(409),
            url: Some("https://example.com/api".into()),
        };
        condition.validate().unwrap();
        let event = json!({"kind":"response","data":{"requestId":id,"method":"POST","status":409,"url":"https://example.com/api"}});
        assert!(condition.matches(&event));
        let mut other = event.clone();
        other["data"]["requestId"] = json!(Uuid::new_v4());
        assert!(!condition.matches(&other));
        for url in [
            "https://example.com/api?secret=Acme",
            "https://user:Acme@example.com/api",
            "data:text/plain,Acme",
        ] {
            assert!(
                EventCondition {
                    url: Some(url.into()),
                    ..condition.clone()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            EventCondition {
                event: DiagnosticKind::Request,
                ..condition
            }
            .validate()
            .is_err()
        );
    }
    fn reference() -> Reference {
        Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }
    }
    #[test]
    fn metadata_selectors_keep_worker_identity_privacy_and_cursor_progress() {
        let mut history = Diagnostics::default();
        let context = reference();
        let worker = reference();
        let other = reference();
        history.push(
            context,
            None,
            DiagnosticKind::Console,
            json!({"worker":worker,"level":"error","text":"Acme private failure"}),
            0,
        );
        history.push(
            context,
            None,
            DiagnosticKind::Console,
            json!({"worker":other,"level":"error","text":"Acme other failure"}),
            0,
        );
        history.push(
            context,
            None,
            DiagnosticKind::Request,
            json!({"resourceType":"Fetch"}),
            0,
        );
        history.push(
            context,
            None,
            DiagnosticKind::Response,
            json!({"resourceType":"fetch"}),
            0,
        );
        let read = history
            .read(
                context,
                &Query {
                    worker: Some(worker),
                    levels: vec!["ERROR".into()],
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(read["events"].as_array().unwrap().len(), 1);
        assert_eq!(read["events"][0]["sequence"], 1);
        assert_eq!(read["nextCursor"], 4);
        assert!(!read.to_string().contains("Acme private"));
        let read = history
            .read(
                context,
                &Query {
                    resource_types: vec!["FETCH".into()],
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(read["events"].as_array().unwrap().len(), 2);
        assert_eq!(read["events"][0]["sequence"], 3);
        assert_eq!(
            history
                .read(
                    context,
                    &Query {
                        sequence: Some(2),
                        worker: Some(worker),
                        ..Default::default()
                    }
                )
                .unwrap_err(),
            "DIAGNOSTIC_EVENT_NOT_FOUND"
        );
        for query in [
            Query {
                levels: vec!["log".into(); 33],
                ..Default::default()
            },
            Query {
                resource_types: vec!["".into()],
                ..Default::default()
            },
            Query {
                levels: vec!["Acme\n".into()],
                ..Default::default()
            },
        ] {
            assert_eq!(
                history.read(context, &query).unwrap_err(),
                "DIAGNOSTIC_QUERY_BOUNDS"
            );
        }
    }

    #[test]
    fn exact_sequence_lookup_never_substitutes_a_later_or_foreign_event() {
        let mut history = Diagnostics::default();
        let context = reference();
        let other = reference();
        history.push(
            context,
            None,
            DiagnosticKind::Console,
            json!({"text":"Acme first"}),
            0,
        );
        history.push(
            other,
            None,
            DiagnosticKind::Console,
            json!({"text":"Acme foreign"}),
            0,
        );
        history.push(
            context,
            None,
            DiagnosticKind::PageError,
            json!({"text":"Acme later"}),
            0,
        );
        let query = Query {
            sequence: Some(1),
            ..Default::default()
        };
        let read = history.read(context, &query).unwrap();
        assert_eq!(read["events"].as_array().unwrap().len(), 1);
        assert_eq!(read["events"][0]["sequence"], 1);
        assert_eq!(read["hasMore"], false);
        assert!(!read.to_string().contains("Acme first"));
        for query in [
            Query {
                sequence: Some(2),
                ..Default::default()
            },
            Query {
                sequence: Some(4),
                ..Default::default()
            },
            Query {
                sequence: Some(1),
                kind: Some(DiagnosticKind::PageError),
                ..Default::default()
            },
        ] {
            assert_eq!(
                history.read(context, &query).unwrap_err(),
                "DIAGNOSTIC_EVENT_NOT_FOUND"
            );
        }
        for query in [
            Query {
                sequence: Some(0),
                ..Default::default()
            },
            Query {
                sequence: Some(1),
                after: Some(0),
                ..Default::default()
            },
        ] {
            assert_eq!(
                history.read(context, &query).unwrap_err(),
                "DIAGNOSTIC_QUERY_BOUNDS"
            );
        }
        for _ in 0..MAX_RECORDS {
            history.push(context, None, DiagnosticKind::Console, json!({}), 0);
        }
        assert_eq!(
            history.read(context, &query).unwrap_err(),
            "DIAGNOSTIC_CURSOR_EXPIRED"
        );
    }

    #[test]
    fn request_identity_and_partial_subscriptions_retain_their_limits() {
        let mut objects = super::super::objects::ObjectTable::default();
        let root = objects.lookup("", "Root").unwrap();
        let wire = objects.create(&root, "request", "Request").unwrap();
        let mut history = Diagnostics::default();
        let (id, missing) = history.request(wire.clone(), false);
        assert!(missing);
        assert_eq!(history.request(wire.clone(), false), (id, true));
        assert_eq!(history.request(wire.clone(), true), (id, false));
        history.forget_object(&wire);
        assert!(history.known_request(&wire).is_none());
        objects.dispose(&wire).unwrap();
        let wire = objects.create(&root, "request", "Request").unwrap();
        assert_ne!(history.request(wire, true).0, id);
        let context = reference();
        history.begin_subscription(context, true);
        assert!(history.active(context));
        assert_eq!(
            history.read(context, &Query::default()).unwrap()["subscriptionPending"],
            true
        );
        history.enable(context, true);
        assert_eq!(
            history.read(context, &Query::default()).unwrap()["subscriptionPending"],
            false
        );
        history.begin_subscription(context, false);
        assert!(history.active(context));
        history.enable(context, false);
        assert!(!history.active(context));
        history.close();
        assert!(history.requests.is_empty());
    }
    #[test]
    fn history_is_bounded_and_expired_cursors_are_explicit() {
        let mut history = Diagnostics::default();
        let context = reference();
        for _ in 0..MAX_RECORDS + 88 {
            history.push(
                context,
                None,
                DiagnosticKind::Console,
                json!({"text":"Acme-private-message","level":"error"}),
                0,
            );
        }
        assert_eq!(history.events.len(), MAX_RECORDS);
        assert_eq!(
            history
                .read(
                    context,
                    &Query {
                        after: Some(0),
                        ..Default::default()
                    }
                )
                .unwrap_err(),
            "DIAGNOSTIC_CURSOR_EXPIRED"
        );
        let read = history
            .read(
                context,
                &Query {
                    limit: 2,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(read["events"].as_array().unwrap().len(), 2);
        assert_eq!(read["hasMore"], true);
        assert!(!read.to_string().contains("Acme-private-message"));
        assert_eq!(read["dropped"], 88);
        let read = history
            .read(
                context,
                &Query {
                    limit: 1,
                    include_text: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(read["events"][0]["data"]["text"], "Acme-private-message");
        assert!(history.bytes <= MAX_BYTES);
    }
    #[test]
    fn filters_do_not_cross_contexts_and_empty_queries_advance_the_cursor() {
        let mut history = Diagnostics::default();
        let context = reference();
        let other = reference();
        history.push(
            context,
            None,
            DiagnosticKind::Console,
            json!({"level":"log"}),
            0,
        );
        history.push(
            other,
            None,
            DiagnosticKind::PageError,
            json!({"text":"Acme"}),
            0,
        );
        let read = history
            .read(
                context,
                &Query {
                    kind: Some(DiagnosticKind::PageError),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(read["events"], json!([]));
        assert_eq!(read["nextCursor"], 2);
        for (input, expected) in [
            (
                "https://name:secret@example.com/api?token=secret#secret",
                "https://example.com/api",
            ),
            ("data:text/plain,secret", "[non-network URL]"),
            ("https://example.com/path", "https://example.com/path"),
        ] {
            assert_eq!(url(input), expected);
        }
    }
}
