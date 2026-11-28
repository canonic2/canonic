//! Fixed native collectors and offline converters; no arbitrary commands or SQL.
mod files;
pub(super) mod perfetto;
#[cfg(test)]
mod tests;
use crate::{
    error::{Error, Result},
    process,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const MAX_ARTIFACT: usize = 4 * 1024 * 1024;
pub(super) const MAX_ENVELOPE: usize = 8 * 1024 * 1024;
pub(super) const ACTIVITY_MONITOR: &str = "Activity Monitor";
pub(super) const ACTIVITY_MONITOR_TABLE: &str = "sysmon-process";
pub(super) const ACTIVITY_MONITOR_FORMAT: &str = "activity-monitor-json";
pub(super) const XCTRACE_TEMPLATES: &[&str] = &[
    "Time Profiler",
    "Points of Interest",
    "System Trace",
    "Hangs",
    "Allocations",
    "Leaks",
    ACTIVITY_MONITOR,
];
pub fn duration() -> u64 {
    3000
}
pub fn frequency() -> u64 {
    99
}
fn disabled(value: &bool) -> bool {
    !*value
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Options {
    pub backend: String,
    pub mode: String,
    pub target_id: String,
    pub app_id: String,
    pub process_id: Option<u32>,
    pub path: Option<PathBuf>,
    pub tool_path: Option<PathBuf>,
    pub template: Option<String>,
    #[serde(default = "duration")]
    pub duration_ms: u64,
    #[serde(default = "frequency")]
    pub frequency_hz: u64,
    #[serde(default)]
    pub allow_system_trace: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perfetto_buffer_kib: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perfetto_fill_policy: Option<perfetto::FillPolicy>,
    #[serde(default, skip_serializing_if = "disabled")]
    pub wait_for_perfetto_start: bool,
    #[serde(default)]
    pub allow_attach: bool,
    #[serde(default, skip_serializing_if = "disabled")]
    pub allow_allocation_recording: bool,
    #[serde(default, skip_serializing_if = "disabled")]
    pub allow_heap_inspection: bool,
}
pub(crate) struct Capture {
    adb: crate::android::Android,
    options: Options,
    directory: PathBuf,
    bytes: Option<Vec<u8>>,
    control: Option<(tokio::sync::watch::Receiver<Option<String>>, Instant)>,
    pub actions: Vec<String>,
    report: Value,
    remote: Option<(String, String, String)>,
    perfetto_start: Option<tokio::sync::oneshot::Sender<Instant>>,
}
impl Options {
    pub(super) fn validate_retention_budget(&self, max_bytes: u64) -> Result<()> {
        if self.perfetto_capture() {
            let buffer_bytes = u64::from(self.buffer_kib()) * 1024;
            let encoded_capacity = buffer_bytes.div_ceil(3) * 4;
            if max_bytes <= encoded_capacity {
                return Err(Error::new(
                    "NATIVE_TRACE_BOUNDS",
                    "Increase limits.maxBytes or reduce perfettoBufferKib to fit the trace and its metadata",
                )
                .details(json!({"maxBytes":max_bytes,"encodedBufferBytes":encoded_capacity})));
            }
        }
        Ok(())
    }

    fn perfetto_capture(&self) -> bool {
        self.backend == "perfetto" && self.mode == "capture"
    }

    fn buffer_kib(&self) -> u32 {
        self.perfetto_buffer_kib
            .unwrap_or(perfetto::DEFAULT_BUFFER_KIB)
    }

    fn activity_monitor(&self) -> bool {
        self.backend == "xctrace" && self.template.as_deref() == Some(ACTIVITY_MONITOR)
    }

    fn record_grace_ms(&self) -> u64 {
        if self.activity_monitor() {
            15_000
        } else {
            4_000
        }
    }

    pub fn validate(&self) -> Result<()> {
        for s in [&self.target_id, &self.app_id] {
            super::model::label(s, 256)?;
        }
        if !matches!(
            self.backend.as_str(),
            "perfetto" | "simpleperf" | "xctrace" | "stack_sample"
        ) || !matches!(self.mode.as_str(), "capture" | "import")
            || !(100..=10000).contains(&self.duration_ms)
            || !(1..=1000).contains(&self.frequency_hz)
            || self.process_id == Some(0)
        {
            return Err(Error::invalid(
                "Select native backend perfetto/simpleperf/xctrace/stack_sample, mode capture/import, durationMs 100..10000 and frequencyHz 1..1000",
            ));
        }
        if (self.mode == "import") != self.path.is_some()
            || self.path.as_ref().is_some_and(|p| !p.is_absolute())
            || self.tool_path.as_ref().is_some_and(|p| !p.is_absolute())
        {
            return Err(Error::invalid(
                "Import requires an absolute path; capture refuses input paths; toolPath must be an absolute trusted native executable",
            ));
        }
        if matches!(self.backend.as_str(), "perfetto" | "simpleperf") && self.tool_path.is_none() {
            return Err(Error::invalid(
                "Select the installed native trace-processor or simpleperf executable through toolPath; no wrapper download is performed",
            ));
        }
        if self.backend == "xctrace"
            && self
                .template
                .as_deref()
                .is_none_or(|template| !XCTRACE_TEMPLATES.contains(&template))
        {
            return Err(Error::invalid(
                "Select an explicit xctrace template: Time Profiler, Points of Interest, System Trace, Hangs, Allocations, Leaks or Activity Monitor",
            ));
        }
        if self.backend != "xctrace" && self.template.is_some() {
            return Err(Error::invalid("template is xctrace-only"));
        }
        if (self.perfetto_buffer_kib.is_some()
            || self.perfetto_fill_policy.is_some()
            || self.wait_for_perfetto_start)
            && !self.perfetto_capture()
        {
            return Err(Error::invalid(
                "Perfetto buffer and start options require Perfetto capture",
            ));
        }
        if let Some(kib) = self.perfetto_buffer_kib
            && !(perfetto::MIN_BUFFER_KIB..=perfetto::MAX_BUFFER_KIB).contains(&kib)
        {
            return Err(Error::invalid(format!(
                "perfettoBufferKib must be {}..={}",
                perfetto::MIN_BUFFER_KIB,
                perfetto::MAX_BUFFER_KIB
            )));
        }
        if self.activity_monitor()
            && (!self.target_id.starts_with("device:")
                || self.process_id.is_none()
                || self.allow_attach
                || (self.mode == "capture" && !self.allow_system_trace))
        {
            return Err(Error::invalid(
                "Activity Monitor requires a physical iOS device:UDID and the app's exact processId; capture requires allowSystemTrace:true; omit allowAttach",
            ));
        }
        if self.mode == "capture" {
            self.validate_capture()?;
        }
        if self.mode == "capture"
            && matches!(self.backend.as_str(), "perfetto" | "simpleperf")
            && !super::android::parse::package(&self.app_id)
        {
            return Err(Error::invalid("Select an exact Android package"));
        }
        Ok(())
    }
    fn validate_capture(&self) -> Result<()> {
        if self.template.as_deref() == Some("Allocations") && !self.allow_allocation_recording {
            return Err(Error::invalid(
                "Allocations capture additionally requires allowAllocationRecording:true; recording can perturb allocation behavior",
            ));
        }
        if self.template.as_deref() == Some("Leaks") && !self.allow_heap_inspection {
            return Err(Error::invalid(
                "Leaks capture additionally requires allowHeapInspection:true; inspection can stop/perturb the process",
            ));
        }
        if self.backend == "perfetto"
            && (!self.target_id.starts_with("android:") || !self.allow_system_trace)
        {
            return Err(Error::invalid(
                "Perfetto capture requires an explicit Android target and allowSystemTrace:true",
            ));
        }
        if self.backend == "simpleperf"
            && (!self.target_id.starts_with("android:") || self.process_id.is_none())
        {
            return Err(Error::invalid(
                "simpleperf capture requires Android target and exact processId",
            ));
        }
        if matches!(self.backend.as_str(), "xctrace" | "stack_sample")
            && !self.activity_monitor()
            && (!self.allow_attach || self.process_id.is_none())
        {
            return Err(Error::invalid(
                "Native attach requires exact processId and allowAttach:true",
            ));
        }
        if self.backend == "xctrace"
            && !(self.target_id.starts_with("simulator:")
                || self.target_id.starts_with("device:")
                || self.target_id == "host:macos")
        {
            return Err(Error::invalid(
                "xctrace capture requires simulator:UUID, device:UDID or host:macos",
            ));
        }
        if self.backend == "xctrace" && self.target_id != "host:macos" {
            let id = self.target_id.split_once(':').unwrap().1;
            if id.is_empty()
                || id.starts_with('-')
                || id.len() > 128
                || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err(Error::invalid("Select an exact Apple UUID/UDID token"));
            }
        }
        if self.backend == "stack_sample" && self.target_id != "host:macos" {
            return Err(Error::invalid(
                "Lightweight sample attaches only to an explicit host:macos process; import device hang reports separately",
            ));
        }
        // Instruments and sample ship with macOS; an explicit toolPath or
        // TESTRUN_XCRUN selects another program on purpose.
        if matches!(self.backend.as_str(), "xctrace" | "stack_sample")
            && !cfg!(target_os = "macos")
            && self.tool_path.is_none()
            && std::env::var_os("TESTRUN_XCRUN").is_none()
        {
            return Err(Error::new(
                "APPLE_HOST_UNSUPPORTED",
                "xctrace and sample capture require macOS",
            ));
        }
        Ok(())
    }
}
impl Capture {
    pub fn new(options: Options) -> Result<Self> {
        options.validate()?;
        let directory =
            std::env::temp_dir().join(format!("testrun-native-{}", uuid::Uuid::new_v4()));
        crate::artifacts::directory(&directory)?;
        Ok(Self {
            adb: crate::android::Android::default(),
            options,
            directory,
            bytes: None,
            control: None,
            actions: vec![],
            report: json!({"adapter":"native-trace/1","cleanup":"not_started"}),
            remote: None,
            perfetto_start: None,
        })
    }
    pub(super) fn perfetto_start(&mut self) -> Option<tokio::sync::oneshot::Receiver<Instant>> {
        self.options.wait_for_perfetto_start.then(|| {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            self.perfetto_start = Some(sender);
            receiver
        })
    }
    pub fn control(
        &mut self,
        stopped: tokio::sync::watch::Receiver<Option<String>>,
        budget: Duration,
    ) {
        self.control = Some((stopped, Instant::now() + budget));
    }
    fn budget(&self, max: Duration) -> Result<Duration> {
        if let Some((stop, until)) = &self.control {
            if stop.borrow().is_some() {
                return Err(Error::new(
                    "CANCELLED",
                    "Native operation stopped; inspect retained cleanup status",
                ));
            }
            let d = max.min(until.saturating_duration_since(Instant::now()));
            if d.is_zero() {
                return Err(Error::new("TIMEOUT", "Native operation duration expired"));
            }
            return Ok(d);
        }
        Ok(max)
    }
    async fn run(
        &self,
        program: &Path,
        args: &[String],
        max: usize,
        deadline: Duration,
    ) -> Result<Vec<u8>> {
        let budget = self.budget(deadline)?;
        let argv = args.iter().map(String::as_str).collect::<Vec<_>>();
        let task = process::run(program, &argv, budget, max);
        tokio::pin!(task);
        if let Some((stop, _)) = &self.control {
            let mut stop = stop.clone();
            tokio::select! {biased; _=stop.changed()=>{
                // Preserve the process owner through its finite deadline/reap.
                let _=task.await;
                Err(Error::new("CANCELLED","Native command settled after cancellation; no replay"))
            }, result=&mut task=>result.map(|o|o.bytes)}
        } else {
            task.await.map(|o| o.bytes)
        }
    }
    async fn shell(
        &self,
        transport: &str,
        args: &[String],
        max: usize,
        deadline: Duration,
    ) -> Result<Vec<u8>> {
        let command = args
            .iter()
            .map(|s| format!("'{}'", s.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join(" ");
        self.run(
            &self.adb.adb,
            &["-t".into(), transport.into(), "shell".into(), command],
            max,
            deadline,
        )
        .await
    }
    async fn android_identity(&self, transport: &str) -> Result<Value> {
        let boot = self
            .shell(
                transport,
                &["cat".into(), "/proc/sys/kernel/random/boot_id".into()],
                256,
                Duration::from_secs(3),
            )
            .await?;
        let boot = String::from_utf8_lossy(&boot).trim().to_owned();
        if boot.len() != 36 || !boot.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
            return Err(Error::new(
                "NATIVE_IDENTITY_UNKNOWN",
                "Selected Android boot identity was not readable",
            ));
        }
        let packages = self
            .shell(
                transport,
                &[
                    "cmd".into(),
                    "package".into(),
                    "list".into(),
                    "packages".into(),
                    "--user".into(),
                    "0".into(),
                    "-U".into(),
                    self.options.app_id.clone(),
                ],
                16384,
                Duration::from_secs(3),
            )
            .await?;
        let package = String::from_utf8_lossy(&packages)
            .lines()
            .find(|s| {
                s.split_whitespace().next()
                    == Some(format!("package:{}", self.options.app_id).as_str())
            })
            .map(str::to_owned)
            .ok_or_else(|| {
                Error::new(
                    "NATIVE_APP_UNAVAILABLE",
                    "Selected installed package was not observed",
                )
            })?;
        let mut identity = json!({"bootId":boot,"package":package,"transportId":transport,"pid":self.options.process_id});
        if let Some(pid) = self.options.process_id {
            let stat = self
                .shell(
                    transport,
                    &["cat".into(), format!("/proc/{pid}/stat")],
                    4096,
                    Duration::from_secs(3),
                )
                .await?;
            let stat = String::from_utf8_lossy(&stat);
            let fields: Vec<_> = stat
                .rsplit_once(") ")
                .ok_or_else(|| Error::new("NATIVE_IDENTITY_UNKNOWN", "Missing process birth"))?
                .1
                .split_whitespace()
                .collect();
            let status = self
                .shell(
                    transport,
                    &["cat".into(), format!("/proc/{pid}/status")],
                    16384,
                    Duration::from_secs(3),
                )
                .await?;
            let status = String::from_utf8_lossy(&status);
            let uid = status
                .lines()
                .find_map(|s| s.strip_prefix("Uid:"))
                .and_then(|s| s.split_whitespace().next())
                .ok_or_else(|| Error::invalid("Missing process UID"))?;
            if !package
                .split_whitespace()
                .any(|s| s == format!("uid:{uid}"))
            {
                return Err(Error::new(
                    "NATIVE_PROCESS_MISMATCH",
                    "Selected PID does not have the installed package UID",
                ));
            }
            let start = fields
                .get(19)
                .filter(|s| s.parse::<u64>().is_ok())
                .ok_or_else(|| {
                    Error::new("NATIVE_IDENTITY_UNKNOWN", "Missing numeric process birth")
                })?;
            identity["startTicks"] = json!(start);
            identity["pausedBeforeAttach"] = json!(matches!(fields.first(), Some(&"T" | &"t")));
            identity["uid"] = json!(uid);
        }
        Ok(identity)
    }
    async fn host_identity(&self) -> Result<Value> {
        let started = process::start_time(self.options.process_id.unwrap()).ok_or_else(|| {
            Error::new(
                "NATIVE_IDENTITY_UNKNOWN",
                "Cannot read selected host process birth",
            )
        })?;
        let pid = self.options.process_id.unwrap().to_string();
        let bytes = self
            .run(
                Path::new("/bin/ps"),
                &["-p".into(), pid, "-o".into(), "lstart=,state=,comm=".into()],
                16384,
                Duration::from_secs(3),
            )
            .await?;
        if process::start_time(self.options.process_id.unwrap()) != Some(started) {
            return Err(Error::new(
                "NATIVE_IDENTITY_CHANGED",
                "Selected host process changed during identity read",
            ));
        }
        let value = String::from_utf8_lossy(&bytes).trim().to_owned();
        if value.is_empty() {
            return Err(Error::new(
                "NATIVE_PROCESS_UNAVAILABLE",
                "Selected host PID was not observed",
            ));
        }
        // The mutable state is kept separately from the birth/executable witness.
        let fields: Vec<_> = value.split_whitespace().collect();
        if fields.len() < 7 {
            return Err(Error::new(
                "NATIVE_IDENTITY_UNKNOWN",
                "Host process identity was incomplete",
            ));
        }
        Ok(
            json!({"birth":fields[..5].join(" "),"kernelBirth":started,"executable":fields[6..].join(" "),"state":fields[5],"pausedBeforeAttach":fields[5].contains('T'),"pid":self.options.process_id,"appAssociation":"caller_declared"}),
        )
    }
    async fn record_android(
        &mut self,
        transport: &str,
        identity: &Value,
        artifact_path: &Path,
        duration: Duration,
    ) -> Result<()> {
        if self.options.backend == "perfetto" {
            let config = perfetto::config(
                self.options.duration_ms,
                &self.options.app_id,
                self.options.buffer_kib(),
                self.options.perfetto_fill_policy.unwrap_or_default(),
            );
            let config_path = self.directory.join("config.pbtxt");
            crate::artifacts::write(&config_path, config.as_bytes())?;
            // Config is stdin, trace is stdout, and the optional acknowledgement
            // is a NUL on stderr through a separate remote descriptor.
            let client_start = self.perfetto_start.take();
            let budget = self.budget(duration + Duration::from_secs(4))?;
            let mut stop = self.control.as_ref().unwrap().0.clone();
            let deadline = Instant::now() + duration;
            let stopped = stop.clone();
            let recording = async {
                if let Some(client) = client_start {
                    let command = "exec perfetto --txt -c - -o - --notify-fd 3 3>&2";
                    process::run_input_with_start_ack(
                        &self.adb.adb,
                        &["-t", transport, "shell", "-T", command],
                        config.as_bytes(),
                        budget,
                        MAX_ARTIFACT,
                        move || {
                            if stopped.borrow().is_none() {
                                let _ = client.send(deadline);
                            }
                        },
                    )
                    .await
                } else {
                    process::run_input(
                        &self.adb.adb,
                        &["-t", transport, "shell", "-T", "perfetto --txt -c - -o -"],
                        config.as_bytes(),
                        budget,
                        MAX_ARTIFACT,
                    )
                    .await
                }
            };
            tokio::pin!(recording);
            let result = tokio::select! {
                biased;
                _ = stop.changed() => {
                    let _ = recording.await;
                    return Err(Error::new("CANCELLED", "Perfetto host client settled; remote termination remains unverified"));
                }
                result = &mut recording => result?,
            };
            crate::artifacts::write(artifact_path, &result.bytes)?;
        } else {
            let remote = format!("/data/local/tmp/testrun-{}.perf.data", uuid::Uuid::new_v4());
            self.remote = Some((
                transport.to_owned(),
                remote.clone(),
                identity["bootId"].as_str().unwrap().into(),
            ));
            self.shell(
                transport,
                &[
                    "simpleperf".into(),
                    "record".into(),
                    "-p".into(),
                    self.options.process_id.unwrap().to_string(),
                    "-e".into(),
                    "cpu-clock:u".into(),
                    "-f".into(),
                    self.options.frequency_hz.to_string(),
                    "--call-graph".into(),
                    "dwarf,8192".into(),
                    "--duration".into(),
                    format!("{:.3}", duration.as_secs_f64()),
                    "-o".into(),
                    remote.clone(),
                ],
                65536,
                duration + Duration::from_secs(4),
            )
            .await?;
            let bytes = self
                .run(
                    &self.adb.adb,
                    &[
                        "-t".into(),
                        transport.to_owned(),
                        "exec-out".into(),
                        "cat".into(),
                        remote,
                    ],
                    MAX_ARTIFACT,
                    Duration::from_secs(3),
                )
                .await?;
            crate::artifacts::write(artifact_path, &bytes)?;
        }
        Ok(())
    }
    async fn capture_android(
        &mut self,
        artifact_path: &Path,
        duration: Duration,
        gaps: &mut Vec<Value>,
    ) -> Result<Value> {
        let serial = self.options.target_id.strip_prefix("android:").unwrap();
        let devices = self
            .adb
            .devices_bounded(self.budget(Duration::from_secs(3))?)
            .await?;
        let found: Vec<_> = devices
            .iter()
            .filter(|d| d.serial == serial && d.state == "device")
            .collect();
        if found.len() != 1 || !found[0].transport.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::new(
                "NATIVE_TARGET_UNAVAILABLE",
                "Select one authorized Android transport",
            ));
        }
        let transport = found[0].transport.clone();
        let mut identity = self.android_identity(&transport).await?;
        self.record_android(&transport, &identity, artifact_path, duration)
            .await?;
        let after = match self.android_identity(&transport).await {
            Ok(value) => value,
            Err(e) => {
                gaps.push(json!({"section":"identity_after","unavailable":e.code,"meaning":"native artifact retained; process continuity unavailable"}));
                json!({"unavailable":e.code})
            }
        };
        // Paused state is contextual, not part of process birth.
        let mut a = identity.clone();
        let mut b = after.clone();
        a.as_object_mut().unwrap().remove("pausedBeforeAttach");
        b.as_object_mut().unwrap().remove("pausedBeforeAttach");
        if a != b {
            gaps.push(json!(
                "process_or_boot_changed_during_capture; PID scope not continuous"
            ));
        }
        identity["after"] = after;
        Ok(identity)
    }
    async fn capture_apple(
        &mut self,
        artifact_path: &Path,
        duration: Duration,
        gaps: &mut Vec<Value>,
    ) -> Result<Value> {
        let mut identity = Value::Null;
        if self.options.target_id == "host:macos" {
            identity = self.host_identity().await?;
        } else if self.options.activity_monitor() {
            identity = json!({"selectedAppPid": self.options.process_id, "appAssociation":"caller_declared"});
            gaps.push(json!("device_process_birth_and_app_association_unverified; app PID and device UDID caller_selected"));
        } else {
            gaps.push(json!("device_process_birth_and_app_association_unverified; exact PID/UDID caller_selected"));
        }
        if self.options.backend == "stack_sample" {
            self.run(
                self.options
                    .tool_path
                    .as_deref()
                    .unwrap_or(Path::new("/usr/bin/sample")),
                &[
                    self.options.process_id.unwrap().to_string(),
                    format!("{:.3}", duration.as_secs_f64()),
                    "10".into(),
                    "-file".into(),
                    artifact_path.to_string_lossy().into_owned(),
                ],
                MAX_ARTIFACT,
                // Symbol processing can outlast a short UIKit/Swift
                // recording. The existing native-command ceiling is
                // still clipped by the caller's remaining budget.
                Duration::from_secs(14),
            )
            .await?;
        } else {
            let mut args = vec![
                "xctrace".into(),
                "record".into(),
                "--template".into(),
                self.options.template.clone().unwrap(),
                "--time-limit".into(),
                format!("{}ms", self.options.duration_ms),
                "--output".into(),
                artifact_path.to_string_lossy().into_owned(),
                "--no-prompt".into(),
            ];
            if self.options.activity_monitor() {
                args.push("--all-processes".into());
            } else {
                args.extend([
                    "--attach".into(),
                    self.options.process_id.unwrap().to_string(),
                ]);
            }
            if self.options.target_id != "host:macos" {
                args.extend([
                    "--device".into(),
                    self.options.target_id.split_once(':').unwrap().1.into(),
                ]);
            }
            self.run(
                &self.xcrun(),
                &args,
                65536,
                duration + Duration::from_millis(self.options.record_grace_ms()),
            )
            .await?;
        }
        if self.options.target_id == "host:macos" {
            let after = match self.host_identity().await {
                Ok(value) => value,
                Err(e) => {
                    gaps.push(json!({"section":"identity_after","unavailable":e.code,"meaning":"native artifact retained; host process continuity unavailable"}));
                    json!({"unavailable":e.code})
                }
            };
            if identity["birth"] != after["birth"]
                || identity["kernelBirth"] != after["kernelBirth"]
                || identity["executable"] != after["executable"]
            {
                gaps.push(json!("host_process_generation_changed_during_attach"));
            }
            identity["after"] = after;
        }
        Ok(identity)
    }
    async fn build(&mut self) -> Result<Vec<u8>> {
        let mut sections = vec![];
        let mut identity = Value::Null;
        let mut recording = None;
        let mut gaps = vec![json!(
            "partial_trace; no whole-app coverage or absence/causality guarantee"
        )];
        let bundle_limit =
            (self.options.backend == "xctrace").then_some(if self.options.activity_monitor() {
                files::MAX_ACTIVITY_BUNDLE
            } else {
                MAX_ARTIFACT
            });
        let artifact_path = self.directory.join(if bundle_limit.is_some() {
            "capture.trace"
        } else {
            "capture.native"
        });
        if self.options.mode == "import" {
            files::copy_input(
                self.options.path.as_ref().unwrap(),
                &artifact_path,
                bundle_limit,
            )?;
        } else {
            let duration = Duration::from_millis(self.options.duration_ms);
            let overhead_ms = self.options.record_grace_ms() + 1000;
            let required = duration + Duration::from_millis(overhead_ms);
            if self.budget(required)? < required {
                return Err(Error::new(
                    "NATIVE_DURATION_REQUIRED",
                    format!(
                        "Give the collector durationMs at least source durationMs + {overhead_ms} before native capture"
                    ),
                ));
            }
            if matches!(self.options.backend.as_str(), "perfetto" | "simpleperf") {
                identity = self
                    .capture_android(&artifact_path, duration, &mut gaps)
                    .await?;
            } else {
                identity = self
                    .capture_apple(&artifact_path, duration, &mut gaps)
                    .await?;
            }
        }
        let (artifact, encoding) = files::artifact(&artifact_path, bundle_limit)?;
        let sha256 = super::digest(&artifact);
        match self.options.backend.as_str() {
            "perfetto"=>{
                for (name,sql) in perfetto::QUERIES {
                    match self.run(self.options.tool_path.as_ref().unwrap(),&["query".into(),"--format".into(),"csv".into(),artifact_path.to_string_lossy().into_owned(),(*sql).into()],512*1024,Duration::from_secs(10)).await {
                        Ok(bytes)=>sections.push(json!({"name":name,"format":"csv","text":String::from_utf8(bytes).map_err(|_|Error::invalid("Trace processor output was not UTF-8"))?})),
                        Err(e) if e.code=="CANCELLED"||e.code=="TIMEOUT"=>return Err(e),
                        Err(e)=>gaps.push(json!({"section":name,"unavailable":e.code})),
                    }
                }
            }
            "simpleperf"=>{
                match self.run(self.options.tool_path.as_ref().unwrap(),&["report-sample".into(),"-i".into(),artifact_path.to_string_lossy().into_owned(),"--show-callchain".into(),"--show-execution-type".into(),"--remove-gaps".into(),"0".into()],1024*1024,Duration::from_secs(10)).await {
                    Ok(bytes)=>sections.push(json!({"name":"samples","format":"simpleperf-text","text":String::from_utf8(bytes).map_err(|_|Error::invalid("Invalid simpleperf report"))?})),
                    Err(e) if e.code=="CANCELLED"||e.code=="TIMEOUT"=>return Err(e),
                    Err(e)=>gaps.push(json!({"section":"samples","unavailable":e.code})),
                }
            }
            "xctrace"=>{
                recording = self.export_xctrace(&artifact_path, &mut sections, &mut gaps).await?;
            }
            "stack_sample"=>sections.push(json!({"name":"hang-stacks","format":"sample-text","text":String::from_utf8(artifact.clone()).map_err(|_|Error::invalid("Invalid sample report"))?})),
            _=>unreachable!(),
        }
        self.report = json!({"adapter":"native-trace/1","backend":self.options.backend,"mode":self.options.mode,"identity":identity,"gaps":gaps,"artifactBytes":artifact.len(),"artifactSha256":sha256,"coverage":"one selected process or system trace; not whole-app","remoteLifetime":"capture duration is finite; client loss does not prove remote termination","attachEffects":self.options.allow_attach,"samplingHzRequested":if self.options.backend=="simpleperf"{Some(self.options.frequency_hz)}else{None}});
        self.report["template"] = json!(self.options.template);
        if self.options.perfetto_capture() {
            self.report["perfettoBuffer"] = json!({
                "sizeKib": self.options.buffer_kib(),
                "fillPolicy": self.options.perfetto_fill_policy.unwrap_or_default()
            });
        }
        if self.options.activity_monitor() {
            self.report["coverage"] = json!(
                "one physical-device trace; structured rows select the caller's app PID and backboardd"
            );
            self.report["systemTraceRequested"] = json!(self.options.mode == "capture");
            self.report["selectedAppPid"] = json!(self.options.process_id);
            self.report["xctraceRecording"] = json!(recording);
        }
        self.report["memoryEffects"] = json!({"allocationRecordingRequested":self.options.mode=="capture"&&self.options.template.as_deref()==Some("Allocations"),"heapInspectionRequested":self.options.mode=="capture"&&self.options.template.as_deref()==Some("Leaks"),"forcedGcRequested":false,"pressureOrTrimRequested":false});
        serde_json::to_vec(&json!({"schemaVersion":1,"backend":self.options.backend,"targetId":self.options.target_id,"declaredAppId":self.options.app_id,"actionIds":self.actions,"report":self.report,"artifact":{"encoding":encoding,"sha256":sha256,"base64":base64::engine::general_purpose::STANDARD.encode(artifact)},"sections":sections})).map_err(|_|Error::invalid("Cannot encode native trace"))
    }

    async fn export_xctrace(
        &self,
        artifact_path: &Path,
        sections: &mut Vec<Value>,
        gaps: &mut Vec<Value>,
    ) -> Result<Option<Value>> {
        use super::analysis::decode::native_trace::{
            activity_monitor_export, memory_tables, recording_summary,
        };
        let tool = self.xcrun();
        let args = [
            "xctrace".into(),
            "export".into(),
            "--input".into(),
            artifact_path.to_string_lossy().into_owned(),
        ];
        let activity = self.options.activity_monitor();
        let deadline = Duration::from_secs(if activity { 20 } else { 10 });
        let export =
            async |name: &str, query: &[String], max_bytes, gaps: &mut Vec<Value>| match self
                .run(
                    &tool,
                    &[args.as_slice(), query].concat(),
                    max_bytes,
                    deadline,
                )
                .await
            {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) if e.code == "CANCELLED" || (e.code == "TIMEOUT" && !activity) => Err(e),
                Err(e) => {
                    gaps.push(json!({"section":name,"unavailable":e.code}));
                    Ok(None)
                }
            };
        let memory = matches!(
            self.options.template.as_deref(),
            Some("Allocations" | "Leaks")
        );
        let mut tables = vec![];
        let mut recording = None;
        if let Some(toc) = export("toc", &["--toc".into()], 256 * 1024, gaps).await? {
            if memory {
                match memory_tables(&toc) {
                    Ok(names) => tables = names,
                    Err(e) => gaps.push(json!({"section":"memory_inventory","unavailable":e.code})),
                }
            } else if activity {
                match recording_summary(&toc) {
                    Ok(summary) => recording = summary,
                    Err(e) => {
                        gaps.push(json!({"section":"recording_summary","unavailable":e.code}))
                    }
                }
                if let Some(summary) = &recording
                    && summary["deviceDisconnected"] == true
                {
                    gaps.push(json!({"section":"recording","reason":"device_disconnected","durationSeconds":summary["durationSeconds"],"meaning":"Instruments ended the recording early; samples cover only the observed interval"}));
                }
            }
            sections
                .push(json!({"name":"toc","format":"xml","text":String::from_utf8_lossy(&toc)}));
        }
        if memory {
            if tables.is_empty() {
                gaps.push(json!("No registered memory tables advertised by this trace; GUI-only/unknown schemas remain raw"));
            }
        } else if activity {
            tables.push(ACTIVITY_MONITOR_TABLE.into());
        } else {
            tables
                .extend(["time-profile", "os-signpost", "thread-state", "hang"].map(str::to_owned));
        }
        let max_bytes = if activity {
            4 * 1024 * 1024
        } else {
            512 * 1024
        };
        for schema in tables {
            let query = [
                "--xpath".into(),
                format!("/trace-toc/run[@number=\"1\"]/data/table[@schema=\"{schema}\"]"),
            ];
            let Some(bytes) = export(&schema, &query, max_bytes, gaps).await? else {
                continue;
            };
            if activity {
                match activity_monitor_export(&bytes, self.options.process_id.unwrap()) {
                    Ok(text) => sections
                        .push(json!({"name":schema,"format":ACTIVITY_MONITOR_FORMAT,"text":text})),
                    Err(e) => gaps.push(json!({"section":schema,"unavailable":e.code})),
                }
            } else {
                sections.push(
                    json!({"name":schema,"format":"xml","text":String::from_utf8_lossy(&bytes)}),
                );
            }
        }
        Ok(recording)
    }
    pub async fn read(&mut self, offset: u64) -> Result<(Vec<u8>, bool, Value)> {
        if self.bytes.is_none() {
            let bytes = self.build().await?;
            if bytes.len() > MAX_ENVELOPE {
                return Err(Error::new(
                    "NATIVE_TRACE_BOUNDS",
                    "Native envelope exceeds 8 MiB; select a shorter trace",
                )
                .details(json!({"requiredBytes":bytes.len(),"maxBytes":MAX_ENVELOPE})));
            }
            self.bytes = Some(bytes);
        }
        let bytes = self.bytes.as_ref().unwrap();
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        if start > bytes.len() {
            return Err(Error::invalid("Invalid native cursor"));
        }
        Ok((bytes[start..].to_vec(), true, self.report.clone()))
    }
    pub async fn settle(&mut self) -> Value {
        if let Some((transport, path, boot)) = self.remote.take() {
            // Never signal global profilers or remove someone else's path. A cancelled
            // remote recorder can remain alive for its configured finite duration.
            let check = process::run(
                &self.adb.adb,
                &[
                    "-t",
                    &transport,
                    "shell",
                    "cat /proc/sys/kernel/random/boot_id",
                ],
                Duration::from_secs(1),
                256,
            )
            .await;
            let same = check.is_ok_and(|o| String::from_utf8_lossy(&o.bytes).trim() == boot);
            self.report["remoteFileCleanup"] = json!(if same {
                let command = format!("rm -- '{path}'");
                if process::run(
                    &self.adb.adb,
                    &["-t", &transport, "shell", &command],
                    Duration::from_secs(1),
                    1024,
                )
                .await
                .is_ok()
                {
                    "owned_path_unlinked; remote_recorder_termination_unverified"
                } else {
                    "unverified"
                }
            } else {
                "skipped_changed_or_unavailable_boot"
            });
        }
        self.bytes = None;
        self.report["cleanup"] = json!(if std::fs::remove_dir_all(&self.directory).is_ok() {
            "owned_host_files_removed"
        } else {
            "host_file_cleanup_unverified"
        });
        self.report.clone()
    }
    fn xcrun(&self) -> PathBuf {
        self.options
            .tool_path
            .clone()
            .unwrap_or_else(crate::simulator::device_set::executable)
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
