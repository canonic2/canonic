//! Tasks wire composition; preparation and storage never change the policy files.
use super::{Server, response, rpc};
use crate::review::{
    self,
    preparation_tasks::{Record, Store},
};
use canonic_mcp_kit::{
    RpcError, ToolCall,
    stdio::{Context, Outcome},
    task::{
        self, Metadata, Snapshot, State, TaskId, TaskOptions, TaskRequest, TaskResult, Timestamp,
    },
};
use serde_json::Value;
use std::{sync::Arc, time::Duration};

pub(super) fn policy() -> TaskOptions {
    TaskOptions {
        max_records: 32,
        max_bytes: 640 * 1024 * 1024,
        max_running: 2,
        max_input_rounds: 1,
        execution_timeout: Duration::from_mins(1),
    }
}

fn failure(error: &review::Error) -> RpcError {
    if matches!(error, review::Error::Missing) {
        task::not_found()
    } else {
        RpcError::new(
            rpc::INTERNAL_ERROR,
            "Preparation task storage or execution is unavailable; ordinary policy tools remain usable",
        )
    }
}

fn metadata(record: &Record) -> Result<Metadata, RpcError> {
    let invalid = |_| RpcError::new(rpc::INTERNAL_ERROR, "Invalid retained preparation metadata");
    Metadata::new(
        TaskId::new(record.id.clone()).map_err(invalid)?,
        Timestamp::from_unix_millis(record.created).map_err(invalid)?,
        record.ttl_ms,
    )
    .and_then(|metadata| {
        metadata.updated(Timestamp::from_unix_millis(record.updated)?, record.ttl_ms)
    })
    .and_then(|metadata| metadata.with_message(record.message.clone()))
    .and_then(|metadata| metadata.with_poll_interval(250))
    .map_err(invalid)
}

fn snapshot(store: &Store, record: &Record) -> Result<Snapshot, RpcError> {
    let state = match record.status.as_str() {
        "working" => State::Working,
        "cancelled" => State::Cancelled,
        "failed" => State::Failed(
            RpcError::new(-32000, &record.message)
                .with_data(serde_json::json!({"code":"preparation_failed","policy_changed":false})),
        ),
        "completed" => State::Completed(
            serde_json::from_value(store.result(record).map_err(|e| failure(&e))?).map_err(
                |_| RpcError::new(rpc::INTERNAL_ERROR, "Invalid retained preparation result"),
            )?,
        ),
        _ => {
            return Err(RpcError::new(
                rpc::INTERNAL_ERROR,
                "Invalid preparation status",
            ));
        }
    };
    Snapshot::new(metadata(record)?, state)
        .map_err(|_| RpcError::new(rpc::INTERNAL_ERROR, "Preparation snapshot exceeds bounds"))
}

fn schema(server: &Server) -> Result<&canonic_mcp_kit::ResultSchema, RpcError> {
    if server.task_schema.get().is_none() {
        let schema = super::catalog(None)
            .map_err(|_| {
                RpcError::new(
                    rpc::INTERNAL_ERROR,
                    "Preparation result contract unavailable",
                )
            })?
            .result_schema("policy_propose")
            .ok_or_else(|| {
                RpcError::new(rpc::INTERNAL_ERROR, "Preparation result contract missing")
            })?;
        let _ = server.task_schema.set(schema);
    }
    server
        .task_schema
        .get()
        .ok_or_else(|| RpcError::new(rpc::INTERNAL_ERROR, "Preparation result contract missing"))
}

pub(super) fn create(
    server: &Server,
    review: &Arc<review::Context>,
    call: &ToolCall,
    context: &Context,
) -> Option<Outcome> {
    if call.name != "policy_propose"
        || !context
            .request_context()
            .is_some_and(canonic_mcp_kit::RequestContext::task_eligible)
    {
        return None;
    }
    match review::preparation_tasks::eligible(&call.arguments) {
        Ok(false) => return None,
        Err(error) => {
            return Some(Outcome::Tool(response::tool_result(
                &call.name,
                review.agent.as_deref(),
                review::mcp::admin_error(review, &call.name, &error),
                true,
                &|| Ok(()),
            )));
        }
        Ok(true) => {}
    }
    let Some(contract) = context.result_schema().cloned() else {
        return Some(Outcome::Method(Err(RpcError::new(
            rpc::INTERNAL_ERROR,
            "Preparation result contract missing",
        ))));
    };
    let _ = server.task_schema.set(contract.clone());
    let publish = Box::new(
        move |record: &Record, data: Value| -> Result<Value, review::Error> {
            let result = response::tool_result("policy_propose", None, data, false, &|| Ok(()));
            let mut terminal = record.clone();
            terminal.message =
                "Preparation captured; fresh review and human approval are still required".into();
            let snapshot = Snapshot::new(
                metadata(&terminal).map_err(|_| review::Error::Invalid("task metadata"))?,
                State::Completed(result.clone()),
            )
            .map_err(|_| review::Error::Invalid("task result exceeds bounds"))?;
            contract.validate_snapshot(&snapshot).map_err(|_| {
                review::Error::Invalid("task result differs from its producing schema")
            })?;
            serde_json::to_value(result).map_err(|_| review::Error::Invalid("task result encoding"))
        },
    );
    match server.preparations.start(
        review,
        &call.arguments,
        &|| {
            context
                .check()
                .map_err(|_| crate::policy::Error::Interrupted)
        },
        publish,
    ) {
        Ok(None) => None,
        Ok(Some(record)) => Some(Outcome::Task(
            server
                .preparations
                .store()
                .map_err(|error| failure(&error))
                .and_then(|store| snapshot(&store, &record))
                .map(TaskResult::Created),
        )),
        Err(error) => Some(Outcome::Task(Err(failure(&error)))),
    }
}

pub(super) fn manage(server: &Server, request: &TaskRequest, context: &Context) -> Outcome {
    let result = (|| {
        context
            .check()
            .map_err(|_| RpcError::new(rpc::REQUEST_CANCELLED, "Request cancelled"))?;
        let store = if matches!(request, TaskRequest::Cancel { .. }) {
            server.preparations.store()
        } else {
            server.preparations.observation_store()
        }
        .map_err(|error| failure(&error))?;
        let id = request.id().as_str();
        let record = store.observe(id).map_err(|error| failure(&error))?;
        match request {
            TaskRequest::Get { .. } => {
                let snapshot = snapshot(&store, &record)?;
                schema(server)?.validate_snapshot(&snapshot).map_err(|_| {
                    RpcError::new(
                        rpc::INTERNAL_ERROR,
                        "Retained preparation result differs from its producing contract",
                    )
                })?;
                Ok(TaskResult::Snapshot(snapshot))
            }
            TaskRequest::Cancel { .. } => {
                store.cancel(id).map_err(|error| failure(&error))?;
                Ok(TaskResult::Cancelled)
            }
            TaskRequest::Update { .. } => Ok(TaskResult::Updated),
        }
    })();
    Outcome::Task(result)
}
