//! Testrun's payloads inside the manager's library envelopes: capabilities,
//! the product control kinds and structured results and errors. The
//! library owns the socket, peer checks, framing and the admission
//! acknowledgement: a client that never received admission knows the
//! request was not admitted and may retry it.
use crate::error::{Error, Result};
use serde_json::{Value, json};

pub(crate) const MAX_REQUEST_FRAME: usize = crate::browser::transfer::FIXTURE_FRAME_BYTES;
pub(crate) const LEGACY_MAX_FRAME: usize = 1024 * 1024;
/// Retain the reply budget of clients already connected to the manager.
pub(crate) const MAX_REPLY_FRAME: usize = LEGACY_MAX_FRAME;
/// Managers advertising this capability also accept the larger request frames.
pub(crate) const LARGE_FIXTURES: &str = "browser_large_fixtures_v1";
pub(crate) const BROWSER_STATUS_V2: &str = "browser_status_v2";
pub(crate) const NATIVE_TRACE_ENVELOPE: &str = "diagnostic_native_trace_envelope_v1";

/// What this manager protocol provides; a client refuses before admission
/// when a capability it needs is missing.
pub(crate) const CAPABILITIES: &[&str] = &[
    "device_queues",
    "control",
    "recovery",
    "drain",
    "task_end",
    "browser_lifecycle",
    "browser_batches",
    "browser_control",
    "browser_workflows_v2",
    "browser_native_chrome_v1",
    "browser_preparation_v1",
    BROWSER_STATUS_V2,
    "browser_media_fixtures_v1",
    LARGE_FIXTURES,
    "browser_installed_channels_v1",
    "browser_profiles_v1",
    "browser_attachment_v1",
    "browser_attachment_credentials_v2",
    "browser_app_features_v1",
    "test_builds_v1",
    "project_workflows_v1",
    "named_projects_v2",
    "configuration_location_v1",
    "diagnostic_sessions_v1",
    "diagnostic_analysis_v1",
    "diagnostic_access_v1",
    "diagnostic_android_v1",
    "diagnostic_android_state_v1",
    "diagnostic_apple_v1",
    "diagnostic_browser_v1",
    "diagnostic_react_native_v1",
    "diagnostic_dart_v1",
    "diagnostic_native_trace_v1",
    NATIVE_TRACE_ENVELOPE,
    "diagnostic_memory_v1",
    "diagnostic_performance_v1",
    "diagnostic_network_data_v1",
    "diagnostic_scenario_context_v1",
    "diagnostic_runtime_checks_v1",
    "diagnostic_reproduction_v1",
    "diagnostic_testing_v1",
    "diagnostic_frameworks_v1",
    "diagnostic_services_v1",
    "diagnostic_observability_v1",
    "diagnostic_reports_v1",
];

/// The structured error a frame carries.
pub(crate) fn error(value: &Value) -> Error {
    let mut error = Error::new(
        value["code"].as_str().unwrap_or("OWNER_PROTOCOL"),
        value["message"]
            .as_str()
            .unwrap_or("The manager returned an invalid error"),
    );
    error.retryable = value["retryable"].as_bool().unwrap_or(false);
    if !value["details"].is_null() {
        error.details = Some(value["details"].clone());
    }
    error
}

pub(crate) fn result(frame: &Value) -> Result<Value> {
    if frame["ok"] == true {
        Ok(frame["data"].clone())
    } else {
        Err(error(&frame["error"]))
    }
}

pub(crate) fn frame(result: &Result<Value>) -> Value {
    match result {
        Ok(data) => json!({"ok":true,"data":data}),
        Err(error) => json!({"ok":false,"error":error}),
    }
}
