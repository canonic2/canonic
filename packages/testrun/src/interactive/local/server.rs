//! Retained QA startup, request workflows and inspectable awaited shutdown.
use super::{
    endpoint::{Endpoint, endpoint_bind_error},
    transport::{archived_response, check_owner, fence},
};
use super::{read, write};
use crate::{
    cli::{Action, Mobile},
    error::{Error, Result},
    qa::{Server, journal::Archive},
};
use serde_json::{Value, json};
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    time::Duration,
};
use tokio::net::UnixListener;

pub(crate) async fn serve(input: &Mobile) -> Result<Value> {
    serve_with_owner(input, None, &mut Server::new()).await
}
/// Serve one retained controller. A device actor passes its owner record and
/// the supervisor's server, which keeps native resources if this task unwinds.
#[expect(
    clippy::too_many_lines,
    reason = "Retained startup, cancellation selection and awaited cleanup share one owner boundary"
)]
pub(crate) async fn serve_with_owner(
    input: &Mobile,
    owner: Option<&crate::managed::Owner>,
    server: &mut Server,
) -> Result<Value> {
    let Action::Session {
        android_capture,
        boot,
        window,
        runner_team_id,
        control_path,
        directory,
        app_id: bundle_id,
        setup,
        idle_timeout_ms,
        observation_profile,
    } = &input.command
    else {
        return Err(Error::invalid("Expected session command"));
    };
    let target_id = input
        .target
        .as_deref()
        .ok_or_else(|| Error::invalid("--target must name an explicit discovered target"))?;
    if target_id.starts_with("device:")
        && control_path.as_deref() == Some("xctest")
        && bundle_id.is_none()
    {
        return Err(Error::invalid(
            "Physical CLI sessions require --app-id for app-scoped input",
        ));
    }
    if bundle_id.is_some()
        && control_path.as_deref() != Some("xctest")
        && target_id.starts_with("device:")
    {
        return Err(Error::invalid(
            "--app-id is app-scoped inspection input; select --control-path xctest. HID testing uses explicit full-display scope",
        ));
    }
    let platform = if target_id.starts_with("android:") {
        "android"
    } else {
        "ios"
    };
    let directory = directory.clone().unwrap_or_else(|| {
        PathBuf::from("/tmp").join(format!("testrun-cli-{}", uuid::Uuid::new_v4()))
    });
    if std::fs::symlink_metadata(&directory).is_ok() {
        return Err(Error::new("SESSION_ENDPOINT_CONFLICT", "The new controller directory already exists; existing resources were preserved")
            .details(json!({"blockingState":"endpoint_directory_in_use","phase":"control_endpoint_prepare","mediaAllocated":false,"deviceState":"not_checked"})));
    }
    crate::artifacts::directory(&directory)?;
    let mut endpoint = Endpoint {
        socket: directory.join("control.sock"),
        directory,
        identity: None,
    };
    let listener = UnixListener::bind(&endpoint.socket).map_err(endpoint_bind_error)?;
    std::fs::set_permissions(&endpoint.socket, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| Error::new("HOST_IO", "Cannot restrict CLI endpoint"))?;
    let metadata = std::fs::symlink_metadata(&endpoint.socket)
        .map_err(|_| Error::new("HOST_IO", "Cannot identify owned CLI endpoint"))?;
    endpoint.identity = Some((metadata.dev(), metadata.ino()));
    server.control_proxy = Some(crate::control::channel::Proxy {
        socket: endpoint.socket.clone(),
        epoch: owner.map(|owner| owner.epoch().to_owned()),
    });
    let archive = Archive::create(&endpoint.directory)?;
    server.archive_outcomes(archive.clone());
    if let Some(owner) = owner {
        // Device work starts below; earlier endpoint failures
        // have no remote media allocation to reconcile.
        owner.cleanup(false)?;
    }
    let result = tokio::select! {
        result = Box::pin(async {
        // Startup has not admitted input. A task stop can cancel discovery,
        // allocation or observation; the server retains their cleanup owners.
        let common = tokio::select! {
        startup = async {
        let kind = target_id.split_once(':').map_or("simulator", |(kind, _)| kind);
        // Follow target pages until the explicit target appears; never pick another.
        let mut list = json!({"limit":100,"kind":kind});
        let target = loop {
            let discovered = server.call("target_list", list.clone()).await?;
            if let Some(target) = discovered["targets"].as_array().and_then(|t| t.iter().find(|t| t["targetId"] == target_id)) {
                break target.clone();
            }
            match discovered["nextCursor"].as_str() {
                Some(cursor) => list["cursor"] = json!(cursor),
                None => return Err(Error::new("NOT_FOUND", "Explicit target is not available")),
            }
        };
        let run = server.call("run.begin", json!({"goalIds":["cli"]})).await?;
        let mut open=json!({"runId":run["runId"],"goalId":"cli","target":{"targetId":target["targetId"],"generation":target["generation"]},"mode":"control","setup":setup,"boot":boot,"window":window});
        if let Some(team)=runner_team_id {open["runnerTeamId"]=json!(team);}
        if let Some(path)=control_path {open["controlPath"]=json!(path);}
        if let Some(mode)=android_capture {open["androidCapture"]=json!(mode);}
        let opened = server.call("session_open",open).await?;
        let observation_profile=observation_profile.as_deref().unwrap_or(if opened["controlPath"]=="hid" || android_capture.as_deref()==Some("stream") {"fast"} else {"evidence"});
        let reference = json!({"runId":run["runId"],"goalId":"cli","sessionId":opened["sessionId"]});
        let mut initial = reference.clone(); initial["profile"] = json!(observation_profile);
        let observation = match server.call("screenshot", initial).await {
            Ok(observation) => observation,
            // An app that blocks screenshots is in front: connect anyway. Input
            // checks the display against the input service's size, so that size
            // stands in for the picture's.
            Err(error) if error.code == "CAPTURE_BLOCKED" && platform == "android" => {
                let serial = target_id.strip_prefix("android:").unwrap_or(target_id);
                let (width, height) = crate::android::Android::default().dimensions(serial).await?;
                json!({"display":{"displayId":"main","geometryVersion":format!("{width}x{height}")},"observationError":error})
            }
            Err(error) => return Err(error),
        };
        let mut common = reference.clone();common["platform"]=json!(platform);common["leaseId"] = opened["leaseId"].clone();common["display"] = observation["display"].clone();
        common["selectedTargetId"] = json!(target_id);
        if let Some(owner)=owner{common["applicationRecords"]=json!(owner.application_records());}
        common["observationProfile"] = json!(observation_profile);
        if let Some(bundle) = bundle_id { common["scope"] = json!({"kind":"app","appId":bundle}); }
        if opened["controlPath"]=="hid" {common["scope"]=json!({"kind":"display"});}
        if let Some(owner)=owner {
            let status = server.call("session_status", reference.clone()).await?;
            owner.state("ready",json!({"targetId":status["target"]["targetId"],"androidCapture":status["androidCapture"],"observation":observation,"capabilities":status["target"]["capabilities"],"controlPath":opened["controlPath"],"observationProfile":observation_profile,"runnerSetup":opened["runnerSetup"]}))?;
        }
        if owner.is_none() {
            println!("{}", json!({"ready":true,"session":endpoint.socket,"target":target_id,"controlPath":opened["controlPath"],"observation":observation,"idleTimeoutMs":idle_timeout_ms,"runnerSetup":opened["runnerSetup"]}));
        }
        Ok::<_, Error>(common)
        } => startup?,
        ended = owner_ended(owner) => return ended,
        };
        serve_requests(&mut *server, &listener, common, Duration::from_millis(*idle_timeout_ms),owner,&archive).await
    }) => result,
        result = termination(owner) => result.map(|()| json!({"closed":true,"reason":"signal"})),
        result = endpoint.lost() => result,
    };
    let state_error = owner.and_then(|owner| owner.state("closing", json!({})).err());
    let cleanup = if let Some(owner) = owner {
        if result
            .as_ref()
            .is_ok_and(|value| value["reason"] == "signal")
        {
            finish_managed_exit(server, owner).await
        } else {
            finish_resumable(server, &listener, &archive, owner).await
        }
    } else {
        // A foreground session reports failure and exits after best-effort
        // teardown. It has no managed owner to retain for another disconnect.
        finish_with_endpoint(&listener, Some(&archive), None, server.shutdown()).await
    };
    if let Some(error) = state_error {
        cleanup?;
        return Err(error);
    }
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (_, Err(error)) => Err(error),
    }
}
/// Resolves when a device actor should stop serving: the manager wrote its
/// epoch-fenced stop marker to release the device. A foreground
/// `testrun session` process waits forever here.
pub(super) async fn owner_ended(owner: Option<&crate::managed::Owner>) -> Result<Value> {
    let Some(owner) = owner else {
        return std::future::pending().await;
    };
    owner.stopped().await
}
/// A device actor exits with its manager; a foreground `testrun session`
/// process on a process signal.
async fn termination(owner: Option<&crate::managed::Owner>) -> Result<()> {
    if let Some(owner) = owner {
        return owner.terminated().await;
    }
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| Error::new("HOST_IO", "Cannot observe CLI termination"))?;
    tokio::select! { result=tokio::signal::ctrl_c()=>result.map_err(|_|Error::new("HOST_IO","Cannot observe interrupt")),_=terminate.recv()=>Ok(()) }
}
async fn serve_requests(
    server: &mut Server,
    listener: &UnixListener,
    common: Value,
    idle: Duration,
    owner: Option<&crate::managed::Owner>,
    archive: &Archive,
) -> Result<Value> {
    super::requests::serve(server, listener, common, idle, owner, archive).await
}
/// A managed failure retains the server, leases and endpoint. A new epoch-fenced
/// disconnect request admits another attempt; status never dispatches cleanup.
async fn finish_resumable(
    server: &mut Server,
    listener: &UnixListener,
    archive: &Archive,
    owner: &crate::managed::Owner,
) -> Result<()> {
    let terminated = termination(Some(owner));
    tokio::pin!(terminated);
    loop {
        // A request arriving during cleanup can authorize one later attempt;
        // the marker already present here must never trigger a retry loop.
        let cleanup_request = owner.cleanup_request()?;
        owner.state("closing", json!({}))?;
        let result = tokio::select! {
            result = finish_with_endpoint(
            listener,
            Some(archive),
            Some(owner.epoch()),
            server.shutdown_resumable(),
            ) => result,
            signal = &mut terminated => {
                signal?;
                return finish_managed_exit(server, owner).await;
            }
        };
        owner.cleanup_progress(
            server.cleanup_status(),
            result.as_ref().err(),
            cleanup_request.as_deref(),
        )?;
        let Err(error) = result else {
            return Ok(());
        };
        owner.state("failed", json!({"error":error}))?;
        loop {
            let (mut socket, _) = tokio::select! {
                signal = &mut terminated => {
                    signal?;
                    return finish_managed_exit(server, owner).await;
                }
                accepted = listener.accept() => accepted
                    .map_err(|_| Error::new("HOST_IO", "Cleanup listener failed"))?,
                requested = owner.cleanup_requested(cleanup_request.as_deref()) => { requested?; break; }
            };
            let request = async {
                check_owner(&socket)?;
                let mut input = read(&mut socket).await?;
                if !fence(&mut input, Some(owner.epoch())) {
                    return Err(Error::new(
                        "STALE_OWNER",
                        "This request belongs to another connection",
                    ));
                }
                validate_request(&input)?;
                let close = input["kind"] == "close";
                // Managed retries use the durable request, so a delayed socket
                // copy of the same disconnect cannot start another attempt.
                let response = if close {
                    json!({"ok":true,"data":{"state":"closing","closed":false}})
                } else if input["kind"] == "status" {
                    if let Some(command) = input["commandId"].as_str() {
                        archived_response(archive, command)?
                    } else {
                        json!({"ok":true,"data":{"state":"failed","closed":false,"cleanupError":error,"cleanup":server.cleanup_status()}})
                    }
                } else {
                    json!({"ok":false,"error":{"code":"SESSION_CLOSING","message":"Cleanup is unfinished; retry disconnect before further work"}})
                };
                // Admission survives reply loss; cleanup belongs to this server.
                let _ = tokio::time::timeout(Duration::from_secs(1), write(&mut socket, &response))
                    .await;
                Ok::<_, Error>(())
            };
            if let Ok(Err(error)) = tokio::time::timeout(Duration::from_secs(5), request).await {
                let _ = tokio::time::timeout(
                    Duration::from_secs(1),
                    write(&mut socket, &json!({"ok":false,"error":error})),
                )
                .await;
            }
        }
    }
}
/// A process signal requests one-time exit, even for a retained failed owner.
/// Preserve unverified releases and history rather than waiting for another close.
async fn finish_managed_exit(server: &mut Server, owner: &crate::managed::Owner) -> Result<()> {
    let request = owner.cleanup_request()?;
    let result = server.shutdown().await;
    owner.cleanup_progress(
        server.cleanup_status(),
        result.as_ref().err(),
        request.as_deref(),
    )?;
    result
}
pub(super) async fn finish_with_endpoint(
    listener: &UnixListener,
    archive: Option<&Archive>,
    epoch: Option<&str>,
    cleanup: impl std::future::Future<Output = Result<()>>,
) -> Result<()> {
    tokio::pin!(cleanup);
    loop {
        tokio::select! {
            result = &mut cleanup => return result,
            accepted = listener.accept() => {
                let (mut socket, _) = match accepted {
                    Ok(socket) => socket,
                    Err(_) => {
                        let cleanup_error = cleanup.await.err();
                        return Err(Error::new("HOST_IO", "Closing listener failed")
                            .details(json!({"cleanupError":cleanup_error})));
                    }
                };
                let response = async {
                    check_owner(&socket)?;
                    let mut input = read(&mut socket).await?;
                    if !fence(&mut input,epoch) {return Err(Error::new("STALE_OWNER","This request belongs to another connection"));}
                    let response = if input["kind"] == "status" && let (Some(archive), Some(command)) = (archive, input["commandId"].as_str()) {
                        match archived_response(archive, command) {
                            Ok(response) => response,
                            Err(error) => json!({"ok":false,"error":error}),
                        }
                    } else if matches!(input["kind"].as_str(), Some("status" | "close")) {
                        json!({"ok":true,"data":{"state":"closing","closed":false}})
                    } else {
                        let mut error = Error::new("SESSION_CLOSING", "Owned resources are still closing; no new command was dispatched");
                        error.retryable = true;
                        json!({"ok":false,"error":error})
                    };
                    write(&mut socket, &response).await
                };
                tokio::select! {
                    result = &mut cleanup => return result,
                    _ = tokio::time::timeout(Duration::from_secs(5), response) => {},
                }
            }
        }
    }
}
pub(super) fn validate_request(input: &Value) -> Result<()> {
    let allowed: &[&str] = match input["kind"].as_str() {
        Some("close" | "finalize_recordings") => &["kind"],
        Some("status") => &["kind", "commandId"],
        Some("observe") => &["kind", "profile"],
        Some("control") => &["kind", "operation", "arguments"],
        Some("control_status") => &["kind"],
        Some("control_grant") => &["kind", "channelId"],
        Some("viewer_batch") => &[
            "kind",
            "viewerLeaseId",
            "viewerVersion",
            "commandId",
            "display",
            "steps",
            "leaseAtAdmission",
        ],
        Some("batch") => &[
            "kind",
            "commandId",
            "steps",
            "observe",
            "observeAfterEach",
            "observationProfile",
            "settle",
            "leaseAtAdmission",
            "requestedLeaseId",
        ],
        Some("application") => &[
            "kind",
            "operation",
            "actionId",
            "request",
            "leaseAtAdmission",
            "requestedLeaseId",
        ],
        Some("inspection") => &["kind", "tool", "arguments"],
        Some("playback") => &[
            "kind",
            "tool",
            "arguments",
            "commandId",
            "deadlineUnixMs",
            "leaseAtAdmission",
            "requestedLeaseId",
        ],
        Some("network") => &[
            "kind",
            "tool",
            "arguments",
            "commandId",
            "deadlineUnixMs",
            "operation",
            "record",
            "leaseAtAdmission",
            "requestedLeaseId",
        ],
        _ => return Err(Error::invalid("Unsupported CLI session request")),
    };
    if input
        .as_object()
        .is_none_or(|object| object.keys().any(|key| !allowed.contains(&key.as_str())))
    {
        return Err(Error::invalid("Unexpected local request fields"));
    }
    Ok(())
}
async fn application(
    server: &mut Server,
    common: &Value,
    reference: Value,
    input: Value,
) -> Result<Value> {
    let root = common["applicationRecords"]
        .as_str()
        .ok_or_else(|| Error::invalid("App management requires a task-owned controller"))?;
    let mut request: crate::applications::Request =
        serde_json::from_value(input["request"].clone())
            .map_err(|_| Error::invalid("Invalid app request"))?;
    let target = server.application_target(reference).await?;
    if !controller_target(
        &request.target_id,
        &target,
        common["selectedTargetId"].as_str(),
    ) {
        return Err(Error::new(
            "TARGET_KIND_MISMATCH",
            "App operation does not match the controller target",
        ));
    }
    request.target_id = target;
    crate::applications::execute(
        std::path::Path::new(root),
        input["actionId"]
            .as_str()
            .ok_or_else(|| Error::invalid("actionId is required"))?,
        input["operation"].as_str().unwrap_or(""),
        request,
    )
    .await
}
/// A controller opened from an AVD definition owns a concrete emulator.
/// Accept only its originally selected alias or its resolved target; never
/// resolve an arbitrary caller-supplied AVD name to this controller.
fn controller_target(requested: &str, resolved: &str, selected: Option<&str>) -> bool {
    requested == resolved || selected == Some(requested)
}
async fn inspection(server: &mut Server, reference: Value, input: Value) -> Result<Value> {
    let name = input["tool"]
        .as_str()
        .ok_or_else(|| Error::invalid("Choose an inspection operation"))?;
    if !crate::managed::selected_operation(name) {
        return Err(Error::invalid("Unsupported inspection operation"));
    }
    let mut args = input["arguments"]
        .as_object()
        .cloned()
        .ok_or_else(|| Error::invalid("Inspection arguments must be an object"))?;
    if ["runId", "goalId", "sessionId", "leaseId", "commandId"]
        .iter()
        .any(|key| args.contains_key(*key))
    {
        return Err(Error::invalid(
            "Managed inspection cannot replace session authority",
        ));
    }
    for (key, value) in reference.as_object().unwrap() {
        args.insert(key.clone(), value.clone());
    }
    let args = json!(args);
    let definition = crate::catalog::definition(name)
        .ok_or_else(|| Error::invalid("Inspection operation is not registered"))?;
    crate::schema::validate(&definition["inputSchema"], &args)?;
    server.call(name, args).await
}

async fn playback(
    server: &mut Server,
    common: &Value,
    reference: Value,
    input: Value,
) -> Result<Value> {
    let mut reference = reference;
    reference["leaseId"] = common["leaseId"].clone();
    let name = input["tool"]
        .as_str()
        .ok_or_else(|| Error::invalid("Media tool is required"))?;
    if !crate::playback::operation(name) {
        return Err(Error::invalid("Media tool is not registered"));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let remaining = probe_budget(&input, now)?;
    let budget = remaining.min(Duration::from_secs(if name.ends_with("audio_probe") {
        74
    } else {
        29
    }));
    let deadline = tokio::time::Instant::now() + budget;
    let result = tokio::time::timeout_at(
        deadline,
        server.playback_request(
            reference,
            name,
            input["arguments"].clone(),
            input["commandId"].as_str(),
            deadline,
        ),
    )
    .await;
    match result {
        Ok(result) => result,
        Err(_) => {
            server.cancel_dispatched().await?;
            Err(Error::new(
                if crate::playback::mutation(name) {
                    "MEDIA_OUTCOME_UNKNOWN"
                } else {
                    "TIMEOUT"
                },
                "Media operation deadline expired; inspect controller status before continuing",
            ))
        }
    }
}

/// Network switches run with the caller's remaining budget plus the readback
/// polling it asked for.
async fn network(
    server: &mut Server,
    common: &Value,
    reference: Value,
    input: Value,
) -> Result<Value> {
    let mut reference = reference;
    reference["leaseId"] = common["leaseId"].clone();
    let name = input["tool"]
        .as_str()
        .ok_or_else(|| Error::invalid("Network tool is required"))?;
    if !crate::network::operation(name) {
        return Err(Error::invalid("Network tool is not registered"));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let budget = probe_budget(&input, now)?.min(Duration::from_secs(74));
    let deadline = tokio::time::Instant::now() + budget;
    match tokio::time::timeout_at(
        deadline,
        server.network_request(
            reference,
            name,
            input["arguments"].clone(),
            input["commandId"].as_str(),
        ),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            server.cancel_dispatched().await?;
            Err(Error::new(
                "NETWORK_OUTCOME_UNKNOWN",
                "Network operation deadline expired; inspect controller status before continuing",
            ))
        }
    }
}

pub(super) fn probe_budget(input: &Value, now: u64) -> Result<Duration> {
    let remaining = input["deadlineUnixMs"]
        .as_u64()
        .unwrap_or(0)
        .saturating_sub(now);
    if remaining == 0 {
        return Err(Error::new(
            "TIMEOUT",
            "Media operation deadline expired in the controller queue; no native work was admitted",
        )
        .details(json!({"admitted":false,"dispatched":false})));
    }
    Ok(Duration::from_millis(remaining.min(74000)))
}

#[cfg(test)]
mod audio_deadline_tests {
    use super::*;
    #[test]
    fn queued_probe_keeps_its_original_budget_and_expiration_is_before_admission() {
        assert_eq!(
            probe_budget(&json!({"deadlineUnixMs":1500}), 1000).unwrap(),
            Duration::from_millis(500)
        );
        assert_eq!(
            probe_budget(&json!({"deadlineUnixMs":100000}), 1000).unwrap(),
            Duration::from_secs(74)
        );
        for request in [
            json!({}),
            json!({"deadlineUnixMs":1000}),
            json!({"deadlineUnixMs":999}),
        ] {
            let error = probe_budget(&request, 1000).unwrap_err();
            assert_eq!(error.code, "TIMEOUT");
            assert_eq!(error.details.unwrap()["admitted"], false);
        }
    }
}

pub(super) async fn dispatch(
    server: &mut Server,
    common: &mut Value,
    input: Value,
) -> Result<Value> {
    validate_request(&input)?;
    let reference =
        json!({"runId":common["runId"],"goalId":common["goalId"],"sessionId":common["sessionId"]});
    let mut authorized = common.clone();
    if let Some(lease) = input.get("leaseAtAdmission") {
        authorized["leaseId"] = lease.clone();
    }
    match input["kind"].as_str() {
        Some("control") => {
            let result = server.change_control(&reference, &input).await?;
            if result["owner"] == "agent" {
                common["leaseId"] = result["leaseId"].clone();
            }
            Ok(result)
        }
        Some("playback") => playback(server, &authorized, reference, input).await,
        Some("network") => network(server, &authorized, reference, input).await,
        Some("inspection") => inspection(server, reference, input).await,
        Some("application") => {
            server.read_context(&reference)?.authority.admit(
                authorized["leaseId"].as_str().unwrap_or(""),
                std::time::Instant::now(),
            )?;
            application(server, &authorized, reference, input).await
        }
        Some("observe") => {
            let mut args = reference;
            args["profile"] = input
                .get("profile")
                .cloned()
                .unwrap_or_else(|| common["observationProfile"].clone());
            let observation = server.call("screenshot", args).await?;
            common["display"] = observation["display"].clone();
            Ok(observation)
        }
        Some("close") => {
            server
                .call("run.end", json!({"runId":common["runId"]}))
                .await
        }
        Some("finalize_recordings") => server.finalize_recordings().await,
        Some("status") => {
            let mut args = reference;
            let tool = if input["commandId"].is_string() {
                args["commandId"] = input["commandId"].clone();
                "command_status"
            } else {
                "session_status"
            };
            server.call(tool, args).await
        }
        Some("batch" | "viewer_batch") => {
            batch(server, common, authorized, &reference, input).await
        }
        _ => Err(Error::invalid("Unsupported CLI session request")),
    }
}
async fn batch(
    server: &mut Server,
    common: &mut Value,
    authorized: Value,
    reference: &Value,
    input: Value,
) -> Result<Value> {
    let mut args = authorized;
    if input["kind"] == "viewer_batch" {
        // Viewer input sends one complete action at a time; delays are for agents.
        if let Some(index) = input["steps"].as_array().and_then(|steps| {
            steps
                .iter()
                .position(|step| crate::qa::delay_step(step["tool"].as_str().unwrap_or("")))
        }) {
            return Err(Error::invalid("Viewer input cannot include a delay")
                .at_action(index, Some("delay")));
        }
        let authority = server.read_context(reference)?.authority;
        let credential = authority.admit(
            input["viewerLeaseId"].as_str().unwrap_or(""),
            std::time::Instant::now(),
        )?;
        if input["viewerVersion"].as_u64() != Some(credential.version) {
            return Err(Error::new(
                "STALE_CONTROL_LEASE",
                "Viewer lease version changed",
            ));
        }
        args["leaseId"] = json!(credential.id);
        args["display"] = input["display"].clone();
    }
    args.as_object_mut().unwrap().remove("platform");
    args.as_object_mut().unwrap().remove("applicationRecords");
    args.as_object_mut().unwrap().remove("selectedTargetId");
    for key in ["commandId", "steps", "observe"] {
        args[key] = input[key].clone();
    }
    if input["kind"] == "viewer_batch" {
        args["observe"] = json!(false);
    }
    if let Some(each) = input.get("observeAfterEach") {
        args["observeAfterEach"] = each.clone();
        if each == true && input.get("observationProfile").is_none() {
            args["observationProfile"] = json!("fast");
        }
    }
    if let Some(profile) = input.get("observationProfile") {
        args["observationProfile"] = profile.clone();
    }
    if let Some(settle) = input.get("settle") {
        args["settle"] = settle.clone();
    }
    args["timeoutMs"] = json!(120000);
    let definition = crate::catalog::definition("batch").unwrap();
    crate::schema::validate(&definition["inputSchema"], &args)?;
    let result =
        match tokio::time::timeout(Duration::from_secs(120), server.call("batch", args)).await {
            Ok(result) => result,
            Err(_) => {
                server.cancel_dispatched().await?;
                Err(Error::new(
                    "BATCH_OUTCOME_UNKNOWN",
                    "Batch deadline expired; inspect status before another input",
                ))
            }
        };
    let observation = match &result {
        Ok(value) => &value["observation"],
        Err(error) => error
            .details
            .as_ref()
            .map_or(&Value::Null, |v| &v["observation"]),
    };
    if observation["display"].is_object() {
        common["display"] = observation["display"].clone();
    }
    result
}

#[cfg(test)]
mod inspection_tests {
    use super::*;
    #[tokio::test]
    async fn selected_inspection_fences_authority_and_unsupported_operations() {
        let mut server = Server::new();
        let reference = json!({"runId":"owned", "goalId":"owned", "sessionId":"missing"});
        for input in [
            json!({"tool":"tap","arguments":{}}),
            json!({"tool":"ui_snapshot","arguments":{"runId":"foreign"}}),
            json!({"tool":"preview_status","arguments":{"sessionId":"foreign","previewId":"s"}}),
        ] {
            assert_eq!(
                inspection(&mut server, reference.clone(), input)
                    .await
                    .unwrap_err()
                    .code,
                "INVALID_ARGUMENT"
            );
        }
        let result = inspection(
            &mut server,
            reference,
            json!({"tool":"preview_status","arguments":{"previewId":"foreign"}}),
        )
        .await;
        assert_eq!(result.unwrap_err().code, "SESSION_NOT_FOUND");
    }
}

#[cfg(test)]
mod viewer_batch_tests {
    use super::*;
    #[tokio::test]
    async fn viewer_requests_with_a_delay_are_refused_before_admission() {
        let mut server = Server::new();
        let mut common = json!({"runId":"owned","goalId":"owned","sessionId":"missing"});
        let tap = json!({"tool":"tap","arguments":{"point":{"x":0.5,"y":0.5}}});
        let request = |steps: Value| json!({"kind":"viewer_batch","viewerLeaseId":"viewer","viewerVersion":2,"commandId":"viewer","steps":steps});
        let error = dispatch(
            &mut server,
            &mut common,
            request(json!([tap, {"tool":"delay","arguments":{"durationMs":100}}])),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "INVALID_ARGUMENT");
        assert_eq!(error.details.unwrap()["actionIndex"], 1);
        // Without a delay the same request reaches session lookup.
        let error = dispatch(&mut server, &mut common, request(json!([tap])))
            .await
            .unwrap_err();
        assert_eq!(error.code, "SESSION_NOT_FOUND");
    }
    #[tokio::test]
    async fn controller_metadata_is_not_forwarded_as_batch_arguments() {
        let mut server = Server::new();
        let mut common = json!({"runId":"owned","goalId":"owned","sessionId":"missing","leaseId":"lease","display":{"displayId":"main","geometryVersion":"720x1280"},"observationProfile":"fast","platform":"android","applicationRecords":"/acme/records","selectedTargetId":"android:avd:Acme"});
        let tap = json!({"tool":"tap","arguments":{"point":{"x":0.5,"y":0.5}}});
        // The strict batch schema refuses unknown fields; only stripped
        // controller metadata lets the request reach session lookup.
        let error = dispatch(
            &mut server,
            &mut common,
            json!({"kind":"batch","commandId":"acme","steps":[tap],"observe":false}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "SESSION_NOT_FOUND");
    }
}

#[cfg(test)]
mod application_target_tests {
    use super::*;
    #[test]
    fn controller_accepts_its_selected_alias_or_resolved_target_only() {
        let resolved = "android:emulator-5554";
        let selected = Some("android:avd:Acme");
        assert!(controller_target(resolved, resolved, selected));
        assert!(controller_target("android:avd:Acme", resolved, selected));
        assert!(!controller_target("android:avd:Other", resolved, selected));
        assert!(!controller_target(
            "android:emulator-5556",
            resolved,
            selected
        ));
        assert!(!controller_target("android:avd:Acme", resolved, None));
    }
}
