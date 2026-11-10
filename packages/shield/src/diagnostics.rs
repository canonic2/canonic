//! Owns bounded, payload-free diagnostics for one Shield process, separate from protocol I/O.

use crate::cli::Cli;
use serde_json::{Value, json};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
mod retention;
pub(crate) mod trace;

const LIMIT: u64 = 8 * 1024 * 1024;

pub(crate) struct Log {
    path: Option<PathBuf>,
    started: Option<Instant>,
    writer: Arc<Mutex<Option<(File, u64)>>>,
    traces: OnceLock<Arc<trace::Recorder>>,
    drain: Duration,
}

impl Log {
    pub(crate) fn disabled() -> Self {
        Self {
            path: None,
            started: None,
            writer: Arc::new(Mutex::new(None)),
            traces: OnceLock::new(),
            drain: Duration::ZERO,
        }
    }

    pub(crate) fn open(cli: &Cli) -> Self {
        if cli.no_debug_log {
            return Self::disabled();
        }
        let directory = cli
            .debug_log_dir
            .clone()
            .or_else(|| std::env::var_os("SHIELD_DEBUG_LOG_DIR").map(PathBuf::from))
            .or_else(|| cfg!(debug_assertions).then(|| std::env::temp_dir().join("shield-logs")));
        let Some(directory) = directory else {
            return Self::disabled();
        };
        let transport = match &cli.command {
            crate::cli::PublicCommand::Internal(command) => match command.as_ref() {
                crate::cli::Command::NativeHook(_) => "native_hook",
                _ => "internal",
            },
            command if command.mcp_serve().is_some() => "mcp",
            _ => "operator",
        };
        let drain = if transport == "native_hook" {
            Duration::ZERO
        } else {
            Duration::from_millis(5)
        };
        let Ok(log) = Self::create_with_drain(&directory, drain) else {
            eprintln!("Shield: diagnostic log unavailable; continuing without diagnostics.");
            return Self::disabled();
        };
        log.event("run_start", || json!({"pid":std::process::id(),"version":env!("CARGO_PKG_VERSION"),"debug_build":cfg!(debug_assertions),"transport":transport}));
        log
    }

    #[cfg(test)]
    fn create(directory: &Path) -> std::io::Result<Self> {
        Self::create_with_drain(directory, Duration::from_millis(5))
    }

    fn create_with_drain(directory: &Path, drain: Duration) -> std::io::Result<Self> {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(directory)?;
        if std::fs::symlink_metadata(directory)?
            .file_type()
            .is_symlink()
        {
            return Err(std::io::Error::other("diagnostic directory is a symlink"));
        }
        let (path, file) = retention::create(directory)?;
        Ok(Self {
            path: Some(path),
            started: Some(Instant::now()),
            writer: Arc::new(Mutex::new(Some((file, 0)))),
            traces: OnceLock::new(),
            drain,
        })
    }

    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(crate) fn request(
        self: &Arc<Self>,
        surface: &'static str,
        operation: &'static str,
        id: Option<&Value>,
        connection: Option<&str>,
    ) -> Option<trace::Request> {
        self.path.as_ref()?;
        Some(self.recorder().request(surface, operation, id, connection))
    }

    fn recorder(&self) -> &Arc<trace::Recorder> {
        self.traces.get_or_init(|| {
            Arc::new(trace::Recorder::new(
                self.writer.clone(),
                self.started.expect("enabled diagnostics epoch"),
                self.drain,
            ))
        })
    }

    #[cfg(test)]
    pub(crate) fn hold_writer(&self) -> impl Drop + '_ {
        self.writer.lock().unwrap()
    }

    pub(crate) fn event(&self, event: &str, fields: impl FnOnce() -> Value) {
        if self.path.is_none() {
            return;
        }
        let mut record = json!({"elapsed_ms":self.started.expect("enabled diagnostics epoch").elapsed().as_millis(),"event":event});
        record["fields"] = fields();
        self.recorder().publish(record);
    }

    pub(crate) fn hook_input(&self, agent: &str, bytes: &[u8], read_ok: bool, oversized: bool) {
        if self.path.is_none() {
            return;
        }
        let parsed = crate::json::parse(bytes);
        let host = parsed.as_ref().ok();
        self.event("hook_input", || json!({"agent":agent,"captured_bytes":bytes.len(),"read_ok":read_ok,"oversized":oversized,"valid_json":host.is_some(),
            "host_event":host.and_then(|v| label(&v["hook_event_name"])),
            "tool_name":host.and_then(|v| label(&v["tool_name"])),
            "session_id":host.and_then(|v| label(&v["session_id"])),
            "turn_id":host.and_then(|v| label(&v["turn_id"])),
            "tool_use_id":host.and_then(|v| label(&v["tool_use_id"])),
            "has_cwd":host.is_some_and(|v|v["cwd"].is_string()),
            "has_tool_input":host.is_some_and(|v|v.get("tool_input").is_some()),
            "has_tool_response":host.is_some_and(|v|v.get("tool_response").is_some())}));
    }

    pub(crate) fn hook_output(&self, value: &Value) {
        if self.path.is_none() {
            return;
        }
        let hook = &value["hookSpecificOutput"];
        self.event("hook_output", || json!({"bytes":crate::json::encoded_len(value).expect("hook JSON serializes"),"host_event":label(&hook["hookEventName"]),"permission_decision":label(&hook["permissionDecision"]),"decision_behavior":label(&hook["decision"]["behavior"]),"has_context":hook.get("additionalContext").is_some(),"has_system_message":value.get("systemMessage").is_some(),"has_updated_input":hook.get("updatedInput").is_some()}));
    }

    #[cfg(test)]
    pub(crate) fn mcp_frame(&self, direction: &str, bytes: &[u8]) {
        if self.path.is_none() {
            return;
        }
        self.mcp_frame_at(direction, bytes, Instant::now());
    }

    pub(crate) fn mcp_frame_at(&self, direction: &str, bytes: &[u8], observed: Instant) {
        if self.path.is_none() {
            return;
        }
        let parsed = crate::json::parse(bytes);
        let frame = parsed.as_ref().ok();
        let method = frame.and_then(|v| v["method"].as_str()).filter(|s| {
            matches!(
                *s,
                "initialize"
                    | "notifications/initialized"
                    | "tools/list"
                    | "tools/call"
                    | "notifications/cancelled"
                    | "ping"
            )
        });
        let tool = frame
            .and_then(|v| v["params"]["name"].as_str())
            .filter(|s| tool_names().iter().any(|name| name == *s));
        let observed_us = u64::try_from(
            observed
                .saturating_duration_since(self.started.expect("enabled diagnostics epoch"))
                .as_micros(),
        )
        .unwrap_or(u64::MAX);
        self.event(direction, || json!({"observed_us":observed_us,"bytes":bytes.len(),"valid_json":frame.is_some(),"method":method,"tool":tool,"id":frame.and_then(|v|label(&v["id"]).map(Value::String).or_else(||v["id"].as_i64().map(|id|json!(id)))),"error_code":frame.and_then(|v|v["error"]["code"].as_i64()),"tool_error":frame.and_then(|v|v["result"]["isError"].as_bool())}));
    }
}

fn tool_names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        crate::review::mcp::tool_summaries()
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    })
}

fn label(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| s.len() <= 4096)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests;
