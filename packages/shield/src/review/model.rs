//! Shared durable provenance, links, reasons and bounded review objects.
mod approval;
pub(in crate::review) mod presentation;
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
    /// The enrolled agent or a bootstrap session's explicitly reviewed agent.
    /// Ordinary launch attribution alone cannot select this field.
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
    /// The transport verified bootstrap consent or its enrollment receipt.
    /// Never accepted as a stored or tool-supplied capability boolean.
    pub(crate) confirmed_mcp_forms: bool,
    pub(crate) cwd: Option<String>,
    pub(crate) source: String,
    pub(crate) host_event_name: Option<String>,
    pub(crate) occurred_at: Option<String>,
    /// Reads registry sources for operations in this context.
    pub(crate) fetcher: canonic_registry_kit::acquisition::Fetcher,
    /// Optional request-local observation, never serialized into provenance.
    pub(crate) progress: super::progress::Progress,
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
            confirmed_mcp_forms: false,
            cwd: None,
            source: "caller".into(),
            host_event_name: None,
            occurred_at: None,
            fetcher: crate::review::registries::fetcher(),
            progress: super::progress::Progress::default(),
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
        let mut value = json!({"project_id":self.project_id,
            "cwd":self.relative_cwd(),"surface":self.surface,"agent":self.attributed_agent(),"host_version":self.host_version,
            "session_id":self.session_id,"tool_use_id":self.tool_use_id,"connection_id":self.connection_id,"host_event_name":self.host_event_name});
        let mut sources = serde_json::Map::new();
        for (key, field) in value.as_object().into_iter().flatten() {
            let source = if field.is_null() {
                "unavailable"
            } else {
                match key.as_str() {
                    "project_id" | "surface" => "registration",
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
    /// The reported working folder relative to the project (`.` for its
    /// root), so recorded evidence never holds where the project is. A folder
    /// outside the project is recorded as reported.
    fn relative_cwd(&self) -> Option<String> {
        let cwd = self.cwd.as_deref()?;
        let relative = std::path::Path::new(cwd)
            .strip_prefix(&self.project_root)
            .ok()
            .filter(|_| !self.project_root.is_empty());
        Some(match relative {
            Some(path) if path.as_os_str().is_empty() => ".".into(),
            Some(path) => path.to_string_lossy().replace('\\', "/"),
            None => cwd.into(),
        })
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

/// A policy file's text when a proposal was prepared, and the text applying
/// it writes; `None` is an absent or removed file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Texts {
    pub(crate) before: Option<String>,
    pub(crate) after: Option<String>,
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
    /// The policy applying this proposal writes and the policy it was prepared
    /// against, rebuilt from `files` by `Store::hydrate_proposal`; never stored.
    #[serde(skip)]
    pub(crate) candidate: Value,
    #[serde(skip)]
    pub(crate) base_candidate: Value,
    /// Those candidates' identities, for inspection without rebuilding them.
    #[serde(default)]
    pub(crate) candidate_digest: String,
    #[serde(default)]
    pub(crate) base_digest: String,
    /// Each policy file the proposal touches: its text when prepared and the
    /// text applying it writes. Digest-named pack artifacts live in the
    /// per-user store instead.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) files: BTreeMap<String, Texts>,
    /// Retained-source references an earlier release stored. A proposal that
    /// has only these is never applied; it is prepared again.
    #[expect(dead_code, reason = "accepts the keys earlier checkpoints wrote")]
    #[serde(default, rename = "candidate", skip_serializing)]
    pub(in crate::review) retired_candidate: Option<serde::de::IgnoredAny>,
    #[expect(dead_code, reason = "accepts the keys earlier checkpoints wrote")]
    #[serde(default, rename = "base_candidate", skip_serializing)]
    pub(in crate::review) retired_base_candidate: Option<serde::de::IgnoredAny>,
    pub(crate) base_metadata: Value,
    pub(crate) candidate_metadata: Value,
    pub(crate) receipt: Value,
    pub(crate) comparison: Value,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub(in crate::review) presentation: Value,
    #[serde(deserialize_with = "nullable")]
    pub(crate) operator: Option<Value>,
    pub(crate) retry_key: String,
    pub(crate) retry_owner: String,
    pub(crate) signature: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reviewed_files: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) interaction: Option<super::proposal_lifecycle::Interaction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) closed_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) reviewed_paths: Option<BTreeMap<String, Option<String>>>,
}
impl Proposal {
    /// The exact arguments applying this proposal takes. The proposal must be
    /// hydrated; the digest binds the files it writes.
    pub(crate) fn apply_arguments(&self) -> Value {
        let mut arguments = json!({"proposal_id":self.id,
            "candidate_digest":self.candidate_digest,
            "changes":crate::review::changes::diff::between(&self.base_metadata, &self.base_candidate, &self.candidate_metadata, &self.candidate),
            "review":approval::summary(self)});
        // Returned and validated arguments share the public projection; the
        // digest still binds the original immutable candidate committed on apply.
        redact(&mut arguments);
        arguments
    }
    pub(crate) fn summary(&self) -> Value {
        let mut value = self.metadata_view();
        let changed = redact(&mut value);
        value["redacted"] = json!(changed);
        value
    }
    fn metadata_view(&self) -> Value {
        json!({"schema_version":1,"proposal_id":self.id,"created_at":self.created_at,"closed_reason":self.closed_reason,
            "status":self.status,"provenance":self.provenance,"links":self.links,"reasons":self.reasons,
            "policy_revision":self.policy_revision,"target_scope":self.target_scope,"operation":self.operation,
            "rule_id":self.rule_id,"operator":self.operator,
            "candidate_digest":self.candidate_digest})
    }
    /// The proposal as inspection shows it. Apply arguments need a hydrated
    /// proposal and appear only while it is pending.
    pub(crate) fn public(&self) -> Value {
        let mut value = self.metadata_view();
        value["comparison"] = self.comparison.clone();
        if self.status == "pending" && !self.candidate.is_null() {
            value["apply_arguments"] = self.apply_arguments();
        }
        if let Some(source) = self.candidate["rules"].as_str() {
            value["rules"] = serde_saphyr::from_str::<Value>(source).unwrap_or(Value::Null);
        }
        let private = redact_comparison(&mut value["comparison"]);
        let changed = redact(&mut value);
        value["redacted"] = json!(changed || private);
        value
    }
}

/// Public comparison identities must not disclose local launcher settings.
/// The protected comparison and human-only connection form keep their source.
pub(crate) fn redact_comparison(value: &mut Value) -> bool {
    let mut changed = false;
    for side in ["before", "after"] {
        let Some(scopes) = value
            .get_mut(side)
            .and_then(|side| side.get_mut("scopes"))
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        for scope in scopes {
            for connection in scope
                .get_mut("lock")
                .and_then(|lock| lock.get_mut("mcp"))
                .and_then(Value::as_object_mut)
                .into_iter()
                .flat_map(|map| map.values_mut())
            {
                if connection["local"] == true && connection.get("transport").is_some() {
                    *connection = json!({"local":true,"settings":"private"});
                    changed = true;
                }
            }
        }
    }
    changed
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "persisted settings and per-read history flags, each independent"
)]
pub(crate) struct State {
    /// An earlier format converted in memory for inspection and not saved;
    /// it authorizes nothing. Never accepted from saved state.
    #[serde(skip)]
    pub(in crate::review) unconverted: bool,
    /// Decoded from a checkpoint an earlier release wrote: it still holds a
    /// policy copy, activation, consent or folder identity records. Runtime
    /// only; the one conversion ([`super::storage`]) rewrites it.
    #[serde(skip)]
    pub(in crate::review) earlier_format: bool,
    /// Digest of the policy files this state was brought up to in memory, by
    /// [`super::files::sync`]. Runtime only: a checkpoint never holds the policy.
    #[serde(skip)]
    pub(in crate::review) followed_files: Option<String>,
    pub(crate) history_generation: String,
    /// Journal ownership survives a checkout epoch change without rewriting
    /// immutable observations. Absent in legacy checkpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) history_owner: Option<String>,
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
    /// The folder this state was read from, found by discovery each time.
    /// Runtime only: a checkpoint never records where its folder is.
    #[serde(skip)]
    pub(crate) project_root: String,
    /// The absolute root earlier formats recorded. Read once to recognize an
    /// installation upgraded in place; never written.
    #[serde(rename = "project_root", default, skip_serializing)]
    pub(in crate::review) recorded_root: Option<String>,
    pub(crate) sequence: u64,
    /// The policy files' revision and captured candidate, filled in memory by
    /// [`super::files::sync`] for workflows that compare against the policy.
    /// Runtime only: the files are the policy, so a checkpoint never holds a
    /// copy. Checkpoint-only reads, such as hooks, leave them empty.
    #[serde(skip)]
    pub(crate) policy_revision: String,
    #[serde(skip)]
    pub(crate) candidates: BTreeMap<String, Value>,
    /// The policy copy earlier releases kept: its revision, candidate
    /// references and the digest of the files it followed. Their presence
    /// marks an earlier format for the one conversion; never written.
    #[serde(default, rename = "policy_revision", skip_serializing)]
    pub(in crate::review) retired_policy_revision: Option<serde::de::IgnoredAny>,
    #[serde(default, rename = "candidates", skip_serializing)]
    pub(in crate::review) retired_candidates: Option<serde::de::IgnoredAny>,
    #[serde(default, rename = "followed_files", skip_serializing)]
    pub(in crate::review) retired_followed_files: Option<serde::de::IgnoredAny>,
    pub(crate) registry_checkpoints: BTreeMap<String, Value>,
    /// The activation record earlier releases kept; dropped by the one
    /// conversion. The policy files are what requests are decided by.
    #[serde(default, rename = "active", skip_serializing)]
    pub(in crate::review) retired_active: Option<serde::de::IgnoredAny>,
    pub(crate) read_requests: Vec<super::read_approval::ReadRequest>,
    pub(crate) run_requests: Vec<super::run_approval::RunRequest>,
    /// Non-authorizing terminal outcomes, independent of live grant capacity/history.
    #[serde(
        default,
        deserialize_with = "super::authorization_receipts::Registry::deserialize_optional",
        skip_serializing_if = "super::authorization_receipts::Registry::is_empty"
    )]
    pub(crate) terminal_receipts: super::authorization_receipts::Registry,
    /// Refused native requests awaiting, or holding, in-session approval.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) hook_requests: Vec<super::hook_approval::HookRequest>,
    /// Reusable, human-confirmed conversation authority; independent of history.
    #[serde(default)]
    pub(crate) session_grants: super::session_grants::Registry,
    pub(crate) proposals: Vec<Proposal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) policy_drafts: Vec<super::proposal_lifecycle::drafts::Draft>,
    /// Advisory session review suppression, never execution authorization.
    #[serde(default)]
    pub(crate) dismissed_reviews: Vec<String>,
    /// Non-authorizing, session-scoped web save offers, independent of history.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) web_reviews: Vec<super::web_reviews::Entry>,
    pub(crate) events: Vec<Value>,
    /// The configuration change log earlier releases kept; dropped by the
    /// one conversion.
    #[serde(default, rename = "configuration_changes", skip_serializing)]
    pub(super) retired_configuration_changes: Option<serde::de::IgnoredAny>,
    pub(crate) authorization_events: Vec<Value>,
    /// The settings in effect, read from the policy files when this state is
    /// loaded ([`super::settings::load`]). Runtime only: a checkpoint never
    /// holds settings, and commands change them by writing the files.
    #[serde(skip)]
    pub(crate) settings: crate::configuration::settings::Values,
    /// The retention limit before a history setting change in this command,
    /// so rewriting history never restores records the old limit expired.
    #[serde(skip)]
    pub(crate) history_previous_limit: Option<usize>,
    /// Settings an earlier checkpoint held, read once to move them into
    /// `config.local.yml`; never written.
    #[serde(default, rename = "history_enabled", skip_serializing)]
    pub(in crate::review) legacy_history_enabled: Option<bool>,
    #[serde(default, rename = "history_limit", skip_serializing)]
    pub(in crate::review) legacy_history_limit: Option<usize>,
    #[serde(default, rename = "enforcement", skip_serializing)]
    pub(in crate::review) legacy_enforcement: Option<super::mode::Mode>,
    /// The operator settings records of an earlier checkpoint, taken out of
    /// `operator` on load ([`super::settings::Legacy`]).
    #[serde(skip)]
    pub(in crate::review) legacy_settings: super::settings::Legacy,
}
impl State {
    /// Check retained diagnostic evidence at the selected checkout without
    /// authorizing its old epoch there. Callers load it through `Store::inspect`;
    /// operation paths must continue to use the strict check below.
    pub(crate) fn check_inspection(&self, context: &Context) -> Result<(), Error> {
        if context.project_root.len() > 4096
            || context.project_root.contains('\0')
            || !std::path::Path::new(&context.project_root).is_absolute()
        {
            return Err(Error::Invalid("invalid diagnostic project root"));
        }
        let mut retained = context.clone();
        retained.project_root.clone_from(&self.project_root);
        self.check(&retained)
    }
    pub(crate) fn policy_lineage(&self) -> &str {
        self.operator["policy_lineage"]
            .as_str()
            .unwrap_or(&self.project_id)
    }
    /// An unsaved in-memory conversion is for inspection and cannot authorize work.
    pub(crate) fn check_operations(&self) -> Result<(), Error> {
        if self.unconverted {
            return Err(Error::Invalid(
                "an unconverted earlier checkpoint cannot authorize operations",
            ));
        }
        Ok(())
    }

    pub(crate) fn check(&self, context: &Context) -> Result<(), Error> {
        if !self.operator.is_object()
            || self.operator["installed"]
                .as_array()
                .is_none_or(|rows| rows.len() > 64)
        {
            return Err(Error::Invalid("invalid operator configuration"));
        }
        // Candidates are rebuilt from a proposal's files on demand; what a
        // checkpoint holds is the metadata and those files.
        if self.proposals.iter().any(|proposal| {
            [&proposal.base_metadata, &proposal.candidate_metadata]
                .iter()
                .any(|value| !value.is_object())
                || (!proposal.files.is_empty() && !proposal.files.contains_key("config.yml"))
        }) {
            return Err(Error::Invalid("incomplete proposal capture"));
        }
        if self.schema_version != 1
            || self.operator.get("policy_lineage").is_some_and(|lineage| {
                lineage.as_str().is_none_or(|lineage| {
                    lineage.len() != 64 || !lineage.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            })
            || self.project_root != context.project_root
            || self.proposals.len() > 1024
            || self.policy_drafts.len() > 128
            || self.dismissed_reviews.len() > 1024
            || self
                .dismissed_reviews
                .iter()
                .any(|id| id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()))
            || self.read_requests.len() > 128
            || self.run_requests.len() > 128
            || self.hook_requests.len() > 128
            || self.events.len() > 40000
        {
            return Err(Error::Invalid("invalid project state"));
        }
        // The same root with another ID is review state recreated since the
        // context opened.
        if self.project_id != context.project_id {
            return Err(Error::StateRecreated);
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
        let shell_credentials = crate::shell::redact_http_history(&mut value["details"]);
        let changed = if kind == "decision" {
            crate::evidence::redact_with_receipts(&mut value, &["/details/authorization".into()])
        } else {
            redact(&mut value)
        };
        value["redacted"] = json!(changed || shell_credentials);
        value
    }
    pub(crate) fn record(&mut self, event: Value) {
        if event["kind"].as_str().is_some_and(|kind| {
            [
                "proposal_rejected",
                "integration_review_confirmed",
                "read_grant_issued",
                "read_grant_consumed",
                "read_grant_revoked",
                "run_grant_issued",
                "run_grant_consumed",
                "run_grant_revoked",
                "hook_grant_issued",
                "hook_grant_consumed",
            ]
            .contains(&kind)
                || (kind == "proposal_created" && event["details"]["presentation"].is_string())
        }) {
            self.authorization_events.push(event.clone());
        }
        if self.settings.history_enabled {
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
        let retained = retained_events(&fields, self.settings.history_limit);
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
        self.terminal_receipts.compact(&self.project_id, time);
        super::session_grants::compact(self, time);
        self.read_requests.retain(|r| r.live_id(time).is_some());
        self.run_requests.retain(|r| r.live_id(time).is_some());
        super::hook_approval::compact_receipts(self, time);
        self.hook_requests.retain(|r| r.retained_id(time).is_some());
        let ids: std::collections::BTreeSet<_> = self
            .read_requests
            .iter()
            .filter_map(|r| r.live_id(time))
            .chain(self.run_requests.iter().filter_map(|r| r.live_id(time)))
            .chain(
                self.hook_requests
                    .iter()
                    .filter_map(|r| r.retained_id(time)),
            )
            .collect();
        self.authorization_events.retain(|event| {
            (event["kind"] == "proposal_created"
                && self.proposals.iter().any(|proposal| {
                    event["links"]["proposal_id"] == proposal.id
                        && proposal.presentation.is_object()
                        && event["details"]["presentation"] == proposal.presentation["digest"]
                }))
                || (event["kind"] == "project_enrolled"
                    && event["provenance"]["project_id"] == self.project_id
                    && event["details"]["bootstrap"] == self.operator["bootstrap"])
                || (event["kind"] == "integration_review_confirmed"
                    && event["provenance"]["project_id"] == self.project_id
                    && self.operator["integration_review"].is_object()
                    && event["details"]["review_channel"] == self.operator["integration_review"])
                || event["links"]["request_id"]
                    .as_str()
                    .is_some_and(|id| ids.contains(id))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_visible_comparisons_keep_local_connection_settings_private() {
        let local = json!({"local":true,"enabled":true,"agents":{"claude":"acme"},
            "transport":{"type":"stdio","command":"/acme/private/connector","args":["--profile","acme-private"],
                "env":{"ACME_REGION":"acme-private-region"}}});
        let shared = json!({"local":false,"enabled":true,"agents":{"claude":"docs"},
            "transport":{"type":"http","url":"https://docs.example.com/mcp"}});
        let side = |connection: &Value| json!({"scopes":[{"lock":{"mcp":{"acme":connection.clone(),"docs":shared.clone()}}}]});
        let mut comparison = json!({"before":side(&local),"after":side(&local)});
        assert!(redact_comparison(&mut comparison));
        for side in ["before", "after"] {
            let mcp = &comparison[side]["scopes"][0]["lock"]["mcp"];
            assert_eq!(mcp["acme"], json!({"local":true,"settings":"private"}));
            // Committed connections are policy everyone reviews in the files.
            assert_eq!(mcp["docs"], shared);
        }
        assert!(!comparison.to_string().contains("private/connector"));
        assert!(!comparison.to_string().contains("acme-private"));
        // A comparison without local settings is left as it is.
        let mut plain = json!({"before":side(&shared),"after":side(&shared)});
        assert!(!redact_comparison(&mut plain));
    }
}
