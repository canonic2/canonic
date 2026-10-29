//! Shared durable provenance, links, reasons and bounded review objects.
mod approval;
use super::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn nullable<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    input: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(input)
}

#[derive(Clone)]
pub(crate) struct Context {
    pub(crate) project_id: String,
    pub(crate) project_root: String,
    pub(crate) surface: String,
    /// The enrolled agent, which selects the review adapter. A registered
    /// connection sets it only for an agent whose integration is recorded.
    pub(crate) agent: Option<String>,
    /// A registered connection's declared agent whose integration is not
    /// recorded: attribution only, never adapter selection.
    pub(crate) declared_agent: Option<String>,
    /// The MCP client's self-reported name: attribution when no agent is
    /// enrolled or declared, never adapter selection.
    pub(crate) client: Option<String>,
    pub(crate) host_version: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) tool_use_id: Option<String>,
    pub(crate) connection_id: Option<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) source: String,
    pub(crate) host_event_name: Option<String>,
    pub(crate) occurred_at: Option<String>,
    /// Reads registry sources for operations in this context.
    pub(crate) fetcher: canonic_registry::acquisition::Fetcher,
}
impl Context {
    pub(crate) fn new(project_id: String, project_root: String, surface: &str) -> Self {
        Self {
            project_id,
            project_root,
            surface: surface.into(),
            agent: None,
            declared_agent: None,
            client: None,
            host_version: None,
            session_id: None,
            tool_use_id: None,
            connection_id: None,
            cwd: None,
            source: "caller".into(),
            host_event_name: None,
            occurred_at: None,
            fetcher: crate::review::registries::fetcher(),
        }
    }
    /// The agent named in records: the enrolled or declared agent, or the
    /// client's own name.
    fn attributed_agent(&self) -> Option<&String> {
        self.agent
            .as_ref()
            .or(self.declared_agent.as_ref())
            .or(self.client.as_ref())
    }
    pub(crate) fn provenance(&self) -> Value {
        let mut value = json!({"project_id":self.project_id,"project_root":self.project_root,
            "cwd":self.cwd,"surface":self.surface,"agent":self.attributed_agent(),"host_version":self.host_version,
            "session_id":self.session_id,"tool_use_id":self.tool_use_id,"connection_id":self.connection_id,"host_event_name":self.host_event_name});
        let mut sources = serde_json::Map::new();
        for (key, field) in value.as_object().into_iter().flatten() {
            let source = if field.is_null() {
                "unavailable"
            } else {
                match key.as_str() {
                    "project_id" | "project_root" | "surface" => "registration",
                    "agent" if self.surface == "hook" => "registration",
                    "connection_id" => "shield",
                    _ => &self.source,
                }
            };
            sources.insert(key.clone(), json!(source));
        }
        value["sources"] = json!(sources);
        value
    }
    pub(crate) fn retry_owner(&self) -> String {
        json!([
            self.attributed_agent(),
            self.session_id.as_ref().or(self.connection_id.as_ref()),
            self.surface
        ])
        .to_string()
    }
}

/// Host-reported completion identity and status; owned child exits use a separate path.
#[derive(Serialize)]
pub(crate) struct EventInput {
    pub(crate) delivery_id: String,
    pub(crate) request_id: Option<String>,
    pub(crate) status: String,
}
impl EventInput {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.delivery_id.is_empty()
            || self.delivery_id.len() > 128
            || self.request_id.as_ref().is_some_and(|id| id.len() > 128)
            || !matches!(
                self.status.as_str(),
                "completed" | "failed" | "cancelled" | "unknown"
            )
        {
            return Err(Error::Invalid("invalid completion evidence"));
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Proposal {
    #[serde(rename = "proposal_id")]
    pub(crate) id: String,
    pub(crate) created_at: String,
    pub(crate) status: String,
    pub(crate) provenance: Value,
    pub(crate) links: Value,
    pub(crate) reasons: Value,
    pub(crate) policy_revision: String,
    pub(crate) target_scope: String,
    pub(crate) operation: String,
    pub(crate) rule_id: String,
    pub(crate) candidate: Value,
    pub(crate) base_candidate: Value,
    pub(crate) base_metadata: Value,
    pub(crate) candidate_metadata: Value,
    pub(crate) receipt: Value,
    pub(crate) comparison: Value,
    #[serde(deserialize_with = "nullable")]
    pub(crate) operator: Option<Value>,
    pub(crate) retry_key: String,
    pub(crate) retry_owner: String,
    pub(crate) signature: String,
}
impl Proposal {
    pub(crate) fn apply_arguments(&self) -> Value {
        let mut arguments = json!({"proposal_id":self.id,
            "candidate_digest":crate::policy::fingerprint(self.candidate.to_string().as_bytes()),
            "changes":crate::review::changes::diff::between(&self.base_metadata, &self.base_candidate, &self.candidate_metadata, &self.candidate),
            "review":approval::summary(self)});
        // Returned and validated arguments share the public projection; the
        // digest still binds the original immutable candidate committed on apply.
        redact(&mut arguments);
        arguments
    }
    pub(crate) fn summary(&self) -> Value {
        let mut value = self.public(false);
        if let Some(object) = value.as_object_mut() {
            object.remove("rules");
            object.remove("comparison");
            object.remove("apply_arguments");
        }
        value
    }
    pub(crate) fn public(&self, reveal: bool) -> Value {
        let mut value = json!({"schema_version":1,"proposal_id":self.id,"created_at":self.created_at,
            "status":self.status,"provenance":self.provenance,"links":self.links,"reasons":self.reasons,
            "policy_revision":self.policy_revision,"target_scope":self.target_scope,"operation":self.operation,
            "rule_id":self.rule_id,"comparison":self.comparison,"operator":self.operator,
            "candidate_digest":crate::policy::fingerprint(self.candidate.to_string().as_bytes())});
        value["apply_arguments"] = self.apply_arguments();
        if reveal {
            value["candidate"] = self.candidate.clone();
        } else if let Some(source) = self.candidate["rules"].as_str() {
            value["rules"] = serde_saphyr::from_str::<Value>(source).unwrap_or(Value::Null);
        }
        if reveal {
            value["redacted"] = json!(false);
        } else {
            let changed = redact(&mut value);
            value["redacted"] = json!(changed);
        }
        value
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "persisted settings and per-read history flags, each independent"
)]
pub(crate) struct State {
    pub(crate) history_generation: String,
    #[serde(skip, default = "history_available")]
    pub(crate) history_available: bool,
    #[serde(skip)]
    pub(crate) pending_history: Vec<Value>,
    #[serde(skip)]
    pub(crate) history_rewrite: Option<bool>,
    #[serde(skip)]
    pub(crate) history_control: bool,
    /// The reader holds fewer requests than the limit because older segments
    /// were evicted by the byte cap.
    #[serde(skip)]
    pub(crate) history_evicted: bool,
    pub(crate) operator: Value,
    pub(crate) schema_version: u8,
    pub(crate) project_id: String,
    pub(crate) project_root: String,
    pub(crate) sequence: u64,
    pub(crate) policy_revision: String,
    pub(crate) candidates: BTreeMap<String, Value>,
    pub(crate) registry_checkpoints: BTreeMap<String, Value>,
    #[serde(deserialize_with = "nullable")]
    pub(crate) active: Option<super::runtime::Active>,
    pub(crate) read_requests: Vec<super::read_approval::ReadRequest>,
    pub(crate) run_requests: Vec<super::run_approval::RunRequest>,
    pub(crate) proposals: Vec<Proposal>,
    /// Advisory session review suppression, never execution authorization.
    #[serde(default)]
    pub(crate) dismissed_reviews: Vec<String>,
    pub(crate) events: Vec<Value>,
    pub(crate) authorization_events: Vec<Value>,
    pub(crate) history_enabled: bool,
    pub(crate) history_limit: usize,
}
impl State {
    pub(crate) fn check(&self, context: &Context) -> Result<(), Error> {
        if !self.operator.is_object()
            || !self.operator["sources"].is_object()
            || self.operator["installed"]
                .as_array()
                .is_none_or(|rows| rows.len() > 64)
            || (!self.operator["default"].is_null() && !self.operator["default"].is_string())
        {
            return Err(Error::Invalid("invalid operator configuration"));
        }
        if self.proposals.iter().any(|proposal| {
            [
                &proposal.base_candidate,
                &proposal.candidate,
                &proposal.base_metadata,
                &proposal.candidate_metadata,
            ]
            .iter()
            .any(|value| !value.is_object())
        }) {
            return Err(Error::Invalid("incomplete proposal capture"));
        }
        if self.schema_version != 1
            || self.project_id != context.project_id
            || self.project_root != context.project_root
            || self.proposals.len() > 1024
            || self.dismissed_reviews.len() > 1024
            || self
                .dismissed_reviews
                .iter()
                .any(|id| id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()))
            || self.read_requests.len() > 128
            || self.run_requests.len() > 128
            || self.events.len() > 40000
            || !(1..=10000).contains(&self.history_limit)
        {
            return Err(Error::Invalid("invalid project state"));
        }
        Ok(())
    }
    pub(crate) fn id(&mut self, kind: &str) -> String {
        self.sequence += 1;
        format!("{kind}-{}-{}", &self.project_id[..16], self.sequence)
    }
    pub(crate) fn event(
        &mut self,
        kind: &str,
        context: &Context,
        links: Value,
        reasons: Value,
        details: Value,
    ) -> Value {
        let mut all_links = json!({"request_id":null,"proposal_id":null,"approval_id":null,
            "source_entry_id":null,"decision_event_id":null,"supersedes_proposal_id":null});
        if let Value::Object(fields) = links {
            for (k, v) in fields {
                all_links[&k] = v;
            }
        }
        let mut all_reasons =
            json!({"agent_reason":null,"policy_reason":null,"operator_reason":null});
        if let Value::Object(fields) = reasons {
            for (k, v) in fields {
                all_reasons[&k] = v;
            }
        }
        let mut value = json!({"schema_version":1,"event_id":self.id("event"),"kind":kind,"recorded_at":now(),"occurred_at":context.occurred_at,
            "provenance":context.provenance(),"links":all_links,"reasons":all_reasons,"operator":null});
        value["details"] = details;
        let changed = redact(&mut value);
        value["redacted"] = json!(changed);
        value
    }
    pub(crate) fn record(&mut self, event: Value) {
        if event["kind"].as_str().is_some_and(|kind| {
            [
                "proposal_rejected",
                "runtime_activated",
                "compiled_cache_prepared",
                "read_grant_issued",
                "read_grant_consumed",
                "read_grant_revoked",
                "run_grant_issued",
                "run_grant_consumed",
                "run_grant_revoked",
            ]
            .contains(&kind)
        }) {
            self.authorization_events.push(event.clone());
        }
        if self.history_enabled {
            let event = super::storage::history::bound_event(event);
            self.pending_history.push(event.clone());
            self.events.push(event);
            self.trim();
        }
    }
    pub(crate) fn trim(&mut self) {
        let fields: Vec<_> = self
            .events
            .iter()
            .map(|e| {
                (
                    e["kind"].as_str().unwrap_or_default(),
                    e["links"]["request_id"].as_str(),
                )
            })
            .collect();
        let retained = retained_events(&fields, self.history_limit);
        let mut index = 0;
        self.events.retain(|_| {
            let keep = retained[index];
            index += 1;
            keep
        });
    }

    pub(crate) fn compact_authorization(&mut self) {
        let time = super::clock::best_effort_ms();
        self.compact_authorization_at(time);
    }
    pub(crate) fn compact_authorization_at(&mut self, time: u64) {
        self.read_requests.retain(|r| r.live_id(time).is_some());
        self.run_requests.retain(|r| r.live_id(time).is_some());
        let ids: std::collections::BTreeSet<_> = self
            .read_requests
            .iter()
            .filter_map(|r| r.live_id(time))
            .chain(self.run_requests.iter().filter_map(|r| r.live_id(time)))
            .collect();
        self.authorization_events.retain(|event| {
            event["links"]["request_id"]
                .as_str()
                .is_some_and(|id| ids.contains(id))
                || self
                    .active
                    .as_ref()
                    .is_some_and(|a| event["links"]["activation_id"] == a.activation_id())
        });
    }
}

/// Shared retention over small correlation fields, without copying event payloads.
pub(in crate::review) fn retained_events(
    fields: &[(&str, Option<&str>)],
    limit: usize,
) -> Vec<bool> {
    use std::collections::BTreeSet;
    let mut seen = BTreeSet::new();
    let original: Vec<_> = fields
        .iter()
        .filter_map(|(kind, request)| request.filter(|id| *kind == "decision" && seen.insert(*id)))
        .collect();
    let ids: BTreeSet<_> = original.into_iter().rev().take(limit).collect();
    let mut uncorrelated = 0;
    let mut keep: Vec<_> = fields
        .iter()
        .map(|(kind, request)| {
            request.is_some_and(|id| ids.contains(id))
                || (*kind == "completion" && request.is_none())
        })
        .collect();
    for (index, (_, request)) in fields.iter().enumerate().rev() {
        if keep[index] && request.is_none() {
            uncorrelated += 1;
            keep[index] = uncorrelated <= 128;
        }
    }
    let mut count = keep.iter().filter(|v| **v).count();
    while count > 40000 {
        let oldest = fields
            .iter()
            .zip(&keep)
            .find(|(_, kept)| **kept)
            .map(|((_, id), _)| *id);
        for ((_, id), retained) in fields.iter().zip(&mut keep) {
            if *retained && Some(*id) == oldest {
                *retained = false;
                count -= 1;
            }
        }
    }
    keep
}

fn history_available() -> bool {
    true
}

pub(crate) use crate::evidence::redact;

pub(crate) use crate::evidence::now;
