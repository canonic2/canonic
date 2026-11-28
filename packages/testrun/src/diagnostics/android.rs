//! Read-only, transport/boot-pinned Android evidence collected in bounded stages.
pub(crate) mod parse;
pub(crate) mod state;
#[cfg(test)]
mod tests;
use crate::{
    android::Android,
    error::{Error, Result},
    process,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(crate) struct Options {
    pub target: String,
    pub app: String,
    pub user: u32,
    pub follow: bool,
    pub reports: bool,
    pub system: bool,
    pub since: Option<String>,
    pub state: Option<state::Options>,
}
pub(crate) struct Capture {
    options: Options,
    adb: Android,
    transport: String,
    boot: String,
    api: u32,
    uid: u32,
    shared: bool,
    stage: u8,
    installed: Value,
    processes: Vec<parse::Process>,
    first_seen: HashMap<String, String>,
    cursors: HashMap<String, parse::Cursor>,
    gaps: BTreeSet<String>,
    reports: Vec<String>,
    report_index: usize,
    cycles: u32,
    last_clock: Option<(u64, u32)>,
    clock: String,
    control: Option<(tokio::sync::watch::Receiver<Option<String>>, Instant)>,
    pub(super) action_ids: Vec<String>,
    state_reads: std::collections::VecDeque<state::Read>,
    state_history: state::History,
}
pub(crate) struct Batch {
    pub bytes: Vec<u8>,
    pub complete: bool,
    pub summary: Value,
}
impl Capture {
    pub fn new(options: Options) -> Result<Self> {
        if let Some(state) = &options.state {
            state.validate()?;
        }
        let sampling_gap = if options.state.is_some() {
            "sequential_snapshots_do_not_establish_continuous_coverage"
        } else {
            "circular_buffers_and_sampling_do_not_prove_complete_delivery"
        };
        let serial = options.target.strip_prefix("android:").ok_or_else(|| {
            Error::invalid("Android evidence requires targetId android:SELECTED-SERIAL")
        })?;
        if serial.is_empty()
            || serial.len() > 256
            || serial.chars().any(char::is_control)
            || !parse::package(&options.app)
            || options.user > 99999
            || options
                .since
                .as_deref()
                .is_some_and(|s| parse::time_key(s).is_none())
        {
            return Err(Error::invalid(
                "Select an Android serial, exact application package/user and optional epoch-seconds cursor",
            ));
        }
        Ok(Self {
            options,
            adb: Android::default(),
            transport: String::new(),
            boot: String::new(),
            api: 0,
            uid: 0,
            shared: false,
            stage: 0,
            installed: Value::Null,
            processes: vec![],
            first_seen: HashMap::new(),
            cursors: HashMap::new(),
            gaps: BTreeSet::from([sampling_gap.into()]),
            reports: vec![],
            report_index: 0,
            cycles: 0,
            last_clock: None,
            clock: String::new(),
            control: None,
            action_ids: vec![],
            state_reads: Default::default(),
            state_history: Default::default(),
        })
    }
    pub fn control(
        &mut self,
        stopped: tokio::sync::watch::Receiver<Option<String>>,
        budget: Duration,
    ) {
        self.control = Some((stopped, Instant::now() + budget));
    }
    fn budget(&self) -> Result<Duration> {
        let mut budget = Duration::from_secs(4);
        if let Some((stopped, deadline)) = &self.control {
            if stopped.borrow().is_some() {
                return Err(Error::new(
                    "CANCELLED",
                    "Android collection stopped before another read",
                ));
            }
            budget = budget.min(deadline.saturating_duration_since(Instant::now()));
        }
        if budget.is_zero() {
            return Err(Error::new("TIMEOUT", "Android collection duration expired"));
        }
        Ok(budget)
    }
    async fn shell(&self, args: &[&str], max: usize) -> Result<Vec<u8>> {
        let command = args
            .iter()
            .map(|arg| format!("'{}'", arg.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join(" ");
        process::run_diagnostic(
            &self.adb.adb,
            &["-t", &self.transport, "shell", &command],
            self.budget()?,
            max,
        )
        .await
        .map(|output| output.bytes)
    }
    async fn boot(&self) -> Result<String> {
        let bytes = self
            .shell(&["cat", "/proc/sys/kernel/random/boot_id"], 256)
            .await?;
        let value = String::from_utf8_lossy(&bytes).trim().to_string();
        if value.len() < 32
            || value.len() > 80
            || !value.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
        {
            return Err(Error::new(
                "ANDROID_IDENTITY_UNKNOWN",
                "Android boot identity was not readable",
            ));
        }
        Ok(value)
    }
    async fn guard(&self) -> Result<()> {
        if self.boot().await? != self.boot {
            Err(Error::new(
                "ANDROID_BOOT_CHANGED",
                "Android rebooted during collection; retained evidence belongs to the earlier boot",
            ))
        } else {
            Ok(())
        }
    }
    fn gap(&mut self, section: &str, error: Error) -> Value {
        let detail = error
            .details
            .as_ref()
            .and_then(|d| d["diagnostic"].as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let status = if detail.contains("permission denied")
            || detail.contains("permission denial")
            || detail.contains("not allowed")
            || detail.contains("access denied")
        {
            "permission_denied"
        } else if error.code == "OUTPUT_LIMIT" {
            "output_limit"
        } else if detail.contains("not found")
            || detail.contains("unknown command")
            || detail.contains("no such file")
        {
            "unavailable"
        } else {
            "read_failed"
        };
        self.gaps.insert(format!("{section}:{status}"));
        json!({"status":status,"code":error.code,"privateDiagnostic":error.details})
    }
    fn packet(&self, kind: &str, data: Value) -> Value {
        json!({"schemaVersion":1,"kind":kind,"appId":self.options.app,"targetId":self.options.target,"userId":self.options.user,"uid":if self.stage>=2{Some(self.uid)}else{None},"transportId":self.transport,"bootId":self.boot,"observedAt":self.clock,"actionIds":self.action_ids,"label":self.options.state.as_ref().and_then(|s|s.label.as_deref()),"buildIdentity":{"versionCode":self.installed["versionCode"],"lastUpdateTime":self.installed["lastUpdateTime"]},"data":data,"coverage":"bounded_sample; not complete history"})
    }
    pub fn summary(&self) -> Value {
        let mut value = json!({"adapter":"android-adb/1","apiLevel":self.api,"uid":if self.stage>=2{Some(self.uid)}else{None},"sharedUid":self.shared,"processes":self.processes.iter().map(|p|json!({"pid":p.pid,"generation":p.generation})).collect::<Vec<_>>(),"cycles":self.cycles,"sourceLoss":self.gaps,"scope":"selected app UID/processes and related system logs; selected package/service/report reads may contain related processes or users","historyComplete":false,"reportsRequested":self.options.reports,"appChanges":false,"deviceSettingChanges":false});
        if let Some(state) = &self.options.state {
            value["adapter"] = json!("android-state-adb/1");
            value["groups"] = json!(state.groups);
            value["intervalMs"] = json!(state.interval_ms);
            value["processLimit"] = json!(state.max_processes);
            value["scope"] = json!(
                "observed app processes and explicitly selected device context; underlying package/service/system reads can include other users; host filters system records before retention"
            );
        }
        value
    }
    pub async fn read(&mut self) -> Result<Batch> {
        if self.stage > 0 {
            self.guard().await?;
        }
        let (kind, data, complete) = match self.stage {
            0 => self.read_readiness().await?,
            1 => self.read_installed().await?,
            2 => {
                let bytes = self
                    .shell(
                        &["dumpsys", "activity", "services", &self.options.app],
                        256 * 1024,
                    )
                    .await;
                let data = match bytes {
                    Ok(bytes) => {
                        json!({"status":"available","text":String::from_utf8_lossy(&bytes)})
                    }
                    Err(e) => self.gap("services", e),
                };
                self.stage = 3;
                ("services", data, false)
            }
            3 => {
                let data = self.sample_processes().await?;
                self.stage = if self.options.state.is_some() {
                    self.state_reads = self.state_plan();
                    11
                } else {
                    4
                };
                ("processes", data, false)
            }
            4 => self.read_exits().await?,
            5 => {
                let data = self.logs(false).await;
                self.stage = 6;
                ("logs", data, false)
            }
            6 => {
                let data = if self.options.system {
                    self.logs(true).await
                } else {
                    json!({"status":"not_requested","entries":[]})
                };
                self.stage = 7;
                ("logs", data, false)
            }
            7 => {
                if self.report_index < self.reports.len() {
                    let path = self.reports[self.report_index].clone();
                    self.report_index += 1;
                    let data = match self.shell(&["cat", &path], 256 * 1024).await {
                        Ok(bytes) => {
                            use base64::Engine;
                            json!({"status":"available","path":path,"dataBase64":base64::engine::general_purpose::STANDARD.encode(&bytes),"bytes":bytes.len(),"scope":"app-linked report; may include multiple threads/processes"})
                        }
                        Err(e) => self.gap("report", e),
                    };
                    ("report", data, false)
                } else {
                    self.stage = 8;
                    (
                        "report",
                        json!({"status":if self.options.reports{"no_accessible_reference"}else{"not_requested"},"absenceProven":false}),
                        false,
                    )
                }
            }
            8 => {
                let data = self.symbols().await;
                self.stage = 9;
                ("symbols", data, false)
            }
            11 => self.read_state_packet().await,
            _ => self.read_window().await?,
        };
        self.guard().await?;
        let mut bytes = serde_json::to_vec(&self.packet(kind, data)).map_err(|_| {
            Error::new(
                "ANDROID_DIAGNOSTIC_FORMAT",
                "Cannot encode Android evidence",
            )
        })?;
        bytes.push(b'\n');
        Ok(Batch {
            bytes,
            complete,
            summary: self.summary(),
        })
    }
    async fn read_state_packet(&mut self) -> (&'static str, Value, bool) {
        if let Some(read) = self.state_reads.pop_front() {
            ("state", self.read_state(read).await, false)
        } else {
            self.stage = 9;
            let config = self.options.state.as_ref().unwrap();
            let unsupported = if self.api < 28 && config.groups.iter().any(|g| g == "power") {
                self.gaps
                    .insert("state_standby_background:unsupported_api".into());
                vec![
                    "standby_bucket_requires_api_28",
                    "RUN_ANY_IN_BACKGROUND_requires_api_28",
                ]
            } else {
                vec![]
            };
            (
                "state",
                json!({"group":"coverage","status":"sampled","fields":{"processLimit":config.max_processes,"observedProcesses":self.processes.len(),"processLimitExceeded":self.processes.len()>config.max_processes,"unsupported":unsupported,"snapshotAtomic":false,"retentionEvidence":"unavailable; snapshots do not establish leaks"},"complete":false,"media":{"operation":"media_status","targetId":self.options.target,"automaticPreparation":false,"reason":"Use existing playback owner; helper preparation and audio routing are separate effects"}}),
                false,
            )
        }
    }
    async fn read_readiness(&mut self) -> Result<(&'static str, Value, bool)> {
        let serial = self.options.target.strip_prefix("android:").unwrap();
        let devices = self.adb.devices_bounded(self.budget()?).await?;
        let selected: Vec<_> = devices.iter().filter(|d| d.serial == serial).collect();
        if selected.len() != 1 {
            return Err(Error::new(
                "ANDROID_TARGET_UNAVAILABLE",
                "Selected Android transport was not uniquely observed",
            ));
        }
        if selected[0].state != "device" {
            return Err(Error::new(
                "ANDROID_TARGET_NOT_READY",
                "Authorize/unlock the selected Android device and inspect target status",
            ));
        }
        selected[0].transport.parse::<u32>().map_err(|_| {
            Error::new(
                "ANDROID_IDENTITY_UNKNOWN",
                "ADB did not report a usable transport identity",
            )
        })?;
        self.transport = selected[0].transport.clone();
        self.boot = self.boot().await?;
        let api = self
            .shell(&["getprop", "ro.build.version.sdk"], 256)
            .await?;
        self.api = String::from_utf8_lossy(&api).trim().parse().map_err(|_| {
            Error::new(
                "ANDROID_IDENTITY_UNKNOWN",
                "Android API level was not readable",
            )
        })?;
        if self.api < 26 {
            return Err(Error::new(
                "ANDROID_DIAGNOSTIC_API",
                "This Android adapter requires API 26 or later; historical exit reasons require API 30",
            ));
        }
        self.stage = 1;
        Ok((
            "readiness",
            json!({"apiLevel":self.api,"status":"transport_pinned"}),
            false,
        ))
    }
    async fn read_installed(&mut self) -> Result<(&'static str, Value, bool)> {
        let user = self.options.user.to_string();
        let list = self
            .shell(
                &["cmd", "package", "list", "packages", "-U", "--user", &user],
                512 * 1024,
            )
            .await?;
        let (uid, owners) =
            parse::uid_inventory(&String::from_utf8_lossy(&list), &self.options.app)?;
        if uid / 100000 != self.options.user {
            return Err(Error::new(
                "ANDROID_USER_SCOPE",
                "Package UID did not match the selected Android user",
            ));
        }
        self.uid = uid;
        self.shared = owners > 1;
        let dump = self
            .shell(&["dumpsys", "package", &self.options.app], 256 * 1024)
            .await?;
        let paths = self
            .shell(
                &["pm", "path", "--user", &user, &self.options.app],
                64 * 1024,
            )
            .await?;
        self.installed = parse::installed(
            &String::from_utf8_lossy(&dump),
            &String::from_utf8_lossy(&paths),
        );
        if self.installed["apkPaths"]
            .as_array()
            .is_none_or(Vec::is_empty)
        {
            return Err(Error::new(
                "ANDROID_APP_UNAVAILABLE",
                "No installed APK/splits were observed for this application/user",
            ));
        }
        if self.shared {
            self.gaps
                .insert("shared_uid_requires_named_process_association".into());
        }
        self.stage = if self.options.state.is_some() { 3 } else { 2 };
        Ok((
            "installed",
            json!({"identity":self.installed,"uid":uid,"sharedUid":self.shared,"packageDump":String::from_utf8_lossy(&dump),"splitPaths":String::from_utf8_lossy(&paths)}),
            false,
        ))
    }
    async fn read_exits(&mut self) -> Result<(&'static str, Value, bool)> {
        let data = if self.api < 30 {
            self.gaps.insert("exit_history:unsupported_api".into());
            json!({"status":"unsupported_api","minimumApi":30})
        } else {
            match self
                .shell(
                    &["dumpsys", "activity", "exit-info", &self.options.app],
                    512 * 1024,
                )
                .await
            {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let entries = parse::exit_entries(&text, self.uid);
                    let recognized = text.contains("ACTIVITY MANAGER PROCESS EXIT INFO")
                        || text.contains("ApplicationExitInfo ");
                    if !recognized {
                        self.gaps.insert("exit_history:unrecognized_format".into());
                    }
                    if self.options.reports {
                        for entry in &entries {
                            if let Some(path) = entry["tracePath"]
                                .as_str()
                                .filter(|p| parse::safe_report(p))
                                && self.reports.len() < 4
                                && !self.reports.iter().any(|p| p == path)
                            {
                                self.reports.push(path.into());
                            }
                        }
                    }
                    json!({"status":if recognized{"available"}else{"unrecognized_format"},"entries":entries,"absenceProven":false})
                }
                Err(e) => self.gap("exit_history", e),
            }
        };
        self.stage = 5;
        Ok(("exits", data, false))
    }
    async fn read_window(&mut self) -> Result<(&'static str, Value, bool)> {
        let user = self.options.user.to_string();
        let list = self
            .shell(
                &["cmd", "package", "list", "packages", "-U", "--user", &user],
                512 * 1024,
            )
            .await?;
        if parse::uid_inventory(&String::from_utf8_lossy(&list), &self.options.app)?.0 != self.uid {
            return Err(Error::new(
                "ANDROID_PACKAGE_CHANGED",
                "Package UID changed during collection",
            ));
        }
        let dump = self
            .shell(&["dumpsys", "package", &self.options.app], 256 * 1024)
            .await?;
        let identity = parse::installed(&String::from_utf8_lossy(&dump), "");
        for key in ["versionCode", "versionName", "lastUpdateTime"] {
            if identity[key] != self.installed[key] {
                return Err(Error::new(
                    "ANDROID_PACKAGE_CHANGED",
                    "Installed application changed during collection; open another capture",
                ));
            }
        }
        self.cycles += 1;
        let complete = !self.options.follow || self.cycles >= 256;
        self.stage = if complete { 10 } else { 3 };
        if !complete {
            let delay =
                Duration::from_millis(self.options.state.as_ref().map_or(500, |s| s.interval_ms));
            if let Some((stopped, deadline)) = &self.control {
                let mut stopped = stopped.clone();
                if stopped.borrow().is_some() {
                    return Err(Error::new(
                        "CANCELLED",
                        "Android collection stopped before the next cycle",
                    ));
                }
                tokio::select! {_ = stopped.changed()=>return Err(Error::new("CANCELLED","Android collection stopped between cycles")),_ = tokio::time::sleep(delay.min(deadline.saturating_duration_since(Instant::now())))=>{}}
            } else {
                tokio::time::sleep(delay).await;
            }
        }
        Ok((
            "window",
            json!({"cycle":self.cycles,"sourceLoss":self.gaps,"complete":false,"mode":if self.options.follow{"follow"}else{"snapshot"}}),
            complete,
        ))
    }
    async fn sample_processes(&mut self) -> Result<Value> {
        let clock = self.shell(&["date", "+%s.%N"], 256).await?;
        let clock = String::from_utf8_lossy(&clock).trim().to_string();
        let key = parse::time_key(&clock).ok_or_else(|| {
            Error::new(
                "ANDROID_CLOCK_UNKNOWN",
                "Device epoch clock was not readable",
            )
        })?;
        if self.last_clock.is_some_and(|old| key < old) {
            return Err(Error::new(
                "ANDROID_CLOCK_CHANGED",
                "Device wall clock moved backwards; start another capture",
            ));
        }
        self.last_clock = Some(key);
        self.clock = clock;
        let list = self
            .shell(&["ps", "-A", "-n", "-o", "PID,UID,NAME"], 256 * 1024)
            .await?;
        let selected = parse::processes(
            &String::from_utf8_lossy(&list),
            self.uid,
            &self.options.app,
            self.shared,
        )?;
        let mut processes = Vec::new();
        for (pid, _) in selected {
            let stat = format!("/proc/{pid}/stat");
            let status = format!("/proc/{pid}/status");
            let cmdline = format!("/proc/{pid}/cmdline");
            match self
                .shell(&["cat", &stat, &status, &cmdline], 32 * 1024)
                .await
            {
                Ok(bytes) => {
                    if let Some(p) = parse::process(&bytes, pid, self.uid, &self.boot) {
                        self.first_seen
                            .entry(p.generation.clone())
                            .or_insert_with(|| self.clock.clone());
                        processes.push(p);
                    } else {
                        self.gaps
                            .insert("process_identity:unrecognized_or_changed".into());
                    }
                }
                Err(e) => {
                    let _ = self.gap("process_identity", e);
                }
            }
        }
        let started: Vec<_> = processes
            .iter()
            .filter(|p| {
                !self
                    .processes
                    .iter()
                    .any(|old| old.generation == p.generation)
            })
            .cloned()
            .collect();
        let gone: Vec<_> = self
            .processes
            .iter()
            .filter(|p| !processes.iter().any(|now| now.generation == p.generation))
            .cloned()
            .collect();
        self.processes = processes;
        Ok(
            json!({"status":"available","processes":self.processes,"firstObserved":started,"noLongerObserved":gone,"deathProven":false}),
        )
    }
    async fn logs(&mut self, system: bool) -> Value {
        let name = if system { "system_context" } else { "app_logs" };
        let cursor = self
            .cursors
            .get(name)
            .and_then(|c| c.time.as_deref())
            .or(self.options.since.as_deref())
            .map(str::to_string);
        let mut args = vec![
            "logcat".to_string(),
            "-d".into(),
            "-v".into(),
            "epoch,uid,usec".into(),
        ];
        for buffer in if system {
            vec!["system", "events", "crash"]
        } else {
            vec!["main", "crash"]
        } {
            args.extend(["-b".into(), buffer.into()]);
        }
        if !system {
            args.push(format!("--uid={}", self.uid));
        }
        if let Some(cursor) = cursor {
            args.extend(["-T".into(), cursor]);
        } else {
            args.extend(["-t".into(), "512".into()]);
        }
        args.push("*:V".into());
        let refs: Vec<_> = args.iter().map(String::as_str).collect();
        let bytes = match self.shell(&refs, 512 * 1024).await {
            Ok(bytes) => bytes,
            Err(e) => return self.gap(name, e),
        };
        let text = String::from_utf8_lossy(&bytes);
        let mut logs = Vec::new();
        let mut unknown = 0usize;
        for line in text.lines() {
            if let Some(log) = parse::log(line) {
                logs.push(log);
            } else if !line.starts_with("---------") && !line.trim().is_empty() {
                unknown += 1;
            }
        }
        if unknown > 0 {
            self.gaps
                .insert(format!("{name}:unparsed_or_truncated_lines"));
        }
        let (logs, regressed) = self.cursors.entry(name.into()).or_default().take(logs);
        if regressed {
            self.gaps
                .insert(format!("{name}:out_of_order_clock_or_cursor"));
        }
        let mut retained = Vec::new();
        let mut excluded = 0usize;
        let tombstones = parse::tombstone_context(&logs, &self.options.app, self.uid);
        for (position, log) in logs.into_iter().enumerate() {
            let process = self.processes.iter().find(|p| p.pid == log.pid);
            let direct = log.uid == Some(self.uid) && (!self.shared || process.is_some());
            let tombstone = tombstones.contains(&position);
            let related = self.related(&log) || tombstone;
            if !(if system { !direct && related } else { direct }) {
                excluded += 1;
                continue;
            }
            let generation = process
                .filter(|_| direct)
                .filter(|p| {
                    self.first_seen
                        .get(&p.generation)
                        .and_then(|t| parse::time_key(t))
                        .zip(parse::time_key(&log.time))
                        .is_some_and(|(start, time)| time >= start)
                })
                .map(|p| p.generation.clone());
            retained.push(json!({"entry":log,"appId":self.options.app,"processGeneration":generation,"systemContext":!direct,"relatedAppId":if !direct{Some(&self.options.app)}else{None},"association":if direct{"current_install_uid; historical_generation_may_be_unknown"}else if tombstone{"tombstone_header_package_and_uid"}else{"explicit_package_and_user_or_uid_context"}}));
        }
        json!({"status":"available","entries":retained,"excluded":excluded,"unparsed":unknown,"cursor":self.cursors.get(name).and_then(|c|c.time.clone()),"cursorSemantics":"inclusive_epoch_and_boundary_multiplicity; ring loss remains unknown","source":name,"complete":false})
    }
    fn related(&self, log: &parse::Log) -> bool {
        if log.uid.is_none_or(|uid| uid >= 10000)
            || !parse::package_token(&log.message, &self.options.app)
        {
            return false;
        }
        if log.message.trim_start().starts_with('[') {
            return log
                .message
                .trim_start()
                .trim_start_matches('[')
                .split(',')
                .next()
                .and_then(|v| v.trim().parse::<u32>().ok())
                == Some(self.options.user);
        }
        let words: Vec<_> = log.message.split_whitespace().collect();
        let numeric_uid = words.iter().enumerate().any(|(i, word)| {
            let value = word
                .strip_prefix("uid=")
                .or_else(|| word.strip_prefix("uid:"))
                .or_else(|| {
                    (i > 0 && matches!(words[i - 1], "uid" | "UID" | "uid:")).then_some(*word)
                });
            value.and_then(|s| {
                s.trim_matches(|c: char| !c.is_ascii_digit())
                    .parse::<u32>()
                    .ok()
            }) == Some(self.uid)
        });
        numeric_uid
            || log.message.contains(&format!("/u{}a", self.options.user))
            || log.message.contains(&format!(" u{} ", self.options.user))
            || log.message.split_whitespace().any(|word| {
                word.strip_prefix("user=").and_then(|v| {
                    v.trim_matches(|c: char| !c.is_ascii_digit())
                        .parse::<u32>()
                        .ok()
                }) == Some(self.options.user)
            })
    }
    async fn symbols(&mut self) -> Value {
        let Some(dir) = self.installed["nativeLibraryDir"]
            .as_str()
            .filter(|p| {
                [
                    "/data/app/",
                    "/data/app-lib/",
                    "/system/",
                    "/system_ext/",
                    "/vendor/",
                    "/product/",
                    "/apex/",
                ]
                .iter()
                .any(|prefix| p.starts_with(prefix))
                    && !p.contains("..")
                    && p.len() < 1024
            })
            .map(str::to_string)
        else {
            return json!({"status":"unknown","reason":"native_library_directory_not_observed"});
        };
        let list = match self.shell(&["ls", "-1", &dir], 32 * 1024).await {
            Ok(bytes) => bytes,
            Err(e) => return self.gap("native_libraries", e),
        };
        let names: Vec<_> = String::from_utf8_lossy(&list)
            .lines()
            .filter(|name| {
                name.ends_with(".so")
                    && name.len() < 256
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            })
            .take(8)
            .map(str::to_string)
            .collect();
        let mut libraries = Vec::new();
        for name in names {
            let path = format!("{dir}/{name}");
            let observed = match self
                .shell(&["readelf", "-h", "-n", "-S", &path], 128 * 1024)
                .await
            {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let recognized = text.contains("ELF Header:")
                        || text.contains("Section Headers:")
                        || text.contains("Build ID:");
                    if !recognized {
                        self.gaps
                            .insert("library_symbols:unrecognized_format".into());
                    }
                    json!({"status":if recognized{"available"}else{"unrecognized_format"},"buildId":text.lines().find_map(|line|line.split_once("Build ID:").map(|(_,v)|v.trim())),"architecture":text.lines().find_map(|line|line.split_once("Machine:").map(|(_,v)|v.trim())),"symtab":if recognized{Some(text.contains(".symtab"))}else{None},"debugInfo":if recognized{Some(text.contains(".debug_info"))}else{None},"raw":text})
                }
                Err(e) => self.gap("library_symbols", e),
            };
            libraries.push(json!({"path":path,"inspection":observed}));
        }
        json!({"status":"sampled","libraries":libraries,"limit":8,"absenceProven":false,"apkContainedLibraries":"not_extracted; matching external symbols may be required"})
    }
}
