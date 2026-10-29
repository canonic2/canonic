//! Owns bounded, payload-free diagnostics for one Shield process, separate from protocol I/O.

use crate::cli::Cli;
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const LIMIT: u64 = 8 * 1024 * 1024;

pub(crate) struct Log {
    path: Option<PathBuf>,
    started: Instant,
    writer: Mutex<Option<(File, u64)>>,
}

impl Log {
    pub(crate) fn disabled() -> Self {
        Self {
            path: None,
            started: Instant::now(),
            writer: Mutex::new(None),
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
        let Ok(log) = Self::create(&directory) else {
            eprintln!("Shield: diagnostic log unavailable; continuing without diagnostics.");
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
        log.event("run_start", json!({"pid":std::process::id(),"version":env!("CARGO_PKG_VERSION"),"debug_build":cfg!(debug_assertions),"transport":transport}));
        log
    }

    fn create(directory: &Path) -> std::io::Result<Self> {
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
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_nanos();
        let path = directory.join(format!("shield-{timestamp}-{}.jsonl", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path)?;
        Ok(Self {
            path: Some(path),
            started: Instant::now(),
            writer: Mutex::new(Some((file, 0))),
        })
    }

    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn hold_writer(&self) -> impl Drop + '_ {
        self.writer.lock().unwrap()
    }

    pub(crate) fn event(&self, event: &str, fields: Value) {
        let Ok(mut writer) = self.writer.lock() else {
            return;
        };
        let Some((file, bytes)) = writer.as_mut() else {
            return;
        };
        let mut record = json!({"elapsed_ms":self.started.elapsed().as_millis(),"event":event});
        record["fields"] = fields;
        let Ok(mut line) = serde_json::to_vec(&record) else {
            return;
        };
        line.push(b'\n');
        if bytes
            .saturating_add(u64::try_from(line.len()).unwrap_or(u64::MAX))
            .saturating_add(64)
            > LIMIT
        {
            let _ = file.write_all(b"{\"event\":\"log_limit_reached\"}\n");
            *writer = None;
        } else if file.write_all(&line).is_ok() {
            *bytes += u64::try_from(line.len()).unwrap_or(0);
        } else {
            *writer = None;
            eprintln!("Shield: diagnostic logging stopped after a write failure.");
        }
    }

    pub(crate) fn hook_input(&self, agent: &str, bytes: &[u8], read_ok: bool, oversized: bool) {
        if self.path.is_none() {
            return;
        }
        let parsed = crate::json::parse(bytes);
        let host = parsed.as_ref().ok();
        self.event("hook_input", json!({"agent":agent,"captured_bytes":bytes.len(),"read_ok":read_ok,"oversized":oversized,"valid_json":host.is_some(),
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
        self.event("hook_output", json!({"bytes":value.to_string().len(),"host_event":label(&hook["hookEventName"]),"permission_decision":label(&hook["permissionDecision"]),"decision_behavior":label(&hook["decision"]["behavior"]),"has_context":hook.get("additionalContext").is_some(),"has_system_message":value.get("systemMessage").is_some(),"has_updated_input":hook.get("updatedInput").is_some()}));
    }

    pub(crate) fn mcp_frame(&self, direction: &str, bytes: &[u8]) {
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
        self.event(direction, json!({"bytes":bytes.len(),"valid_json":frame.is_some(),"method":method,"tool":tool,"id":frame.and_then(|v|label(&v["id"]).map(Value::String).or_else(||v["id"].as_i64().map(|id|json!(id)))),"error_code":frame.and_then(|v|v["error"]["code"].as_i64()),"tool_error":frame.and_then(|v|v["result"]["isError"].as_bool())}));
    }
}

fn tool_names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        crate::review::mcp::tools_for(None)["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
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
