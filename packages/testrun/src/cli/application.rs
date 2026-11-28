//! CLI run acquisition, command orchestration and owned cleanup.
use super::{
    Action, AppCommand, AudioCommand, BrowserCommand, ClipboardCommand, CommandLine,
    ControlCommand, DeveloperModeCommand, DeveloperSupportCommand, MediaCommand, Mobile,
    NetworkCommand, PreviewCommand, RecordingCommand, TargetCommand, UiCommand, VolumeCommand,
};
use super::{
    arguments::{BuildCommand, Command, McpCommand, SigningCommand, TaskCommand},
    presentation::{print_result, work},
};
use crate::{
    error::{Error, Result},
    protocol,
    qa::Server,
};
use serde_json::{Value, json};

/// Runs one device command through the user's manager, on the connected
/// device `--target` names or the only connected device. Discovery and
/// `doctor` run in this process; device preparation is sent to the manager,
/// which runs it in the device's queue.
pub async fn execute(input: Mobile) -> Result<Value> {
    execute_ref(&input).await
}

pub(super) async fn execute_ref(input: &Mobile) -> Result<Value> {
    if let Some(result) = super::browser::execute(input).await {
        return result;
    }
    if let Some(result) = crate::interactive::execute(input).await {
        return result;
    }
    let owned = matches!(
        input.command,
        Action::Target(_) | Action::Doctor | Action::DeveloperMode(_) | Action::DeveloperSupport(_)
    );
    if !owned && let Some(result) = crate::managed::execute(input).await {
        return result;
    }
    let mut server = Server::new();
    server.managed = crate::managed::Client::new();
    let result = execute_owned(&mut server, input).await;
    let cleanup = server.shutdown().await;
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (_, Err(error)) => Err(error),
    }
}

async fn execute_owned(server: &mut Server, input: &Mobile) -> Result<Value> {
    let operation = match &input.command {
        Action::Target(TargetCommand::List {
            kind,
            probe,
            compact,
            page,
            state,
        }) => {
            let mut args = page.arguments();
            args["probe"] = json!(probe);
            if let Some(state) = state {
                args["state"] = json!(state);
            }
            if let Some(kind) = kind {
                args["kind"] = json!(kind);
            }
            let listed = server.call("target_list", args).await?;
            let value = server.managed.with_held_capabilities(listed).await;
            return Ok(if *compact {
                compact_targets(value)
            } else {
                value
            });
        }
        Action::Doctor => "doctor",
        Action::DeveloperMode(DeveloperModeCommand::Reveal) => "developer_mode_reveal",
        Action::DeveloperSupport(DeveloperSupportCommand::Install) => "developer_support_install",
        _ => {
            return Err(Error::new(
                "INTERNAL",
                "This command runs through the manager",
            ));
        }
    };
    let target = input
        .target
        .as_deref()
        .ok_or_else(|| Error::invalid("--target must name the device"))?;
    server.call(operation, json!({"targetId":target})).await
}

fn compact_targets(value: Value) -> Value {
    let targets=value["targets"].as_array().map(|targets|targets.iter().map(|target|{
        let mut compact=json!({"targetId":target["targetId"],"name":target["name"],"platform":target["platform"],"kind":target["kind"],"model":target["model"],"osVersion":target.get("osVersion").unwrap_or(&target["runtime"]),"state":target["state"],"transport":target["transport"],"pairingState":target["pairingState"],"readiness":target["readiness"]});
        compact.as_object_mut().unwrap().retain(|_,value|!value.is_null());
        compact
    }).collect::<Vec<_>>());
    json!({"targets":targets,"nextCursor":value["nextCursor"],"unavailable":value["unavailable"]})
}
pub async fn run(input: CommandLine) -> Result<()> {
    validate_setup_selectors(&input)?;
    let machine = input.machine_output();
    match input.command {
        Command::Docs(command) => run_documentation(command, machine).await,
        Command::Init(options) => super::init::run(options, machine),
        Command::Config {
            project,
            config,
            command,
        } => super::builds::config(project, config, command, machine),
        Command::Project { config, command } => {
            super::projects::run(command, config, input.target, machine).await
        }
        Command::Diagnostics(command) => super::diagnostics::run(command, machine).await,
        Command::Build(command) => run_build_command(command, input.target, machine).await,
        Command::Mcp(McpCommand::Setup(options)) => super::setup::run_mcp(options, machine).await,
        Command::Signing(SigningCommand::Setup {
            runner_team_id,
            non_interactive,
        }) => super::setup::run_signing(runner_team_id, non_interactive, machine).await,
        Command::Mobile(command) => {
            run_mobile(
                Mobile {
                    target: input.target,
                    command,
                },
                machine,
            )
            .await
        }
        #[cfg(target_os = "macos")]
        Command::NativeIdentity(native) => {
            crate::native::worker_identity(&native.developer, &native.id)
        }
        #[cfg(not(target_os = "macos"))]
        Command::NativeIdentity(_) => Err(Error::new(
            "UNSUPPORTED_HOST",
            "Simulator identity requires macOS",
        )),
        #[cfg(target_os = "macos")]
        Command::NativeInput { native, identity } => {
            crate::native::worker_input(&native.developer, &native.id, &identity)
        }
        #[cfg(not(target_os = "macos"))]
        Command::NativeInput { .. } => Err(Error::new(
            "UNSUPPORTED_HOST",
            "Simulator input requires macOS",
        )),
        #[cfg(target_os = "macos")]
        Command::NativeSource(native) => {
            crate::native::worker_source(&native.developer, &native.id)
        }
        #[cfg(not(target_os = "macos"))]
        Command::NativeSource(_) => Err(Error::new(
            "UNSUPPORTED_HOST",
            "Simulator capture requires macOS",
        )),
        Command::Task(TaskCommand::Stop) => print_result(
            crate::managed::Client::new().end_task().await?,
            machine,
            "Task stop",
        ),
        Command::Manager { command } => print_result(
            crate::managed::manager_command(&command, input.target.as_deref()).await?,
            machine,
            "Manager",
        ),
        #[cfg(target_os = "macos")]
        Command::NativeVerifyVideo { path } => verify_video(&path),
        #[cfg(not(target_os = "macos"))]
        Command::NativeVerifyVideo { .. } => Err(crate::error::Error::new(
            "UNSUPPORTED_HOST",
            "Native video verification requires macOS",
        )),
        #[cfg(target_os = "macos")]
        Command::NativeAccessibility(native) => write_accessibility(&native),
        #[cfg(not(target_os = "macos"))]
        Command::NativeAccessibility(_) => Err(crate::error::Error::new(
            "UNSUPPORTED_HOST",
            "Simulator accessibility requires macOS",
        )),
        Command::Mcp(McpCommand::Serve { check }) => run_server(check, machine).await,
    }
}

async fn run_documentation(command: super::arguments::DocsCommand, machine: bool) -> Result<()> {
    let (name, args) = command.tool();
    let data = crate::documentation::call(
        name,
        &args,
        Some(std::time::Instant::now() + std::time::Duration::from_secs(30)),
    )
    .await?;
    if machine {
        print_result(data, true, "Documentation")
    } else {
        use std::io::Write as _;
        let mut data = data;
        data["next_cursor"] = data["nextCursor"].clone();
        let shown = canonic_mcp_kit::docs::human(
            &format!("testrun docs {}", name.trim_start_matches("docs_")),
            &args,
            &data,
        );
        std::io::stdout()
            .lock()
            .write_all(shown.as_bytes())
            .map_err(super::presentation::output_error)
    }
}

async fn run_build_command(
    command: BuildCommand,
    target: Option<String>,
    machine: bool,
) -> Result<()> {
    match command {
        BuildCommand::Start(options) => run_build(*options, target, machine).await,
        BuildCommand::Log {
            operation_id,
            cursor,
            limit,
        } => {
            build_operation(
                "build_log",
                build_arguments(
                    Some(operation_id),
                    None,
                    json!({"cursor":cursor,"limit":limit}),
                ),
                machine,
            )
            .await
        }
        BuildCommand::Status {
            operation_id,
            submission_id,
            limit,
            cursor,
        } => {
            let args = build_arguments(
                operation_id,
                submission_id,
                json!({"limit":limit,"cursor":cursor}),
            );
            build_operation("build_status", args, machine).await
        }
        BuildCommand::Cancel {
            operation_id,
            submission_id,
        } => {
            let args = build_arguments(operation_id, submission_id, json!({}));
            build_operation("build_cancel", args, machine).await
        }
    }
}

async fn run_build(
    options: super::builds::Options,
    target: Option<String>,
    machine: bool,
) -> Result<()> {
    let result = work("Build app", machine, super::builds::run(options, target)).await?;
    print_result(result, machine, "Build app")
}

async fn run_server(check: bool, machine: bool) -> Result<()> {
    if check {
        protocol::check()?;
        print_result(
            json!({"ok":true,"server":"canonic-testrun","tools":protocol::tools().listing()["tools"]}),
            machine,
            "MCP registration",
        )
    } else {
        let mut server = Server::new();
        server.managed = crate::managed::Client::new();
        protocol::serve(server).await
    }
}

fn build_arguments(
    operation_id: Option<String>,
    submission_id: Option<String>,
    mut arguments: Value,
) -> Value {
    if let Some(id) = operation_id {
        arguments["operationId"] = json!(id);
    }
    if let Some(id) = submission_id {
        arguments["submissionId"] = json!(id);
    }
    arguments
        .as_object_mut()
        .expect("arguments")
        .retain(|_, value| !value.is_null());
    arguments
}

async fn build_operation(name: &str, arguments: Value, machine: bool) -> Result<()> {
    let title = match name {
        "build_status" => "Build status",
        "build_log" => "Operation log",
        _ => "Build cancellation",
    };
    let mut client = crate::managed::Client::new();
    print_result(client.call(name, arguments).await?, machine, title)
}

#[cfg(target_os = "macos")]
fn verify_video(path: &std::path::Path) -> Result<()> {
    let result = crate::native::verify_video_track(path);
    match &result {
        Ok(value) => println!("{value}"),
        Err(error) => println!("{}", json!({"error":error})),
    }
    result?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn write_accessibility(native: &super::arguments::Native) -> Result<()> {
    use std::io::Write;
    std::io::stdout()
        .write_all(&crate::native::accessibility(
            &native.developer,
            &native.id,
        )?)
        .map_err(|_| {
            crate::error::Error::new("TRANSPORT_IO", "Cannot write accessibility response")
        })?;
    Ok(())
}

fn validate_setup_selectors(input: &CommandLine) -> Result<()> {
    if matches!(&input.command, Command::Mcp(McpCommand::Setup(_))) && input.target.is_some() {
        return Err(Error::invalid("MCP setup does not accept --target"));
    }
    Ok(())
}

async fn run_mobile(mobile: Mobile, machine: bool) -> Result<()> {
    let title = operation_name(&mobile.command);
    let value = if long_operation(&mobile.command) {
        work(title, machine, super::setup::execute(&mobile, machine)).await?
    } else {
        super::setup::execute(&mobile, machine).await?
    };
    if matches!(mobile.command, Action::Act { .. }) {
        super::presentation::print_keyboard_notice(&value, machine)?;
    }
    print_result(value, machine, title)
}

fn long_operation(action: &Action) -> bool {
    matches!(
        action,
        Action::Connect { .. }
            | Action::App(AppCommand::Install { .. })
            | Action::DeveloperSupport(_)
            | Action::Browser(BrowserCommand::Connect { .. })
            | Action::Wait { .. }
            | Action::Audio(_)
            | Action::Network(_)
    )
}

fn operation_name(action: &Action) -> &'static str {
    match action {
        Action::Target(TargetCommand::List { .. }) => "Targets",
        Action::Doctor => "Device readiness",
        Action::DeveloperMode(DeveloperModeCommand::Reveal) => "Developer Mode",
        Action::DeveloperSupport(DeveloperSupportCommand::Install) => "Install developer support",
        Action::Connect { .. } => "Connect",
        Action::Disconnect => "Disconnect",
        Action::Status { .. } => "Status",
        Action::Observe { .. } => "Observation",
        Action::Act { .. } => "Actions",
        Action::Cancel { .. } => "Cancel action",
        Action::Wait { .. } => "Wait for conditions",
        Action::Ui(UiCommand::Snapshot { .. }) => "Accessibility snapshot",
        Action::Ui(UiCommand::Find { .. }) => "Accessibility matches",
        Action::Ui(UiCommand::Read { .. }) => "Accessibility element",
        Action::App(AppCommand::List { .. }) => "Apps",
        Action::App(AppCommand::Install { .. }) => "Install app",
        Action::App(AppCommand::Uninstall { .. }) => "Uninstall app",
        Action::Control(ControlCommand::Status) => "Input control",
        Action::Control(ControlCommand::Acquire { .. }) => "Acquire input control",
        Action::Control(ControlCommand::Release { .. }) => "Release input control",
        Action::Audio(AudioCommand::Probe { .. }) => "Measure audio",
        Action::Media(MediaCommand::Status) => "Media status",
        Action::Media(MediaCommand::Send { .. }) => "Media command",
        Action::Volume(VolumeCommand::Set { .. }) => "Set volume",
        Action::Volume(VolumeCommand::Get { .. }) => "Volume",
        Action::Network(NetworkCommand::Set { .. }) => "Set network",
        Action::Clipboard(ClipboardCommand::Read) => "Clipboard",
        Action::Preview(PreviewCommand::Start { .. }) => "Start preview",
        Action::Preview(PreviewCommand::Status { .. }) => "Preview",
        Action::Preview(PreviewCommand::Stop { .. }) => "Stop preview",
        Action::Recording(RecordingCommand::Start { .. }) => "Start recording",
        Action::Recording(RecordingCommand::Status { .. }) => "Recording",
        Action::Recording(RecordingCommand::Stop { .. }) => "Stop recording",
        Action::Browser(BrowserCommand::List(_)) => "Browsers",
        Action::Browser(BrowserCommand::Doctor { .. }) => "Browser readiness",
        Action::Browser(BrowserCommand::Connect { .. }) => "Prepare and connect browser",
        Action::Browser(BrowserCommand::FixtureRegister { .. }) => "Register browser fixture",
        Action::Browser(BrowserCommand::Status { .. }) => "Browser status",
        Action::Browser(BrowserCommand::Send { .. }) => "Browser request",
        Action::Browser(BrowserCommand::Cancel { .. }) => "Cancel browser operation",
        Action::Browser(BrowserCommand::Disconnect) => "Disconnect browser",
        Action::ManagedOwner { .. }
        | Action::Session { .. }
        | Action::Batch { .. }
        | Action::Close { .. } => "Retained session",
    }
}
