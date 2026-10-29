//! Docker command, runtime and management-effect recognition.
use super::{Effect, Hints, NestedExecution, arguments};
use std::collections::BTreeSet;

pub(super) fn classify(hints: &mut Hints) {
    let args = &hints.request.arguments;
    let globals = arguments::parse(
        args,
        &[
            "--config",
            "--context",
            "-c",
            "--host",
            "-H",
            "--log-level",
            "-l",
            "--tlscacert",
            "--tlscert",
            "--tlskey",
        ],
        &["--debug", "-D", "--tls", "--tlsverify"],
        arguments::Syntax::DockerGlobals,
    );
    let mut at = globals.command_at;
    hints.global_options = globals.options;
    if !hints.global_options.is_empty() {
        hints.effects.insert(Effect::Configuration);
    }
    if let Some(reason) = globals.unresolved {
        hints.unresolved.push(reason);
        return;
    }
    let Some(first) = args.get(at) else {
        hints.unresolved.push("missing-command");
        return;
    };
    hints.command_path = vec![first.clone()];
    let mut command = first.as_str();
    if matches!(
        command,
        "container"
            | "image"
            | "volume"
            | "network"
            | "system"
            | "builder"
            | "swarm"
            | "service"
            | "stack"
            | "plugin"
            | "secret"
            | "config"
            | "context"
            | "buildx"
            | "compose"
    ) {
        at += 1;
        if let Some(operation) = args.get(at).filter(|arg| !arg.starts_with('-')) {
            hints.command_path.push(operation.clone());
            command = match (first.as_str(), operation.as_str()) {
                ("container", "ls") => "ps",
                ("image", "ls") => "images",
                ("image", "rm") => "rmi",
                (_, value) => value,
            };
        } else {
            hints.unresolved.push("compound-operation");
        }
    }
    hints.command = Some(hints.command_path.join(" "));
    hints.effects.extend(
        if hints.command_path.len() == 2 && !matches!(first.as_str(), "container" | "image") {
            docker_management_effects(first, command)
        } else {
            docker_effects(command)
        },
    );
    let builder_inspection = first == "buildx" && command == "inspect";
    if (hints.command_path.len() == 1 || first == "container")
        && matches!(command, "run" | "create" | "exec")
    {
        docker_runtime(hints, at + 1, command == "exec");
    }
    if builder_inspection {
        docker_builder_inspection(hints, at + 1);
    }
    hints.unresolved.extend([
        "command-options",
        "daemon-and-context",
        "nested-program-or-build-definition",
    ]);
}

fn docker_builder_inspection(hints: &mut Hints, start: usize) {
    let options = arguments::interspersed(
        &hints.request.arguments[start..],
        &["--format"],
        &["--bootstrap"],
    );
    if options.unresolved.is_some() {
        hints.unresolved.push("builder-inspection-options");
    }
    if options
        .options
        .iter()
        .rev()
        .find(|option| option.name == "--bootstrap")
        .is_some_and(|option| option.value.as_deref() != Some("false"))
    {
        hints
            .effects
            .extend([Effect::DaemonWrite, Effect::Network, Effect::CodeExecution]);
    }
}

fn docker_management_effects(family: &str, operation: &str) -> &'static [Effect] {
    let plugin = matches!(family, "compose" | "buildx" | "plugin");
    if matches!(
        operation,
        "ls" | "list"
            | "inspect"
            | "info"
            | "df"
            | "du"
            | "history"
            | "ps"
            | "logs"
            | "events"
            | "top"
            | "version"
            | "show"
            | "tasks"
            | "images"
            | "port"
    ) {
        return if plugin {
            &[Effect::CodeExecution]
        } else {
            &[]
        };
    }
    if family == "context" {
        return &[Effect::Configuration, Effect::Network];
    }
    if matches!(operation, "rm" | "remove" | "prune") {
        return &[
            Effect::DataRemoval,
            Effect::DaemonWrite,
            Effect::CodeExecution,
        ];
    }
    if family == "compose"
        || (matches!(family, "builder" | "buildx") && matches!(operation, "build" | "bake"))
    {
        return docker_effects("compose");
    }
    if family == "plugin" {
        return &[
            Effect::CodeExecution,
            Effect::DaemonWrite,
            Effect::HostExposure,
            Effect::PrivilegedRuntime,
            Effect::Configuration,
            Effect::Network,
        ];
    }
    if matches!(
        operation,
        "create"
            | "update"
            | "connect"
            | "disconnect"
            | "init"
            | "join"
            | "leave"
            | "unlock"
            | "unlock-key"
            | "deploy"
            | "rollback"
            | "scale"
            | "set"
            | "enable"
            | "disable"
            | "start"
            | "stop"
            | "use"
            | "bootstrap"
    ) {
        return &[
            Effect::DaemonWrite,
            Effect::Configuration,
            Effect::CodeExecution,
            Effect::Network,
        ];
    }
    docker_effects(family)
}

fn docker_effects(command: &str) -> &'static [Effect] {
    match command {
        "ps" | "images" | "inspect" | "logs" | "stats" | "version" | "info" | "events" | "top"
        | "history" | "wait" => &[],
        "run" | "create" | "exec" | "start" | "restart" | "attach" => {
            &[Effect::CodeExecution, Effect::DaemonWrite]
        }
        "build" | "buildx" => &[
            Effect::ImageBuild,
            Effect::CodeExecution,
            Effect::DaemonWrite,
            Effect::Network,
        ],
        "pull" | "push" => &[Effect::Network, Effect::DaemonWrite],
        "rm" | "rmi" | "prune" => &[Effect::DataRemoval, Effect::DaemonWrite],
        "stop" | "kill" | "pause" | "unpause" | "rename" | "update" | "commit" | "tag" | "load"
        | "import" => &[Effect::DaemonWrite],
        "cp" | "save" | "export" => &[Effect::WorktreeWrite, Effect::DaemonWrite],
        "login" | "logout" | "context" => &[Effect::Configuration, Effect::Network],
        "compose" | "container" | "image" | "volume" | "network" | "system" | "builder"
        | "swarm" | "service" | "stack" | "plugin" | "secret" | "config" => &[
            Effect::CodeExecution,
            Effect::DaemonWrite,
            Effect::DataRemoval,
            Effect::Network,
            Effect::Configuration,
            Effect::HostExposure,
            Effect::ImageBuild,
        ],
        _ => &[Effect::CodeExecution],
    }
}

fn docker_runtime(hints: &mut Hints, start: usize, exec: bool) {
    let args = &hints.request.arguments;
    let options = arguments::parse(
        &args[start..],
        &[
            "--name",
            "--mount",
            "--volume",
            "-v",
            "--device",
            "--cap-add",
            "--cap-drop",
            "--env",
            "-e",
            "--env-file",
            "--workdir",
            "-w",
            "--user",
            "-u",
            "--entrypoint",
            "--network",
            "--publish",
            "-p",
            "--hostname",
            "-h",
            "--memory",
            "-m",
            "--cpus",
            "--platform",
            "--restart",
        ],
        &[
            "--privileged",
            "--read-only",
            "--rm",
            "--detach",
            "-d",
            "--interactive",
            "-i",
            "--tty",
            "-t",
            "--init",
        ],
        arguments::Syntax::DockerRuntime,
    );
    runtime_effects(&mut hints.effects, &options.options);
    if let Some(reason) = options.unresolved {
        hints.unresolved.push(reason);
        return;
    }
    let operand = start + options.command_at;
    if args.get(operand).is_none() {
        hints.unresolved.push("missing-runtime-operand");
        return;
    }
    let entrypoint = options
        .options
        .iter()
        .rev()
        .find(|option| option.name == "--entrypoint")
        .and_then(|option| option.value.as_deref())
        .filter(|value| !value.is_empty());
    let mut program = entrypoint
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    program.extend_from_slice(&args[operand + 1..]);
    if program.is_empty() {
        hints
            .unresolved
            .push("image-entrypoint-or-container-program");
    } else {
        hints.nested.push(NestedExecution {
            kind: if exec {
                "container-exec"
            } else {
                "container-command"
            },
            source: None,
            argv: program,
            argument_index: operand + 1,
            namespace: "container",
        });
    }
}

fn runtime_effects(effects: &mut BTreeSet<Effect>, options: &[arguments::OptionValue]) {
    for option in options {
        if matches!(
            option.name.as_str(),
            "-v" | "--volume" | "--mount" | "--device"
        ) {
            effects.insert(Effect::HostExposure);
        }
        if option.name == "--cap-add"
            || (option.name == "--privileged" && option.value.as_deref() != Some("false"))
        {
            effects.insert(Effect::PrivilegedRuntime);
        }
        if matches!(
            option.name.as_str(),
            "-e" | "--env" | "--env-file" | "--entrypoint" | "-u" | "--user" | "-w" | "--workdir"
        ) {
            effects.insert(Effect::Configuration);
        }
    }
}
