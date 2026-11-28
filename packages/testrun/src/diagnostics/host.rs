//! Selected borrowed process snapshots. No process census, launch, signals or attach.
use crate::{
    error::{Error, Result},
    process,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;
#[cfg(target_os = "linux")]
use std::{io::Read, path::Path};
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Options {
    pub target_id: String,
    pub service_id: String,
    pub host_id: String,
    pub process_id: u32,
    pub expected_generation: Option<String>,
}
pub(crate) struct Capture {
    options: Options,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct BoxOptions {
    pub target_id: String,
    pub service_id: String,
    pub project: std::path::PathBuf,
    pub workspace: String,
    pub operation_id: String,
    pub stream: String,
}
impl BoxOptions {
    pub fn validate(&self) -> Result<()> {
        for s in [
            &self.target_id,
            &self.service_id,
            &self.workspace,
            &self.operation_id,
        ] {
            super::model::label(s, 256)?;
        }
        if !self.project.is_absolute()
            || self.workspace.starts_with('-')
            || self.operation_id.starts_with('-')
            || !matches!(self.stream.as_str(), "stdout" | "stderr")
        {
            return Err(Error::invalid(
                "Select an absolute Box project, exact workspace/operationId and stdout/stderr stream; selectors cannot be flags",
            ));
        }
        Ok(())
    }
    pub async fn read(&self, budget: Duration) -> Result<Vec<u8>> {
        self.read_with(std::path::Path::new("box"), budget).await
    }
    async fn read_with(&self, program: &std::path::Path, budget: Duration) -> Result<Vec<u8>> {
        self.validate()?;
        if budget.is_zero() {
            return Err(Error::new("TIMEOUT", "Box log read budget expired"));
        }
        let project = self.project.to_str().ok_or_else(invalid)?;
        let output = process::run_diagnostic(
            program,
            &[
                "--project",
                project,
                "logs",
                &self.workspace,
                "--operation",
                &self.operation_id,
                "--stream",
                &self.stream,
                "--json",
            ],
            budget.min(Duration::from_secs(3)),
            512 * 1024,
        )
        .await?;
        let v = super::analysis::apple_json(&output.bytes)?;
        if v["workspace"] != self.workspace
            || v["operation"] != self.operation_id
            || v["stream"] != self.stream
        {
            return Err(Error::new(
                "SCOPE_MISMATCH",
                "Box returned different workspace/operation/stream identity",
            ));
        }
        Ok(output.bytes)
    }
}
fn invalid() -> Error {
    Error::invalid(
        "Select process:HOST:PID, matching serviceId/hostId and processId 1..2147483647; expectedGeneration fences a previously observed birth",
    )
}
pub(super) fn metric(name: &str, value: f64, unit: &str, definition: &str) -> Value {
    json!({"name":name,"value":value,"unit":unit,"definition":definition})
}
impl Capture {
    pub fn new(options: Options) -> Result<Self> {
        for s in [&options.target_id, &options.service_id, &options.host_id] {
            super::model::label(s, 256)?;
        }
        if let Some(g) = &options.expected_generation {
            super::model::label(g, 256)?;
        }
        if options.process_id == 0
            || options.process_id > i32::MAX as u32
            || options.target_id != format!("process:{}:{}", options.host_id, options.process_id)
        {
            return Err(invalid());
        }
        Ok(Self { options })
    }
    pub async fn read(&self, budget: Duration) -> Result<Vec<u8>> {
        if budget.is_zero() {
            return Err(Error::new("TIMEOUT", "Process snapshot budget expired"));
        }
        let (generation, parent, metrics, native) = observe(
            self.options.process_id,
            budget.min(Duration::from_secs(3)),
            self.options.expected_generation.as_deref(),
        )
        .await?;
        if self
            .options
            .expected_generation
            .as_ref()
            .is_some_and(|g| g != &generation)
        {
            return Err(Error::new(
                "PROCESS_GENERATION_CHANGED",
                "Selected PID has a different birth; inspect the intended process before retrying",
            ));
        }
        serde_json::to_vec(&json!({"schemaVersion":1,"targetId":self.options.target_id,"serviceId":self.options.service_id,"hostId":self.options.host_id,"hostOs":std::env::consts::OS,"pid":self.options.process_id,"processGeneration":generation,"parentPid":parent,"ownership":"borrowed","coverage":"snapshot","metrics":metrics,"native":native})).map_err(|_|invalid())
    }
}
fn unavailable() -> Error {
    Error::new(
        "PROCESS_UNAVAILABLE",
        "Selected process birth or permitted native fields are unavailable; no other PID or stronger privilege was tried",
    )
}
fn counter(value: u64) -> Result<f64> {
    if value > 1 << 53 {
        Err(Error::new(
            "ANALYSIS_BOUNDS",
            "Native counter exceeds exact numeric query range",
        ))
    } else {
        Ok(value as f64)
    }
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn check(expected: Option<&str>, observed: &str) -> Result<()> {
    if expected.is_some_and(|g| g != observed) {
        Err(Error::new(
            "PROCESS_GENERATION_CHANGED",
            "Selected PID has a different birth; no snapshot was collected",
        ))
    } else {
        Ok(())
    }
}
#[cfg(target_os = "linux")]
fn read_file(path: &Path) -> Result<String> {
    let mut text = String::new();
    std::fs::File::open(path)
        .map_err(|_| unavailable())?
        .take(65537)
        .read_to_string(&mut text)
        .map_err(|_| unavailable())?;
    if text.len() > 65536 {
        return Err(unavailable());
    }
    Ok(text)
}
#[cfg(target_os = "linux")]
async fn observe(
    pid: u32,
    _budget: Duration,
    expected: Option<&str>,
) -> Result<(String, Option<u32>, Vec<Value>, Value)> {
    let before = process::start_time(pid).ok_or_else(unavailable)?;
    let boot = read_file(Path::new("/proc/sys/kernel/random/boot_id"))?
        .trim()
        .to_owned();
    super::model::label(&boot, 64)?;
    check(expected, &format!("linux:{boot}:{pid}:{before}"))?;
    let base = std::path::PathBuf::from(format!("/proc/{pid}"));
    let stat = read_file(&base.join("stat"))?;
    let status = read_file(&base.join("status"))?;
    let io = read_file(&base.join("io")).ok();
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .ok_or_else(unavailable)?
        .1
        .split_whitespace()
        .collect();
    if fields.len() < 20 || fields[19].parse::<u64>().ok() != Some(before) || fields[0] == "Z" {
        return Err(unavailable());
    }
    let parent = fields[1].parse::<u32>().map_err(|_| unavailable())?;
    let mut metrics = vec![];
    for (name, position) in [("user CPU ticks", 11), ("system CPU ticks", 12)] {
        let n = fields[position].parse::<u64>().map_err(|_| unavailable())?;
        metrics.push(metric(
            name,
            counter(n)?,
            "kernel_clock_ticks",
            "procfs lifetime task CPU counter; not elapsed time or a rate",
        ));
    }
    for line in status.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let parts: Vec<_> = value.split_whitespace().collect();
        if key == "VmRSS" && parts.len() == 2 && parts[1] == "kB" {
            let n = parts[0]
                .parse::<u64>()
                .map_err(|_| unavailable())?
                .checked_mul(1024)
                .ok_or_else(unavailable)?;
            metrics.push(metric(
                "resident memory",
                counter(n)?,
                "bytes",
                "procfs VmRSS snapshot; not retained heap or peak memory",
            ));
        }
        if key == "Threads" && parts.len() == 1 {
            metrics.push(metric(
                "threads",
                parts[0].parse::<u32>().map_err(|_| unavailable())? as f64,
                "count",
                "procfs task thread count snapshot",
            ));
        }
    }
    if let Some(text) = &io {
        for line in text.lines() {
            if let Some((key, value)) = line.split_once(':') {
                if matches!(key, "read_bytes" | "write_bytes" | "rchar" | "wchar") {
                    let n = value.trim().parse::<u64>().map_err(|_| unavailable())?;
                    metrics.push(metric(key,counter(n)?,"bytes","procfs lifetime I/O counter; storage-accounted bytes and character counts are distinct; not network traffic"));
                }
            }
        }
    }
    if process::start_time(pid) != Some(before)
        || read_file(Path::new("/proc/sys/kernel/random/boot_id"))?.trim() != boot
    {
        return Err(Error::new(
            "PROCESS_GENERATION_CHANGED",
            "Process/boot changed during snapshot",
        ));
    }
    Ok((
        format!("linux:{boot}:{pid}:{before}"),
        Some(parent),
        metrics,
        json!({"state":fields[0],"ioAvailability":if io.is_some(){"observed"}else{"unavailable"},"children":"not_enumerated","environmentRead":false}),
    ))
}
#[cfg(any(target_os = "macos", test))]
fn ps_snapshot(pid: u32, text: &str) -> Result<(Option<u32>, Vec<Value>, Value)> {
    let f: Vec<_> = text.split_whitespace().collect();
    if f.len() < 5 || f[0].parse::<u32>().ok() != Some(pid) {
        return Err(unavailable());
    }
    let parent = f[1].parse::<u32>().map_err(|_| unavailable())?;
    let rss = f[2]
        .parse::<u64>()
        .map_err(|_| unavailable())?
        .checked_mul(1024)
        .ok_or_else(unavailable)?;
    let cpu = f[3].parse::<f64>().map_err(|_| unavailable())?;
    if !cpu.is_finite() || cpu < 0.0 {
        return Err(unavailable());
    }
    Ok((
        Some(parent),
        vec![
            metric(
                "resident memory",
                counter(rss)?,
                "bytes",
                "ps resident snapshot; not retained heap or peak memory",
            ),
            metric(
                "reported CPU",
                cpu,
                "percent",
                "ps reported CPU average; native implementation/window, not an instantaneous sample",
            ),
        ],
        json!({"commandLabel":f[4..].join(" "),"children":"not_enumerated","environmentRead":false}),
    ))
}
#[cfg(target_os = "macos")]
async fn observe(
    pid: u32,
    budget: Duration,
    expected: Option<&str>,
) -> Result<(String, Option<u32>, Vec<Value>, Value)> {
    let before = process::start_time(pid).ok_or_else(unavailable)?;
    check(expected, &format!("macos:{pid}:{before}"))?;
    let id = pid.to_string();
    let output = process::run_diagnostic(
        "/bin/ps",
        &["-p", &id, "-o", "pid=,ppid=,rss=,pcpu=,comm="],
        budget,
        16384,
    )
    .await?;
    let (parent, metrics, native) = ps_snapshot(
        pid,
        std::str::from_utf8(&output.bytes).map_err(|_| unavailable())?,
    )?;
    if process::start_time(pid) != Some(before) {
        return Err(Error::new(
            "PROCESS_GENERATION_CHANGED",
            "Process changed during snapshot",
        ));
    }
    Ok((format!("macos:{pid}:{before}"), parent, metrics, native))
}
#[cfg(windows)]
async fn observe(
    pid: u32,
    budget: Duration,
    expected: Option<&str>,
) -> Result<(String, Option<u32>, Vec<Value>, Value)> {
    let guard = if let Some(g) = expected {
        let ticks = g
            .strip_prefix(&format!("windows:{pid}:"))
            .and_then(|n| n.parse::<i64>().ok())
            .filter(|n| *n > 0)
            .ok_or_else(invalid)?;
        format!("if($b -ne {ticks}){{throw 'process generation changed'}};")
    } else {
        String::new()
    };
    let script = format!(
        "$ErrorActionPreference='Stop'; $p=[Diagnostics.Process]::GetProcessById({pid}); $b=$p.StartTime.ToUniversalTime().Ticks; {guard} $r=@{{birth=$b.ToString();rss=$p.WorkingSet64;cpuMs=$p.TotalProcessorTime.TotalMilliseconds;threads=$p.Threads.Count;handles=$p.HandleCount}}; $q=[Diagnostics.Process]::GetProcessById({pid}); if($q.StartTime.ToUniversalTime().Ticks -ne $b){{throw 'process generation changed'}}; $r | ConvertTo-Json -Compress"
    );
    let output = process::run_diagnostic(
        "powershell.exe",
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ],
        budget,
        16384,
    )
    .await?;
    let v = super::analysis::apple_json(&output.bytes)?;
    let birth = v["birth"]
        .as_str()
        .filter(|s| s.parse::<u64>().is_ok())
        .ok_or_else(unavailable)?;
    let mut metrics = vec![];
    for (key, name, unit, definition) in [
        (
            "rss",
            "working set",
            "bytes",
            "native working set snapshot; not managed heap",
        ),
        (
            "cpuMs",
            "CPU lifetime",
            "milliseconds",
            "native lifetime processor time; not a rate",
        ),
        (
            "threads",
            "threads",
            "count",
            "native process thread count snapshot",
        ),
        (
            "handles",
            "handles",
            "count",
            "native process handle count snapshot",
        ),
    ] {
        let value = if key == "cpuMs" {
            v[key]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .ok_or_else(unavailable)?
        } else {
            counter(v[key].as_u64().ok_or_else(unavailable)?)?
        };
        metrics.push(metric(name, value, unit, definition));
    }
    Ok((
        format!("windows:{pid}:{birth}"),
        None,
        metrics,
        json!({"parentPid":"unknown","children":"not_enumerated","environmentRead":false}),
    ))
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
async fn observe(
    _pid: u32,
    _budget: Duration,
    _expected: Option<&str>,
) -> Result<(String, Option<u32>, Vec<Value>, Value)> {
    Err(Error::new(
        "UNSUPPORTED_HOST",
        "Host process snapshots require macOS, Linux or Windows",
    ))
}
