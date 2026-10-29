//! Composes Shield's supported application entry points.

mod agent_message;
mod agents;
mod application;
pub mod cli;
pub mod configuration;
mod diagnostics;
pub mod effects;
pub mod enforcement;
mod evidence;
mod execution;
mod http_client;
mod json;
mod location;
mod mcp;
mod mcp_connections;
mod mcp_io;
pub mod policy;
mod policy_files;
mod proposal_draft;
mod review;
pub mod runtime;
pub mod shell;

/// Runs the selected CLI command.
#[must_use]
pub fn run(cli: &cli::Cli) -> std::process::ExitCode {
    // Shared so the MCP server's frame observer can outlive one borrow.
    let log = std::sync::Arc::new(diagnostics::Log::open(cli));
    let serve = cli.command.mcp_serve();
    if !matches!(cli.command, cli::PublicCommand::Internal(_))
        && serve.is_none()
        && let Some(path) = log.path()
    {
        eprintln!("Shield debug log: {}", path.display());
    }
    let exit = match (&cli.command, serve) {
        (_, Some(input)) => mcp_io::run(cli.project.as_deref(), input, &log),
        (cli::PublicCommand::Internal(command), None) => run_internal(command, &log),
        _ => review::operator::run(cli),
    };
    log.event(
        "run_end",
        serde_json::json!({"success":exit == std::process::ExitCode::SUCCESS}),
    );
    exit
}

fn run_internal(
    command: &cli::Command,
    log: &std::sync::Arc<diagnostics::Log>,
) -> std::process::ExitCode {
    let result = match command {
        cli::Command::Run(input) => return execution::run(input),
        cli::Command::EnforceRequest(input) => return enforcement::protocol::run(&input.project),
        cli::Command::NativeHook(input) => return agents::run(input, log),
        cli::Command::McpConnect(input) => return mcp_connections::launcher::run(input),
        cli::Command::HistoryEvents(input) => {
            let (value, code) = review::history_events(&input.project).map_or_else(
                |error| (serde_json::json!({"schema_version":1,"error":{"code":error.code(),"message":error.to_string()}}), 2),
                |value| (value, 0),
            );
            println!("{value}");
            return std::process::ExitCode::from(code);
        }
        cli::Command::Validate(selection) => {
            application::validate(selection).map(|value| (value, 0))
        }
        cli::Command::TestPack(input) => application::test_pack(input),
    };
    let (value, code) = result.unwrap_or_else(|error| (serde_json::json!({"schema_version":1,"mode":"preview","decision":"deny","error":{"code":error.code(),"message":error.to_string()}}), 2));
    println!("{value}");
    std::process::ExitCode::from(code)
}
