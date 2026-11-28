//! Translate managed CLI arguments into the same manager workflows as MCP.
//! `--target` is the tool's `targetId`: a connected target. Without it a
//! command uses the only connected target.
use super::Client;
use crate::error::{Error, Result};
use serde_json::{Value, json};

#[expect(
    clippy::too_many_lines,
    reason = "One CLI dispatch maps the managed command families without acquiring separate ownership"
)]
pub(crate) async fn execute(input: &crate::cli::Mobile) -> Option<Result<Value>> {
    use crate::cli::{
        Action, AppCommand, AudioCommand, ClipboardCommand, ControlCommand, MediaCommand,
        NetworkCommand, PreviewCommand, RecordingCommand, UiCommand, VolumeCommand,
    };
    let (name, args) = match &input.command {
        Action::App(AppCommand::Install {
            path,
            artifact_ref,
            replace,
            play_protect,
        }) => ("app_install", {
            let mut args =
                json!({"targetId":input.target,"replace":replace,"playProtect":play_protect});
            if let Some(path) = path {
                args["path"] = json!(path);
            }
            if let Some(reference) = artifact_ref {
                args["artifactRef"] = json!(reference);
            }
            args
        }),
        Action::App(AppCommand::Uninstall { app_id }) => (
            "app_uninstall",
            json!({"targetId":input.target,"appId":app_id}),
        ),
        Action::ManagedOwner { root } => return Some(super::host::serve(root).await),
        Action::Connect {
            engine,
            mode,
            browser_connection,
            context,
            proxy,
            media_fixtures,
            android_capture,
            boot,
            window,
            control_path,
            app_id,
            setup,
            prepare_support,
            runner_team_id,
        } => {
            if let Some(engine) = engine {
                if input.target.is_some()
                    || *boot
                    || *window
                    || *setup
                    || *prepare_support
                    || android_capture.is_some()
                    || control_path.is_some()
                    || app_id.is_some()
                    || runner_team_id.is_some()
                {
                    return Some(Err(Error::invalid(
                        "Browser connect takes --engine and --mode; device options are not accepted",
                    )));
                }
                let mut args =
                    json!({"engine":engine,"mode":mode.as_deref().unwrap_or("headless")});
                media_fixtures.add_to(&mut args);
                browser_connection.add_to(&mut args);
                if let Some(context) = context {
                    match serde_json::from_str::<Value>(context) {
                        Ok(context) => args["context"] = context,
                        Err(_) => return Some(Err(Error::invalid("--context must be JSON"))),
                    }
                }
                if let Some(proxy) = proxy {
                    match serde_json::from_str::<Value>(proxy) {
                        Ok(proxy) => args["proxy"] = proxy,
                        Err(_) => return Some(Err(Error::invalid("--proxy must be JSON"))),
                    }
                }
                return Some(Client::new().call("connect", args).await);
            }
            if mode.is_some() || context.is_some() || proxy.is_some() {
                return Some(Err(Error::invalid(
                    "--mode, --context and --proxy require --engine",
                )));
            }
            let mut args = json!({"targetId":input.target,"boot":boot,"window":window,"setup":setup,"prepareSupport":prepare_support});
            for (field, value) in [
                ("androidCapture", android_capture),
                ("controlPath", control_path),
                ("appId", app_id),
                ("runnerTeamId", runner_team_id),
            ] {
                if let Some(value) = value {
                    args[field] = json!(value);
                }
            }
            ("connect", args)
        }
        Action::Act {
            actions,
            final_only,
            profile,
            action_id,
            timeout_ms,
        } => {
            let value = match serde_json::from_str::<Value>(actions) {
                Ok(value) => value,
                Err(_) => return Some(Err(Error::invalid("Actions must be JSON"))),
            };
            let browser = input
                .target
                .as_deref()
                .is_some_and(|target| target.starts_with("browser:"));
            if browser && (*final_only || profile.is_some())
                || !browser && (action_id.is_some() || timeout_ms.is_some())
            {
                return Some(Err(Error::invalid(
                    "Browser act takes --action-id/--timeout-ms; mobile act takes --final-only/--profile",
                )));
            }
            let mut args = json!({"actions":if value.is_array(){value}else{json!([value])}});
            if !browser {
                args["observeAfterEach"] = json!(!final_only);
            }
            if let Some(id) = action_id {
                args["actionId"] = json!(id);
            }
            if let Some(timeout) = timeout_ms {
                args["timeoutMs"] = json!(timeout);
            }
            if let Some(profile) = profile {
                args["profile"] = json!(profile);
            }
            ("act", args)
        }
        Action::Observe { profile, page } => {
            let mut args = profile
                .as_ref()
                .map_or(json!({}), |profile| json!({"profile":profile}));
            if let Some(page) = page {
                match serde_json::from_str::<Value>(page) {
                    Ok(page) => args["page"] = page,
                    Err(_) => return Some(Err(Error::invalid("--page must be a JSON reference"))),
                }
            }
            ("observe", args)
        }
        Action::Ui(UiCommand::Snapshot {
            scope,
            allow_partial,
            page,
        }) => ("ui_snapshot", {
            let mut args = json!({"scope":scope.value(),"allowPartial":allow_partial});
            page.add_to(&mut args);
            args
        }),
        Action::Ui(UiCommand::Find {
            identifier,
            role,
            label,
            page,
            scope,
            allow_partial,
        }) => {
            let query = crate::cli::find_query(identifier.clone(), role.clone(), label.clone());
            ("ui_find", {
                let mut args =
                    json!({"scope":scope.value(),"query":query,"allowPartial":allow_partial});
                page.add_to(&mut args);
                args
            })
        }
        Action::Ui(UiCommand::Read {
            element,
            allow_partial,
        }) => {
            let element = match serde_json::from_str::<Value>(element) {
                Ok(value) => value,
                Err(_) => return Some(Err(Error::invalid("The element must be JSON"))),
            };
            (
                "ui_read",
                json!({"element":element,"allowPartial":allow_partial}),
            )
        }
        Action::Wait {
            conditions,
            timeout_ms,
            allow_partial,
        } => {
            let conditions = match serde_json::from_str::<Value>(conditions) {
                Ok(value) => value,
                Err(_) => return Some(Err(Error::invalid("Conditions must be a JSON array"))),
            };
            (
                "wait",
                json!({"conditions":conditions,"timeoutMs":timeout_ms,"allowPartial":allow_partial}),
            )
        }
        Action::App(AppCommand::List { all, name, page }) => {
            let mut args = json!({"all":all});
            page.add_to(&mut args);
            if let Some(name) = name {
                args["name"] = json!({"text":name,"mode":"substring"});
            }
            ("app_list", args)
        }
        Action::Clipboard(ClipboardCommand::Read) => ("clipboard_read", json!({})),
        Action::Preview(PreviewCommand::Start { format }) => (
            "preview_start",
            format
                .as_ref()
                .map_or(json!({}), |format| json!({"format":format})),
        ),
        Action::Preview(PreviewCommand::Status { preview_id }) => {
            ("preview_status", json!({"previewId":preview_id}))
        }
        Action::Preview(PreviewCommand::Stop { preview_id }) => {
            ("preview_stop", json!({"previewId":preview_id}))
        }
        Action::Recording(RecordingCommand::Start { max_duration_ms }) => (
            "recording_start",
            max_duration_ms.map_or(json!({}), |ms| json!({"maxDurationMs":ms})),
        ),
        Action::Recording(RecordingCommand::Status { recording_id }) => {
            ("recording_status", json!({"recordingId":recording_id}))
        }
        Action::Recording(RecordingCommand::Stop { recording_id }) => {
            ("recording_stop", json!({"recordingId":recording_id}))
        }
        Action::Status {
            page,
            action_id,
            wait_ms,
            include_details,
            endpoint: None,
            command_id: None,
        } => {
            let mut args = action_id
                .as_ref()
                .map_or(json!({}), |id| json!({"actionId":id}));
            page.add_to(&mut args);
            if let Some(wait) = wait_ms {
                args["waitMs"] = json!(wait);
            }
            if *include_details {
                args["includeDetails"] = json!(true);
            }
            ("status", args)
        }
        Action::Cancel { action_id } => ("cancel", json!({"actionId":action_id})),
        Action::Disconnect => ("disconnect", json!({})),
        Action::Control(ControlCommand::Status) => ("control_status", json!({})),
        Action::Control(ControlCommand::Acquire {
            page,
            owner,
            viewer_id,
            expected_version,
        }) => {
            let mut args = json!({"owner":owner,"expectedVersion":expected_version});
            if let Some(page) = page {
                match serde_json::from_str::<Value>(page) {
                    Ok(page) => args["page"] = page,
                    Err(_) => return Some(Err(Error::invalid("--page must be a JSON reference"))),
                }
            }
            if let Some(viewer) = viewer_id {
                args["viewerId"] = json!(viewer);
            }
            ("control_acquire", args)
        }
        Action::Control(ControlCommand::Release { expected_version }) => (
            "control_release",
            json!({"expectedVersion":expected_version}),
        ),
        Action::Audio(AudioCommand::Probe {
            duration_ms,
            retain_clip,
            accept_route_change,
        }) => {
            let mut args = json!({"durationMs":duration_ms,"retainClip":retain_clip});
            if *accept_route_change {
                args["acceptRouteChange"] = json!(true);
            }
            ("audio_probe", args)
        }
        Action::Media(MediaCommand::Status) => ("media_status", json!({})),
        Action::Media(MediaCommand::Send {
            command,
            media_session,
            system_player: _,
            observe_for_ms,
        }) => {
            let mut args = json!({"command":command,"destination":destination(media_session.as_ref(), "system_player")});
            if let Some(window) = observe_for_ms {
                args["observeForMs"] = json!(window);
            }
            ("media_send", args)
        }
        Action::Volume(VolumeCommand::Get { media_session }) => (
            "volume_get",
            json!({"destination":destination(media_session.as_ref(), "music_stream")}),
        ),
        Action::Volume(VolumeCommand::Set {
            media_session,
            value,
            adjust,
            observe_for_ms,
        }) => {
            let mut args =
                json!({"destination":destination(media_session.as_ref(), "music_stream")});
            if let Some(value) = value {
                args["value"] = json!(value);
            }
            if let Some(adjust) = adjust {
                args["adjust"] = json!(adjust);
            }
            if let Some(window) = observe_for_ms {
                args["observeForMs"] = json!(window);
            }
            ("volume_set", args)
        }
        Action::Network(NetworkCommand::Set {
            mode,
            method,
            timeout_ms,
        }) => {
            let mut args = json!({"mode":mode});
            if let Some(method) = method {
                args["method"] = json!(method);
            }
            if let Some(timeout) = timeout_ms {
                args["timeoutMs"] = json!(timeout);
            }
            ("network_set", args)
        }
        _ => return None,
    };
    let mut args = args;
    if let Some(target) = &input.target {
        args["targetId"] = json!(target);
    }
    let mut client = Client::new();
    Some(crate::device_setup::managed_call(&mut client, name, args).await)
}

/// The manager's control subset: its pool, blockers, release retry, stop
/// and drain.
pub(crate) async fn manager(
    command: &crate::cli::ManagerCommand,
    target: Option<&str>,
) -> Result<Value> {
    use crate::cli::{ManagerCommand, ManagerOperationCommand};
    let client = Client::new();
    match command {
        ManagerCommand::Status => client.control("pool", json!({})).await,
        ManagerCommand::Blockers => client.control("blockers", json!({})).await,
        ManagerCommand::Retry => {
            let fields = target.map_or(json!({}), |target| json!({"targetId":target}));
            client.control("retry", fields).await
        }
        ManagerCommand::Operation(ManagerOperationCommand::Show { operation_id }) => {
            client
                .control("operation", json!({"operationId":operation_id}))
                .await
        }
        ManagerCommand::Stop => client.control("retire", json!({})).await,
        ManagerCommand::Drain => client.control("drain", json!({})).await,
    }
}

/// A media destination: the named Android media session, or `fallback`.
fn destination(media_session: Option<&String>, fallback: &str) -> Value {
    media_session.map_or(
        json!({"kind":fallback}),
        |id| json!({"kind":"media_session","id":id}),
    )
}
