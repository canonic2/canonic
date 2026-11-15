//! Bounded native leaf-command orchestration; unknown identity/effects never launch.
use crate::cli;
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
use crate::{enforcement, policy, review, runtime::Project};
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
use serde_json::{Value, json};
use std::process::ExitCode;
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
use std::{path::Path, time::Instant};

#[cfg(target_os = "macos")]
mod binding;
#[cfg(any(target_os = "linux", test))]
mod elf;
#[cfg(target_os = "linux")]
mod linux_binding;
#[cfg(any(target_os = "linux", test))]
mod linux_cache;
#[cfg(any(target_os = "macos", test))]
mod macho;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod process;
#[cfg(target_os = "linux")]
use linux_binding as binding;
#[cfg(windows)]
mod windows_binding;
#[cfg(windows)]
mod windows_git;
#[cfg(any(windows, test))]
mod windows_pe;
#[cfg(windows)]
mod windows_process;
#[cfg(windows)]
use windows_binding as binding;
#[cfg(windows)]
use windows_process as process;

mod approval;
mod git;
#[cfg(not(windows))]
pub(crate) use git::{EXECUTABLE as GIT_EXECUTABLE, SETTINGS as GIT_SETTINGS};
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
mod launch;
#[cfg(all(test, any(target_os = "macos", target_os = "linux", windows)))]
pub(crate) use approval::capture as approval_fixture;
pub(crate) use approval::{
    evaluate_approval, valid_approval_binding, valid_evaluated_approval_binding,
    verify_approval_binding,
};

#[cfg(not(windows))]
fn supported(request: &crate::policy::Request) -> bool {
    match request.executable.as_str() {
        "/bin/echo" => true,
        "/usr/bin/true" | "/usr/bin/false" => request.arguments.is_empty(),
        "/bin/sleep" => {
            request.arguments.len() == 1
                && request.arguments[0]
                    .parse::<f64>()
                    .is_ok_and(|n| n.is_finite() && (0.0..=86_400.0).contains(&n))
                && request.arguments[0]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'.')
        }
        git::EXECUTABLE => git::supported(request),
        _ => false,
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", windows, test))]
#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("unsupported native execution binding")]
    Unsupported,
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[error("execution binding changed")]
    Changed,
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[error("owned processes did not stop within the cleanup observation window")]
    CleanupIncomplete,
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[error("execution interrupted before launch")]
    Interrupted(u8),
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub(crate) fn run(input: &cli::Run) -> ExitCode {
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    {
        match execute(input) {
            Ok(code) => ExitCode::from(code),
            Err(error) => {
                eprintln!("Shield blocked execution: {error}");
                let code = match error.downcast_ref::<enforcement::Error>() {
                    Some(enforcement::Error::ApprovalRequired) => 3,
                    Some(enforcement::Error::Denied) => 4,
                    _ => match error.downcast_ref::<Error>() {
                        Some(Error::Interrupted(code)) => *code,
                        _ => 2,
                    },
                };
                ExitCode::from(code)
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        let _ = input;
        eprintln!("Shield blocked execution: no verified native binding for this platform");
        ExitCode::from(2)
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
fn execute(input: &cli::Run) -> Result<u8, Box<dyn std::error::Error>> {
    let started = Instant::now();
    let signals = process::Signals::new()?;
    let PreparedRun {
        context,
        request,
        operation,
        evidence,
        binding,
        launch,
        resource,
    } = prepare(input)?;
    let mut details = json!({"operation":operation,"request":request,"evaluation":evidence.evaluation(),"identity":evidence.identity(),"binding":binding.metadata(),"timeout_ms":input.timeout_ms,"execution":"unobserved"});
    let mut child = None;
    let mut prelaunch_error = None;
    let handoff = review::run_approval::gate(
        &context,
        &evidence,
        &resource,
        input.approval_request.as_deref(),
        || {
            if signals.interrupted()
                || started.elapsed().as_millis() >= u128::from(input.timeout_ms)
            {
                prelaunch_error = Some(Error::Interrupted(signals.exit_code().unwrap_or(124)));
                return false;
            }
            match binding.verify() {
                Ok(()) => true,
                Err(error) => {
                    prelaunch_error = Some(error);
                    false
                }
            }
        },
        |decision| {
            details["approval"] = decision["approval"].clone();
            details["code"] = decision["code"].clone();
            if signals.interrupted()
                || started.elapsed().as_millis() >= u128::from(input.timeout_ms)
            {
                return Err(Error::Interrupted(signals.exit_code().unwrap_or(124)));
            }
            binding.verify()?;
            child = Some(process::OwnedChild::spawn(&launch)?);
            Ok(())
        },
    );
    // A release failure may follow successful spawn. Keep child ownership outside
    // the callback so every error path still terminates/reaps it.
    let disposition = if child.is_some() { "allow" } else { "deny" };
    details["execution"] = json!(if child.is_some() {
        "spawned"
    } else {
        "not-launched"
    });
    details["child_pid"] = json!(child.as_ref().map(process::OwnedChild::id));
    if let Ok((decision, _)) = &handoff {
        details["approval"] = decision["approval"].clone();
        details["code"] = decision["code"].clone();
    }
    // Stop a child spawned before a failed handoff before recording, which can
    // wait for the history lock.
    let aborted = match &handoff {
        Err(_) => child.take().map(|mut child| child.abort()),
        Ok(_) => None,
    };
    let receipt = observe(&context, details, disposition);
    match handoff {
        Ok((decision, result)) => {
            if let Some(result) = result {
                result?;
            }
            if decision["allowed"] != true {
                return Err(refusal(&context, &decision));
            }
        }
        Err(error) => {
            match aborted {
                Some(Ok(outcome)) => record_completion(&context, &receipt, &operation, &outcome),
                Some(Err(cleanup)) => eprintln!("Shield cleanup after handoff failure: {cleanup}"),
                None => {}
            }
            return Err(prelaunch_error.map_or_else(|| error.into(), Into::into));
        }
    }
    let mut child = child.ok_or(Error::Changed)?;
    let outcome = match child.wait(&signals, started, input.timeout_ms) {
        Ok(outcome) => outcome,
        Err(error) => {
            match child.abort() {
                Ok(outcome) => record_completion(&context, &receipt, &operation, &outcome),
                Err(cleanup) => eprintln!("Shield cleanup after wait failure: {cleanup}"),
            }
            return Err(error.into());
        }
    };
    record_completion(&context, &receipt, &operation, &outcome);
    Ok(outcome.code)
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
fn launch_request(request: &policy::Request) -> policy::Request {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if request.executable == git::EXECUTABLE {
        return git::launch(request);
    }
    #[cfg(windows)]
    if windows_git::application_directory(request).is_some() {
        return git::launch(request);
    }
    request.clone()
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
struct PreparedRun {
    context: review::Context,
    request: policy::Request,
    operation: Value,
    evidence: crate::runtime::EvaluatedRequest,
    binding: binding::Binding,
    launch: launch::EffectiveLaunch,
    resource: Value,
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
fn refusal(context: &review::Context, decision: &Value) -> Box<dyn std::error::Error> {
    let in_session = context.agent.is_some();
    if let Some(id) = decision["approval"]["request_id"].as_str() {
        eprintln!("{}", request_line(id, decision, in_session));
        if decision["code"] == "approval-required" {
            if in_session {
                eprintln!(
                    "Call request_approve with {{\"request_id\":\"{id}\"}} for human confirmation, then retry with --approval-request {id} and the same attribution."
                );
            } else {
                eprintln!(
                    "Approve with shield request approve {id} and retry with --approval-request {id}."
                );
            }
        } else {
            eprintln!(
                "This grant cannot launch; a fresh request and operator review are required."
            );
        }
    }
    if decision["code"] == "policy-deny" {
        enforcement::Error::Denied
    } else {
        enforcement::Error::ApprovalRequired
    }
    .into()
}

/// The refused native request's identity, decision code and inspection route.
#[cfg(any(target_os = "macos", target_os = "linux", windows, test))]
fn request_line(id: &str, decision: &serde_json::Value, in_session: bool) -> String {
    format!(
        "Shield native request {id}: {}. Inspect with {}.",
        decision["code"].as_str().unwrap_or("unknown"),
        if in_session {
            format!("request_show using request_id {id}")
        } else {
            format!("shield request show {id}")
        }
    )
}

/// A runtime failure, naming the registered project in the cache repair route.
#[cfg(any(target_os = "macos", target_os = "linux", windows, test))]
fn runtime_failure(error: crate::runtime::Error, project_root: &str) -> Box<dyn std::error::Error> {
    if matches!(error, crate::runtime::Error::CacheUnavailable) {
        let route = crate::agent_message::cache_repair(project_root)
            .unwrap_or_else(|| "shield doctor".into());
        format!("compiled policy folder unusable; run {route} to find the cause").into()
    } else {
        error.into()
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
fn prepare(input: &cli::Run) -> Result<PreparedRun, Box<dyn std::error::Error>> {
    let mut context = review::cli_context(&input.context)?;
    context.surface = "run".into();
    let cwd = input
        .cwd
        .as_deref()
        .unwrap_or(Path::new(&context.project_root))
        .canonicalize()?;
    if !cwd.is_dir()
        || !cwd.starts_with(&context.project_root)
        || protected_cwd(&cwd, Path::new(&context.project_root))
    {
        return Err(Error::Unsupported.into());
    }
    context.cwd = Some(cwd.to_str().ok_or(Error::Unsupported)?.into());
    let request = policy::Request {
        executable: input.argv[0].clone(),
        arguments: input.argv[1..].to_vec(),
        cwd: context.cwd.clone().ok_or(Error::Unsupported)?,
        project: context.project_root.clone(),
    };
    request.validate()?;
    let submitted = request;
    let launch = launch::EffectiveLaunch::prepare(&submitted)?;
    let request = launch.request().clone();
    let operation = json!({"kind":"native-effective-launch","request":request,"environment":launch.environment(),"original_input":submitted});
    let evidence = Project::open(Path::new(&context.project_root))
        .and_then(|project| project.snapshot())
        .and_then(|snapshot| {
            snapshot.evaluate_resource(request.clone(), launch.resource(&submitted))
        })
        .map_err(|error| runtime_failure(error, &context.project_root))?;
    let binding = match binding::Binding::open(&submitted) {
        Ok(binding) => binding,
        Err(error) => {
            observe(
                &context,
                json!({"operation":operation,"request":request,"evaluation":evidence.evaluation(),"identity":evidence.identity(),"code":"unverified-operation","execution":"not-launched"}),
                "deny",
            );
            return Err(error.into());
        }
    };
    let resource = approval::resource(&submitted, &launch, input.timeout_ms, &binding.metadata());
    Ok(PreparedRun {
        context,
        request,
        operation,
        evidence,
        binding,
        launch,
        resource,
    })
}

fn protected_cwd(cwd: &std::path::Path, project: &std::path::Path) -> bool {
    crate::layout::in_shield_files(project, cwd)
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
fn observe(context: &review::Context, mut details: Value, disposition: &str) -> Value {
    details["host_decision"] = json!(disposition);
    details["decision"] = details["evaluation"]["decision"].clone();
    let receipt = review::observations::decision(context, details)
        .unwrap_or_else(|_| json!({"status":"failed","request_id":null,"event_id":null}));
    if receipt["status"] == "failed" {
        eprintln!("Shield history unavailable; execution disposition is unchanged.");
    }
    receipt
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
fn record_completion(
    context: &review::Context,
    receipt: &Value,
    operation: &Value,
    outcome: &process::Outcome,
) {
    if let Err(error) = review::observations::execution_completion(
        context,
        receipt["request_id"].as_str(),
        operation,
        outcome.status,
        outcome.exit_code,
        outcome.signal,
    ) {
        eprintln!("Shield completion history unavailable: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::{request_line, runtime_failure};
    use crate::runtime::Error;

    #[test]
    fn refused_request_lines_print_the_bare_decision_code() {
        let decision = serde_json::json!({"code":"approval-required"});
        assert_eq!(
            request_line("run-acme", &decision, false),
            "Shield native request run-acme: approval-required. Inspect with shield request show run-acme."
        );
        assert!(request_line("run-acme", &serde_json::json!({}), false).contains(": unknown."));
        assert_eq!(
            request_line("run-acme", &decision, true),
            "Shield native request run-acme: approval-required. Inspect with request_show using request_id run-acme."
        );
    }

    #[test]
    fn cache_repair_names_the_registered_project() {
        let project = crate::test_support::absolute("/workspace/acme");
        let message = runtime_failure(Error::CacheUnavailable, &project).to_string();
        assert!(
            message.contains(&format!("shield doctor --project '{project}'")),
            "{message}"
        );
        assert_eq!(
            runtime_failure(Error::Locked, "/workspace/acme").to_string(),
            Error::Locked.to_string()
        );
    }
}
