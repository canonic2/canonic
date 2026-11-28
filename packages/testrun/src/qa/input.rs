//! Owned QA input workflows and platform coordination.
use super::Server;
use super::arguments::{SessionRef, deadline, field, only_fields, parse, session_ref};
use crate::error::{Error, Result};
use crate::process;
use crate::targets::Target;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

impl Server {
    pub(super) async fn mutate(&mut self, name: &str, args: Value) -> Result<Value> {
        let authority = self.session(&session_ref(&args)?)?.authority();
        let permit = authority.admit(field(&args, "leaseId")?, std::time::Instant::now())?;
        let result = {
            let operation = self.mutate_owned(name, args);
            tokio::pin!(operation);
            tokio::select! {
                result = &mut operation => Some(result),
                () = authority.lost(&permit) => None,
            }
        };
        if let Some(result) = result {
            return result;
        }
        self.cancel_dispatched().await?;
        Err(Error::new(
            "OUTCOME_UNKNOWN",
            "Input authority was revoked; observe the result and do not replay",
        ))
    }
    async fn mutate_owned(&mut self, name: &str, args: Value) -> Result<Value> {
        let extras = input_fields(name);
        let mut allowed = vec![
            "runId",
            "goalId",
            "sessionId",
            "leaseId",
            "commandId",
            "timeoutMs",
        ];
        allowed.extend(extras);
        only_fields(&args, &allowed)?;
        let reference = session_ref(&args)?;
        let command_id = field(&args, "commandId")?.to_owned();
        if command_id.len() > 128 {
            return Err(Error::invalid("commandId exceeds 128 bytes"));
        }
        let session = self.session(&reference)?;
        if !session.control {
            return Err(Error::new(
                "CONTROL_NOT_OWNED",
                "A current controller lease is required",
            ));
        }
        let signature = json!(format!(
            "{:x}",
            Sha256::digest(json!({"tool":name,"arguments":args}).to_string().as_bytes())
        ));
        if let Some((previous, status)) = session.journal.get(&command_id) {
            if previous != &signature {
                return Err(Error::new(
                    "COMMAND_ID_CONFLICT",
                    "Command ID already has different arguments",
                ));
            }
            return crate::mobile::completed(status.clone());
        }
        if session.journal.len() >= 10000 {
            return Err(Error::new(
                "RESOURCE_LIMIT",
                "Session command journal is full; open a new session",
            ));
        }
        if session.journal.values().any(|(_, status)| {
            matches!(
                status["state"].as_str(),
                Some("dispatched" | "outcome_unknown")
            )
        }) {
            return Err(Error::new(
                "SESSION_UNCERTAIN",
                "A prior command has an unknown outcome; observe it and close this session before acquiring a new controller",
            ));
        }
        let timeout = deadline(args.get("timeoutMs").and_then(Value::as_u64))?;
        let target = self.live_target(&reference).await?;
        let timeout = typing_timeout(timeout, name, &target, &args);
        let args = self.resolve_semantic_input(name, args).await?;
        #[cfg(target_os = "macos")]
        if target.kind == "device"
            && self.session(&reference)?.control_path == "hid"
            && !name.starts_with("app_")
        {
            return self.hid_mutate(&reference, signature, name, args).await;
        }
        if target.kind != "simulator" {
            return self
                .external(
                    reference,
                    signature,
                    crate::mobile::Mutation {
                        operation: name.into(),
                        arguments: args,
                        target,
                        command_id,
                        timeout,
                    },
                )
                .await;
        }
        if matches!(name, "app_launch" | "app_restart") {
            return self
                .simulator_app(&reference, signature, name, &args, &target, timeout)
                .await;
        }
        let command = prepare_simulator(name, &args, &target, &reference, self).await?;
        self.simulator_input_verified(&reference, name);
        self.record_command(
            &reference.session_id,
            command_id.clone(),
            signature.clone(),
            json!({"commandId":command_id,"state":"dispatched","tool":name}),
        )?;
        self.progress(1, "Command dispatched");
        let result = command.perform(timeout).await;
        let status = crate::mobile::status(&command_id, result.map(|_| json!({})));
        self.record_command(&reference.session_id, command_id, signature, status.clone())?;
        self.progress(2, "Native command returned; UI effect requires observation");
        crate::mobile::completed(status)
    }
    /// Simulator launch or restart; the journal records the stage outcomes.
    async fn simulator_app(
        &mut self,
        reference: &SessionRef,
        signature: Value,
        name: &str,
        args: &Value,
        target: &Target,
        timeout: Duration,
    ) -> Result<Value> {
        crate::app_lifecycle::validate(args)?;
        let command_id = field(args, "commandId")?.to_owned();
        self.record_command(
            &reference.session_id,
            command_id.clone(),
            signature.clone(),
            json!({"commandId":command_id,"state":"dispatched","tool":name}),
        )?;
        self.progress(1, "Command dispatched");
        let result =
            crate::app_lifecycle::simulator(name, args, &target.platform_id, timeout).await;
        let status = crate::mobile::status(&command_id, result);
        self.record_command(&reference.session_id, command_id, signature, status.clone())?;
        self.progress(2, "App lifecycle command returned");
        crate::mobile::completed(status)
    }
    #[cfg(target_os = "macos")]
    async fn hid_mutate(
        &mut self,
        reference: &SessionRef,
        signature: Value,
        name: &str,
        args: Value,
    ) -> Result<Value> {
        crate::hid::validate(name, &args)?;
        let command_id = field(&args, "commandId")?.to_owned();
        let source = self
            .session(reference)?
            .video
            .clone()
            .ok_or_else(|| Error::new("CAPTURE_STALLED", "HID session has no video source"))?;
        let (width, height) = source.fresh_geometry().await?;
        let display: crate::ui::Display = parse(args["display"].clone())?;
        display.validate(width, height)?;
        let hid = self
            .session(reference)?
            .hid
            .clone()
            .ok_or_else(|| Error::new("SESSION_UNCERTAIN", "HID controller is not active"))?;
        self.record_command(
            &reference.session_id,
            command_id.clone(),
            signature.clone(),
            json!({"commandId":command_id,"state":"dispatched","tool":name,"controlPath":"hid"}),
        )?;
        let timeout = deadline(args.get("timeoutMs").and_then(Value::as_u64))?;
        let result = tokio::time::timeout(timeout, hid.input(name, &args))
            .await
            .unwrap_or_else(|_| {
                Err(Error::new(
                    "OUTCOME_UNKNOWN",
                    "HID action deadline expired; inspect and reacquire control",
                ))
            });
        if result.is_ok() && matches!(name, "key" | "type_text") {
            crate::targets::capabilities::keyboard(
                &mut self.sessions.get_mut(&reference.session_id).unwrap().target,
            );
        }
        let status = crate::mobile::status(&command_id, result);
        self.record_command(&reference.session_id, command_id, signature, status.clone())?;
        crate::mobile::completed(status)
    }
    async fn external(
        &mut self,
        reference: SessionRef,
        signature: Value,
        mutation: crate::mobile::Mutation,
    ) -> Result<Value> {
        crate::mobile::validate(&mutation)?;
        let args = &mutation.arguments;
        if args.get("display").is_some() {
            let display: crate::ui::Display = parse(args["display"].clone())?;
            if mutation.target.kind != "device"
                && !matches!(mutation.operation.as_str(), "tap" | "long_press" | "swipe")
            {
                let bytes = self.capture_target(&mutation.target, &reference).await?;
                let (width, height) = crate::ui::png_dimensions(&bytes)?;
                display.validate(width, height)?;
            }
        }
        if let Err(error) = crate::mobile::preflight(&mutation).await {
            let status = crate::mobile::status(&mutation.command_id, Err(error));
            self.record_command(
                &reference.session_id,
                mutation.command_id.clone(),
                signature,
                status.clone(),
            )?;
            return crate::mobile::completed(status);
        }
        self.record_command(
            &reference.session_id,
            mutation.command_id.clone(),
            signature.clone(),
            json!({"commandId":mutation.command_id,"state":"dispatched","tool":mutation.operation}),
        )?;
        self.progress(1, "Command dispatched");
        let status = crate::mobile::status(
            &mutation.command_id,
            crate::mobile::perform(&mutation, &self.runners).await,
        );
        self.record_command(
            &reference.session_id,
            mutation.command_id.clone(),
            signature,
            status.clone(),
        )?;
        self.progress(
            2,
            "Platform command returned; UI effect requires observation",
        );
        crate::mobile::completed(status)
    }
}

enum NativeCommand {
    Process(Vec<String>),
    Input(
        std::sync::Arc<crate::simulator::input::Input>,
        crate::simulator::input::protocol::Action,
        [u32; 2],
    ),
}
impl NativeCommand {
    async fn perform(self, timeout: Duration) -> Result<process::Output> {
        match self {
            Self::Process(arguments) => {
                let values: Vec<_> = arguments.iter().map(String::as_str).collect();
                process::run(
                    crate::simulator::device_set::executable(),
                    &values,
                    timeout,
                    64 * 1024,
                )
                .await
            }
            Self::Input(input, action, geometry) => input
                .perform(action, geometry, timeout)
                .await
                .map(|()| process::Output { bytes: Vec::new() }),
        }
    }
}
async fn prepare_simulator(
    name: &str,
    args: &Value,
    target: &Target,
    reference: &SessionRef,
    server: &Server,
) -> Result<NativeCommand> {
    crate::mobile::validate_backend_options(&target.kind, None, name, args)?;
    let mut gesture = None;
    let mut keyboard = None;
    let geometry;
    if matches!(name, "app_terminate" | "open_url") {
        return prepare_app(name, args, target);
    }
    match name {
        "tap" | "long_press" | "swipe" | "key" | "type_text" => {
            let display: crate::ui::Display = parse(
                args.get("display")
                    .cloned()
                    .ok_or_else(|| Error::invalid("display is required"))?,
            )?;
            let (width, height) = server
                .simulator_source(target, reference)
                .await?
                .geometry()
                .await?;
            geometry = [width, height];
            display.validate(width, height)?;
            if matches!(name, "key" | "type_text") {
                keyboard = Some(prepare_keyboard(name, args, target).await?);
            } else {
                let point = |field_name: &str| -> Result<[f64; 2]> {
                    let point: crate::ui::Point = parse(
                        args.get(field_name)
                            .cloned()
                            .ok_or_else(|| Error::invalid(format!("{field_name} is required")))?,
                    )?;
                    point.validate()
                };
                let start = point(if name == "swipe" { "from" } else { "point" })?;
                let end = if name == "swipe" { point("to")? } else { start };
                let duration = if name == "tap" {
                    170
                } else {
                    args.get("durationMs")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| Error::invalid("durationMs is required"))?
                };
                crate::ui::validate_gesture_duration(duration)?;
                gesture = Some((start, end, duration as u32));
            }
        }
        _ => {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Unsupported simulator input operation",
            ));
        }
    }

    let input = server.simulator_input(reference, target).await?;
    if let Some(request) = keyboard {
        Ok(NativeCommand::Input(
            input,
            crate::simulator::input::protocol::Action::Keys {
                strokes: request.strokes()?,
            },
            geometry,
        ))
    } else if let Some((start, end, duration)) = gesture {
        Ok(NativeCommand::Input(
            input,
            crate::simulator::input::protocol::Action::Gesture {
                start,
                end,
                duration,
            },
            geometry,
        ))
    } else {
        Err(Error::new("INTERNAL", "Native command plan is empty"))
    }
}

async fn prepare_keyboard(
    name: &str,
    args: &Value,
    target: &Target,
) -> Result<crate::keyboard::Request> {
    if name == "key" {
        let modifiers: Vec<String> = parse(args.get("modifiers").cloned().unwrap_or(json!([])))?;
        let name = field(args, "key")?.to_owned();
        crate::keyboard::key(&name, &modifiers)?;
        return Ok(crate::keyboard::Request::Key { name, modifiers });
    }
    let text = field(args, "text")?.to_owned();
    crate::keyboard::text(&text)?;
    let snapshot = crate::native::snapshot(&target.platform_id).await?;
    if !crate::ui::focused_editable(&snapshot["root"]) {
        return Err(Error::new(
            "FOCUS_NOT_PROVEN",
            "Native accessibility does not report a focused editable field",
        ));
    }
    Ok(crate::keyboard::Request::Text {
        text,
        replace: args
            .get("replace")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

impl Server {
    pub(super) async fn simulator_input(
        &self,
        reference: &SessionRef,
        target: &Target,
    ) -> Result<std::sync::Arc<crate::simulator::input::Input>> {
        let session = self.session(reference)?;
        let input = if let Some(input) = session.simulator_input.get() {
            input.clone()
        } else {
            let identity = self.simulator_incarnation(reference, target).await?;
            let input =
                crate::simulator::input::Input::start(&target.platform_id, identity).await?;
            // Request workflows serialize session mutations. Store ownership before
            // waiting for native readiness so cancellation cannot orphan startup.
            let _ = session.simulator_input.set(input.clone());
            input
        };
        input.ready().await?;
        Ok(input)
    }
}
/// Android typing may extend the normal budget; explicit deadlines still win.
fn typing_timeout(timeout: Duration, name: &str, target: &Target, args: &Value) -> Duration {
    if name == "type_text"
        && target.kind == "android"
        && args["replace"] != true
        && args.get("timeoutMs").is_none()
    {
        timeout.max(crate::android::text_timeout(
            args["text"].as_str().unwrap_or(""),
        ))
    } else {
        timeout
    }
}

fn input_fields(name: &str) -> &'static [&'static str] {
    match name {
        "app_launch" | "app_restart" => &["appId", "args", "env", "readiness"],
        "app_terminate" => &["appId"],
        "open_url" => &["url"],
        "tap" => &["display", "point", "element", "allowPartial", "scope"],
        "long_press" => &[
            "display",
            "point",
            "element",
            "allowPartial",
            "durationMs",
            "scope",
        ],
        "swipe" => &["display", "from", "to", "durationMs", "scope"],
        "key" => &["display", "key", "modifiers", "scope"],
        "type_text" => &["display", "text", "replace", "scope", "element"],
        _ => &[],
    }
}

fn prepare_app(name: &str, args: &Value, target: &Target) -> Result<NativeCommand> {
    let mut owned = crate::simulator::device_set::DeviceSet::current()?.arguments(&[]);
    match name {
        "app_terminate" => {
            let bundle = crate::app_lifecycle::bundle(args)?;
            owned.extend([
                "terminate".into(),
                target.platform_id.clone(),
                bundle.into(),
            ]);
        }
        "open_url" => {
            let url = crate::mobile::url(args)?;
            owned.extend(["openurl".into(), target.platform_id.clone(), url.into()]);
        }
        _ => {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Unsupported simulator application operation",
            ));
        }
    }
    Ok(NativeCommand::Process(owned))
}

#[cfg(test)]
mod tests {
    use super::prepare_simulator;
    use serde_json::json;
    fn reference() -> super::SessionRef {
        super::SessionRef {
            run_id: "r".into(),
            goal_id: "g".into(),
            session_id: "s".into(),
        }
    }

    #[tokio::test]
    async fn simulator_app_scope_refuses_before_native_work() {
        let target = crate::targets::qa_fixture_target();
        let error = match prepare_simulator(
            "tap",
            &json!({"point":{"x":0.5,"y":0.5},"scope":{"kind":"app","appId":"com.example.acme"}}),
            &target,
            &reference(),
            &super::Server::new(),
        )
        .await
        {
            Ok(_) => panic!("Unsupported scope must not reach native input"),
            Err(error) => error,
        };
        assert_eq!(error.code, "CAPABILITY_UNAVAILABLE");
    }

    #[tokio::test]
    async fn simulator_element_target_refuses_before_native_geometry_or_focus_reads() {
        let target = crate::targets::qa_fixture_target();
        let error = match prepare_simulator(
            "type_text",
            &json!({"text":"Acme","element":{"snapshotId":"snapshot","elementId":"field"}}),
            &target,
            &reference(),
            &super::Server::new(),
        )
        .await
        {
            Ok(_) => panic!("Unsupported selector must not prepare focused typing"),
            Err(error) => error,
        };
        assert_eq!(error.code, "CAPABILITY_UNAVAILABLE");
    }
}
