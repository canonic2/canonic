//! Diagnostic identity, admission limits and durable lifecycle records; no I/O.
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Binding {
    pub target_id: Option<String>,
    pub app_id: Option<String>,
    pub build_id: Option<String>,
    pub image_id: Option<String>,
    pub process_id: Option<u32>,
    pub process_generation: Option<String>,
    pub runtime_id: Option<String>,
    pub runtime_generation: Option<String>,
    pub qa_run_id: Option<String>,
    pub qa_session_id: Option<String>,
    pub action_ids: Vec<String>,
}
impl Binding {
    pub fn validate(&self) -> Result<()> {
        for value in [
            &self.target_id,
            &self.app_id,
            &self.build_id,
            &self.image_id,
            &self.process_generation,
            &self.runtime_id,
            &self.runtime_generation,
            &self.qa_run_id,
            &self.qa_session_id,
        ]
        .into_iter()
        .flatten()
        .chain(self.action_ids.iter())
        {
            label(value, 256)?;
        }
        if self.action_ids.len() > 32
            || self.process_id.is_some() != self.process_generation.is_some()
            || self.runtime_id.is_some() != self.runtime_generation.is_some()
            || self.process_id == Some(0)
        {
            return Err(Error::invalid(
                "Use at most 32 actionIds; process/runtime identities require their generation",
            ));
        }
        Ok(())
    }
}
pub(crate) fn label(value: &str, max: usize) -> Result<()> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(Error::invalid(format!(
            "Expected 1..{max} bytes without control characters"
        )));
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Source {
    ObservabilityHttp {
        #[serde(flatten)]
        options: super::observability::Options,
    },
    HostProcess {
        #[serde(flatten)]
        options: super::host::Options,
    },
    BoxLogs {
        #[serde(flatten)]
        options: super::host::BoxOptions,
    },
    EmbeddedCdp {
        #[serde(flatten)]
        options: super::embedded::Options,
    },
    Marker {
        name: String,
        phase: String,
        #[serde(rename = "actionId")]
        action_id: String,
    },
    NativeTrace {
        #[serde(flatten)]
        options: super::native_trace::Options,
    },
    Dart {
        #[serde(flatten)]
        options: super::dart::Options,
    },
    ReactNative {
        endpoint: String,
        #[serde(rename = "targetId")]
        target_id: String,
        #[serde(rename = "appId")]
        app_id: String,
        #[serde(rename = "pageId")]
        page_id: Option<String>,
        #[serde(rename = "logicalDeviceId")]
        logical_device_id: Option<String>,
        #[serde(default = "super::react_native::mode")]
        mode: String,
        #[serde(default)]
        network: bool,
        #[serde(default)]
        scripts: bool,
        #[serde(default)]
        probe: bool,
        #[serde(default, rename = "allowDebuggerTakeover")]
        allow_debugger_takeover: bool,
        #[serde(default, rename = "allowProfilerControl")]
        allow_profiler_control: bool,
        #[serde(default = "super::react_native::sample_ms", rename = "sampleMs")]
        sample_ms: u64,
    },
    BrowserOperation {
        #[serde(rename = "targetId")]
        target_id: String,
        #[serde(rename = "operationId")]
        operation_id: String,
        request: serde_json::Value,
    },
    Ios {
        #[serde(rename = "targetId")]
        target_id: String,
        #[serde(rename = "appId")]
        app_id: String,
        #[serde(default = "super::apple::usb")]
        transport: String,
        #[serde(default = "enabled", rename = "collectLogs")]
        collect_logs: bool,
        #[serde(default = "enabled", rename = "systemContext")]
        system_context: bool,
        #[serde(default = "super::apple::seconds", rename = "logSeconds")]
        log_seconds: u64,
        #[serde(rename = "deviceSet")]
        device_set: Option<PathBuf>,
        #[serde(rename = "reportDirectory")]
        report_directory: Option<PathBuf>,
    },
    Import {
        path: PathBuf,
        format: String,
    },
    BuildOutput {
        #[serde(rename = "operationId")]
        operation_id: String,
    },
    Android {
        #[serde(rename = "targetId")]
        target_id: String,
        #[serde(rename = "appId")]
        app_id: String,
        #[serde(rename = "userId")]
        user_id: u32,
        #[serde(default = "snapshot")]
        mode: String,
        #[serde(default, rename = "includeReports")]
        include_reports: bool,
        #[serde(default = "enabled", rename = "systemContext")]
        system_context: bool,
        since: Option<String>,
    },
    AndroidState {
        #[serde(rename = "targetId")]
        target_id: String,
        #[serde(rename = "appId")]
        app_id: String,
        #[serde(rename = "userId")]
        user_id: u32,
        #[serde(default = "snapshot")]
        mode: String,
        #[serde(default = "super::android::state::groups")]
        groups: Vec<String>,
        #[serde(default = "super::android::state::interval", rename = "intervalMs")]
        interval_ms: u64,
        #[serde(
            default = "super::android::state::process_limit",
            rename = "maxProcesses"
        )]
        max_processes: usize,
        label: Option<String>,
    },
}
fn snapshot() -> String {
    "snapshot".into()
}
fn enabled() -> bool {
    true
}
impl Source {
    pub fn kind(&self) -> &str {
        match self {
            Self::HostProcess { .. } => "host_process",
            Self::ObservabilityHttp { .. } => "observability_http",
            Self::BoxLogs { .. } => "box_logs",
            Self::EmbeddedCdp { .. } => "embedded_cdp",
            Self::Marker { .. } => "marker",
            Self::Import { .. } => "import",
            Self::BuildOutput { .. } => "build_output",
            Self::Android { .. } => "android",
            Self::AndroidState { .. } => "android_state",
            Self::Ios { .. } => "ios",
            Self::BrowserOperation { .. } => "browser_operation",
            Self::ReactNative { .. } => "react_native",
            Self::Dart { .. } => "dart",
            Self::NativeTrace { .. } => "native_trace",
        }
    }
    pub fn format(&self) -> &str {
        match self {
            Self::HostProcess { .. } => "host-process-v1",
            Self::ObservabilityHttp { .. } => "observability-evidence-v1",
            Self::BoxLogs { .. } => "box-log-v1",
            Self::EmbeddedCdp { .. } => "embedded-cdp-v1",
            Self::Marker { .. } => "testrun-events-v1",
            Self::Import { format, .. } => format,
            Self::BuildOutput { .. } => "utf8-project-log-projection",
            Self::Android { .. } | Self::AndroidState { .. } => "android-diagnostics-v1",
            Self::Ios { .. } => "apple-diagnostics-v1",
            Self::BrowserOperation { .. } => "browser-operation-v1",
            Self::ReactNative { .. } => "react-native-diagnostics-v1",
            Self::Dart { .. } => "dart-diagnostics-v1",
            Self::NativeTrace { .. } => "native-trace-v1",
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Limits {
    pub duration_ms: u64,
    pub max_bytes: u64,
    pub max_events: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            duration_ms: 30_000,
            max_bytes: 1024 * 1024,
            max_events: 4096,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        if !(1..=300_000).contains(&self.duration_ms)
            || !(1..=8 * 1024 * 1024).contains(&self.max_bytes)
            || !(1..=65536).contains(&self.max_events)
        {
            return Err(Error::invalid(
                "Diagnostic limits: durationMs 1..300000, maxBytes 1..8388608, maxEvents 1..65536",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Admitted,
    Preparing,
    Collecting,
    Stopping,
    Finalizing,
    Complete,
    Partial,
    Failed,
    CleanupPending,
}
impl Phase {
    pub fn settled(self) -> bool {
        matches!(self, Self::Complete | Self::Partial | Self::Failed)
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Collector {
    pub id: String,
    pub signature: String,
    pub source: Source,
    pub source_identity: String,
    pub limits: Limits,
    pub phase: Phase,
    pub phases: Vec<Phase>,
    pub bytes: u64,
    pub events: u64,
    pub source_offset: u64,
    pub reason: Option<String>,
    pub sha256: Option<String>,
    #[serde(default)]
    pub cost: Option<CollectionCost>,
    #[serde(default)]
    pub source_report: Option<serde_json::Value>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct CollectionCost {
    pub run_wall_ms: f64,
    pub source_read_calls: u64,
}
impl Collector {
    pub fn phase(&mut self, phase: Phase) {
        if self.phase != phase {
            self.phases.push(phase);
            self.phase = phase;
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Investigation {
    pub version: u32,
    pub id: String,
    pub signature: String,
    pub owner: String,
    pub question: String,
    pub binding: Binding,
    pub closed: bool,
    pub collectors: Vec<Collector>,
}
