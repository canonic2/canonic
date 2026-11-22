//! Composes Shield's supported application entry points.

mod agent_message;
mod agents;
mod application;
pub mod cli;
pub mod configuration;
mod diagnostics;
mod docker;
pub mod effects;
pub mod enforcement;
mod evidence;
mod execution;
mod graphql;
mod http_client;
mod json;
mod layout;
mod location;
mod mcp;
mod mcp_connections;
mod mcp_io;
pub mod policy;
mod policy_files;
mod proposal_draft;
mod review;
pub mod runtime;
mod runtime_protocol;
pub mod shell;
#[cfg(test)]
mod test_support;
#[cfg(windows)]
mod windows_console;
#[cfg(windows)]
mod windows_security;

/// Runs the selected CLI command.
pub fn run(cli: &cli::Cli) -> std::process::ExitCode {
    if let Some(config) = &cli.config {
        // Launchers agent setup writes resolve their argument against the
        // project they serve; operator commands against their own cwd.
        let launched = matches!(cli.command, cli::PublicCommand::Internal(_))
            || cli.command.mcp_serve().is_some();
        layout::bind(
            config,
            if launched {
                layout::Base::Project
            } else {
                layout::Base::Invocation
            },
        );
    }
    if (!matches!(cli.command, cli::PublicCommand::Internal(_))
        || cli.command.mcp_serve().is_some())
        && let Err(error) = review::pack_updates::enabled()
    {
        return review::operator::startup_error(cli, &error);
    }
    // Shared so the MCP server's frame observer can outlive one borrow.
    let log = std::sync::Arc::new(diagnostics::Log::open(cli));
    let serve = cli.command.mcp_serve();
    // Internal callbacks already record fixed hook stages. They must not start
    // an asynchronous request writer whose shutdown can delay or warn on hooks.
    let trace = (serve.is_none() && !matches!(cli.command, cli::PublicCommand::Internal(_)))
        .then(|| log.request("command", "command", None, None))
        .flatten();
    let _trace_scope = trace.as_ref().map(diagnostics::trace::Request::enter);
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
        || serde_json::json!({"success":exit == std::process::ExitCode::SUCCESS}),
    );
    if let Some(trace) = &trace {
        trace.finish(if exit == std::process::ExitCode::SUCCESS {
            "success"
        } else {
            "failure"
        });
    }
    exit
}

fn run_internal(
    command: &cli::Command,
    log: &std::sync::Arc<diagnostics::Log>,
) -> std::process::ExitCode {
    let result = match command {
        cli::Command::Mcp(_) => unreachable!("MCP launches are routed by PublicCommand::mcp_serve"),
        cli::Command::Run(input) => return execution::run(input),
        cli::Command::EnforceRequest(input) => return enforcement::protocol::run(&input.project),
        cli::Command::RuntimeRequest(input) => {
            return runtime_protocol::run(&input.project, input.timeout_ms);
        }
        cli::Command::SdkReadOpen(input) => {
            return match review::sdk_read::mint(&input.project, &input.mapping_digest) {
                Ok(context) => {
                    println!("{}", context.launch_value());
                    std::process::ExitCode::SUCCESS
                }
                Err(_) => std::process::ExitCode::from(2),
            };
        }
        cli::Command::SdkReadClose(input) => {
            return if review::sdk_read::close(&input.project).is_ok() {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::from(2)
            };
        }
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
