//! Owns MCP process startup: the enrolled project context, diagnostics and
//! the stdio connection's lifetime.
use crate::{
    cli::{McpServe, ReviewContext},
    mcp,
};
use std::{io, path::Path, process::ExitCode, sync::Arc, time::Duration};

pub(crate) fn run(
    project: Option<&Path>,
    input: &McpServe,
    log: &Arc<crate::diagnostics::Log>,
) -> ExitCode {
    let context = ReviewContext {
        project: project.unwrap_or(Path::new(".")).to_path_buf(),
        agent: input.agent.clone(),
        session_id: input.session_id.clone(),
        tool_use_id: input.tool_use_id.clone(),
    };
    log.event("mcp_start", serde_json::json!({"project":context.project,"session_id":context.session_id,"agent":context.agent,"request_timeout_ms":input.request_timeout_ms}));
    let Ok(context) = crate::review::cli::context(&context) else {
        eprintln!("Cannot start Shield MCP server: enrolled project required");
        return ExitCode::FAILURE;
    };
    let Ok(server) = mcp::Server::for_review(context) else {
        eprintln!("Cannot start Shield project review server");
        return ExitCode::FAILURE;
    };
    let options = mcp::options(
        Duration::from_millis(input.request_timeout_ms),
        Some(log.clone()),
    );
    match mcp::serve(
        server,
        io::BufReader::new(io::stdin()),
        io::stdout().lock(),
        &options,
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
