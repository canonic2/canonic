//! Optional task protocol values and pure lifecycle validation; consumers own storage and execution.

#[cfg(test)]
mod tests;
mod time;

use crate::{Decision, RequestContext, RpcError, ToolResult, modern, rpc};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::Duration;
pub use time::Timestamp;

/// The stable modern Tasks extension identifier.
pub const EXTENSION: &str = "io.modelcontextprotocol/tasks";
/// Maximum UTF-8 bytes in a task identifier or input key.
pub const MAX_ID_BYTES: usize = 128;
/// Maximum UTF-8 bytes in a status message.
pub const MAX_STATUS_BYTES: usize = 4096;
/// Maximum JSON-encoded bytes in a detailed task snapshot.
pub const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;
/// Maximum JSON-encoded bytes in pending input requests.
pub const MAX_INPUT_BYTES: usize = 64 * 1024;
/// Maximum JSON-encoded bytes in an update's response map.
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024;
/// Largest exactly representable JSON integer used for durations.
pub const MAX_MILLIS: u64 = (1 << 53) - 1;

/// Invalid task data or a refused lifecycle change; no I/O or effects have occurred.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TaskError {
    /// A field, shape or numeric bound is invalid.
    #[error("Invalid task {0}.")]
    Invalid(&'static str),
    /// The retained value is larger or deeper than its operating bound.
    #[error("Task value exceeds its size or nesting limit.")]
    Limit,
    /// The proposed change conflicts with immutable identity or terminal state.
    #[error("Task transition conflicts with its current state.")]
    Transition,
    /// The task is past its advertised retention window.
    #[error("Task has expired.")]
    Expired,
    /// The consumer's declared capacity cannot admit more work.
    #[error("Task capacity is full.")]
    Capacity,
}

impl TaskError {
    /// Maps bad client input to invalid parameters. Consumer/storage faults must
    /// instead be reported as internal errors by the adapter.
    #[must_use]
    pub fn invalid_params(&self) -> RpcError {
        RpcError::new(rpc::INVALID_PARAMS, self.to_string())
    }
}

/// Explicit consumer quotas. This declares obligations; it allocates no store or workers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskOptions {
    /// Maximum retained records, including cleanup/replay bookkeeping.
    pub max_records: usize,
    /// Maximum retained encoded bytes, including accepted inputs and bookkeeping.
    pub max_bytes: usize,
    /// Maximum concurrently executing jobs.
    pub max_running: usize,
    /// Maximum input rounds over one task's lifetime.
    pub max_input_rounds: u64,
    /// Consumer-owned execution deadline, separate from RPC deadlines.
    pub execution_timeout: Duration,
    /// Consumer-owned input-wait deadline.
    pub input_timeout: Duration,
}

impl TaskOptions {
    /// Checks explicit finite quotas before session construction.
    ///
    /// # Errors
    /// Returns an invalid-policy error for zero or non-interoperable limits.
    pub fn validate(&self) -> Result<(), TaskError> {
        if self.max_records == 0
            || self.max_bytes == 0
            || self.max_running == 0
            || self.max_input_rounds == 0
            || self.max_input_rounds > MAX_MILLIS
            || [self.execution_timeout, self.input_timeout]
                .iter()
                .any(|timeout| {
                    timeout.as_millis() == 0 || timeout.as_millis() > u128::from(MAX_MILLIS)
                })
        {
            return Err(TaskError::Invalid("policy"));
        }
        Ok(())
    }

    /// Checks a consumer reservation, including room for failure settlement.
    /// `bytes` is the proposed total after admission, not merely the new record.
    /// The consumer performs this check atomically with its own reservation.
    ///
    /// # Errors
    /// Returns capacity or invalid-policy errors without evicting any record.
    pub fn admit(&self, records: usize, bytes: usize, running: usize) -> Result<(), TaskError> {
        self.validate()?;
        if records > self.max_records || bytes > self.max_bytes || running > self.max_running {
            return Err(TaskError::Capacity);
        }
        Ok(())
    }
}

/// Opaque task identity. Consumers generate unguessable, unique IDs and check access scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct TaskId(String);

impl TaskId {
    /// Validates an ID without resolving it against any external system.
    ///
    /// # Errors
    /// Returns an error for empty, excessive or control-containing identifiers.
    pub fn new(value: impl Into<String>) -> Result<Self, TaskError> {
        let value = value.into();
        identifier(&value)?;
        Ok(Self(value))
    }

    /// The exact identifier, case-sensitive.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn identifier(value: &str) -> Result<(), TaskError> {
    if value.is_empty() || value.len() > MAX_ID_BYTES || value.chars().any(char::is_control) {
        return Err(TaskError::Invalid("identifier"));
    }
    Ok(())
}

fn millis(value: u64) -> Result<(), TaskError> {
    if value == 0 || value > MAX_MILLIS {
        return Err(TaskError::Invalid("duration"));
    }
    Ok(())
}

/// Validated operational metadata, independent of a task's status payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    task_id: TaskId,
    created_at: Timestamp,
    last_updated_at: Timestamp,
    ttl_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    status_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    poll_interval_ms: Option<u64>,
}

impl Metadata {
    /// Creates finite-retention metadata. The consumer chooses the TTL explicitly.
    ///
    /// # Errors
    /// Returns an invalid-duration error for zero or excessive TTL.
    pub fn new(task_id: TaskId, created_at: Timestamp, ttl_ms: u64) -> Result<Self, TaskError> {
        millis(ttl_ms)?;
        Ok(Self {
            task_id,
            last_updated_at: created_at.clone(),
            created_at,
            ttl_ms,
            status_message: None,
            poll_interval_ms: None,
        })
    }

    /// Sets a bounded human-readable status message.
    ///
    /// # Errors
    /// Returns an error for a message exceeding the UTF-8 byte bound.
    pub fn with_message(mut self, message: impl Into<String>) -> Result<Self, TaskError> {
        let message = message.into();
        if message.len() > MAX_STATUS_BYTES {
            return Err(TaskError::Invalid("status message"));
        }
        self.status_message = Some(message);
        Ok(self)
    }

    /// Sets a suggested positive polling interval.
    ///
    /// # Errors
    /// Returns an error for a zero or excessive interval.
    pub fn with_poll_interval(mut self, interval_ms: u64) -> Result<Self, TaskError> {
        millis(interval_ms)?;
        self.poll_interval_ms = Some(interval_ms);
        Ok(self)
    }

    /// Revises metadata at a non-regressing time, without reviving an expired task.
    ///
    /// # Errors
    /// Returns expiry, time-regression or invalid-duration errors.
    pub fn updated(mut self, now: Timestamp, ttl_ms: u64) -> Result<Self, TaskError> {
        self.ensure_live(&now)?;
        millis(ttl_ms)?;
        if now < self.last_updated_at {
            return Err(TaskError::Transition);
        }
        self.last_updated_at = now;
        self.ttl_ms = ttl_ms;
        self.ensure_live(&self.last_updated_at)?;
        Ok(self)
    }

    /// The stable record ID.
    #[must_use]
    pub fn id(&self) -> &TaskId {
        &self.task_id
    }

    /// The chosen TTL, measured from creation rather than last access.
    #[must_use]
    pub fn ttl_ms(&self) -> u64 {
        self.ttl_ms
    }

    /// The persisted update time, for the consumer's execution/input deadlines.
    #[must_use]
    pub fn updated_at(&self) -> &Timestamp {
        &self.last_updated_at
    }

    /// Checks retention using an explicit consumer clock value.
    ///
    /// # Errors
    /// Returns expiry at or beyond creation plus TTL, or a clock regression error.
    pub fn ensure_live(&self, now: &Timestamp) -> Result<(), TaskError> {
        if now < &self.created_at {
            return Err(TaskError::Transition);
        }
        if now.elapsed_ms(&self.created_at) >= u128::from(self.ttl_ms) {
            return Err(TaskError::Expired);
        }
        Ok(())
    }
}

/// A persisted yes/no question; its key must never be reused within a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    key: String,
    message: String,
}

impl Question {
    /// Creates a bounded form question using the originating request's capabilities.
    ///
    /// # Errors
    /// Returns a missing-capability error or invalid question/budget error.
    pub fn new(context: &RequestContext, key: &str, message: &str) -> Result<Self, RpcError> {
        context.require_tasks()?;
        context.require_form()?;
        identifier(key).map_err(|error| error.invalid_params())?;
        if message.len() > MAX_INPUT_BYTES {
            return Err(modern::invalid("Task question exceeds its budget"));
        }
        let question = Self {
            key: key.into(),
            message: message.into(),
        };
        modern::bounded(&question.value(), MAX_INPUT_BYTES, 32)?;
        Ok(question)
    }

    /// The lifetime-unique key the consumer records with the question.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    fn value(&self) -> Value {
        json!({&self.key:{"method":"elicitation/create","params":{"mode":"form",
            "message":self.message,"requestedSchema":{"type":"object","properties":{}}}}})
    }
}

/// Domain state for a detailed snapshot. Failure is a JSON-RPC failure;
/// an ordinary tool error belongs in `Completed` with `is_error: true`.
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// Execution is progressing.
    Working,
    /// Execution waits for one persisted yes/no answer.
    InputRequired(Question),
    /// Execution waits for a validated set of rich form/URL answers.
    RichInputRequired(crate::elicitation::Requests),
    /// The original tool payload, without a polling envelope.
    Completed(ToolResult),
    /// A protocol/executor failure.
    Failed(RpcError),
    /// Execution was observed cancelled; this does not imply rollback.
    Cancelled,
}

/// A validated immutable snapshot. Serialization is the persistence format;
/// protocol replies must use [`RequestContext::encode_task`], which omits storage-only fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    #[serde(rename = "inputKind", skip_serializing_if = "Option::is_none")]
    input_kind: Option<InputKind>,
    #[serde(skip)]
    questions: Option<crate::elicitation::Requests>,
    #[serde(flatten)]
    metadata: Metadata,
    status: Status,
    #[serde(flatten)]
    payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum InputKind {
    Rich,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Working,
    InputRequired,
    Completed,
    Failed,
    Cancelled,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::InputRequired => "input_required",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl Snapshot {
    pub(crate) fn completed_result(&self) -> Option<&Value> {
        (self.status == Status::Completed).then(|| &self.payload["result"])
    }
    /// Restores a consumer-persisted snapshot through the same shape and budget checks.
    /// This does not authenticate storage, validate access scope or recover execution.
    ///
    /// # Errors
    /// Returns malformed-field, status/payload or aggregate-bound errors.
    pub fn restore(value: &Value) -> Result<Self, TaskError> {
        let invalid = || TaskError::Invalid("stored snapshot");
        modern::bounded(value, MAX_SNAPSHOT_BYTES, 64).map_err(|_| TaskError::Limit)?;
        if !rpc::only(
            value,
            &[
                "taskId",
                "status",
                "createdAt",
                "lastUpdatedAt",
                "ttlMs",
                "statusMessage",
                "pollIntervalMs",
                "inputRequests",
                "inputKind",
                "result",
                "error",
            ],
        ) {
            return Err(invalid());
        }
        let string = |key| value[key].as_str().ok_or_else(invalid);
        let mut metadata = Metadata::new(
            TaskId::new(string("taskId")?)?,
            Timestamp::parse(string("createdAt")?)?,
            value["ttlMs"].as_u64().ok_or_else(invalid)?,
        )?;
        metadata.last_updated_at = Timestamp::parse(string("lastUpdatedAt")?)?;
        if metadata.last_updated_at < metadata.created_at {
            return Err(invalid());
        }
        if let Some(message) = value.get("statusMessage") {
            metadata = metadata.with_message(message.as_str().ok_or_else(invalid)?)?;
        }
        if let Some(interval) = value.get("pollIntervalMs") {
            metadata = metadata.with_poll_interval(interval.as_u64().ok_or_else(invalid)?)?;
        }
        let state = match string("status")? {
            "working" => State::Working,
            "cancelled" => State::Cancelled,
            "completed" => State::Completed(
                serde_json::from_value(value["result"].clone()).map_err(|_| invalid())?,
            ),
            "failed" => {
                let error = &value["error"];
                if !rpc::only(error, &["code", "message", "data"]) {
                    return Err(invalid());
                }
                State::Failed(RpcError {
                    code: error["code"]
                        .as_i64()
                        .and_then(|code| i32::try_from(code).ok())
                        .ok_or_else(invalid)?,
                    message: error["message"].as_str().ok_or_else(invalid)?.into(),
                    data: error.get("data").cloned(),
                })
            }
            "input_required" => restore_input(&value["inputRequests"], value.get("inputKind"))?,
            _ => return Err(invalid()),
        };
        let snapshot = Self::new(metadata, state)?;
        // Prevent contradictory payloads and silent loss of unrecognized stored data.
        if &serde_json::to_value(&snapshot).map_err(|_| invalid())? != value {
            return Err(invalid());
        }
        Ok(snapshot)
    }
    /// Constructs a status-consistent snapshot and checks all aggregate bounds once.
    ///
    /// # Errors
    /// Returns a size/depth error rather than accepting an unreadable result.
    pub fn new(metadata: Metadata, state: State) -> Result<Self, TaskError> {
        let questions = match &state {
            State::RichInputRequired(requests) => Some(requests.clone()),
            _ => None,
        };
        let (status, payload) = match state {
            State::Working => (Status::Working, json!({})),
            State::InputRequired(question) => (
                Status::InputRequired,
                json!({"inputRequests":question.value()}),
            ),
            State::RichInputRequired(requests) => (
                Status::InputRequired,
                json!({"inputRequests":requests.as_value()}),
            ),
            State::Completed(result) => {
                if let Some(structured) = &result.structured_content {
                    modern::bounded(structured, MAX_SNAPSHOT_BYTES, 62)
                        .map_err(|_| TaskError::Limit)?;
                }
                crate::json::measure(&result, MAX_SNAPSHOT_BYTES, &|| Ok(()))
                    .map_err(|_| TaskError::Limit)?;
                (Status::Completed, json!({"result":result}))
            }
            State::Failed(error) => {
                if let Some(data) = &error.data {
                    modern::bounded(data, MAX_SNAPSHOT_BYTES, 62).map_err(|_| TaskError::Limit)?;
                }
                crate::json::measure(&error.message, MAX_SNAPSHOT_BYTES, &|| Ok(()))
                    .map_err(|_| TaskError::Limit)?;
                (Status::Failed, json!({"error":error.to_value()}))
            }
            State::Cancelled => (Status::Cancelled, json!({})),
        };
        let snapshot = Self {
            input_kind: questions.as_ref().map(|_| InputKind::Rich),
            questions,
            metadata,
            status,
            payload,
        };
        let value = serde_json::to_value(&snapshot).map_err(|_| TaskError::Limit)?;
        modern::bounded(&value, MAX_SNAPSHOT_BYTES, 64).map_err(|_| TaskError::Limit)?;
        Ok(snapshot)
    }

    /// Operational metadata. Polling must not revise it.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// The status name from the validated snapshot.
    #[must_use]
    pub fn status(&self) -> &str {
        self.status.as_str()
    }

    /// Whether execution has reached an immutable terminal result.
    #[must_use]
    pub fn terminal(&self) -> bool {
        matches!(self.status(), "completed" | "failed" | "cancelled")
    }

    /// The pending question's key, if any.
    #[must_use]
    pub fn pending_key(&self) -> Option<&str> {
        self.payload
            .get("inputRequests")?
            .as_object()?
            .keys()
            .next()
            .map(String::as_str)
    }

    /// Every pending key. Persist lifetime-unique issuance history for each one.
    pub fn pending_keys(&self) -> impl Iterator<Item = &str> {
        self.payload["inputRequests"]
            .as_object()
            .into_iter()
            .flat_map(|map| map.keys().map(String::as_str))
    }

    /// Checks a proposed stored successor. The consumer must commit with a
    /// compare-and-set or equivalent ownership guard; this helper performs no write.
    ///
    /// # Errors
    /// Refuses expired, identity-changing, time-regressing or terminal rewrites.
    pub fn validate_successor(&self, next: &Self, now: &Timestamp) -> Result<(), TaskError> {
        self.metadata.ensure_live(now)?;
        next.metadata.ensure_live(now)?;
        if self.metadata.task_id != next.metadata.task_id
            || self.metadata.created_at != next.metadata.created_at
            || next.metadata.last_updated_at < self.metadata.last_updated_at
            || next.metadata.last_updated_at > *now
            || (self.terminal() && self != next)
            || self.payload["inputRequests"]
                .as_object()
                .is_some_and(|map| {
                    map.iter().any(|(key, request)| {
                        next.payload["inputRequests"]
                            .get(key)
                            .is_some_and(|next| request != next)
                    })
                })
        {
            return Err(TaskError::Transition);
        }
        Ok(())
    }

    /// Resolves only the currently pending response. Unknown/consumed keys are
    /// ignored; the consumer atomically consumes this key before resuming effects.
    ///
    /// # Errors
    /// Returns expiry, capability, wrong-task or malformed-known-answer errors.
    pub fn input_decision(
        &self,
        request: &TaskRequest,
        context: &RequestContext,
        now: &Timestamp,
    ) -> Result<Option<Decision>, RpcError> {
        if self.questions.is_some() {
            return Err(modern::invalid("Use typed rich task answers"));
        }
        context.require_tasks()?;
        if !context.task_request_matches(request) {
            return Err(modern::invalid(
                "Task update does not match its request context",
            ));
        }
        self.metadata.ensure_live(now).map_err(|_| not_found())?;
        if request.id() != self.metadata.id() {
            return Err(not_found());
        }
        let TaskRequest::Update { responses, .. } = request else {
            return Err(modern::invalid("Expected task update"));
        };
        let Some(answer) = self.pending_key().and_then(|key| responses.0.get(key)) else {
            return Ok(None);
        };
        context.require_form()?;
        modern::confirmation_answer(answer).map(Some)
    }

    /// Checks capability for every retained question without consuming input.
    ///
    /// # Errors
    /// Returns the missing current form/URL capability.
    pub fn require_input_capabilities(&self, context: &RequestContext) -> Result<(), RpcError> {
        if let Some(questions) = &self.questions {
            questions.require_capabilities(&context.capabilities)
        } else if self.pending_key().is_some() {
            context.require_form()
        } else {
            Ok(())
        }
    }

    /// Validates an entire rich update before the consumer atomically persists answers and intent.
    /// Unknown or consumed keys are ignored; no answer is consumed by this pure helper.
    ///
    /// # Errors
    /// Refuses wrong operation/record/context, expiry, missing modes and malformed known answers.
    pub fn input_answers(
        &self,
        request: &TaskRequest,
        context: &RequestContext,
        now: &Timestamp,
    ) -> Result<std::collections::BTreeMap<String, crate::elicitation::Response>, RpcError> {
        context.require_tasks()?;
        if !context.task_request_matches(request) {
            return Err(modern::invalid("Task update context mismatch"));
        }
        self.metadata.ensure_live(now).map_err(|_| not_found())?;
        if request.id() != self.metadata.id() {
            return Err(not_found());
        }
        let TaskRequest::Update { responses, .. } = request else {
            return Err(modern::invalid("Expected task update"));
        };
        self.require_input_capabilities(context)?;
        if let Some(questions) = &self.questions {
            return questions.answers(&responses.0);
        }
        let mut answers = std::collections::BTreeMap::new();
        if let Some(key) = self.pending_key()
            && let Some(value) = responses.0.get(key)
        {
            let decision = modern::confirmation_answer(value)?;
            let action = match value["action"].as_str() {
                Some("accept") => crate::elicitation::Action::Accept,
                Some("cancel") => crate::elicitation::Action::Cancel,
                _ => crate::elicitation::Action::Decline,
            };
            answers.insert(
                key.to_owned(),
                crate::elicitation::Response {
                    action,
                    content: (decision == Decision::Accepted).then(|| json!({})),
                    meta: value.get("_meta").cloned(),
                },
            );
        }
        Ok(answers)
    }

    pub(crate) fn creation_value(&self) -> Value {
        let mut value = serde_json::to_value(&self.metadata).expect("serializable metadata");
        value["status"] = json!(self.status);
        value
    }

    pub(crate) fn protocol_value(&self) -> Result<Value, serde_json::Error> {
        let mut value = serde_json::to_value(self)?;
        if let Some(fields) = value.as_object_mut() {
            fields.remove("inputKind");
        }
        Ok(value)
    }
}

fn restore_input(value: &Value, kind: Option<&Value>) -> Result<State, TaskError> {
    let invalid = || TaskError::Invalid("stored input requests");
    if let Some(kind) = kind {
        if kind != "rich" {
            return Err(invalid());
        }
        return Ok(State::RichInputRequired(
            crate::elicitation::Requests::restore(value).map_err(|_| invalid())?,
        ));
    }
    let requests = value.as_object().ok_or_else(invalid)?;
    if requests.len() == 1 {
        let (key, request) = requests.iter().next().ok_or_else(invalid)?;
        if request["params"]["mode"] == "form"
            && request["params"]["requestedSchema"] == json!({"type":"object","properties":{}})
        {
            identifier(key)?;
            let question = Question {
                key: key.clone(),
                message: request["params"]["message"]
                    .as_str()
                    .ok_or_else(invalid)?
                    .into(),
            };
            modern::bounded(&question.value(), MAX_INPUT_BYTES, 32)
                .map_err(|_| TaskError::Limit)?;
            if question.value() != *value {
                return Err(invalid());
            }
            return Ok(State::InputRequired(question));
        }
    }
    Err(invalid())
}

/// Structurally bounded task input responses. Known question semantics are
/// checked only against a consumer's pending snapshot, never another workflow.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct InputResponses(Value);

impl InputResponses {
    /// Validates the map before any consumer dispatch. Unknown keys remain opaque objects.
    ///
    /// # Errors
    /// Returns invalid-parameters errors for bad keys, non-objects or exceeded bounds.
    pub fn new(value: Value) -> Result<Self, RpcError> {
        modern::bounded(&value, MAX_RESPONSE_BYTES, 32)?;
        let map = value
            .as_object()
            .ok_or_else(|| modern::invalid("Invalid task inputResponses"))?;
        for (key, answer) in map {
            identifier(key).map_err(|error| error.invalid_params())?;
            if !answer.is_object() {
                return Err(modern::invalid("Invalid task input response"));
            }
        }
        Ok(Self(value))
    }
}

/// One validated management operation. All operations have independent RPC lifetimes.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskRequest {
    /// Read a snapshot without waiting for execution.
    Get {
        /// The retained task.
        task_id: TaskId,
    },
    /// Durably accept answers before acknowledging.
    Update {
        /// The retained task.
        task_id: TaskId,
        /// Bounded input responses.
        responses: InputResponses,
    },
    /// Record cancellation intent, not a promise of rollback or immediate stop.
    Cancel {
        /// The retained task.
        task_id: TaskId,
    },
}

impl TaskRequest {
    /// The exact protocol method.
    #[must_use]
    pub fn method(&self) -> &'static str {
        match self {
            Self::Get { .. } => "tasks/get",
            Self::Update { .. } => "tasks/update",
            Self::Cancel { .. } => "tasks/cancel",
        }
    }
    /// The record to access-check before querying or changing it.
    #[must_use]
    pub fn id(&self) -> &TaskId {
        match self {
            Self::Get { task_id } | Self::Update { task_id, .. } | Self::Cancel { task_id } => {
                task_id
            }
        }
    }
    pub(crate) fn parse(method: &str, params: &Value) -> Result<Self, RpcError> {
        let allowed = if method == "tasks/update" {
            &["taskId", "inputResponses", "_meta"][..]
        } else {
            &["taskId", "_meta"][..]
        };
        if !rpc::only(params, allowed) {
            return Err(modern::invalid("Invalid task parameters"));
        }
        let task_id = TaskId::new(
            params["taskId"]
                .as_str()
                .ok_or_else(|| modern::invalid("Invalid taskId"))?,
        )
        .map_err(|error| error.invalid_params())?;
        match method {
            "tasks/get" => Ok(Self::Get { task_id }),
            "tasks/update" => Ok(Self::Update {
                task_id,
                responses: InputResponses::new(params["inputResponses"].clone())?,
            }),
            "tasks/cancel" => Ok(Self::Cancel { task_id }),
            _ => Err(RpcError::new(rpc::METHOD_NOT_FOUND, "Method not found")),
        }
    }
}

/// Typed outcomes distinguish creation, retrieval and the two acknowledgements.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskResult {
    /// Return only after the consumer has durably published the task.
    Created(Snapshot),
    /// A consistent snapshot of the requested task.
    Snapshot(Snapshot),
    /// Input responses were durably accepted (or were already consumed/unknown).
    Updated,
    /// Cancellation intent was durably recorded, or the task was already terminal.
    Cancelled,
}

/// A generic error for unknown, expired and inaccessible task IDs.
#[must_use]
pub fn not_found() -> RpcError {
    modern::invalid("Task not found or unavailable.")
}

pub(crate) fn method(method: &str) -> bool {
    matches!(method, "tasks/get" | "tasks/update" | "tasks/cancel")
}

/// Consumer-persisted issued-key set. Keep it with the task and count its bytes
/// in the consumer quota; it is not a global library registry.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(transparent)]
pub struct InputHistory(std::collections::BTreeSet<String>);

impl InputHistory {
    /// Restores bounded history and refuses duplicate or malformed keys.
    ///
    /// # Errors
    /// Returns invalid-key, duplicate-key or input-round capacity errors.
    pub fn restore(keys: Vec<String>, options: &TaskOptions) -> Result<Self, TaskError> {
        let mut history = Self::default();
        for key in keys {
            history.issue(&key, options)?;
        }
        Ok(history)
    }

    /// Records a newly issued question. Commit this with its snapshot atomically.
    /// Consumed and superseded keys must remain in this set until record cleanup.
    ///
    /// # Errors
    /// Refuses reuse, invalid keys and the consumer's finite input-round limit.
    pub fn issue(&mut self, key: &str, options: &TaskOptions) -> Result<(), TaskError> {
        options.validate()?;
        identifier(key)?;
        if self.0.contains(key) {
            return Err(TaskError::Transition);
        }
        if u64::try_from(self.0.len()).unwrap_or(u64::MAX) >= options.max_input_rounds {
            return Err(TaskError::Capacity);
        }
        self.0.insert(key.into());
        Ok(())
    }
}
