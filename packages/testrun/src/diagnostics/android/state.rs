//! Bounded resource snapshots using the Android collector's identity and process owner.
pub(crate) mod parse;
#[cfg(test)]
mod tests;
use super::*;
use std::collections::{BTreeMap, HashSet, VecDeque};

pub(crate) fn groups() -> Vec<String> {
    ["process", "memory", "power"].map(str::to_string).to_vec()
}
pub(crate) fn interval() -> u64 {
    1000
}
pub(crate) fn process_limit() -> usize {
    4
}
#[derive(Clone)]
pub(crate) struct Options {
    pub groups: Vec<String>,
    pub interval_ms: u64,
    pub max_processes: usize,
    pub label: Option<String>,
}
impl Options {
    pub fn validate(&self) -> Result<()> {
        if self.groups.is_empty()
            || self.groups.len() > 8
            || self.groups.iter().collect::<HashSet<_>>().len() != self.groups.len()
            || self.groups.iter().any(|g| {
                !matches!(
                    g.as_str(),
                    "process"
                        | "memory"
                        | "scheduling"
                        | "power"
                        | "frames"
                        | "connectivity"
                        | "media"
                        | "device_context"
                )
            })
            || !(500..=60000).contains(&self.interval_ms)
            || !(1..=8).contains(&self.max_processes)
        {
            return Err(Error::invalid(
                "Select distinct Android state groups, intervalMs 500..60000 and maxProcesses 1..8",
            ));
        }
        if let Some(label) = &self.label {
            crate::diagnostics::model::label(label, 256)?;
        }
        Ok(())
    }
    fn has(&self, group: &str) -> bool {
        self.groups.iter().any(|g| g == group)
    }
}
pub(super) enum Read {
    Process(super::parse::Process),
    Text(&'static str, Vec<String>),
    Connectivity,
}
#[derive(Default)]
pub(super) struct History {
    hz: Option<u64>,
    hz_probed: bool,
    counters: BTreeMap<String, parse::Counters>,
    frames: HashSet<String>,
}
impl Capture {
    pub(super) fn state_plan(&self) -> VecDeque<Read> {
        let options = self.options.state.as_ref().unwrap();
        let mut reads = VecDeque::new();
        if options.has("process") || options.has("memory") || options.has("frames") {
            reads.extend(
                self.processes
                    .iter()
                    .take(options.max_processes)
                    .cloned()
                    .map(Read::Process),
            );
        }
        let app = &self.options.app;
        let user = self.options.user.to_string();
        if options.has("scheduling") {
            for (group, args) in [
                ("services", vec!["dumpsys", "activity", "services", app]),
                ("jobs", vec!["dumpsys", "jobscheduler", app]),
                ("alarms", vec!["dumpsys", "alarm"]),
            ] {
                reads.push_back(Read::Text(
                    group,
                    args.into_iter().map(str::to_string).collect(),
                ));
            }
        }
        if options.has("power") {
            for (group, args) in [
                ("battery", vec!["dumpsys", "battery"]),
                ("idle", vec!["dumpsys", "deviceidle"]),
                ("thermal", vec!["dumpsys", "thermalservice"]),
                ("power", vec!["dumpsys", "power"]),
                ("manufacturer", vec!["getprop", "ro.product.manufacturer"]),
            ] {
                reads.push_back(Read::Text(
                    group,
                    args.into_iter().map(str::to_string).collect(),
                ));
            }
            if self.api >= 28 {
                reads.push_back(Read::Text(
                    "standby",
                    ["am", "get-standby-bucket", "--user", &user, app]
                        .map(str::to_string)
                        .to_vec(),
                ));
                reads.push_back(Read::Text(
                    "background",
                    [
                        "cmd",
                        "appops",
                        "get",
                        "--user",
                        &user,
                        app,
                        "RUN_ANY_IN_BACKGROUND",
                    ]
                    .map(str::to_string)
                    .to_vec(),
                ));
            }
        }
        if options.has("connectivity") {
            reads.push_back(Read::Connectivity);
        }
        if options.has("media") {
            for (group, service) in [("audio", "audio"), ("media_sessions", "media_session")] {
                reads.push_back(Read::Text(group, vec!["dumpsys".into(), service.into()]));
            }
        }
        if options.has("device_context") {
            for (group, property) in [
                ("locale", "persist.sys.locale"),
                ("timezone", "persist.sys.timezone"),
            ] {
                reads.push_back(Read::Text(group, vec!["getprop".into(), property.into()]));
            }
            for name in ["font_scale", "user_rotation", "accelerometer_rotation"] {
                reads.push_back(Read::Text(
                    name,
                    vec![
                        "settings".into(),
                        "--user".into(),
                        user.clone(),
                        "get".into(),
                        "system".into(),
                        name.into(),
                    ],
                ));
            }
        }
        reads
    }
    pub(super) async fn read_state(&mut self, read: Read) -> Value {
        match read {
            Read::Process(p) => self.state_process(&p).await,
            Read::Text(group, args) => {
                let refs: Vec<_> = args.iter().map(String::as_str).collect();
                match self.shell(&refs, 256 * 1024).await {
                    Ok(bytes) => {
                        let mut data = parse::context(
                            group,
                            &String::from_utf8_lossy(&bytes),
                            &self.options.app,
                            self.options.user,
                            self.uid,
                        );
                        data["source"] = json!(args);
                        if data["status"] == "unrecognized_format" {
                            self.gaps
                                .insert(format!("state_{group}:unrecognized_format"));
                        }
                        data
                    }
                    Err(e) => {
                        let mut data = self.gap(&format!("state_{group}"), e);
                        data["group"] = json!(group);
                        data
                    }
                }
            }
            Read::Connectivity => {
                // Reuse the exact readback and parser owned by network control. No switch plan runs.
                let budget = match self.budget() {
                    Ok(budget) => budget,
                    Err(e) => return self.gap("state_connectivity", e),
                };
                match process::run_diagnostic(
                    &self.adb.adb,
                    &[
                        "-t",
                        &self.transport,
                        "shell",
                        crate::network::android::READBACK,
                    ],
                    budget,
                    65536,
                )
                .await
                {
                    Ok(out) => {
                        let text = String::from_utf8_lossy(&out.bytes);
                        if !text.contains("#end") {
                            return json!({"group":"connectivity","status":"unrecognized_format","source":"network::android::READBACK"});
                        }
                        json!({"group":"connectivity","status":"sampled","scope":"device","fields":crate::network::Readback::parse(&text).to_json(),"source":"network::android::READBACK","complete":false})
                    }
                    Err(e) => {
                        let mut data = self.gap("state_connectivity", e);
                        data["group"] = json!("connectivity");
                        data
                    }
                }
            }
        }
    }
    async fn process_identity(
        &self,
        p: &super::parse::Process,
    ) -> Result<(super::parse::Process, Vec<u8>)> {
        let pid = p.pid.to_string();
        let bytes = self
            .shell(
                &[
                    "cat",
                    &format!("/proc/{pid}/stat"),
                    &format!("/proc/{pid}/status"),
                    &format!("/proc/{pid}/cmdline"),
                ],
                32768,
            )
            .await?;
        let observed = super::parse::process(&bytes, p.pid, self.uid, &self.boot)
            .filter(|now| now.generation == p.generation)
            .ok_or_else(|| {
                Error::new(
                    "ANDROID_PROCESS_CHANGED",
                    "Process identity changed or became inaccessible during the state sample",
                )
            })?;
        Ok((observed, bytes))
    }
    async fn state_process(&mut self, p: &super::parse::Process) -> Value {
        let result = self.state_process_checked(p).await;
        match result {
            Ok(mut data) => {
                data["processGeneration"] = json!(p.generation);
                data["pid"] = json!(p.pid);
                data["group"] = json!("process");
                data
            }
            Err(e) => {
                let mut data = self.gap("state_process", e);
                data["group"] = json!("process");
                data["pid"] = json!(p.pid);
                data["processGeneration"] = json!(p.generation);
                data
            }
        }
    }
    async fn state_process_checked(&mut self, p: &super::parse::Process) -> Result<Value> {
        let (_, identity) = self.process_identity(p).await?;
        let text = String::from_utf8_lossy(&identity);
        let options = self.options.state.as_ref().unwrap().clone();
        let mut data = json!({"status":"sampled","scope":"process","metrics":[],"readings":{},"complete":false});
        let mut metrics = Vec::new();
        let mut baseline = None;
        let mut frame_keys = Vec::new();
        data["readings"]["proc"] = json!({"status":"sampled","source":"proc stat/status/cmdline; identity checked before and after the complete process sample","raw":text});
        if options.has("process") {
            baseline = self
                .state_process_metrics(p, &text, &mut data, &mut metrics)
                .await?;
        }
        if options.has("memory") {
            match self
                .shell(
                    &["dumpsys", "meminfo", "-s", &p.pid.to_string()],
                    128 * 1024,
                )
                .await
            {
                Ok(bytes) => {
                    let parsed = parse::memory(&String::from_utf8_lossy(&bytes), p.pid);
                    if let Some(values) = parsed["metrics"].as_array() {
                        metrics.extend(values.iter().cloned());
                    }
                    data["readings"]["memory"] = parsed;
                }
                Err(e) => data["readings"]["memory"] = self.gap("state_memory", e),
            }
        }
        if options.has("frames") {
            match self
                .shell(
                    &["dumpsys", "gfxinfo", &p.pid.to_string(), "framestats"],
                    256 * 1024,
                )
                .await
            {
                Ok(bytes) => {
                    let parsed = parse::frames(&String::from_utf8_lossy(&bytes), p.pid);
                    for frame in parsed["frames"].as_array().into_iter().flatten() {
                        let key = format!(
                            "{}:{}:{}",
                            p.generation, frame["intendedVsyncNs"], frame["frameCompletedNs"]
                        );
                        if !self.state_history.frames.contains(&key)
                            && !frame_keys.contains(&key)
                            && self.state_history.frames.len() + frame_keys.len() < 32768
                        {
                            let mut metric = parse::metric(
                                "frame_duration",
                                frame["durationMs"].as_f64().unwrap(),
                                "milliseconds",
                                "gfxinfo FrameCompleted minus IntendedVsync for Flags=0; recent frame history, not CPU time",
                            );
                            metric["frame"] = frame.clone();
                            metrics.push(metric);
                            frame_keys.push(key);
                        }
                    }
                    data["readings"]["frames"] = parsed;
                    if self.state_history.frames.len() + frame_keys.len() >= 32768 {
                        self.gaps.insert("state_frames:history_limit".into());
                        data["frameHistoryLimitReached"] = json!(true);
                    }
                }
                Err(e) => data["readings"]["frames"] = self.gap("state_frames", e),
            }
        }
        // Commit counters/frames only after the complete sample still belongs to this generation.
        self.process_identity(p).await?;
        for (name, reading) in data["readings"].as_object().unwrap() {
            if reading["status"] == "unrecognized_format" {
                self.gaps
                    .insert(format!("state_{name}:unrecognized_format"));
            }
        }
        if let Some(baseline) = baseline {
            self.state_history
                .counters
                .insert(p.generation.clone(), baseline);
        }
        self.state_history
            .counters
            .retain(|key, _| self.processes.iter().any(|p| &p.generation == key));
        self.state_history.frames.extend(frame_keys);
        data["metrics"] = json!(metrics);
        Ok(data)
    }
    async fn state_process_metrics(
        &mut self,
        p: &super::parse::Process,
        text: &str,
        data: &mut Value,
        metrics: &mut Vec<Value>,
    ) -> Result<Option<parse::Counters>> {
        let mut baseline = None;
        metrics.extend(parse::status_metrics(text));
        data["fields"] = parse::process_fields(text);
        if !self.state_history.hz_probed {
            self.state_history.hz_probed = true;
            if let Ok(bytes) = self.shell(&["getconf", "CLK_TCK"], 256).await {
                self.state_history.hz = String::from_utf8_lossy(&bytes)
                    .trim()
                    .parse::<u64>()
                    .ok()
                    .filter(|n| *n > 0 && *n <= 1000000);
            }
        }
        data["readings"]["cpuClock"] = json!({"status":if self.state_history.hz.is_some(){"sampled"}else{"unavailable"},"clockTicksPerSecond":self.state_history.hz});
        if self.state_history.hz.is_none() {
            self.gaps.insert("state_cpu_clock:unavailable".into());
        }
        let uptime = self.shell(&["cat", "/proc/uptime"], 256).await;
        if let Ok(bytes) = uptime {
            data["readings"]["uptime"] = json!({"status":"sampled","source":"/proc/uptime","raw":String::from_utf8_lossy(&bytes)});
            let uptime = String::from_utf8_lossy(&bytes)
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v >= 0.0);
            if let Some(uptime) = uptime
                && let Some(now) = parse::Counters::read(text, uptime, self.state_history.hz)
            {
                metrics.extend(now.metrics(self.state_history.counters.get(&p.generation)));
                data["counterWindow"] = json!({"uptimeSeconds":uptime,"clockTicksPerSecond":self.state_history.hz,"previousUptimeSeconds":self.state_history.counters.get(&p.generation).map(|v|v.uptime),"clock":"boot-relative kernel uptime; reads are sequential, not atomic"});
                baseline = Some(now);
            }
        } else {
            data["readings"]["uptime"] = self.gap("state_uptime", uptime.err().unwrap());
        }
        for (name, args) in [
            ("io", vec!["cat".into(), format!("/proc/{}/io", p.pid)]),
            (
                "fds",
                vec!["ls".into(), "-1".into(), format!("/proc/{}/fd", p.pid)],
            ),
            (
                "threads",
                vec![
                    "ps".into(),
                    "-T".into(),
                    "-p".into(),
                    p.pid.to_string(),
                    "-n".into(),
                    "-o".into(),
                    "PID,TID,UID,STAT,NAME".into(),
                ],
            ),
        ] {
            let refs: Vec<_> = args.iter().map(String::as_str).collect();
            match self.shell(&refs, 65536).await {
                Ok(bytes) => {
                    let reading = String::from_utf8_lossy(&bytes);
                    let parsed = match name {
                        "io" => parse::io(&reading),
                        "fds" => parse::fds(&reading),
                        _ => parse::threads(&reading, p.pid, self.uid),
                    };
                    if let Some(values) = parsed["metrics"].as_array() {
                        metrics.extend(values.iter().cloned());
                    }
                    data["readings"][name] = parsed;
                }
                Err(e) => data["readings"][name] = self.gap(&format!("state_{name}"), e),
            }
        }
        Ok(baseline)
    }
}
