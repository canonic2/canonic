//! The client every CLI command and MCP server uses to reach the user's
//! manager. It sends its identity with each request, starts an absent manager
//! only for requests whose role allows it, keeps an MCP server's connection
//! open so the manager stays running while the server does, and retries a
//! request only when its non-admission is definite: the manager never
//! acknowledged it.
use super::{
    CONTROL, Caller, Kind, PROTOCOL, data_root, host, identity, storage, transition, wire,
};
use crate::error::{Error, Result};
use canonic_process_kit::net::Stream;
use manager::Target;
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

mod manager;

/// Abandons an admitted request when its caller's future is dropped.
struct Abandon(canonic_process_kit::client::Abandon);
impl Drop for Abandon {
    fn drop(&mut self) {
        self.0.abandon();
    }
}

/// Run a blocking library exchange with the manager for `root`.
async fn exchange<T: Send + 'static>(
    root: &Path,
    start: bool,
    work: impl FnOnce(Target) -> Result<T> + Send + 'static,
) -> Result<T> {
    let target = Target::new(root, start)?;
    tokio::task::spawn_blocking(move || work(target))
        .await
        .map_err(|_| Error::new("INTERNAL", "The manager exchange stopped unexpectedly"))?
}

fn prepare_browser_call(name: &str, args: &Value, message: &mut Value) {
    if !crate::browser::surface::handles(name) {
        return;
    }
    message["requires"] = json!(["device_queues", "browser_lifecycle"]);
    if args["request"]["operation"] == "batch" {
        message["requires"] = json!(["device_queues", "browser_lifecycle", "browser_batches"]);
    }
    if name.starts_with("browser_control_") {
        message["requires"] = json!(["device_queues", "browser_lifecycle", "browser_control"]);
    }
    let requirements = message["requires"].as_array_mut().unwrap();
    if name == "browser_status"
        && (args.get("includeDetails").is_some()
            || (args["operationId"].is_null()
                && (args["waitMs"].as_u64().unwrap_or(0) > 0
                    || (args["targetId"].is_string() && args.get("limit").is_some()))))
    {
        requirements.push(json!(wire::BROWSER_STATUS_V2));
    }
    if crate::browser::api::requires_workflows(args) {
        requirements.push(json!("browser_workflows_v2"));
    }
    if requires_browser_app_features(&args["request"]) {
        requirements.push(json!("browser_app_features_v1"));
    }
    if requires_large_browser_fixtures(&args["request"]) {
        requirements.push(json!(wire::LARGE_FIXTURES));
    }
    if name == "browser_connect" && args["engine"] == "chrome" {
        requirements.push(json!("browser_native_chrome_v1"));
    }
    if matches!(name, "browser_connect" | "browser_cancel") {
        requirements.push(json!("browser_preparation_v1"));
    }
    if name != "browser_connect" {
        return;
    }
    for (field, capability) in [
        ("mediaFixtures", "browser_media_fixtures_v1"),
        ("channel", "browser_installed_channels_v1"),
        ("profile", "browser_profiles_v1"),
        ("attachment", "browser_attachment_v1"),
    ] {
        if args.get(field).is_some() {
            requirements.push(json!(capability));
        }
    }
    let attachment = &args["attachment"];
    if attachment.get("credentialPath").is_some() || attachment.get("caPath").is_some() {
        requirements.push(json!("browser_attachment_credentials_v2"));
    }
    // Older managers require an explicit mode in the request envelope.
    if let Some(fields) = message.get_mut("args").and_then(Value::as_object_mut) {
        fields
            .entry("mode")
            .or_insert_with(|| json!(crate::browser::contract::Mode::default()));
    }
}

fn requires_browser_app_features(request: &Value) -> bool {
    match request["operation"].as_str() {
        Some("web_mcp" | "web_mcp_debug" | "browser_extension" | "browser_pwa") => true,
        Some("batch") => request["requests"]
            .as_array()
            .is_some_and(|requests| requests.iter().any(requires_browser_app_features)),
        _ => false,
    }
}

fn requires_large_browser_fixtures(request: &Value) -> bool {
    use crate::browser::transfer::LEGACY_FIXTURE_BASE64_BYTES;
    match request["operation"].as_str() {
        Some("fixture_register") => request["fixture"]["dataBase64"]
            .as_str()
            .is_some_and(|data| {
                data.len() > LEGACY_FIXTURE_BASE64_BYTES
                    || (data.len() == LEGACY_FIXTURE_BASE64_BYTES && !data.ends_with("=="))
            }),
        Some("batch") => request["requests"]
            .as_array()
            .is_some_and(|requests| requests.iter().any(requires_large_browser_fixtures)),
        _ => false,
    }
}

#[derive(Clone)]
pub(crate) struct Client {
    identity: Option<identity::Identity>,
    /// Why this client cannot reach the manager, such as an invalid label.
    refusal: Option<Error>,
    kind: Kind,
    connection_id: Option<String>,
    root: PathBuf,
    root_error: Option<Error>,
    pub(crate) request_id: Option<String>,
    pub(crate) media_deadline: Option<tokio::time::Instant>,
    /// When the MCP server received the current request.
    pub(crate) received: Option<std::time::Instant>,
    /// When this connection's host task end completed: requests received
    /// before it belonged to the ended task and are refused.
    task_ended: Arc<std::sync::Mutex<Option<std::time::Instant>>>,
    /// An open connection that keeps the manager running for this MCP server.
    attachment: Arc<tokio::sync::Mutex<Option<Stream>>>,
    /// Whether this client may start an absent manager.
    start: bool,
}
impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

/// Requests that inspect and never start a manager or renew a device's idle timeout.
fn inspects(name: &str) -> bool {
    if crate::diagnostics::inspects(name) {
        return true;
    }
    matches!(
        name,
        "status"
            | "control_status"
            | "browser_status"
            | "browser_control_status"
            | "build_status"
            | "build_cancel"
            | "build_log"
            | "project_status"
    )
}

impl Client {
    /// A client with the identity of this process's environment.
    pub(crate) fn new() -> Self {
        let (identity, refusal) = match identity::Identity::from_environment() {
            Ok(identity) => (Some(identity), None),
            Err(error) => (None, Some(error)),
        };
        let (root, root_error) = match data_root() {
            Ok(path) => (path, None),
            Err(error) => (PathBuf::new(), Some(error)),
        };
        Self {
            identity,
            refusal,
            kind: Kind::Cli,
            connection_id: None,
            root,
            root_error,
            request_id: None,
            media_deadline: None,
            received: None,
            task_ended: Arc::default(),
            attachment: Arc::default(),
            start: true,
        }
    }
    #[cfg(test)]
    pub(crate) fn in_root(root: &Path, identity: identity::Identity) -> Self {
        Self {
            identity: Some(identity),
            refusal: None,
            kind: Kind::Cli,
            connection_id: None,
            root: root.to_owned(),
            root_error: None,
            request_id: None,
            media_deadline: None,
            received: None,
            task_ended: Arc::default(),
            attachment: Arc::default(),
            start: false,
        }
    }
    /// Make this client an MCP server's connection. It opens its connection
    /// to the manager with its first use request and keeps it until it ends.
    pub(crate) fn attach_mcp(&mut self, connection: String) {
        self.kind = Kind::Mcp;
        self.connection_id = Some(connection);
    }
    fn identity(&self) -> Result<&identity::Identity> {
        if let Some(refusal) = self.root_error.as_ref().or(self.refusal.as_ref()) {
            return Err(refusal.clone());
        }
        self.identity.as_ref().ok_or_else(|| {
            Error::new(
                "IDENTITY_UNRESOLVED",
                "No client identity could be resolved",
            )
        })
    }
    /// End this MCP server's connection. Devices stay connected.
    pub(crate) async fn shutdown(&self) -> Result<()> {
        self.attachment.lock().await.take();
        Ok(())
    }
    pub(crate) async fn call(&mut self, name: &str, mut args: Value) -> Result<Value> {
        if crate::diagnostics::access::local(name) {
            return crate::diagnostics::access::call(name, &args).await;
        }
        if crate::browser::api::selected(name, &args) {
            let (name, args) = crate::browser::api::translate(name, args)?;
            return Box::pin(self.call(&name, args))
                .await
                .map(crate::browser::api::result);
        }
        crate::browser::api::validate_mobile(name, &args)?;
        crate::browser::api::prepare_host_paths(name, &mut args)?;
        let identity = self.identity()?.clone();
        let request_id = self
            .request_id
            .take()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let deadline = self.media_deadline.take();
        let received = self.received.take();
        if !inspects(name)
            && let (Some(received), Some(ended)) = (
                received,
                *self
                    .task_ended
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            )
            && received <= ended
        {
            return Err(Error::new(
                "TASK_ENDED",
                "This host task ended before this request; it was not sent",
            )
            .details(json!({"admitted":false,"dispatched":false})));
        }
        let mut message = json!({
            "protocol":PROTOCOL,"control":CONTROL,"kind":"call","requestId":request_id,
            "requires":["device_queues"],"identity":identity,
            "client":{"kind":self.kind,"connection":self.connection_id},
            "name":name,"args":args,
        });
        prepare_browser_call(name, &args, &mut message);
        diagnostic_requirements(name, &args, &mut message);
        if crate::builds::handles(name) || args.get("artifactRef").is_some() {
            message["requires"] = json!(["device_queues", "test_builds_v1"]);
        }
        if name.starts_with("project_") || name == "build_log" || args.get("prepare").is_some() {
            message["requires"] =
                json!(["device_queues", "test_builds_v1", "project_workflows_v1"]);
        }
        if args.get("root").is_some() && (name.starts_with("project_") || name == "build_start") {
            message["requires"]
                .as_array_mut()
                .unwrap()
                .push(json!("named_projects_v2"));
        }
        // An older manager would reject or ignore the selected file; refuse before admission.
        if args.get("config").is_some() && message["requires"].is_array() {
            message["requires"]
                .as_array_mut()
                .unwrap()
                .push(json!("configuration_location_v1"));
        }
        if let Some(deadline) = deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(Error::new(
                    "TIMEOUT",
                    "Media operation deadline expired before controller dispatch",
                )
                .details(json!({"admitted":false,"dispatched":false})));
            }
            message["deadlineMs"] = json!(remaining.as_millis());
        }
        if inspects(name) {
            return self.inspect(name, args, &identity, &message).await;
        }
        if matches!(name, "disconnect" | "browser_disconnect") {
            let start = transition::recovery_needed(&self.root);
            return match self.send(&message, start).await {
                Err(error) if error.code == "OWNER_ABSENT" => {
                    Ok(json!({"state":"closed","closed":true,"owner":"absent"}))
                }
                // A draining manager releases every device itself once
                // nothing runs on it, as a closing manager reports.
                Err(error) if error.code == "OWNER_DRAINING" => {
                    Ok(json!({"state":"closing","closed":false,"owner":"draining"}))
                }
                result => result,
            };
        }
        if self.kind == Kind::Mcp {
            self.ensure_attached().await?;
        }
        match self.send(&message, self.start).await {
            // The manager this connection was attached to exited or drains;
            // attach to its successor for later requests.
            Err(error) if matches!(error.code.as_str(), "OWNER_ABSENT" | "OWNER_DRAINING") => {
                self.attachment.lock().await.take();
                Err(error)
            }
            result => result,
        }
    }
    async fn inspect(
        &self,
        name: &str,
        args: Value,
        identity: &identity::Identity,
        message: &Value,
    ) -> Result<Value> {
        // Inspection runs on the manager's control lane, so it still answers
        // while the manager drains, and never starts one.
        let message = message.clone();
        let inspected = exchange(&self.root, false, move |target| {
            target.control("inspect", message)
        })
        .await;
        match inspected {
            Err(error) if error.code == "OWNER_ABSENT" => {
                if crate::diagnostics::inspects(name) {
                    let owners = crate::diagnostics::Owners::default();
                    return owners.inspect(&self.root, &identity.workspace_key(), name, &args);
                }
                if name == "project_status" {
                    let mut result = crate::project::status(&args).await?;
                    result["activeOperations"] = json!([]);
                    result["recentOperations"] = json!([]);
                    return Ok(result);
                }
                if name == "build_log" {
                    return crate::builds::Owners::default().log(
                        &self.root,
                        &identity.workspace_key(),
                        &args,
                    );
                }
                if matches!(name, "build_status" | "build_cancel") {
                    return crate::builds::Owners::default().query(
                        &self.root,
                        &identity.workspace_key(),
                        &args,
                        name == "build_cancel",
                    );
                }
                self.offline(name, args).await
            }
            result => result,
        }
    }

    /// Status read from durable records while no manager runs.
    async fn offline(&self, name: &str, args: Value) -> Result<Value> {
        if matches!(name, "browser_status" | "browser_control_status") {
            return crate::browser::runtime::offline(
                super::selection::pool_directory(&self.root, identity::POOL)?,
                false,
                &args,
            )
            .await;
        }
        if name != "status" {
            return Err(Error::new(
                "NOT_CONNECTED",
                "No manager is running, so nothing is connected. Connect first",
            ));
        }
        let mut caller = Caller::new(None, self.root.clone(), self.kind);
        if let Some(target) = args["targetId"].as_str() {
            if !caller.recorded(target) {
                return Err(super::service::not_connected(Some(target)));
            }
            caller.target = Some(target.to_owned());
        }
        // Boxed: the workflow future would otherwise nest inside every CLI
        // command's future and exceed the compiler's layout depth.
        Box::pin(caller.call(name, args)).await
    }
    pub(crate) async fn ensure_attached(&self) -> Result<()> {
        if let Some(error) = &self.root_error {
            return Err(error.clone());
        }
        let mut attachment = self.attachment.lock().await;
        // A manager that exited or drained closed its end: attach to the
        // manager that serves the next request instead.
        let open = attachment.as_mut().is_some_and(|stream| {
            let mut byte = [0_u8; 1];
            matches!(stream.read(&mut byte), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        });
        if open {
            return Ok(());
        }
        let connection = self.connection_id.clone().unwrap_or_default();
        let stream = exchange(&self.root, self.start, move |target| {
            target.attach(&connection)
        })
        .await?;
        stream
            .set_nonblocking(true)
            .map_err(|_| Error::new("HOST_IO", "Cannot keep the manager attachment"))?;
        *attachment = Some(stream);
        Ok(())
    }
    /// Deliver `message` once to a manager of this protocol, starting one
    /// when `start` allows it.
    async fn send(&self, message: &Value, start: bool) -> Result<Value> {
        let submitted = message.clone();
        let admitted = exchange(&self.root, start, move |target| target.submit(&submitted)).await?;
        // Dropping this future abandons the request: the manager observes
        // the closed connection and never dispatches queued work.
        let _abandon = Abandon(
            admitted
                .abandon_handle()
                .map_err(|_| host::outcome_unknown(&message["requestId"]))?,
        );
        let timeout = manager::result_timeout(message);
        let reply = tokio::task::spawn_blocking(move || admitted.result_with_timeout(timeout))
            .await
            .map_err(|_| Error::new("INTERNAL", "The manager exchange stopped unexpectedly"))?;
        manager::outcome(reply, &message["requestId"])
    }
    /// Host task end: requests this MCP server received before it are
    /// refused. Devices stay connected for other clients.
    pub(crate) async fn end_task(&self) -> Result<Value> {
        let identity = self.identity()?;
        let arguments = json!({
            "kind":"task_end","requestId":uuid::Uuid::new_v4().to_string(),
            "hostLabel":identity.label,
        });
        let ended = exchange(&self.root, false, move |target| {
            target.control("task_end", arguments)
        })
        .await;
        let result = match ended {
            Err(error) if error.code == "OWNER_ABSENT" => {
                Ok(json!({"released":[],"owner":"absent"}))
            }
            result => result,
        };
        if self.kind == Kind::Mcp {
            self.attachment.lock().await.take();
            *self
                .task_ended
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(std::time::Instant::now());
        }
        result
    }
    /// One control-subset request, such as `pool` or `retry`.
    pub(crate) async fn control(&self, kind: &str, mut fields: Value) -> Result<Value> {
        if let Some(error) = &self.root_error {
            return Err(error.clone());
        }
        fields["kind"] = json!(kind);
        if fields.get("requestId").is_none() {
            fields["requestId"] = json!(uuid::Uuid::new_v4().to_string());
        }
        // Recovery starts an absent manager only when durable records show
        // unfinished cleanup; nothing else does.
        let start = kind == "retry" && transition::recovery_needed(&self.root);
        let action = kind.to_owned();
        let controlled = exchange(&self.root, start, move |target| {
            target.control(&action, fields)
        })
        .await;
        match controlled {
            Err(error) if error.code == "OWNER_ABSENT" => Ok(self.offline_control(kind)),
            result => result,
        }
    }
    fn offline_control(&self, kind: &str) -> Value {
        match kind {
            "pool" => {
                json!({"live":false,"owner":"absent","pool":recorded_pool(&self.root)})
            }
            "retry" => {
                json!({"owner":"absent","recovered":false,"message":"Nothing to recover: no manager runs and no durable record shows unfinished cleanup"})
            }
            "retire" | "drain" => json!({"owner":"absent","retired":true}),
            _ => json!({"owner":"absent"}),
        }
    }
}

/// The pool's durable record, never reported as live.
fn recorded_pool(root: &Path) -> Value {
    let record = super::selection::pool_directory(root, identity::POOL)
        .ok()
        .and_then(|directory| {
            storage::read::<Value>(&directory.join("pool.json"))
                .ok()
                .flatten()
        });
    match record {
        Some(mut record) => {
            record["live"] = json!(false);
            record
        }
        None => Value::Null,
    }
}

fn diagnostic_requirements(name: &str, args: &Value, message: &mut Value) {
    if crate::diagnostics::handles(name) {
        message["requires"] = if matches!(name, "diagnostics_project" | "diagnostics_export") {
            json!([
                "diagnostic_sessions_v1",
                "diagnostic_analysis_v1",
                "diagnostic_access_v1"
            ])
        } else if matches!(
            name,
            "diagnostics_analyze"
                | "diagnostics_analysis_status"
                | "diagnostics_analysis_cancel"
                | "diagnostics_query"
                | "diagnostics_compare"
        ) {
            json!(["diagnostic_sessions_v1", "diagnostic_analysis_v1"])
        } else {
            json!(["diagnostic_sessions_v1"])
        };
        if (name == "diagnostics_query"
            && matches!(args["view"].as_str(), Some("investigation" | "report")))
            || (name == "diagnostics_export"
                && matches!(
                    args["format"].as_str(),
                    Some("investigation-json" | "investigation-markdown")
                ))
        {
            message["requires"]
                .as_array_mut()
                .unwrap()
                .push(json!("diagnostic_reports_v1"));
        }
        if name == "diagnostics_start" && args["source"]["kind"] == "android" {
            message["requires"] = json!(["diagnostic_sessions_v1", "diagnostic_android_v1"]);
        }
        if name == "diagnostics_start" && args["source"]["kind"] == "android_state" {
            message["requires"] = json!(["diagnostic_sessions_v1", "diagnostic_android_state_v1"]);
        }
        if name == "diagnostics_start" && args["source"]["kind"] == "ios" {
            message["requires"] = json!(["diagnostic_sessions_v1", "diagnostic_apple_v1"]);
        }
        if name == "diagnostics_start" && args["source"]["kind"] == "browser_operation" {
            message["requires"] = json!(["diagnostic_sessions_v1", "diagnostic_browser_v1"]);
        }
        if name == "diagnostics_start" && args["source"]["kind"] == "react_native" {
            message["requires"] = json!(["diagnostic_sessions_v1", "diagnostic_react_native_v1"]);
        }
        if name == "diagnostics_start" && args["source"]["kind"] == "dart" {
            message["requires"] = json!(["diagnostic_sessions_v1", "diagnostic_dart_v1"]);
        }
        diagnostic_evidence_requirements(name, args, message);
        diagnostic_query_requirements(name, args, message);
    }
}
fn diagnostic_evidence_requirements(name: &str, args: &Value, message: &mut Value) {
    if (name == "diagnostics_start"
        && (args["source"]["kind"] == "observability_http"
            || matches!(
                args["source"]["format"].as_str(),
                Some(
                    "observability-evidence-v1"
                        | "otlp-json"
                        | "prometheus-json"
                        | "loki-json"
                        | "sentry-json"
                        | "jaeger-json"
                        | "cloudwatch-json"
                        | "crashlytics-json"
                        | "play-vitals-json"
                        | "deployment-evidence-v1"
                )
            )))
        || (name == "diagnostics_query" && args["view"] == "observability")
    {
        message["requires"]
            .as_array_mut()
            .unwrap()
            .push(json!("diagnostic_observability_v1"));
    }
    if name == "diagnostics_start"
        && (matches!(
            args["source"]["kind"].as_str(),
            Some("host_process" | "box_logs")
        ) || matches!(
            args["source"]["format"].as_str(),
            Some(
                "service-evidence-v1"
                    | "host-process-v1"
                    | "box-log-v1"
                    | "js-cpu-profile"
                    | "jfr-json"
                    | "pprof"
                    | "stackprof-json"
                    | "xdebug-cachegrind"
                    | "windows-minidump"
            )
        ))
    {
        message["requires"]
            .as_array_mut()
            .unwrap()
            .push(json!("diagnostic_services_v1"));
    }
    if name == "diagnostics_start"
        && (args["source"]["kind"] == "embedded_cdp"
            || matches!(
                args["source"]["format"].as_str(),
                Some("framework-evidence-v1" | "runtime-samples-v1" | "embedded-cdp-v1")
            ))
    {
        message["requires"]
            .as_array_mut()
            .unwrap()
            .push(json!("diagnostic_frameworks_v1"));
    }
}
fn diagnostic_query_requirements(name: &str, args: &Value, message: &mut Value) {
    if name == "diagnostics_query" && args["view"] == "assertions"
        || name == "diagnostics_start"
            && matches!(
                args["source"]["format"].as_str(),
                Some("test-results-v1" | "junit-xml")
            )
    {
        message["requires"]
            .as_array_mut()
            .unwrap()
            .push(json!("diagnostic_testing_v1"));
    }
    if name == "diagnostics_start"
        && (args["source"]["kind"] == "marker" || args["source"]["format"] == "reproduction-v1")
    {
        message["requires"]
            .as_array_mut()
            .unwrap()
            .push(json!("diagnostic_reproduction_v1"));
    }
    if name == "diagnostics_start"
        && matches!(
            args["source"]["format"].as_str(),
            Some("runtime-checks-v1" | "sanitizer-report-text")
        )
    {
        message["requires"]
            .as_array_mut()
            .unwrap()
            .push(json!("diagnostic_runtime_checks_v1"));
    }
    if name == "diagnostics_query" && (args["view"] == "scenario" || args.get("scenario").is_some())
        || name == "diagnostics_start"
            && args["source"]["kind"] == "android_state"
            && args["source"]["groups"]
                .as_array()
                .is_some_and(|groups| groups.iter().any(|g| g == "media" || g == "device_context"))
    {
        message["requires"]
            .as_array_mut()
            .unwrap()
            .push(json!("diagnostic_scenario_context_v1"));
    }
    if name == "diagnostics_query"
        && (args["view"] == "network_data" || args.get("networkData").is_some())
    {
        message["requires"]
            .as_array_mut()
            .unwrap()
            .push(json!("diagnostic_network_data_v1"));
    }
    if name == "diagnostics_query"
        && (args["view"] == "performance" || args.get("performance").is_some())
    {
        message["requires"]
            .as_array_mut()
            .unwrap()
            .push(json!("diagnostic_performance_v1"));
    }
    if name == "diagnostics_query" && (args["view"] == "memory" || args.get("memory").is_some())
        || name == "diagnostics_start"
            && matches!(
                args["source"]["template"].as_str(),
                Some("Allocations" | "Leaks")
            )
    {
        message["requires"]
            .as_array_mut()
            .unwrap()
            .push(json!("diagnostic_memory_v1"));
    }
    if name == "diagnostics_start" && args["source"]["kind"] == "native_trace" {
        let required = message["requires"].as_array_mut().unwrap();
        required.push(json!("diagnostic_native_trace_v1"));
        required.push(json!(wire::NATIVE_TRACE_ENVELOPE));
    } else if name == "diagnostics_query"
        && ["view", "nativeUtid", "nativeUpid"]
            .iter()
            .any(|field| args.get(field).is_some())
        || name == "diagnostics_export" && args["format"] == "native_artifact"
    {
        message["requires"]
            .as_array_mut()
            .unwrap()
            .push(json!("diagnostic_native_trace_v1"));
    }
    if name == "diagnostics_analyze" && args["metadata"]["symbolicate"] == true {
        message["requires"]
            .as_array_mut()
            .unwrap()
            .push(json!("diagnostic_apple_v1"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::managed::service::Service;
    use crate::managed::{host, tests, wire};
    use std::time::Duration;

    #[test]
    fn browser_status_and_native_trace_requirements_match_manager_generation() {
        use canonic_process_kit::protocol::Identity as ManagerIdentity;

        let capabilities: Vec<String> = wire::CAPABILITIES
            .iter()
            .map(|name| (*name).into())
            .collect();
        let current = ManagerIdentity::new("acme".into(), PROTOCOL, capabilities.clone()).unwrap();
        let older = ManagerIdentity::new(
            "acme".into(),
            PROTOCOL,
            capabilities
                .into_iter()
                .filter(|name| {
                    !matches!(
                        name.as_str(),
                        wire::BROWSER_STATUS_V2 | wire::NATIVE_TRACE_ENVELOPE
                    )
                })
                .collect(),
        )
        .unwrap();
        let operation = uuid::Uuid::new_v4();
        let (shared_name, shared_args) = crate::browser::api::translate(
            "status",
            json!({
                "targetId":format!("browser:{}", uuid::Uuid::new_v4()), "includeDetails":true
            }),
        )
        .unwrap();
        for (name, args, supported_by_older) in [
            ("browser_status", json!({}), true),
            (
                "browser_status",
                json!({"operationId":operation,"waitMs":30000}),
                true,
            ),
            ("browser_status", json!({"waitMs":30000}), false),
            ("browser_status", json!({"includeDetails":false}), false),
            ("browser_status", json!({"limit":1}), true),
            (
                "browser_status",
                json!({"targetId":"browser:acme","limit":1}),
                false,
            ),
            (shared_name.as_str(), shared_args, false),
            (
                "diagnostics_start",
                json!({"source":{"kind":"native_trace","backend":"stack_sample","mode":"import"}}),
                false,
            ),
            (
                "diagnostics_start",
                json!({"source":{"kind":"native_trace","backend":"perfetto","mode":"capture","waitForPerfettoStart":true}}),
                false,
            ),
            (
                "diagnostics_export",
                json!({"format":"native_artifact"}),
                true,
            ),
        ] {
            let mut message = json!({"requires":["device_queues"]});
            prepare_browser_call(name, &args, &mut message);
            diagnostic_requirements(name, &args, &mut message);
            let requires: Vec<String> =
                serde_json::from_value(message["requires"].clone()).unwrap();
            assert!(
                current.require(PROTOCOL, &requires).is_ok(),
                "{name} {args}"
            );
            assert_eq!(
                older.require(PROTOCOL, &requires).is_ok(),
                supported_by_older,
                "{name} {args}"
            );
        }
    }

    #[test]
    fn browser_connect_options_require_manager_support_before_admission() {
        let mut args = json!({"engine":"chrome"});
        let mut expected = vec![
            "device_queues",
            "browser_lifecycle",
            "browser_native_chrome_v1",
            "browser_preparation_v1",
        ];
        let mut message = json!({});
        prepare_browser_call("browser_connect", &args, &mut message);
        assert_eq!(message["requires"], json!(expected));

        for (field, value, capability) in [
            (
                "mediaFixtures",
                json!({"microphonePath":"/private/acme.wav"}),
                "browser_media_fixtures_v1",
            ),
            ("channel", json!("chrome"), "browser_installed_channels_v1"),
            (
                "profile",
                json!({"kind":"disposable"}),
                "browser_profiles_v1",
            ),
            (
                "attachment",
                json!({"endpoint":"ws://127.0.0.1:9222/devtools/browser/acme"}),
                "browser_attachment_v1",
            ),
        ] {
            args[field] = value;
            expected.push(capability);
            prepare_browser_call("browser_connect", &args, &mut message);
            assert_eq!(message["requires"], json!(expected));
            assert!(wire::CAPABILITIES.contains(&capability));
        }
        args["attachment"]["credentialPath"] = json!("/private/acme.json");
        expected.push("browser_attachment_credentials_v2");
        prepare_browser_call("browser_connect", &args, &mut message);
        assert_eq!(message["requires"], json!(expected));
        assert!(wire::CAPABILITIES.contains(&"browser_attachment_credentials_v2"));
    }

    #[test]
    fn browser_app_features_require_manager_support_in_direct_batch_and_shared_act() {
        let feature = json!("browser_app_features_v1");
        assert!(wire::CAPABILITIES.contains(&"browser_app_features_v1"));
        for operation in [
            "web_mcp",
            "web_mcp_debug",
            "browser_extension",
            "browser_pwa",
        ] {
            let mut message = json!({});
            for request in [
                json!({"operation":operation,"action":{"kind":"list"}}),
                json!({"operation":"batch","requests":[{"operation":operation,"action":{"kind":"list"}}]}),
            ] {
                prepare_browser_call("browser_send", &json!({"request":request}), &mut message);
                assert!(message["requires"].as_array().unwrap().contains(&feature));
            }
        }
        let (name, args) = crate::browser::api::translate(
            "act",
            json!({"targetId":format!("browser:{}",uuid::Uuid::new_v4()),"actions":[{"action":"browser_pwa","input":{"action":"list"}}]}),
        )
        .unwrap();
        let mut message = json!({});
        prepare_browser_call(&name, &args, &mut message);
        assert!(message["requires"].as_array().unwrap().contains(&feature));
        let args = json!({"request":{"operation":"evaluate","argument":{"operation":"web_mcp"}}});
        prepare_browser_call("browser_send", &args, &mut message);
        assert!(!message["requires"].as_array().unwrap().contains(&feature));
    }

    #[test]
    fn large_fixture_manager_requests_fit_the_frame_and_negotiate_support() {
        use crate::browser::transfer::FIXTURE_BYTES;
        use base64::{Engine as _, engine::general_purpose::STANDARD};

        let capability = json!("browser_large_fixtures_v1");
        let mut message = json!({});
        for (size, large) in [
            (256 * 1024, false),
            (256 * 1024 + 1, true),
            (FIXTURE_BYTES, true),
        ] {
            let request = json!({"operation":"fixture_register","fixture":{
                "name":"acme.bin","mimeType":"application/octet-stream",
                "dataBase64":STANDARD.encode(vec![42; size]),
                "provenance":"synthetic_generated","cleared":{"by":"Acme","method":"Generated"}
            }});
            for request in [
                request.clone(),
                json!({"operation":"batch","requests":[request]}),
            ] {
                let args = json!({"request":request});
                prepare_browser_call("browser_send", &args, &mut message);
                assert_eq!(
                    message["requires"]
                        .as_array()
                        .unwrap()
                        .contains(&capability),
                    large
                );
                message["args"] = args;
                assert!(
                    canonic_process_kit::frame::encode_json(&message, wire::MAX_REQUEST_FRAME)
                        .is_ok()
                );
            }
        }
        assert!(wire::CAPABILITIES.contains(&"browser_large_fixtures_v1"));
    }

    #[test]
    fn chrome_connection_negotiates_native_support_before_owner_admission() {
        let mut message = json!({});
        prepare_browser_call("browser_connect", &json!({"engine":"chrome"}), &mut message);
        assert!(
            message["requires"]
                .as_array()
                .unwrap()
                .contains(&json!("browser_native_chrome_v1"))
        );
        assert!(wire::CAPABILITIES.contains(&"browser_native_chrome_v1"));
        for (name, args) in [
            ("browser_connect", json!({"engine":"firefox"})),
            (
                "browser_send",
                json!({"request":{"operation":"evaluate","argument":{"engine":"chrome"}}}),
            ),
        ] {
            let mut message = json!({});
            prepare_browser_call(name, &args, &mut message);
            assert!(
                !message["requires"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("browser_native_chrome_v1"))
            );
        }
    }

    #[tokio::test]
    async fn browser_connect_sends_mode_to_a_manager_that_requires_it() {
        use canonic_process_kit::{
            core::{Admission, IdleGrace},
            endpoint::Lease,
            protocol::{Identity, ProductError, Request},
            server::{self, Context, Handler},
        };

        struct RequiresMode;

        impl Handler for RequiresMode {
            fn call(
                &self,
                request: &Request,
                _: &Context<'_>,
            ) -> std::result::Result<Value, ProductError> {
                let args = &request.payload["args"];
                if request.payload["name"] != "browser_connect" || !args["mode"].is_string() {
                    return Err(ProductError(json!(Error::invalid(
                        "Expected browser_connect with an explicit mode"
                    ))));
                }
                Ok(args.clone())
            }
        }

        let root = tests::root();
        let state = root.path().join("state");
        let lease = Lease::acquire(&host::root(&state).unwrap(), Duration::ZERO).unwrap();
        let identity = Identity::new(
            crate::managed::BUILD_ID.into(),
            PROTOCOL,
            wire::CAPABILITIES
                .iter()
                .map(|&capability| capability.to_owned())
                .collect(),
        )
        .unwrap();
        let gate = Admission::new();
        let hold = gate.hold().unwrap();
        let serving = gate.clone();
        let running = tokio::task::spawn_blocking(move || {
            server::serve(
                lease,
                identity,
                serving,
                Arc::new(RequiresMode),
                &IdleGrace(Duration::from_secs(1)),
                server::Options::default(),
            )
        });
        host::wait_for_fixture_socket(&state, &running).await;

        let mut client = Client::in_root(&state, tests::identity("acme", root.path()));
        let mut replies = Vec::new();
        for name in ["browser_connect", "connect"] {
            for args in [
                json!({"engine":"chrome"}),
                json!({"engine":"chrome","mode":"headed"}),
            ] {
                replies.push(client.call(name, args).await);
            }
        }
        let _ = gate.drain();
        drop(hold);
        running.await.unwrap().unwrap();
        for (reply, mode) in replies
            .into_iter()
            .zip(["headless", "headed", "headless", "headed"])
        {
            assert_eq!(reply.unwrap(), json!({"engine":"chrome","mode":mode}));
        }
    }

    #[tokio::test]
    async fn inspection_against_an_absent_owner_never_starts_one() {
        let root = tests::root();
        let state = root.path().join("state");
        let mut client = Client::in_root(&state, tests::identity("acme", root.path()));
        client.start = true;
        let status = client.call("status", json!({})).await.unwrap();
        assert_eq!(status["owner"], "absent", "{status}");
        assert_eq!(status["state"], "not_connected");
        assert_eq!(status["targets"], json!([]));
        let error = client
            .call("status", json!({"targetId":"android:Acme-unconnected"}))
            .await
            .unwrap_err();
        assert_eq!(error.code, "NOT_CONNECTED");
        assert_eq!(error.details.unwrap()["dispatched"], false);
        assert!(!state.join("owner.log").exists());
        assert!(!host::socket(&state).exists());
        let pool = client.control("pool", json!({})).await.unwrap();
        assert_eq!(pool["live"], false);
        // Recovery with nothing durable to settle does not start one either.
        let retry = client.control("retry", json!({})).await.unwrap();
        assert_eq!(retry["recovered"], false);
        assert!(!state.join("owner.log").exists());
    }

    #[tokio::test]
    async fn project_status_inspects_configuration_without_starting_a_manager() {
        let root = tests::root();
        let state = root.path().join("state");
        std::fs::write(root.path().join("testrun.yml"),json!({"version":1,"builds":{"ios-dev":{"builder":"custom","platform":"ios","target_kind":"device","custom":{"command":["must-not-run"]},"artifact":{"path":"dist/Acme.app"},"development":{"app_id":"com.example.acme"}}}}).to_string()).unwrap();
        let mut client = Client::in_root(&state, tests::identity("acme", root.path()));
        client.start = true;
        let result = client
            .call(
                "project_status",
                json!({"project":root.path(),"profile":"ios-dev"}),
            )
            .await
            .unwrap();
        assert_eq!(result["appId"], "com.example.acme");
        assert_eq!(result["activeOperations"], json!([]));
        assert_eq!(result["checks"]["installedApp"]["state"], "not_checked");
        assert!(!state.join("owner.log").exists());
        assert!(!host::socket(&state).exists());
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn an_mcp_server_eof_leaves_its_devices_connected_for_other_clients() {
        let root = tests::root();
        let state = root.path().join("state");
        let service = Service::fixture(state.clone(), PROTOCOL, Duration::from_secs(45));
        // Held until the device connects: the empty manager's idle grace is 200 ms.
        let (mut running, _stop, held) = host::fixture_held(service.clone()).await;
        let mut client = Client::in_root(&state, tests::identity("acme-mcp", root.path()));
        client.attach_mcp("acme-connection".into());
        let target = format!("android:Acme-{}", uuid::Uuid::new_v4().simple());
        let connected = client
            .call("connect", json!({"targetId":target}))
            .await
            .unwrap();
        drop(held);
        assert_eq!(connected["state"], "ready", "{connected}");
        assert!(client.attachment.lock().await.is_some());
        // The MCP server's EOF releases nothing: a CLI client in another
        // workspace finds the device still connected and uses it.
        client.shutdown().await.unwrap();
        let elsewhere = tests::root();
        let mut cli = Client::in_root(&state, tests::identity("acme-cli", elsewhere.path()));
        let status = cli
            .call("status", json!({"targetId":target}))
            .await
            .unwrap();
        assert_eq!(status["state"], "ready", "{status}");
        cli.call("observe", json!({"targetId":target}))
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(300), &mut running)
                .await
                .is_err()
        );
        cli.call("disconnect", json!({})).await.unwrap();
        let exited = tokio::time::timeout(Duration::from_secs(10), running)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(exited["reason"], "empty");
        assert_eq!(service.supervisor.alive(), 0);
    }
}
