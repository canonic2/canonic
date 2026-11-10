//! Bounded terminal action outcomes. These receipts contain no grant and every
//! consumer replay refuses execution, independently of optional request history.
use super::{Context, Error};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const PER_KIND: usize = 128;
const BYTES: usize = 1024;
const RETENTION_MS: u64 = 600_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Read,
    Execution,
    Hook,
}
impl Kind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Read => "read-request-",
            Self::Execution => "run-request-",
            Self::Hook => "hook-request-",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Decision {
    Approve,
    Deny,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Consumed,
    Revoked,
}
impl Status {
    fn name(self) -> &'static str {
        match self {
            Self::Consumed => "consumed",
            Self::Revoked => "revoked",
        }
    }
    fn code(self) -> &'static str {
        match self {
            Self::Consumed => "approval-consumed",
            Self::Revoked => "approval-revoked",
        }
    }
}

/// Exact current consumer evidence, supplied by its owning grant implementation.
#[derive(Clone, Copy)]
pub(crate) struct Claim<'a> {
    pub(crate) kind: Kind,
    pub(crate) id: &'a str,
    pub(crate) project: &'a str,
    pub(crate) agent: Option<&'a str>,
    pub(crate) session: Option<&'a str>,
    pub(crate) consumer: &'a str,
    pub(crate) binding: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) struct Granted<'a> {
    pub(crate) id: &'a str,
    pub(crate) issued: u64,
    pub(crate) expires: u64,
}

/// A terminal transition is captured before its full request is compacted.
#[derive(Clone, Copy)]
pub(crate) struct Capture<'a> {
    pub(crate) claim: Claim<'a>,
    pub(crate) decision: Decision,
    pub(crate) status: Status,
    pub(crate) grant: Option<Granted<'a>>,
    pub(crate) reason: Option<&'a str>,
    pub(crate) at: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct OperatorReplay<'a> {
    pub(crate) context: &'a Context,
    pub(crate) id: &'a str,
    pub(crate) decision: Decision,
    pub(crate) ttl_seconds: Option<u64>,
    pub(crate) reason: Option<&'a str>,
    pub(crate) at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Registry {
    records: Vec<Receipt>,
    /// Bounded by the enclosing checkpoint; never inspected as authority.
    #[serde(
        default,
        deserialize_with = "opaque",
        skip_serializing_if = "Option::is_none"
    )]
    unavailable: Option<Value>,
}

fn opaque<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    approval: String,
    issued: u64,
    expires: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: u8,
    kind: Kind,
    id: String,
    project: String,
    consumer: String,
    owner: String,
    session_known: bool,
    decision: Decision,
    status: Status,
    grant: Option<Grant>,
    reason: Option<String>,
    at: u64,
    until: u64,
    seal: String,
}

fn hash(value: &impl Serialize) -> String {
    crate::policy::fingerprint(&serde_json::to_vec(value).expect("typed receipt serializes"))
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn owner(project: &str, agent: Option<&str>, session: Option<&str>) -> String {
    hash(&(project, agent, session))
}

impl Claim<'_> {
    fn valid(self) -> bool {
        let text = |value: &str, max: usize| {
            !value.is_empty() && value.len() <= max && !value.contains('\0')
        };
        self.id.starts_with(self.kind.prefix())
            && text(self.id, 128)
            && valid_hash(self.project)
            && valid_hash(self.binding)
            && text(self.consumer, 128)
            && self.agent.is_none_or(|agent| text(agent, 128))
            && self.session.is_none_or(|session| text(session, 4096))
    }
    fn digest(self) -> String {
        hash(&(
            self.kind,
            self.id,
            self.project,
            self.agent,
            self.session,
            self.consumer,
            self.binding,
        ))
    }
    fn owner(self) -> String {
        let agent = if self.kind == Kind::Read {
            super::approvals::read_operator_agent(self.agent)
        } else {
            self.agent
        };
        owner(self.project, agent, self.session)
    }
}

impl Receipt {
    fn seal(&self) -> String {
        hash(&(
            self.schema,
            self.kind,
            &self.id,
            &self.project,
            &self.consumer,
            &self.owner,
            self.session_known,
            self.decision,
            self.status,
            &self.grant,
            &self.reason,
            self.at,
            self.until,
        ))
    }
    fn valid(&self) -> bool {
        self.schema == 1
            && self.id.starts_with(self.kind.prefix())
            && self.id.len() <= 128
            && !self.id.contains('\0')
            && valid_hash(&self.project)
            && valid_hash(&self.consumer)
            && valid_hash(&self.owner)
            && self.reason.as_deref().is_none_or(valid_hash)
            && self.until.checked_sub(self.at) == Some(RETENTION_MS)
            && match (self.decision, self.status) {
                (Decision::Approve, Status::Consumed) => self.grant.is_some(),
                (Decision::Deny, Status::Revoked) => true,
                _ => false,
            }
            && self.grant.as_ref().is_none_or(|grant| {
                valid_hash(&grant.approval)
                    && grant
                        .expires
                        .checked_sub(grant.issued)
                        .is_some_and(|ttl| (1000..=3_600_000).contains(&ttl) && ttl % 1000 == 0)
                    && (self.status != Status::Consumed
                        || (self.at >= grant.issued && self.at < grant.expires))
            })
            && self.seal == self.seal()
            && crate::json::encoded_len(self).is_ok_and(|bytes| bytes <= BYTES)
    }
    fn owns(&self, context: &Context) -> bool {
        self.project == context.project_id
            && (context.surface == "cli"
                || (context.surface == "mcp"
                    && context.agent.is_some()
                    && self.owner
                        == owner(
                            &context.project_id,
                            context.agent.as_deref(),
                            context.session_id.as_deref(),
                        )))
    }
    fn summary(&self) -> Value {
        json!({"request_id":self.id,"status":self.status.name(),"terminal":true,"allowed":false,
            "replayed":true,"next_action":"This request is terminal. Its recorded outcome grants no permission."})
    }
}

impl Registry {
    /// Optional historical outcomes cannot invalidate active authorization.
    /// Malformed receipts become unavailable; they never regain authority.
    pub(crate) fn deserialize_optional<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        match serde_json::from_value::<Self>(value.clone()) {
            Ok(registry) if registry.validate().is_ok() => Ok(registry),
            _ => {
                crate::diagnostics::trace::count("state.invalid_terminal_receipts", 1);
                Ok(Self {
                    records: vec![],
                    unavailable: Some(value),
                })
            }
        }
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.records.is_empty() && self.unavailable.is_none()
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        let mut ids = BTreeSet::new();
        if self.records.len() > 3 * PER_KIND
            || self
                .records
                .iter()
                .any(|row| !row.valid() || !ids.insert(&row.id))
            || [Kind::Read, Kind::Execution, Kind::Hook]
                .iter()
                .any(|kind| self.records.iter().filter(|row| row.kind == *kind).count() > PER_KIND)
        {
            return Err(Error::Invalid("invalid terminal authorization receipts"));
        }
        Ok(())
    }

    pub(crate) fn compact(&mut self, project: &str, at: u64) {
        self.records
            .retain(|row| row.project == project && at < row.until);
    }

    pub(crate) fn capture(&mut self, input: Capture<'_>) -> Result<(), Error> {
        if !input.claim.valid() {
            return Err(Error::Invalid("invalid terminal request binding"));
        }
        let mut receipt = Receipt {
            schema: 1,
            kind: input.claim.kind,
            id: input.claim.id.into(),
            project: input.claim.project.into(),
            consumer: input.claim.digest(),
            owner: input.claim.owner(),
            session_known: input.claim.session.is_some(),
            decision: input.decision,
            status: input.status,
            grant: input.grant.map(|grant| Grant {
                approval: hash(&grant.id),
                issued: grant.issued,
                expires: grant.expires,
            }),
            reason: input.reason.map(|reason| hash(&reason)),
            at: input.at,
            until: input
                .at
                .checked_add(RETENTION_MS)
                .ok_or(Error::Invalid("terminal receipt time overflow"))?,
            seal: String::new(),
        };
        receipt.seal = receipt.seal();
        if !receipt.valid() {
            return Err(Error::Invalid("invalid terminal authorization receipt"));
        }
        if let Some(previous) = self.records.iter().find(|row| row.id == receipt.id) {
            // Repeated delivery preserves the original terminal time and lifetime.
            receipt.at = previous.at;
            receipt.until = previous.until;
            receipt.seal = receipt.seal();
            return if &receipt == previous {
                Ok(())
            } else {
                Err(Error::Invalid("terminal request outcome differs"))
            };
        }
        self.compact(input.claim.project, input.at);
        let kind = receipt.kind;
        self.records.push(receipt);
        if self.records.iter().filter(|row| row.kind == kind).count() > PER_KIND
            && let Some(index) = self
                .records
                .iter()
                .enumerate()
                .filter(|(_, row)| row.kind == kind)
                .min_by_key(|(_, row)| row.at)
                .map(|(index, _)| index)
        {
            self.records.remove(index);
        }
        Ok(())
    }

    fn record(&self, project: &str, id: &str, at: u64) -> Option<&Receipt> {
        self.records.iter().find(|row| {
            row.project == project && row.id == id && at >= row.at && at < row.until && row.valid()
        })
    }

    pub(crate) fn inspect(&self, context: &Context, id: &str, at: u64) -> Option<(Kind, Value)> {
        let row = self.record(&context.project_id, id, at)?;
        row.owns(context).then(|| (row.kind, row.summary()))
    }

    pub(crate) fn replay_operator(
        &self,
        input: OperatorReplay<'_>,
    ) -> Result<Option<Value>, Error> {
        let Some(row) = self.record(&input.context.project_id, input.id, input.at) else {
            return Ok(None);
        };
        if !row.owns(input.context) {
            return Err(Error::Missing);
        }
        if row.decision != input.decision
            || row.reason != input.reason.map(|reason| hash(&reason))
            || (input.decision == Decision::Approve
                && row
                    .grant
                    .as_ref()
                    .and_then(|g| g.expires.checked_sub(g.issued))
                    != input.ttl_seconds.and_then(|ttl| ttl.checked_mul(1000)))
        {
            return Err(Error::Invalid("terminal request decision differs"));
        }
        Ok(Some(row.summary()))
    }

    pub(crate) fn replay_approval_group(
        &self,
        context: &Context,
        ids: &[&str],
        at: u64,
    ) -> Result<Option<Vec<Value>>, Error> {
        if ids.is_empty() || ids.len() > 8 {
            return Err(Error::Invalid("request group exceeds bounds"));
        }
        let rows = ids
            .iter()
            .map(|id| self.record(&context.project_id, id, at))
            .collect::<Vec<_>>();
        if rows.iter().flatten().any(|row| !row.owns(context)) {
            return Err(Error::Missing);
        }
        if rows.iter().all(Option::is_none) {
            return Ok(None);
        }
        if rows.iter().any(Option::is_none) {
            return Err(Error::Grouped {
                request: "request_ids".into(),
                reason: "the group mixes terminal and unavailable or live requests",
                recovery: "Call request_approve again with only the remaining live request IDs",
            });
        }
        let rows = rows.into_iter().flatten().collect::<Vec<_>>();
        if rows.len() > 1
            && rows
                .iter()
                .any(|row| !row.session_known || row.owner != rows[0].owner)
        {
            return Err(Error::Grouped {
                request: "request_ids".into(),
                reason: "terminal outcomes belong to different or unidentified sessions",
                recovery: "Inspect each terminal request separately",
            });
        }
        rows.iter()
            .map(|row| {
                self.replay_operator(OperatorReplay {
                    context,
                    id: &row.id,
                    decision: Decision::Approve,
                    ttl_seconds: Some(300),
                    reason: None,
                    at,
                })
                .and_then(|value| value.ok_or(Error::Missing))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }

    pub(crate) fn replay_consumer(&self, claim: Claim<'_>, at: u64) -> Option<Value> {
        if !claim.valid() {
            return None;
        }
        let row = self.record(claim.project, claim.id, at)?;
        let matched = row.kind == claim.kind && row.consumer == claim.digest();
        Some(
            json!({"code":if matched {row.status.code()} else {"approval-stale"},"allowed":false,
            "approval":{"request_id":claim.id,"status":if matched {row.status.name()} else {"stale"},"approval_id":null}}),
        )
    }
}

#[cfg(test)]
mod tests;
