//! Separate CoreSimulator and physical-service routes. No host-log substitution.
pub(crate) mod reports;
#[cfg(test)]
mod tests;
use crate::{
    error::{Error, Result},
    process,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
pub(crate) fn usb() -> String {
    "usb".into()
}
pub(crate) fn seconds() -> u64 {
    3
}
pub(crate) fn bundle(value: &str) -> bool {
    value.len() <= 255
        && value.contains('.')
        && value.split('.').all(|part| {
            !part.is_empty()
                && !part.starts_with('-')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        })
}
pub(crate) fn bundle_token(text: &str, app: &str) -> bool {
    text.match_indices(app).any(|(i, _)| {
        let id = |b: u8| b.is_ascii_alphanumeric() || b"._-".contains(&b);
        (i == 0 || !id(text.as_bytes()[i - 1]))
            && (i + app.len() == text.len() || !id(text.as_bytes()[i + app.len()]))
    })
}
pub(crate) struct Capture {
    target: String,
    app: String,
    id: String,
    sim: bool,
    transport: String,
    logs: bool,
    system: bool,
    seconds: u64,
    set: crate::simulator::device_set::DeviceSet,
    report_directory: Option<PathBuf>,
    report_files: Vec<PathBuf>,
    report_index: usize,
    xcrun: PathBuf,
    stage: u8,
    installed: Value,
    processes: Vec<Value>,
    target_identity: Value,
    gaps: BTreeSet<String>,
    control: Option<(tokio::sync::watch::Receiver<Option<String>>, Instant)>,
    pub(super) actions: Vec<String>,
}
impl Capture {
    #[expect(
        clippy::too_many_arguments,
        reason = "One source schema with explicit effects and scope"
    )]
    pub fn new(
        target: &str,
        app: &str,
        transport: &str,
        logs: bool,
        system: bool,
        seconds: u64,
        set: Option<PathBuf>,
        reports: Option<PathBuf>,
    ) -> Result<Self> {
        let (kind, id) = target
            .split_once(':')
            .ok_or_else(|| Error::invalid("Select simulator:UUID or device:UDID"))?;
        if !matches!(kind, "simulator" | "device")
            || id.is_empty()
            || id.starts_with('-')
            || id.len() > 128
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || !bundle(app)
            || !matches!(transport, "usb" | "network")
            || !(1..=30).contains(&seconds)
            || kind == "device" && seconds > 4
            || kind == "simulator" && transport != "usb"
            || kind == "device" && set.is_some()
        {
            return Err(Error::invalid(
                "Select exact Apple target/app, usb/network physical transport, logSeconds 1..30 and simulator-only deviceSet",
            ));
        }
        // Simulator and physical routes both start from Xcode's xcrun
        // (simctl or devicectl); TESTRUN_XCRUN selects another one explicitly.
        if !cfg!(target_os = "macos") && std::env::var_os("TESTRUN_XCRUN").is_none() {
            return Err(Error::new(
                "APPLE_HOST_UNSUPPORTED",
                "Apple simulator and device diagnostics require macOS with Xcode",
            ));
        }
        let set = match set {
            Some(p) => crate::simulator::device_set::DeviceSet::at(p)?,
            None => crate::simulator::device_set::DeviceSet {
                path: None,
                identity: "default".into(),
            },
        };
        if reports.as_ref().is_some_and(|p| {
            !p.is_absolute() || !std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir())
        }) {
            return Err(Error::invalid(
                "reportDirectory must be an explicit absolute regular directory, not a symlink",
            ));
        }
        Ok(Self {
            target: target.into(),
            app: app.into(),
            id: id.into(),
            sim: kind == "simulator",
            transport: transport.into(),
            logs,
            system,
            seconds,
            set,
            report_directory: reports,
            report_files: vec![],
            report_index: 0,
            xcrun: crate::simulator::device_set::executable(),
            stage: 0,
            installed: Value::Null,
            processes: vec![],
            target_identity: Value::Null,
            gaps: BTreeSet::from(["bounded_observations_do_not_establish_complete_history".into()]),
            control: None,
            actions: vec![],
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
        let mut d = Duration::from_secs(4);
        if let Some((stopped, deadline)) = &self.control {
            if stopped.borrow().is_some() {
                return Err(Error::new(
                    "CANCELLED",
                    "Apple collection stopped before another read",
                ));
            }
            d = d.min(deadline.saturating_duration_since(Instant::now()));
        }
        if d.is_zero() {
            return Err(Error::new("TIMEOUT", "Apple collection duration expired"));
        }
        Ok(d)
    }
    async fn run(&self, args: &[&str], max: usize) -> Result<Vec<u8>> {
        Ok(
            process::run_diagnostic(&self.xcrun, args, self.budget()?, max)
                .await?
                .bytes,
        )
    }
    async fn simctl(&self, args: &[&str], max: usize) -> Result<Vec<u8>> {
        let args = self.set.arguments(args);
        self.run(&args.iter().map(String::as_str).collect::<Vec<_>>(), max)
            .await
    }
    async fn device_json(&self, args: &[&str]) -> Result<Value> {
        let directory = Temporary::new()?;
        let path = directory.0.join("result.json");
        let mut args = args.to_vec();
        args.extend(["--json-output", path.to_str().unwrap(), "--timeout", "3"]);
        self.run(&args, 65536).await?;
        let bytes = read_file(&path, 1024 * 1024)?;
        serde_json::from_slice(&bytes).map_err(|_| {
            Error::new(
                "APPLE_TOOL_FORMAT",
                "Selected Apple tool did not return supported JSON",
            )
        })
    }
    fn gap(&mut self, section: &str, e: Error) -> Value {
        let text = e
            .details
            .as_ref()
            .map(Value::to_string)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let status = if text.contains("locked") {
            "locked"
        } else if text.contains("trust") || text.contains("pair") {
            "untrusted"
        } else if text.contains("developer") || text.contains("ddi") {
            "developer_support_unavailable"
        } else if text.contains("permission") || text.contains("denied") {
            "permission_denied"
        } else if e.code == "PREREQUISITE_MISSING" {
            "dependency_unavailable"
        } else {
            "unavailable"
        };
        self.gaps.insert(format!("{section}:{status}"));
        json!({"status":status,"code":e.code,"privateDiagnostic":e.details,"recovery":"Inspect selected target trust/lock/developer support and preinstalled tool availability; collection does not enable or install them"})
    }
    fn summary(&self) -> Value {
        json!({"adapter":"apple-diagnostics/1","route":if self.sim{"simulator"}else{"physical_service"},"transport":if self.sim{"simulator"}else{&self.transport},"deviceSetIdentity":if self.sim{Some(&self.set.identity)}else{None},"stage":self.stage,"sourceLoss":self.gaps,"processes":self.processes.iter().map(|p|json!({"pid":p["pid"],"generation":p["generation"]})).collect::<Vec<_>>(),"reportsInventoried":self.report_files.len(),"historyComplete":false,"appChanges":false,"deviceSettingChanges":false,"reportRetrieval":"explicit host reportDirectory; no crash mover or device deletion","sysdiagnose":"explicit bounded text import only; no automatic generation"})
    }
    async fn identity(&self) -> Result<Value> {
        if self.sim {
            let value: Value = serde_json::from_slice(
                &self
                    .simctl(&["list", "devices", "--json"], 1024 * 1024)
                    .await?,
            )
            .map_err(|_| Error::new("APPLE_TOOL_FORMAT", "Invalid simulator inventory"))?;
            let selected: Vec<_> = value["devices"]
                .as_object()
                .into_iter()
                .flat_map(|v| v.iter())
                .flat_map(|(runtime, list)| {
                    list.as_array()
                        .into_iter()
                        .flatten()
                        .filter(|d| d["udid"] == self.id)
                        .map(move |d| (runtime, d))
                })
                .collect();
            if selected.len() != 1 || selected[0].1["state"] != "Booted" {
                return Err(Error::new(
                    "APPLE_TARGET_UNAVAILABLE",
                    "Selected simulator must be uniquely present and Booted; collection never boots it",
                ));
            }
            let actual = self
                .simctl(&["getenv", &self.id, "SIMULATOR_UDID"], 256)
                .await?;
            if String::from_utf8_lossy(&actual).trim() != self.id {
                return Err(Error::new(
                    "APPLE_TARGET_CHANGED",
                    "Simulator identity did not match the selected UUID",
                ));
            }
            Ok(
                json!({"udid":self.id,"runtime":selected[0].0,"state":"Booted","deviceSetIdentity":self.set.identity}),
            )
        } else {
            let value = self.device_json(&["devicectl", "list", "devices"]).await?;
            let devices = value["result"]["devices"].as_array().ok_or_else(|| {
                Error::new("APPLE_TOOL_FORMAT", "Device inventory omitted devices")
            })?;
            let matches: Vec<_> = devices
                .iter()
                .filter(|d| d["hardwareProperties"]["udid"] == self.id)
                .collect();
            if matches.len() != 1 {
                return Err(Error::new(
                    "APPLE_TARGET_UNAVAILABLE",
                    "Selected physical UDID was not uniquely observed",
                ));
            }
            let d = matches[0];
            let connection = &d["connectionProperties"];
            if d["hardwareProperties"]["platform"] != "iOS"
                || connection["transportType"]
                    != if self.transport == "usb" {
                        "wired"
                    } else {
                        "localNetwork"
                    }
            {
                return Err(Error::new(
                    "APPLE_TRANSPORT_UNAVAILABLE",
                    "Selected device/platform/transport did not match; no USB/network fallback occurred",
                ));
            }
            if connection["pairingState"] != "paired" {
                return Err(Error::new(
                    "APPLE_DEVICE_UNTRUSTED",
                    "Pair and trust the selected physical device before collection",
                ));
            }
            if d["deviceProperties"]["isLocked"] == true {
                return Err(Error::new(
                    "APPLE_DEVICE_LOCKED",
                    "Unlock the selected device; no unlock or support preparation is performed",
                ));
            }
            Ok(
                json!({"udid":self.id,"platform":"iOS","transport":self.transport,"osVersion":d["deviceProperties"]["osVersionNumber"],"developerMode":d["deviceProperties"]["developerModeStatus"],"developerSupport":d["deviceProperties"]["ddiServicesAvailable"],"pairing":"paired"}),
            )
        }
    }
    async fn installed(&self) -> Result<Value> {
        if self.sim {
            let path = self
                .simctl(&["get_app_container", &self.id, &self.app, "app"], 4096)
                .await?;
            let path = PathBuf::from(String::from_utf8_lossy(&path).trim());
            if !path.is_absolute() || !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
                return Err(Error::new(
                    "APPLE_APP_UNAVAILABLE",
                    "Selected simulator's installed app container was not readable",
                ));
            }
            let bytes = read_file(&path.join("Info.plist"), 128 * 1024)?;
            let info: plist::Value = plist::from_bytes(&bytes).map_err(|_| {
                Error::new("APPLE_APP_FORMAT", "Installed Info.plist was not readable")
            })?;
            let dict = info.as_dictionary().ok_or_else(|| {
                Error::new(
                    "APPLE_APP_FORMAT",
                    "Installed Info.plist was not a dictionary",
                )
            })?;
            let s = |key: &str| dict.get(key).and_then(plist::Value::as_string);
            if s("CFBundleIdentifier") != Some(self.app.as_str()) {
                return Err(Error::new(
                    "APPLE_APP_CHANGED",
                    "Installed bundle identifier did not match",
                ));
            }
            let executable = s("CFBundleExecutable")
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= 128
                        && s.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                })
                .ok_or_else(|| {
                    Error::new(
                        "APPLE_APP_FORMAT",
                        "Installed executable name was unavailable",
                    )
                })?;
            let background = dict
                .get("UIBackgroundModes")
                .and_then(plist::Value::as_array)
                .map(|v| {
                    v.iter()
                        .filter_map(plist::Value::as_string)
                        .take(32)
                        .collect::<Vec<_>>()
                });
            Ok(
                json!({"appId":self.app,"version":s("CFBundleShortVersionString"),"build":s("CFBundleVersion"),"path":path,"executable":path.join(executable),"backgroundModes":background,"identityProvenance":"selected installed simulator Info.plist","entitlements":null}),
            )
        } else {
            let value = self
                .device_json(&[
                    "devicectl",
                    "device",
                    "info",
                    "apps",
                    "--device",
                    &self.id,
                    "--include-all-apps",
                ])
                .await?;
            let apps = value["result"]["apps"].as_array().ok_or_else(|| {
                Error::new("APPLE_TOOL_FORMAT", "Device app inventory omitted apps")
            })?;
            let selected: Vec<_> = apps
                .iter()
                .filter(|a| a["bundleIdentifier"] == self.app)
                .collect();
            if selected.len() != 1 {
                return Err(Error::new(
                    "APPLE_APP_UNAVAILABLE",
                    "Installed app was not uniquely observed on the selected physical device",
                ));
            }
            let a = selected[0];
            let path=a["url"].as_str().and_then(file_url).ok_or_else(||Error::new("APPLE_APP_FORMAT","Installed device app URL was unavailable; process association cannot be guessed from display name"))?;
            Ok(
                json!({"appId":self.app,"version":a["version"],"build":a["bundleVersion"],"path":path,"executable":null,"backgroundModes":null,"entitlements":null,"identityProvenance":"selected device CoreDevice installed-app observation"}),
            )
        }
    }
    async fn processes(&self) -> Result<Vec<Value>> {
        if self.sim {
            let bytes = self
                .simctl(&["spawn", &self.id, "launchctl", "list"], 256 * 1024)
                .await?;
            let prefix = format!("UIKitApplication:{}[", self.app);
            Ok(String::from_utf8_lossy(&bytes).lines().filter_map(|line|{
                let w:Vec<_>=line.split_whitespace().collect();if w.len()<3||!w[2].starts_with(&prefix){return None;}
                let pid=w[0].parse::<u32>().ok().filter(|v|*v>0)?;
                Some(json!({"pid":pid,"generation":process::start_time(pid).map(|birth|format!("sim:{}:{}:{pid}:{birth}",self.set.identity,self.id)),"executable":self.installed["executable"],"association":"selected simulator launchctl exact bundle label; birth from host kernel when available"}))
            }).take(16).collect())
        } else {
            let value = self
                .device_json(&[
                    "devicectl",
                    "device",
                    "info",
                    "processes",
                    "--device",
                    &self.id,
                ])
                .await?;
            let rows = value["result"]["processes"]
                .as_array()
                .ok_or_else(|| Error::new("APPLE_TOOL_FORMAT", "Device tool omitted processes"))?;
            let root = self.installed["path"].as_str().unwrap_or_default();
            Ok(rows.iter().filter_map(|p|{
                let exe=p["executable"].as_str().and_then(file_url).or_else(||p["executable"]["path"].as_str().map(str::to_string))?;
                if !exe.strip_prefix(root).is_some_and(|s|s.starts_with('/')&&!s[1..].contains('/')){return None;}
                let pid=p["processIdentifier"].as_u64().filter(|v|*v>0&&*v<=u32::MAX as u64)?;
                Some(json!({"pid":pid,"generation":null,"executable":exe,"association":"current device executable inside selected installed app; process birth unavailable, PID reuse not excluded"}))
            }).take(16).collect())
        }
    }
    pub async fn read(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        self.budget()?;
        let (kind, data, complete) = match self.stage {
            0 => self.read_readiness().await?,
            1 => {
                self.installed = self.installed().await?;
                self.stage = 2;
                (
                    "installed",
                    json!({"status":"observed","identity":self.installed}),
                    false,
                )
            }
            2 => {
                let data = match self.processes().await {
                    Ok(p) => {
                        self.processes = p;
                        json!({"status":"sampled","processes":self.processes,"complete":false})
                    }
                    Err(e) => self.gap("processes", e),
                };
                self.stage = 3;
                ("processes", data, false)
            }
            3 => {
                let data = if self.logs {
                    self.logs().await
                } else {
                    json!({"status":"not_requested"})
                };
                self.stage = 4;
                ("logs", data, false)
            }
            4 => {
                let data = self.resources().await;
                self.stage = 5;
                ("resources", data, false)
            }
            5 => {
                let data = self.entitlements().await;
                self.stage = 6;
                ("installed_details", data, false)
            }
            6 => {
                let data = self.report_inventory()?;
                self.stage = 7;
                ("report_inventory", data, false)
            }
            7 => {
                if self.report_index < self.report_files.len() {
                    let path = self.report_files[self.report_index].clone();
                    self.report_index += 1;
                    let data = match read_file(&path, 8 * 1024 * 1024) {
                        Ok(bytes) => {
                            use base64::Engine;
                            json!({"status":"available","format":if path.extension().and_then(|s|s.to_str())==Some("ips"){"apple-ips"}else{"apple-crash-text"},"dataBase64":base64::engine::general_purpose::STANDARD.encode(&bytes),"scope":"caller-selected host directory; target/device attribution is not independently established"})
                        }
                        Err(e) => self.gap("report", e),
                    };
                    ("report", data, false)
                } else {
                    self.stage = 8;
                    (
                        "coverage",
                        json!({"complete":false,"physicalResourceState":"unavailable without a separately supported device profiler; host ps is never substituted","nativeReportRetrieval":"unregistered; select exported reportDirectory or explicit import; no crash mover runs","sysdiagnose":"explicit bounded text import only; generation/archive expansion unregistered","audio":{"operation":"audio_probe","automaticCapture":false},"missingLogsDoNotProveAbsence":true}),
                        false,
                    )
                }
            }
            _ => {
                if self.identity().await? != self.target_identity {
                    return Err(Error::new(
                        "APPLE_TARGET_CHANGED",
                        "Selected Apple target changed during the sample",
                    ));
                }
                let now = self.installed().await?;
                if now != self.installed {
                    return Err(Error::new(
                        "APPLE_APP_CHANGED",
                        "Installed Apple app changed during the sample",
                    ));
                }
                self.stage = 9;
                (
                    "window",
                    json!({"complete":false,"sourceLoss":self.gaps}),
                    true,
                )
            }
        };
        self.budget()?;
        let mut bytes=serde_json::to_vec(&json!({"schemaVersion":1,"kind":kind,"targetId":self.target,"appId":self.app,"route":if self.sim{"simulator"}else{"physical_service"},"transport":self.transport,"actionIds":self.actions,"observedAt":crate::project::now(),"data":data})).map_err(|_|Error::new("APPLE_FORMAT","Cannot retain Apple evidence"))?;
        bytes.push(b'\n');
        Ok((bytes, complete, self.summary()))
    }
    async fn read_readiness(&mut self) -> Result<(&'static str, Value, bool)> {
        self.target_identity = self.identity().await?;
        let tool_version = self
            .run(&["--version"], 4096)
            .await
            .ok()
            .map(|b| String::from_utf8_lossy(&b).trim().to_string());
        self.stage = 1;
        Ok((
            "readiness",
            json!({"status":"observed","target":self.target_identity,"hostOs":std::env::consts::OS,"hostArchitecture":std::env::consts::ARCH,"xcrunVersion":tool_version,"hostOsVersion":host_version(),"nativeVersionRangeCertified":false}),
            false,
        ))
    }
    async fn logs(&mut self) -> Value {
        let began = Instant::now();
        let mut service_version = None;
        let bytes = if self.sim {
            let mut clauses: Vec<String> = self
                .processes
                .iter()
                .filter_map(|p| p["pid"].as_u64().map(|p| format!("processID == {p}")))
                .collect();
            if self.system {
                clauses.push(format!("eventMessage CONTAINS[c] \"{}\"", self.app));
            }
            if clauses.is_empty() {
                return json!({"status":"process_unavailable","entries":[],"complete":false});
            }
            self.simctl(
                &[
                    "spawn",
                    &self.id,
                    "log",
                    "show",
                    "--style",
                    "ndjson",
                    "--last",
                    &format!("{}s", self.seconds),
                    "--predicate",
                    &clauses.join(" OR "),
                ],
                512 * 1024,
            )
            .await
        } else {
            let mut names: Vec<String> = self
                .processes
                .iter()
                .filter_map(|p| p["pid"].as_u64().map(|p| p.to_string()))
                .collect();
            if self.system {
                names.extend(SYSTEM.iter().map(|s| s.to_string()));
            }
            if names.is_empty() {
                return json!({"status":"process_unavailable","entries":[],"complete":false});
            }
            let tool = std::env::var_os("TESTRUN_IDEVICESYSLOG")
                .map(PathBuf::from)
                .unwrap_or_else(|| "idevicesyslog".into());
            let budget = match self.budget() {
                Ok(b) => b,
                Err(e) => return self.gap("logs", e),
            };
            match process::run_diagnostic(&tool, &["--version"], budget, 4096).await {
                Ok(o) => {
                    service_version = Some(String::from_utf8_lossy(&o.bytes).trim().to_string())
                }
                Err(e) => return self.gap("device_log_dependency", e),
            }
            let filters = names.join("|");
            let mut args = vec![
                "--udid",
                &self.id,
                "--exit",
                "--no-colors",
                "--process",
                &filters,
            ];
            if self.transport == "network" {
                args.push("--network");
            }
            match self.budget() {
                Ok(budget) => process::sample(
                    tool,
                    &args,
                    budget.min(Duration::from_secs(self.seconds)),
                    512 * 1024,
                )
                .await
                .map(|o| o.bytes),
                Err(e) => Err(e),
            }
        };
        let bytes = match bytes {
            Ok(b) => b,
            Err(e) => return self.gap("logs", e),
        };
        let current = match self.processes().await {
            Ok(p) => p,
            Err(e) => return self.gap("log_identity", e),
        };
        let (entries, unparsed, excluded) = self.log_entries(&bytes, &current);
        json!({"status":"sampled","entries":entries,"unparsed":unparsed,"excluded":excluded,"serviceVersion":service_version,"hostReadWallMs":began.elapsed().as_secs_f64()*1000.0,"requestedSeconds":self.seconds,"windowMeaning":if self.sim{"historical log-store lookback"}else{"bounded live service sample plus unknown service backlog"},"complete":false,"loss":"bounded sample; device service backlog/redaction/loss and truncation unknown; timestamp text retains producer clock"})
    }
    fn log_entries(&self, bytes: &[u8], current: &[Value]) -> (Vec<Value>, usize, usize) {
        let mut entries = Vec::new();
        let mut unparsed = 0;
        let mut excluded = 0;
        for line in String::from_utf8_lossy(bytes).lines() {
            let parsed = if self.sim {
                serde_json::from_str::<Value>(line).ok().and_then(|v|Some(json!({"pid":v["processID"],"process":v["processImagePath"].as_str()?.rsplit('/').next()?,"processImagePath":v["processImagePath"],"message":v["eventMessage"].as_str()?,"timestampText":v["timestamp"],"subsystem":v["subsystem"],"category":v["category"]})))
            } else {
                syslog(line)
            };
            let Some(mut entry) = parsed else {
                unparsed += 1;
                continue;
            };
            let p = self.processes.iter().find(|p| {
                p["pid"] == entry["pid"]
                    && current.iter().any(|now| now == *p)
                    && (!self.sim || entry["processImagePath"] == self.installed["executable"])
            });
            let related = self.system
                && entry["process"]
                    .as_str()
                    .is_some_and(|s| SYSTEM.contains(&s))
                && entry["message"]
                    .as_str()
                    .is_some_and(|m| bundle_token(m, &self.app));
            if p.is_none() && !related {
                excluded += 1;
                continue;
            }
            // Historical log-store/service backlog cannot inherit today's PID birth.
            entry["processGeneration"] = Value::Null;
            entry["systemContext"] = json!(p.is_none());
            entry["association"] = json!(if p.is_some() {
                "currently observed app process/path; historical log generation unverified"
            } else {
                "OS process-name candidate and exact bundle context; privilege/birth and causality not established"
            });
            entries.push(entry);
        }
        (entries, unparsed, excluded)
    }
    async fn resources(&mut self) -> Value {
        if !self.sim {
            return json!({"status":"unavailable","scope":"physical_device","reason":"No registered physical resource profiler; host counters are not device observations"});
        }
        let mut rows = Vec::new();
        for p in self.processes.clone() {
            let Some(pid) = p["pid"].as_u64().map(|p| p as u32) else {
                continue;
            };
            let before = process::start_time(pid);
            if before.is_none() {
                continue;
            }
            if p["generation"]
                != json!(format!(
                    "sim:{}:{}:{pid}:{}",
                    self.set.identity,
                    self.id,
                    before.unwrap()
                ))
            {
                continue;
            }
            let budget = match self.budget() {
                Ok(b) => b,
                Err(e) => return self.gap("simulator_resources", e),
            };
            let tool = std::env::var_os("TESTRUN_APPLE_PS")
                .map(PathBuf::from)
                .unwrap_or_else(|| "/bin/ps".into());
            let bytes = match process::run_diagnostic(
                tool,
                &[
                    "-p",
                    &pid.to_string(),
                    "-o",
                    "pid=,rss=,%cpu=,time=,state=,comm=",
                ],
                budget,
                65536,
            )
            .await
            {
                Ok(o) => o.bytes,
                Err(e) => return self.gap("simulator_resources", e),
            };
            if process::start_time(pid) != before {
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            // ps spacing varies; numeric columns are parsed independently, executable must still match.
            let words: Vec<_> = text.split_whitespace().collect();
            if words.len() < 6
                || words[0].parse::<u32>().ok() != Some(pid)
                || !text
                    .trim_end()
                    .ends_with(self.installed["executable"].as_str().unwrap_or("\0"))
            {
                continue;
            }
            if let Some(rss) = words[1]
                .parse::<u64>()
                .ok()
                .and_then(|n| n.checked_mul(1024))
            {
                rows.push(json!({"pid":pid,"processGeneration":p["generation"],"rssBytes":rss,"cpuPercent":words[2].parse::<f64>().ok().filter(|n|n.is_finite()),"cpuTimeText":words[3],"state":words[4],"scope":"selected simulator app host-kernel process; not physical device","cpuDefinition":"ps %cpu estimate; not an interval sampler","liveHeapBytes":null,"retainedOwnership":null}));
            }
        }
        json!({"status":if rows.is_empty(){"unavailable"}else{"sampled"},"processes":rows,"complete":false})
    }
    async fn entitlements(&mut self) -> Value {
        if !self.sim {
            return json!({"status":"unavailable","entitlements":null,"backgroundModes":null,"reason":"Physical installed entitlement/background-mode retrieval unavailable; local source configuration is not installed evidence"});
        }
        let tool = std::env::var_os("TESTRUN_CODESIGN")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/usr/bin/codesign".into());
        let path = self.installed["executable"].as_str().unwrap_or_default();
        let budget = match self.budget() {
            Ok(b) => b,
            Err(e) => return self.gap("entitlements", e),
        };
        match process::run_diagnostic(tool, &["-d", "--entitlements", ":-", path], budget, 65536)
            .await
        {
            Ok(o) => match plist::from_bytes::<plist::Value>(&o.bytes) {
                Ok(p) => {
                    json!({"status":"observed","entitlements":p.into_dictionary().map(|d|d.into_iter().filter(|(k,_)|["application-identifier","com.apple.developer.team-identifier","get-task-allow","com.apple.developer.background-modes"].contains(&k.as_str())).collect::<plist::Dictionary>()),"backgroundModes":self.installed["backgroundModes"],"scope":"installed simulator executable signature"})
                }
                Err(_) => json!({"status":"unrecognized_format","entitlements":null}),
            },
            Err(e) => self.gap("entitlements", e),
        }
    }
    fn report_inventory(&mut self) -> Result<Value> {
        let Some(dir) = &self.report_directory else {
            return Ok(
                json!({"status":"not_requested","route":"explicit host reportDirectory or import; no automatic global-host scan or crash mover"}),
            );
        };
        let mut count = 0;
        for entry in std::fs::read_dir(dir).map_err(|_| {
            Error::new(
                "APPLE_REPORT_DIRECTORY",
                "Cannot inspect selected report directory",
            )
        })? {
            self.budget()?;
            count += 1;
            if count > 256 {
                self.gaps.insert("report_inventory:entry_limit".into());
                break;
            }
            let entry = entry
                .map_err(|_| Error::new("APPLE_REPORT_DIRECTORY", "Cannot inspect report entry"))?;
            let path = entry.path();
            if !matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("ips" | "crash" | "hang")
            ) {
                continue;
            }
            if entry.file_type().is_ok_and(|t| t.is_file())
                && entry.metadata().is_ok_and(|m| m.len() <= 8 * 1024 * 1024)
            {
                self.report_files.push(path);
            } else {
                self.gaps
                    .insert("report_inventory:non_regular_or_oversized".into());
            }
        }
        self.report_files.sort();
        if self.report_files.len() > 4 {
            self.report_files.truncate(4);
            self.gaps.insert("report_inventory:report_limit".into());
        }
        Ok(
            json!({"status":"sampled","selectedFiles":self.report_files.len(),"entriesInspected":count,"scope":"caller-selected host reports; device attribution unknown until report metadata; no report deletion","limit":4,"complete":false}),
        )
    }
}
const SYSTEM: [&str; 8] = [
    "SpringBoard",
    "runningboardd",
    "backboardd",
    "mediaserverd",
    "audiomxd",
    "powerd",
    "thermalmonitord",
    "ReportCrash",
];
fn host_version() -> Option<String> {
    let bytes = read_file(
        Path::new("/System/Library/CoreServices/SystemVersion.plist"),
        128 * 1024,
    )
    .ok()?;
    let p: plist::Value = plist::from_bytes(&bytes).ok()?;
    p.as_dictionary()?
        .get("ProductVersion")?
        .as_string()
        .map(str::to_string)
}
fn file_url(s: &str) -> Option<String> {
    let url = url::Url::parse(s).ok()?;
    if url.scheme() != "file"
        || url
            .host_str()
            .is_some_and(|s| !s.is_empty() && s != "localhost")
    {
        return None;
    }
    url.to_file_path().ok()?.to_str().map(str::to_string)
}
pub(crate) fn read_file(path: &Path, max: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = opts.open(path).map_err(|_| {
        Error::new(
            "APPLE_FILE_UNAVAILABLE",
            "Cannot read selected regular evidence file",
        )
    })?;
    let before = file
        .metadata()
        .map_err(|_| Error::new("APPLE_FILE_UNAVAILABLE", "Cannot inspect selected file"))?;
    if !before.is_file() || before.len() > max {
        return Err(Error::new(
            "APPLE_FILE_BOUNDS",
            "Selected evidence must be a bounded regular file",
        ));
    }
    let mut bytes = Vec::new();
    (&file)
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::new("APPLE_FILE_UNAVAILABLE", "Cannot read selected evidence"))?;
    let after = file
        .metadata()
        .map_err(|_| Error::new("APPLE_FILE_UNAVAILABLE", "Cannot recheck selected evidence"))?;
    if bytes.len() as u64 > max
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(Error::new(
            "EVIDENCE_CHANGED",
            "Selected file changed while reading",
        ));
    }
    Ok(bytes)
}
pub(crate) struct Temporary(pub PathBuf);
impl Temporary {
    pub fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!("testrun-apple-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).map_err(|_| {
            Error::new("HOST_IO", "Cannot allocate private Apple scratch directory")
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| Error::new("HOST_IO", "Cannot protect scratch directory"))?;
        }
        Ok(Self(path))
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn syslog(line: &str) -> Option<Value> {
    let (prefix, message) = line.split_once(": ")?;
    let (before, rest) = prefix.rsplit_once('[')?;
    let pid = rest.split(']').next()?.parse::<u32>().ok()?;
    let name = before.split_whitespace().last()?;
    if message.len() > 16384 {
        return None;
    }
    Some(
        json!({"pid":pid,"process":name,"message":message,"timestampText":prefix,"processGeneration":null}),
    )
}
