//! A durable Acme report workflow. Run with an explicit private store directory.

#[path = "tasks/store.rs"]
mod store;

use canonic_mcp_kit::{
    Catalog, Client, Content, ModernOptions, RpcError, ServerInfo, SessionOptions, Tool,
    ToolResult,
    stdio::{self, Admission, Context, Handler, Lane, Outcome, Request},
    task::{TaskRequest, TaskResult, Timestamp},
};
use serde_json::json;
use std::{
    io,
    path::Path,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const SCOPE: &str = "acme-local";
struct Acme(store::Store);

fn now() -> Result<Timestamp, RpcError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|time| u64::try_from(time.as_millis()).ok())
        .ok_or_else(|| RpcError::new(-32603, "Acme clock is unavailable."))?;
    Timestamp::from_unix_millis(millis)
        .map_err(|_| RpcError::new(-32603, "Acme clock is out of range."))
}

impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(identity())
    }
    fn admit(&self, request: &Request) -> Admission {
        Admission {
            lane: usize::from(!matches!(request, Request::Task(TaskRequest::Get { .. }))),
            ..Admission::default()
        }
    }
    fn call(&self, request: Request, context: &Context) -> Outcome {
        if context.check().is_err() {
            return Outcome::Method(Err(RpcError::new(-32800, "Request cancelled")));
        }
        let now = match now() {
            Ok(now) => now,
            Err(error) => return Outcome::Method(Err(error)),
        };
        if let Request::Tool(call) = &request
            && !call.arguments["deferred"].as_bool().unwrap_or(false)
        {
            return Outcome::Tool(ToolResult::success(vec![Content::text(
                "Acme report preview.",
            )]));
        }
        let Some(protocol) = context.request_context() else {
            return Outcome::Tool(ToolResult::error(vec![Content::text(
                "Deferred reports require a modern host with Tasks and form support; use deferred: false for a preview.",
            )]));
        };
        let result = match request {
            Request::Tool(_) => self
                .0
                .create(protocol, SCOPE, &now)
                .map(TaskResult::Created),
            Request::Task(TaskRequest::Get { task_id }) => {
                self.0.get(&task_id, SCOPE, &now).map(TaskResult::Snapshot)
            }
            Request::Task(request @ TaskRequest::Update { .. }) => self
                .0
                .accept(&request, protocol, SCOPE, &now)
                .and_then(|decision| {
                    if decision.is_some() {
                        self.0.finish_report(request.id(), SCOPE, &now)?;
                    }
                    Ok(TaskResult::Updated)
                }),
            Request::Task(TaskRequest::Cancel { task_id }) => self
                .0
                .cancel(&task_id, SCOPE, &now)
                .map(|()| TaskResult::Cancelled),
            _ => Err(RpcError::new(-32601, "Method not found")),
        };
        Outcome::Task(result)
    }
}

fn identity() -> ServerInfo {
    ServerInfo::new("acme","1.0").with_instructions("Use acme_report for a preview, or deferred: true to request report generation with user confirmation. Poll returned task IDs and answer pending input through Tasks; tools grant no authority.")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os()
        .nth(1)
        .ok_or("Pass a private Acme task-store directory.")?;
    let clock = now().map_err(|error| io::Error::other(error.message))?;
    let store = store::Store::open(Path::new(&directory), &clock)?;
    let catalog = Catalog::new(vec![Tool::new(
        "acme_report",
        "Return a preview or start a report task that asks for user confirmation.",
        json!({"type":"object","properties":{"deferred":{"type":"boolean","default":false}},"additionalProperties":false}),
    )])?;
    let options = stdio::Options {
        session: SessionOptions {
            modern: Some(ModernOptions::new(identity())),
            tasks: Some(store::policy()),
            ..SessionOptions::default()
        },
        lanes: vec![Lane::new("reads", 2, 8), Lane::new("changes", 1, 8)],
        ..stdio::Options::default()
    };
    stdio::serve(
        Arc::new(Acme(store)),
        Arc::new(catalog),
        io::BufReader::new(io::stdin()),
        io::stdout(),
        &options,
    )?;
    Ok(())
}
