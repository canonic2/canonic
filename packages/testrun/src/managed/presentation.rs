//! Simple QA actions and results; process/lease details remain in private diagnostics.
use crate::error::Error;
use serde_json::{Value, json};

pub(crate) fn clean(mut value: Value) -> Value {
    match &mut value {
        Value::Object(object) => {
            if let Some(env) = object.get_mut("env").and_then(Value::as_object_mut) {
                for value in env.values_mut() {
                    *value = json!("<redacted>");
                }
            }
            for key in [
                "runId",
                "goalId",
                "sessionId",
                "leaseId",
                "requestedLeaseId",
                "leaseVersion",
                "session",
                "socket",
                "lockPath",
                "reportedOwnerPid",
                "pid",
                "ownerEpoch",
                "commandId",
                "cleanupRequestId",
            ] {
                object.remove(key);
            }
            for item in object.values_mut() {
                *item = clean(item.take());
            }
        }
        Value::Array(items) => {
            for item in items {
                *item = clean(item.take());
            }
        }
        _ => {}
    }
    value
}
pub(crate) fn validation_error(mut error: Error) -> Error {
    error
        .message
        .push_str(". No actions dispatched; correct the arguments and resubmit.");
    let mut details = error.details.take().unwrap_or(json!({}));
    if !details.is_object() {
        details = json!({"cause":details});
    }
    details["phase"] = json!("validation");
    details["admitted"] = json!(false);
    details["inputDispatched"] = json!(false);
    details["actionsDispatched"] = json!(0);
    error.details(details)
}

/// Observation deadlines preserve their evidence without implying input dispatch.
pub(super) fn observation_timeout(mut error: Error) -> Error {
    let details = clean(error.details.take().unwrap_or(json!({})));
    error.details(json!({"nextAction":"wait","result":details}))
}

/// Target and manager refusals whose details are their documented contract:
/// they stay at the top level beside the next action.
const DIRECT: &[(&str, &str)] = &[
    ("PROJECT_NOT_CONFIGURED", "config_show"),
    ("PROJECT_TASK_MISSING", "config_show"),
    ("PROJECT_NOT_READY", "project_status"),
    ("PROJECT_SERVER_EXISTS", "project_status"),
    ("EXPO_PROJECT_MISMATCH", "project_status"),
    ("TARGET_REQUIRED", "status"),
    ("OWNER_DRAINING", "status"),
    ("TASK_ENDED", "status"),
];

pub(crate) fn error(mut error: Error) -> Error {
    if error.code.starts_with("SIGNING_") {
        error.details = error.details.take().map(clean);
        return error;
    }
    if let Some((_, next)) = DIRECT.iter().find(|(code, _)| *code == error.code) {
        let mut details = error.details.take().unwrap_or_else(|| json!({}));
        if details.get("nextAction").is_none() && details.is_object() {
            details["nextAction"] = json!(next);
        }
        return error.details(details);
    }
    if error.details.as_ref().is_some_and(|details| {
        details["phase"] == "validation" && details["inputDispatched"] == false
    }) {
        let details = clean(error.details.take().unwrap());
        return error.details(json!({"nextAction":"act","result":details}));
    }
    let Some((message, next)) = guidance(&error.code) else {
        let details = clean(error.details.take().unwrap_or(json!({})));
        return error.details(json!({"nextAction":"status","result":details}));
    };
    error.message = message.into();
    error.retryable = matches!(
        error.code.as_str(),
        "RECOVERING"
            | "TARGET_BUSY"
            | "DEVICE_IN_USE"
            | "CAPTURE_CONFLICT"
            | "RUNNER_BUSY"
            | "RELEASE_PENDING"
            | "CLEANUP_FAILED"
            | "CONTROL_NOT_OWNED"
    );
    let details = clean(error.details.take().unwrap_or(json!({})));
    error.details(json!({"nextAction":next,"result":details}))
}
fn guidance(code: &str) -> Option<(&'static str, &'static str)> {
    Some(match code {
        "OWNER_PROTOCOL_UNSUPPORTED" => (
            "Finish cleanup with the owning Testrun build before using this protocol.",
            "use_owning_build",
        ),
        "VERSION_MISMATCH" => (
            "The selected controller uses another build. Reconnect to finish its cleanup and start the current build; no input was repeated.",
            "connect",
        ),
        "DEVELOPER_SUPPORT_REQUIRED" => (
            "Compatible developer support exposing display and HID services is required. Use doctor, then developer_support_install on the selected device.",
            "developer_support_install",
        ),
        "HOST_ACCESS_REQUIRED" => (
            "The QA host is blocking Testrun's local or USB sockets. Grant this invocation host access through the QA integration, then connect again.",
            "connect",
        ),
        "SESSION_ENDPOINT_CONFLICT" => (
            "Testrun could not create a fresh controller endpoint after recovery. Existing resources were preserved; connect again.",
            "connect",
        ),
        "USB_UNAVAILABLE" | "DEVICE_DISCONNECTED" => (
            "The selected device has no USB connection. Reconnect that device, then connect again.",
            "connect",
        ),
        "DEVICE_LOCKED" => ("Unlock the selected device, then connect again.", "connect"),
        "DEVICE_MEDIA_UNAVAILABLE" => (
            "The selected device's video transport could not open after recovery. Check its USB connection and device trust, then connect again.",
            "connect",
        ),
        "TARGET_BUSY" | "DEVICE_IN_USE" => (
            "Another Testrun manager or process holds this device outside this manager. Wait for it to release the device.",
            "status",
        ),
        "RUNNER_BUSY" => (
            "The inspection runner is still working; this request was not queued. Check status, or disconnect and reconnect with runner setup to recover.",
            "status",
        ),
        "INSPECTION_TIMEOUT" => (
            "Accessibility inspection expired. Check inspection health in status; disconnect and reconnect with runner setup if it remains busy. Input was not replayed.",
            "status",
        ),
        "CAPTURE_CONFLICT" => (
            "Video is still in use or finishing cleanup. Check status before reconnecting.",
            "status",
        ),
        "CAPTURE_STALLED" => (
            "Live video is unavailable after recovery. Check that the selected device is awake and unlocked, then reconnect.",
            "connect",
        ),
        "NOT_CONNECTED" => ("That target is not connected. Connect it first.", "connect"),
        "CONTROL_NOT_OWNED" | "STALE_CONTROL_LEASE" | "CONTROL_HANDOFF_PENDING" => (
            "Input authority changed or a handoff is pending. Read control status and explicitly acquire the current lease before new input.",
            "control_status",
        ),
        "SESSION_NOT_FOUND" | "SESSION_ENDPOINT_LOST" | "TRANSPORT_IO" => (
            "The connection was interrupted. Check status; Testrun will preserve the last action without repeating it.",
            "status",
        ),
        "BATCH_OUTCOME_UNKNOWN" | "OUTCOME_UNKNOWN" | "TIMEOUT" => (
            "The action may have run. Testrun will not repeat it. Inspect its status and current screen before choosing another action.",
            "status",
        ),
        "BATCH_FAILED" => (
            "The action or its observation failed. Inspect the retained results and current screen before further input.",
            "status",
        ),
        "SESSION_UNCERTAIN" => (
            "The previous input needs recovery. Reconnect to this device for a fresh observation; the previous action will not be repeated.",
            "connect",
        ),
        "RECOVERING" => (
            "Testrun is still finishing recovery. Check status before further input.",
            "status",
        ),
        "RUNNER_SETUP_REQUIRED" => (
            "No action sent: inspection, evidence screenshots and guarded typing need the signed XCTest runner. Call connect with setup: true; signing uses the saved team or first-use selection. HID remains available. Host signing default: testrun signing setup.",
            "connect",
        ),
        "UNTRUSTED_DEVELOPER" => (
            "User: trust the runner in iPhone Settings > General > VPN & Device Management > Developer App. Then connect with setup: true; Personal Team profiles expire after 7 days.",
            "connect",
        ),
        "RUNNER_PROFILE_INVALID" | "RUNNER_PROFILE_EXPIRED" | "RUNNER_SIGNATURE_INVALID" => (
            "Runner signing or provisioning is invalid/expired. Inspect the cause, renew signing in Xcode, then connect with setup: true. Personal Team profiles expire after 7 days.",
            "connect",
        ),
        "RUNNER_BUILD_FAILED" | "RUNNER_INSTALL_FAILED" | "RUNNER_CONFIGURE_FAILED" => (
            "Runner setup failed. Inspect the reported step, diagnostic and private log; fix the Xcode/devicectl cause, then connect with setup: true.",
            "connect",
        ),
        "CLEANUP_FAILED" | "RELEASE_PENDING" => (
            "This device's release is unfinished. Testrun retries it automatically and no other device is affected; status reports the cause.",
            "status",
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_choices_and_repair_guidance_survive_the_managed_boundary() {
        for code in [
            "SIGNING_TEAM_REQUIRED",
            "SIGNING_IDENTITY_REQUIRED",
            "SIGNING_PREFERENCE_INVALID",
        ] {
            let details = json!({"runnerTeamId":"ACME000001","availableTeamIds":["ACME000002"],"availableTeams":{"ACME000002":"Acme"},"nextAction":"testrun signing setup"});
            let result = error(
                Error::new(code, "Select or repair development signing").details(details.clone()),
            );
            assert_eq!(result.code, code);
            assert_eq!(result.details.unwrap(), details);
        }
    }
}
