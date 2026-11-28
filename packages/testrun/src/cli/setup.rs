//! Terminal presentation for MCP and signing setup; shared signing workflows never prompt.
use super::{Action, Mobile};
use crate::{
    error::{Error, Result},
    host_setup,
};
use serde_json::{Value, json};
use std::io::{self, IsTerminal};

fn interactive() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

pub(super) async fn run_mcp(options: crate::agent_setup::Options, machine: bool) -> Result<()> {
    let root = std::env::current_dir().map_err(|e| Error::new("HOST_IO", e.to_string()))?;
    let context = mcp_setup_context(&root)?;
    let result =
        tokio::task::spawn_blocking(move || crate::agent_setup::run(&context, options, machine))
            .await
            .map_err(|e| Error::new("HOST_IO", e.to_string()))?;
    let value = mcp_setup_result(result)?;
    if machine {
        return super::presentation::print_result(value, true, "MCP setup");
    }
    use std::io::Write;
    writeln!(
        std::io::stdout().lock(),
        "{}",
        crate::agent_setup::human(&value)
    )
    .map_err(super::presentation::output_error)
}

fn mcp_setup_context(root: &std::path::Path) -> Result<crate::agent_setup::Context> {
    // Every Testrun tool is approved by default; the permissions page can ask instead.
    let tools = crate::protocol::tools()
        .tools()
        .iter()
        .map(|tool| crate::agent_setup::Tool::new(&tool.name, &tool.description, true))
        .collect();
    let mut context =
        crate::agent_setup::Context::local("testrun", root, vec!["mcp".into(), "serve".into()])
            .map_err(mcp_setup_error)?
            .with_tools(tools);
    context.check_arguments.push("--json".into());
    Ok(context)
}

pub(super) async fn run_signing(
    runner_team_id: Option<String>,
    non_interactive: bool,
    machine: bool,
) -> Result<()> {
    let selection = select_default(
        runner_team_id,
        !machine && !non_interactive && interactive(),
        host_setup::inspect,
        |requested| async move { host_setup::setup(requested.as_deref()).await },
        prompt,
    )
    .await?;
    super::presentation::print_result(
        json!({"platform":"ios","signing":selection}),
        machine,
        "iOS signing setup",
    )
}

fn mcp_setup_error(error: crate::agent_setup::Error) -> Error {
    use crate::agent_setup::Error as SetupError;
    let code = match &error {
        SetupError::Invalid(_) => "AGENT_SETUP",
        SetupError::InvalidSelection(_) => "AGENT_SETUP_INVALID_SELECTION",
        SetupError::Io(_) => "AGENT_SETUP_IO",
        SetupError::Conflict(_) => "AGENT_SETUP_CONFLICT",
        SetupError::ConfirmationRequired(_) => "AGENT_SETUP_CONFIRMATION_REQUIRED",
        SetupError::CatalogCheck(_) => "AGENT_SETUP_CATALOG_CHECK",
        SetupError::Partial { .. } => "AGENT_SETUP_PARTIAL",
    };
    let mapped = Error::new(code, error.to_string());
    if let SetupError::Partial {
        completed,
        pending,
        source,
    } = error
    {
        return mapped.details(json!({
            "completed":completed,
            "pending":pending,
            "cause":mcp_setup_error(*source),
        }));
    }
    mapped
}

fn mcp_setup_result(result: crate::agent_setup::Result<Value>) -> Result<Value> {
    let value = result.map_err(mcp_setup_error)?;
    if value["cancelled_by_user"] == true {
        return Err(Error::new("SETUP_CANCELLED", "MCP setup cancelled"));
    }
    Ok(value)
}

async fn select_default<I, Inspect, S, Setup, P, Prompt>(
    requested: Option<String>,
    may_prompt: bool,
    inspect: I,
    setup: S,
    prompt: P,
) -> Result<host_setup::Selection>
where
    I: FnOnce() -> Inspect,
    Inspect: std::future::Future<Output = Result<Value>>,
    S: FnOnce(Option<String>) -> Setup,
    Setup: std::future::Future<Output = Result<host_setup::Selection>>,
    P: FnOnce(Value) -> Prompt,
    Prompt: std::future::Future<Output = Result<String>>,
{
    if let Some(team) = requested {
        setup(Some(team)).await
    } else if may_prompt {
        let report = inspect().await?;
        let teams = report["availableTeams"].as_object().unwrap();
        let saved_valid = report["savedTeamId"]
            .as_str()
            .is_some_and(|team| teams.contains_key(team));
        if teams.len() > 1 || !teams.is_empty() && report["savedTeamId"].is_string() && !saved_valid
        {
            let selected = prompt(report).await?;
            setup(Some(selected)).await
        } else {
            setup(None).await
        }
    } else {
        setup(None).await
    }
}

fn needs_signing(input: &Mobile) -> bool {
    input
        .target
        .as_deref()
        .is_some_and(|target| target.starts_with("device:"))
        && matches!(
            &input.command,
            Action::Connect {
                setup: true,
                runner_team_id: None,
                ..
            } | Action::Session {
                setup: true,
                runner_team_id: None,
                ..
            }
        )
}

/// The runner owns first-use discovery, persistence and source reporting. Only
/// a refused signing selection may prompt and retry connection startup; input
/// actions and all other failures are returned without replay.
pub(super) async fn execute(input: &Mobile, machine: bool) -> Result<Value> {
    execute_with_selection(
        !machine && interactive() && needs_signing(input),
        || super::application::execute_ref(input),
        |report| async move {
            let selected = prompt(report).await?;
            host_setup::setup(Some(&selected)).await?;
            Ok(())
        },
    )
    .await
}

async fn execute_with_selection<F, Fut, P, Prompt>(
    may_prompt: bool,
    mut execute: F,
    select: P,
) -> Result<Value>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<Value>>,
    P: FnOnce(Value) -> Prompt,
    Prompt: std::future::Future<Output = Result<()>>,
{
    match execute().await {
        Err(error)
            if may_prompt
                && error.code == "SIGNING_TEAM_REQUIRED"
                && error.details.as_ref().is_some_and(|details| {
                    details["availableTeamIds"]
                        .as_array()
                        .is_some_and(|teams| teams.len() > 1)
                }) =>
        {
            select(error.details.unwrap()).await?;
            execute().await
        }
        result => result,
    }
}

async fn prompt(report: Value) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let context = prompt_context(&report);
        prompt_with(&report, |title, labels, saved| {
            choose_team(title, labels, saved, &context)
        })
    })
    .await
    .map_err(|_| Error::new("HOST_IO", "Signing selection prompt failed"))?
}

fn choose_team(
    title: &str,
    labels: Vec<String>,
    saved: Option<usize>,
    context: &str,
) -> io::Result<Option<usize>> {
    use canonic_tui_kit::{picker::Choice, runtime::prompts::choose_choices};
    let choices = labels
        .into_iter()
        .enumerate()
        .map(|(index, label)| {
            let (id, identity) = label.split_once(" — ").unwrap_or((&label, ""));
            let mut choice = Choice::new(index.to_string(), id);
            choice.detail = identity.into();
            choice.current = Some(index) == saved;
            choice
        })
        .collect();
    choose_choices(
        title,
        context,
        choices,
        saved.map(|index| index.to_string()).as_deref(),
    )?
    .map(|id| id.parse().map_err(io::Error::other))
    .transpose()
}

fn prompt_context(report: &Value) -> String {
    match report["savedTeamId"].as_str() {
        Some(team) if report["availableTeams"].get(team).is_none() => format!(
            "Saved team {team} is unavailable. Choose a replacement for this OS user or cancel to preserve it."
        ),
        Some(team) => format!(
            "Saved default: {team} (*). Applies to this OS user across projects; device preparation is separate."
        ),
        None => {
            "Choose the default for this OS user across projects. Device preparation is separate."
                .into()
        }
    }
}

fn prompt_with(
    report: &Value,
    choose: impl FnOnce(&str, Vec<String>, Option<usize>) -> io::Result<Option<usize>>,
) -> Result<String> {
    let teams = report["availableTeams"]
        .as_object()
        .ok_or_else(|| Error::new("INTERNAL", "Signing choices are unavailable"))?;
    let choices: Vec<_> = teams.keys().collect();
    let saved = choices
        .iter()
        .position(|team| Some(team.as_str()) == report["savedTeamId"].as_str());
    let labels = teams
        .iter()
        .map(|(team, name)| format!("{team} — {}", name.as_str().unwrap_or("")))
        .collect();
    let selected = choose("Choose a signing team", labels, saved).map_err(|error| {
        Error::new("HOST_IO", "Cannot display signing selection").details(json!({
            "cause": error.to_string(),
            "manualAction": "Use a cursor-capable terminal, or run testrun signing setup --runner-team-id TEAM_ID --non-interactive"
        }))
    })?;
    let selected = selected.ok_or_else(|| {
        Error::new(
            "SETUP_CANCELLED",
            "Signing setup cancelled; the saved default was preserved",
        )
    })?;
    choices
        .get(selected)
        .map(|team| (*team).clone())
        .ok_or_else(|| Error::new("INTERNAL", "Signing selection is unavailable"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_setup::tests::Fixture;

    #[test]
    fn mcp_setup_retains_product_tool_defaults_and_json_catalog_probe() {
        let temp = tempfile::tempdir().unwrap();
        let context = mcp_setup_context(temp.path()).unwrap();
        assert_eq!(context.product, "testrun");
        assert_eq!(context.arguments, ["mcp", "serve"]);
        assert_eq!(context.check_arguments, ["--check", "--json"]);
        assert!(context.launch_command.is_none());
        for tool in crate::documentation::tools() {
            let found = context
                .tools
                .iter()
                .find(|candidate| candidate.name == tool["name"].as_str().unwrap())
                .unwrap();
            assert!(found.approved);
        }
    }

    #[test]
    fn mcp_setup_maps_cancellation_and_preserves_other_results() {
        assert_eq!(
            mcp_setup_result(Ok(json!({"cancelled_by_user":true})))
                .unwrap_err()
                .code,
            "SETUP_CANCELLED"
        );
        let applied = json!({"applied":true});
        assert_eq!(mcp_setup_result(Ok(applied.clone())).unwrap(), applied);
        let error = mcp_setup_result(Err(crate::agent_setup::Error::Invalid(
            "Configuration changed".into(),
        )))
        .unwrap_err();
        assert_eq!(error.code, "AGENT_SETUP");
        assert_eq!(error.message, "Configuration changed");
    }

    #[test]
    fn mcp_setup_maps_failure_kinds_and_partial_progress() {
        use crate::agent_setup::Error as SetupError;
        for (error, code) in [
            (
                SetupError::Conflict("Edited entry".into()),
                "AGENT_SETUP_CONFLICT",
            ),
            (
                SetupError::ConfirmationRequired("Use a terminal".into()),
                "AGENT_SETUP_CONFIRMATION_REQUIRED",
            ),
            (
                SetupError::InvalidSelection("Unknown agent".into()),
                "AGENT_SETUP_INVALID_SELECTION",
            ),
            (
                SetupError::CatalogCheck("Invalid catalog".into()),
                "AGENT_SETUP_CATALOG_CHECK",
            ),
            (
                SetupError::Io(io::Error::other("Cannot read settings")),
                "AGENT_SETUP_IO",
            ),
        ] {
            assert_eq!(mcp_setup_error(error).code, code);
        }
        let error = mcp_setup_error(SetupError::Partial {
            completed: vec!["codex".into()],
            pending: vec!["qwen".into()],
            source: Box::new(SetupError::Conflict("Concurrent edit".into())),
        });
        assert_eq!(error.code, "AGENT_SETUP_PARTIAL");
        let details = error.details.unwrap();
        assert_eq!(details["completed"], json!(["codex"]));
        assert_eq!(details["pending"], json!(["qwen"]));
        assert_eq!(details["cause"]["code"], "AGENT_SETUP_CONFLICT");
        assert_eq!(details["cause"]["message"], "Concurrent edit");
    }

    #[tokio::test]
    async fn terminal_first_connection_reports_automatic_from_real_settings() {
        let root = tempfile::tempdir().unwrap();
        let fixture = Fixture::new(root.path(), &["ACME000001"]);
        for (source, lookups) in [("automatic", 1), ("saved", 2)] {
            let result = execute_with_selection(
                true,
                || std::future::ready(fixture.connect()),
                |_| async { panic!("A sole or saved team does not require a prompt") },
            )
            .await
            .unwrap();
            assert_eq!(result["runnerSetup"]["signing"]["source"], source);
            assert_eq!(
                result["runnerSetup"]["signing"]["runnerTeamId"],
                "ACME000001"
            );
            assert_eq!(fixture.discoveries.get(), lookups);
        }
    }

    #[tokio::test]
    async fn setup_command_selects_silently_and_reports_automatic_setup_or_saved() {
        for (may_prompt, requested, expected, lookups) in [
            (true, None, "automatic", 2),
            (false, None, "automatic", 1),
            (true, Some("ACME000001"), "setup", 1),
            (false, Some("ACME000001"), "setup", 1),
        ] {
            let root = tempfile::tempdir().unwrap();
            let fixture = Fixture::new(root.path(), &["ACME000001"]);
            let selected = select_default(
                requested.map(str::to_owned),
                may_prompt,
                || std::future::ready(fixture.inspect()),
                |requested| std::future::ready(fixture.setup(requested.as_deref())),
                |_| async { panic!("A sole team or explicit selection must not prompt") },
            )
            .await
            .unwrap();
            let result = json!({"signing":selected});
            assert_eq!(result["signing"]["source"], expected);
            assert_eq!(fixture.discoveries.get(), lookups);
            let before = std::fs::read(fixture.path()).unwrap();
            let saved = select_default(
                None,
                false,
                || async {
                    panic!("Unattended setup must not inspect to decide whether to prompt")
                },
                |requested| std::future::ready(fixture.setup(requested.as_deref())),
                |_| async { panic!("Unattended setup must not prompt") },
            )
            .await
            .unwrap();
            assert_eq!(json!(saved)["source"], "saved");
            assert_eq!(std::fs::read(fixture.path()).unwrap(), before);
        }
    }

    #[tokio::test]
    async fn setup_command_reports_saved_for_enter_and_setup_for_a_replacement() {
        for (choice, team, source) in [(0, "ACME000001", "saved"), (1, "ACME000002", "setup")] {
            let root = tempfile::tempdir().unwrap();
            let fixture = Fixture::new(root.path(), &["ACME000001", "ACME000002"]);
            fixture.setup(Some("ACME000001")).unwrap();
            let before = std::fs::read(fixture.path()).unwrap();
            #[cfg(unix)]
            let metadata = std::fs::metadata(fixture.path()).unwrap();
            let prompts = std::cell::Cell::new(0);
            let selected = select_default(
                None,
                true,
                || std::future::ready(fixture.inspect()),
                |requested| std::future::ready(fixture.setup(requested.as_deref())),
                |report| {
                    prompts.set(prompts.get() + 1);
                    std::future::ready(prompt_with(&report, |_, _, saved| {
                        assert_eq!(saved, Some(0));
                        Ok(Some(choice))
                    }))
                },
            )
            .await
            .unwrap();
            let result = json!({"signing":selected});
            assert_eq!(result["signing"]["source"], source);
            assert_eq!(result["signing"]["runnerTeamId"], team);
            assert_eq!(prompts.get(), 1);
            if source == "saved" {
                assert_eq!(std::fs::read(fixture.path()).unwrap(), before);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    assert_eq!(
                        std::fs::metadata(fixture.path()).unwrap().ino(),
                        metadata.ino()
                    );
                }
            } else {
                assert_ne!(std::fs::read(fixture.path()).unwrap(), before);
            }
        }
    }

    #[tokio::test]
    async fn setup_command_can_prompt_for_replacing_an_unavailable_saved_team() {
        let root = tempfile::tempdir().unwrap();
        Fixture::new(root.path(), &["ACME000001"])
            .setup(Some("ACME000001"))
            .unwrap();
        let fixture = Fixture::new(root.path(), &["ACME000002"]);
        let selected = select_default(
            None,
            true,
            || std::future::ready(fixture.inspect()),
            |requested| std::future::ready(fixture.setup(requested.as_deref())),
            |report| {
                assert_eq!(report["savedTeamId"], "ACME000001");
                std::future::ready(prompt_with(&report, |_, _, saved| {
                    assert_eq!(saved, None);
                    Ok(Some(0))
                }))
            },
        )
        .await
        .unwrap();
        assert_eq!(json!(selected)["source"], "setup");
        assert_eq!(fixture.inspect().unwrap()["savedTeamId"], "ACME000002");
    }

    #[tokio::test]
    async fn setup_command_returns_unattended_choices_and_never_saves_on_cancel_or_inspection_failure()
     {
        for (ids, may_prompt, expected) in [
            (vec![], true, "SIGNING_TEAM_REQUIRED"),
            (
                vec!["ACME000001", "ACME000002"],
                false,
                "SIGNING_TEAM_REQUIRED",
            ),
            (vec!["ACME000001", "ACME000002"], true, "SETUP_CANCELLED"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let fixture = Fixture::new(root.path(), &ids);
            let error = select_default(
                None,
                may_prompt,
                || std::future::ready(fixture.inspect()),
                |requested| std::future::ready(fixture.setup(requested.as_deref())),
                |report| {
                    assert!(may_prompt && ids.len() > 1);
                    std::future::ready(prompt_with(&report, |_, _, _| Ok(None)))
                },
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, expected);
            assert!(!fixture.path().exists());
        }
        let root = tempfile::tempdir().unwrap();
        let fixture = Fixture::new(root.path(), &["ACME000001"]);
        fixture.setup(None).unwrap();
        std::fs::write(fixture.path(), b"invalid settings").unwrap();
        let error = select_default(
            None,
            true,
            || std::future::ready(fixture.inspect()),
            |_| async { panic!("Failed inspection must stop before setup") },
            |_| async { panic!("Failed inspection must not prompt") },
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "SIGNING_PREFERENCE_INVALID");
        assert_eq!(std::fs::read(fixture.path()).unwrap(), b"invalid settings");
    }
    #[tokio::test]
    async fn successful_connections_preserve_the_runner_source_without_preflight_or_retry() {
        use std::cell::Cell;
        for source in ["automatic", "saved", "override"] {
            let calls = Cell::new(0);
            let expected =
                json!({"runnerSetup":{"signing":{"runnerTeamId":"ACME000001","source":source}}});
            let actual = execute_with_selection(
                true,
                || {
                    calls.set(calls.get() + 1);
                    std::future::ready(Ok(expected.clone()))
                },
                |_| async { panic!("Successful signing must not prompt or select again") },
            )
            .await
            .unwrap();
            assert_eq!(actual, expected);
            assert_eq!(calls.get(), 1);
        }
    }

    fn signing_choices() -> Error {
        Error::new("SIGNING_TEAM_REQUIRED", "Choose a team").details(json!({"availableTeamIds":["ACME000001","ACME000002"],"availableTeams":{"ACME000001":"Acme One","ACME000002":"Acme Two"}}))
    }

    #[tokio::test]
    async fn ambiguous_signing_prompts_then_retries_connection_exactly_once() {
        use std::cell::Cell;
        let calls = Cell::new(0);
        let prompts = Cell::new(0);
        let result = execute_with_selection(
            true,
            || {
                calls.set(calls.get() + 1);
                std::future::ready(if calls.get() == 1 {
                    Err(signing_choices())
                } else {
                    Ok(json!({"runnerSetup":{"signing":{"source":"saved"}}}))
                })
            },
            |report| {
                assert_eq!(calls.get(), 1);
                assert_eq!(
                    report["availableTeamIds"],
                    json!(["ACME000001", "ACME000002"])
                );
                prompts.set(prompts.get() + 1);
                std::future::ready(Ok(()))
            },
        )
        .await
        .unwrap();
        assert_eq!(calls.get(), 2);
        assert_eq!(prompts.get(), 1);
        assert_eq!(result["runnerSetup"]["signing"]["source"], "saved");
    }

    #[tokio::test]
    async fn cancellation_unattended_use_and_other_errors_never_retry() {
        use std::cell::Cell;
        for (may_prompt, code, expected) in [
            (false, "SIGNING_TEAM_REQUIRED", "SIGNING_TEAM_REQUIRED"),
            (true, "SIGNING_TEAM_REQUIRED", "SETUP_CANCELLED"),
            (
                true,
                "SIGNING_IDENTITY_REQUIRED",
                "SIGNING_IDENTITY_REQUIRED",
            ),
            (true, "OUTCOME_UNKNOWN", "OUTCOME_UNKNOWN"),
        ] {
            let calls = Cell::new(0);
            let error = execute_with_selection(
                may_prompt,
                || {
                    calls.set(calls.get() + 1);
                    let mut error = signing_choices();
                    error.code = code.into();
                    std::future::ready(Err(error))
                },
                |_| async { Err(Error::new("SETUP_CANCELLED", "Cancelled")) },
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, expected);
            assert_eq!(calls.get(), 1);
        }
    }

    #[tokio::test]
    async fn failed_selection_retry_is_returned_without_another_prompt() {
        use std::cell::Cell;
        let calls = Cell::new(0);
        let prompts = Cell::new(0);
        let error = execute_with_selection(
            true,
            || {
                calls.set(calls.get() + 1);
                std::future::ready(Err(signing_choices()))
            },
            |_| {
                prompts.set(prompts.get() + 1);
                std::future::ready(Ok(()))
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "SIGNING_TEAM_REQUIRED");
        assert_eq!(calls.get(), 2);
        assert_eq!(prompts.get(), 1);
    }
    #[test]
    fn selection_maps_shared_picker_choices_defaults_cancellation_and_errors() {
        let report = json!({"savedTeamId":"ACME000001","availableTeams":{"ACME000001":"Acme One","ACME000002":"Acme Two"}});
        for (choice, expected) in [(0, "ACME000001"), (1, "ACME000002")] {
            assert_eq!(
                prompt_with(&report, |title, labels, saved| {
                    assert_eq!(title, "Choose a signing team");
                    assert_eq!(labels, ["ACME000001 — Acme One", "ACME000002 — Acme Two"]);
                    assert_eq!(saved, Some(0));
                    Ok(Some(choice))
                })
                .unwrap(),
                expected
            );
        }
        assert_eq!(
            prompt_with(&report, |_, _, _| Ok(None)).unwrap_err().code,
            "SETUP_CANCELLED"
        );
        assert_eq!(
            prompt_with(&report, |_, _, _| Ok(Some(2)))
                .unwrap_err()
                .code,
            "INTERNAL"
        );
        let error = prompt_with(&report, |_, _, _| {
            Err(io::Error::other("terminal unavailable"))
        })
        .unwrap_err();
        assert_eq!(error.code, "HOST_IO");
        assert_eq!(error.details.unwrap()["cause"], "terminal unavailable");
        assert_eq!(
            prompt_with(&json!({}), |_, _, _| panic!(
                "Invalid choices must not open the terminal"
            ))
            .unwrap_err()
            .code,
            "INTERNAL"
        );
        let unavailable =
            json!({"savedTeamId":"ACME000003","availableTeams":{"ACME000001":"Acme"}});
        assert_eq!(
            prompt_with(&unavailable, |_, _, saved| {
                assert_eq!(saved, None);
                Ok(None)
            })
            .unwrap_err()
            .code,
            "SETUP_CANCELLED"
        );
    }
    #[cfg(unix)]
    #[test]
    fn shared_terminal_signing_selection() {
        let Ok(mode) = std::env::var("TESTRUN_SIGNING_PROMPT_PROBE") else {
            let status = std::process::Command::new("python3")
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/signing_prompt.py"
                ))
                .arg(std::env::current_exe().unwrap())
                .status()
                .unwrap();
            assert!(status.success());
            return;
        };
        let report = json!({
            "savedTeamId": if mode == "first" { Value::Null } else { json!("ACME000001") },
            "availableTeams": {"ACME000001":"Acme One", "ACME000002":"Acme Two"}
        });
        let result = prompt_with(&report, |title, labels, saved| {
            choose_team(title, labels, saved, &prompt_context(&report))
        });
        match mode.as_str() {
            "saved" | "rgb" | "ascii" => assert_eq!(result.unwrap(), "ACME000001"),
            "replace" | "number" | "narrow" => assert_eq!(result.unwrap(), "ACME000002"),
            "first" => assert_eq!(result.unwrap(), "ACME000001"),
            "unsupported" => assert_eq!(result.unwrap_err().code, "HOST_IO"),
            _ => assert_eq!(result.unwrap_err().code, "SETUP_CANCELLED"),
        }
    }
    #[test]
    fn only_runner_preparation_without_an_override_can_prompt() {
        use clap::Parser;
        for (args, expected) in [
            (vec!["connect", "--target", "device:Acme", "--setup"], true),
            (vec!["connect", "--target", "device:Acme"], false),
            (
                vec!["connect", "--target", "simulator:Acme", "--setup"],
                false,
            ),
            (vec!["connect", "--target", "android:Acme"], false),
            (
                vec![
                    "connect",
                    "--target",
                    "device:Acme",
                    "--setup",
                    "--runner-team-id",
                    "ACME000001",
                ],
                false,
            ),
        ] {
            let super::super::CommandLine {
                target,
                command: super::super::arguments::Command::Mobile(command),
                ..
            } = super::super::CommandLine::try_parse_from(std::iter::once("testrun").chain(args))
                .unwrap()
            else {
                panic!("Expected mobile command");
            };
            assert_eq!(needs_signing(&Mobile { target, command }), expected);
        }
    }
}
