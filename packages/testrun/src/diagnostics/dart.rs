//! Dart VM Service / Flutter source with exact generations and bounded typed reads.
#[cfg(test)]
mod tests;
mod transport;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::sync::watch;
pub(crate) fn mode() -> String {
    "discover".into()
}
pub(crate) fn sample_ms() -> u64 {
    1000
}
/// Repeated `getVM`/`getIsolate` reads of one live Flutter Android VM reported
/// millisecond `startTime` values 1 ms apart. Births within this bound name the same
/// VM or isolate; the VM PID and isolate ID, root library and group still match exactly.
const BIRTH_TOLERANCE_MS: u64 = 2;
fn same_birth(observed: &Value, selected: u64) -> bool {
    observed
        .as_u64()
        .is_some_and(|n| n.abs_diff(selected) <= BIRTH_TOLERANCE_MS)
}
/// A Debug Flutter allocation profile lists every loaded class (5,851 for a small
/// app, most without instances). Snapshots keep at most this many classes.
const MAX_CLASSES: usize = 1024;
const COUNTERS: [&str; 4] = [
    "bytesCurrent",
    "instancesCurrent",
    "accumulatedSize",
    "instancesAccumulated",
];
/// Distinct sampled stacks kept per CPU read, and frames kept per stack (leaf first).
const MAX_STACKS: usize = 4096;
const MAX_FRAMES: usize = 128;
/// A Debug Flutter CPU reply for one second was 17 MB: a full VM object per function
/// and one entry per sample. Keep each function's kind, name, owner, script URL and
/// ticks; group samples by exact (leaf-first, bounded) stack with counts and threads.
/// Returns whether any stack or frame was cut.
fn reduce_cpu(result: &mut Value) -> bool {
    let (Some(samples), Some(functions)) = (
        result.get("samples").and_then(Value::as_array),
        result.get("functions").and_then(Value::as_array),
    ) else {
        return false;
    };
    let reported = (samples.len(), functions.len());
    let mut truncated_frames = 0usize;
    let mut groups: std::collections::HashMap<Vec<u64>, (u64, BTreeSet<u64>, Value, Value)> =
        std::collections::HashMap::new();
    for sample in samples {
        let Some(stack) = sample["stack"].as_array() else {
            continue;
        };
        let mut ids: Vec<u64> = stack.iter().filter_map(Value::as_u64).collect();
        if ids.len() > MAX_FRAMES {
            ids.truncate(MAX_FRAMES);
            truncated_frames += 1;
        }
        let entry = groups
            .entry(ids)
            .or_insert_with(|| (0, BTreeSet::new(), sample["timestamp"].clone(), Value::Null));
        entry.0 += 1;
        if let Some(tid) = sample["tid"].as_u64() {
            entry.1.insert(tid);
        }
        entry.3 = sample["timestamp"].clone();
    }
    let distinct = groups.len();
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(&b.0)));
    groups.truncate(MAX_STACKS);
    // Keep only referenced functions, in first-use order, as compact entries.
    let mut remap = std::collections::HashMap::new();
    let mut kept = vec![];
    let compact = |f: &Value| {
        let function = &f["function"];
        json!({"kind":f["kind"],"name":function["name"].as_str().map_or_else(|| function.clone(), |n| json!(n)),"owner":function["owner"]["name"],"resolvedUrl":f["resolvedUrl"],"inclusiveTicks":f["inclusiveTicks"],"exclusiveTicks":f["exclusiveTicks"]})
    };
    let mut stacks = vec![];
    for (ids, (count, tids, first, last)) in groups {
        let mut frames = vec![];
        for id in ids {
            let index = *remap.entry(id).or_insert_with(|| {
                kept.push(functions.get(id as usize).map_or(Value::Null, compact));
                kept.len() - 1
            });
            frames.push(index);
        }
        stacks.push(json!({"stack":frames,"count":count,"tids":tids,"firstTimestamp":first,"lastTimestamp":last}));
    }
    // The decoder bounds one stack's expanded frames at 32 KiB; keep the longest leaf-first
    // prefix within 30,000 bytes of compact frames.
    let sizes: Vec<usize> = kept
        .iter()
        .map(|f| serde_json::to_vec(f).map_or(usize::MAX, |b| b.len()))
        .collect();
    let mut shortened = 0usize;
    for stack in &mut stacks {
        let frames = stack["stack"].as_array_mut().unwrap();
        let mut total = 0usize;
        let keep = frames
            .iter()
            .take_while(|id| {
                total = total.saturating_add(sizes[id.as_u64().unwrap() as usize]);
                total <= 30_000
            })
            .count();
        if keep < frames.len() {
            frames.truncate(keep);
            shortened += 1;
        }
    }
    let cut = distinct > stacks.len() || truncated_frames > 0 || shortened > 0;
    result["samples"] = json!(stacks);
    result["functions"] = json!(kept);
    result["reduction"] = json!({"method":"distinct leaf-first stacks with counts; compact functions","samplesReported":reported.0,"functionsReported":reported.1,"distinctStacks":distinct,"stacksOmittedByLimit":distinct - stacks.len(),"samplesWithFramesCut":truncated_frames,"stacksShortenedByBytes":shortened,"maxFrames":MAX_FRAMES,"maxStackFrameBytes":30_000});
    cut
}
/// Keep classes with any nonzero counter: classes with current instances first, then
/// by current and accumulated bytes, then class ID. Returns whether the limit cut any.
fn reduce_allocations(profile: &mut Value) -> bool {
    let Some(members) = profile.get_mut("members").and_then(Value::as_array_mut) else {
        return false;
    };
    let reported = members.len();
    let count = |m: &Value, field: &str| m[field].as_f64().unwrap_or(0.0);
    let mut kept: Vec<Value> = std::mem::take(members)
        .into_iter()
        .filter(|m| COUNTERS.iter().any(|field| count(m, field) > 0.0))
        .collect();
    let nonzero = kept.len();
    kept.sort_by(|a, b| {
        (count(b, "instancesCurrent") > 0.0)
            .cmp(&(count(a, "instancesCurrent") > 0.0))
            .then(count(b, "bytesCurrent").total_cmp(&count(a, "bytesCurrent")))
            .then(count(b, "accumulatedSize").total_cmp(&count(a, "accumulatedSize")))
            .then_with(|| {
                a["class"]["id"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(b["class"]["id"].as_str().unwrap_or_default())
            })
    });
    kept.truncate(MAX_CLASSES);
    let retained = kept.len();
    *members = kept;
    profile["membersReported"] = json!(reported);
    profile["membersOmittedWithoutCounters"] = json!(reported - nonzero);
    profile["membersOmittedByLimit"] = json!(nonzero - retained);
    nonzero > retained
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Options {
    pub endpoint_file: PathBuf,
    pub target_id: String,
    pub app_id: String,
    pub runtime_kind: String,
    #[serde(default = "mode")]
    pub mode: String,
    pub isolate_id: Option<String>,
    pub vm_start_time: Option<u64>,
    pub isolate_start_time: Option<u64>,
    #[serde(default)]
    pub inspector: bool,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub timeline: bool,
    #[serde(default)]
    pub process_memory: bool,
    #[serde(default)]
    pub include_vm_output: bool,
    #[serde(default)]
    pub allow_unauthenticated: bool,
    #[serde(default)]
    pub class_ids: Vec<String>,
    #[serde(default)]
    pub object_ids: Vec<String>,
    #[serde(default = "sample_ms")]
    pub sample_ms: u64,
    pub build_mode: Option<String>,
    pub build_id: Option<String>,
    pub symbols_build_id: Option<String>,
}
pub(crate) struct Capture {
    options: Options,
    endpoint: transport::Endpoint,
    client: Option<transport::Client>,
    stage: usize,
    vm: Value,
    isolate: Value,
    epoch: String,
    changed: bool,
    lost: bool,
    gaps: BTreeSet<String>,
    events: Vec<Value>,
    subscriptions: Vec<String>,
    group: Option<String>,
    group_disposed: bool,
    group_attempted: bool,
    until: Option<Instant>,
    origin: Option<u64>,
    /// VM clock when the observation window opens; CPU and timeline reads start here.
    window: Option<u64>,
    control: Option<(watch::Receiver<Option<String>>, Instant)>,
    pub(super) actions: Vec<String>,
    cleanup: Value,
    calls: u64,
    lease: Option<crate::lease::ControlLease>,
}
impl Capture {
    pub fn new(options: Options) -> Result<Self> {
        for s in [&options.target_id, &options.app_id, &options.runtime_kind] {
            super::model::label(s, 256)?;
        }
        if !["flutter_native", "flutter_desktop", "dart_cli"]
            .contains(&options.runtime_kind.as_str())
        {
            return Err(Error::new(
                "DART_RUNTIME_KIND",
                "Select flutter_native/flutter_desktop/dart_cli; Flutter web uses browser evidence and has no native VM heap guarantee",
            ));
        }
        let valid_target = match options.runtime_kind.as_str() {
            "flutter_native" => ["android:", "device:", "simulator:"].iter().any(|p| {
                options
                    .target_id
                    .strip_prefix(p)
                    .is_some_and(|v| !v.is_empty())
            }),
            "flutter_desktop" => options
                .target_id
                .strip_prefix("desktop:")
                .is_some_and(|v| !v.is_empty()),
            _ => options
                .target_id
                .strip_prefix("dart:")
                .is_some_and(|v| !v.is_empty()),
        };
        if !valid_target
            || !["discover", "observe", "cpu", "memory", "inspect"].contains(&options.mode.as_str())
            || !(100..=10000).contains(&options.sample_ms)
            || options.class_ids.len() > 4
            || options.object_ids.len() > 4
        {
            return Err(Error::invalid(
                "Select a matching native/desktop:/dart: target, Dart mode and sampleMs 100..10000; at most four classIds/objectIds",
            ));
        }
        if options.mode != "discover"
            && (options.isolate_id.is_none()
                || options.vm_start_time.is_none()
                || options.isolate_start_time.is_none())
        {
            return Err(Error::invalid(
                "Capture requires exact discovered isolateId, vmStartTime and isolateStartTime",
            ));
        }
        for id in options
            .class_ids
            .iter()
            .chain(&options.object_ids)
            .chain(options.isolate_id.iter())
        {
            super::model::label(id, 256)?;
        }
        for s in [&options.build_id, &options.symbols_build_id]
            .into_iter()
            .flatten()
        {
            super::model::label(s, 256)?;
        }
        if options
            .build_mode
            .as_deref()
            .is_some_and(|m| !["debug", "profile", "release", "unknown"].contains(&m))
        {
            return Err(Error::invalid(
                "buildMode must be debug/profile/release/unknown; it remains caller-declared",
            ));
        }
        let endpoint =
            transport::Endpoint::read(&options.endpoint_file, options.allow_unauthenticated)?;
        Ok(Self {
            options,
            endpoint,
            client: None,
            stage: 0,
            vm: Value::Null,
            isolate: Value::Null,
            epoch: uuid::Uuid::new_v4().to_string(),
            changed: false,
            lost: false,
            gaps: BTreeSet::from([
                "partial_window; native device/build attribution unverified".into()
            ]),
            events: vec![],
            subscriptions: vec![],
            group: None,
            group_disposed: false,
            group_attempted: false,
            until: None,
            origin: None,
            window: None,
            control: None,
            actions: vec![],
            cleanup: Value::Null,
            calls: 0,
            lease: None,
        })
    }
    pub fn control(&mut self, stopped: watch::Receiver<Option<String>>, budget: Duration) {
        self.control = Some((stopped, Instant::now() + budget));
    }
    fn deadline(&self) -> Result<tokio::time::Instant> {
        let remaining = if let Some((stop, deadline)) = &self.control {
            if stop.borrow().is_some() {
                return Err(Error::new("CANCELLED", "Dart capture stopped"));
            }
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_secs(3))
        } else {
            Duration::from_secs(3)
        };
        if remaining.is_zero() {
            return Err(Error::new("TIMEOUT", "Dart capture duration expired"));
        }
        Ok(tokio::time::Instant::now() + remaining)
    }
    fn event(&mut self, notification: Value) -> Result<()> {
        if notification["method"] != "streamNotify"
            || notification["params"]["event"].as_object().is_none()
        {
            return Err(Error::new(
                "DART_PROTOCOL",
                "Unexpected VM Service notification",
            ));
        }
        let stream = notification["params"]["streamId"]
            .as_str()
            .ok_or_else(|| Error::invalid("VM event stream missing"))?;
        let event = &notification["params"]["event"];
        let id = event["isolate"]["id"].as_str();
        let selected = self.options.isolate_id.as_deref();
        let kind = event["kind"].as_str().unwrap_or_default();
        if kind == "IsolateReload"
            && (event["isolateGroup"]["id"] == self.isolate["isolateGroupId"]
                || event["isolateGroup"]["id"].is_null()
                || self.isolate.is_null())
        {
            self.changed = true;
            self.gaps
                .insert("hot_reload_or_unknown_group; fresh capture required".into());
        }
        if id == selected
            && selected.is_some()
            && (kind == "IsolateExit" || kind == "IsolateStart" || kind.starts_with("Pause"))
        {
            self.changed = true;
            self.gaps
                .insert("selected_isolate_exited_restarted_or_paused; no resume/reconnect".into());
        }
        if id != selected
            && !matches!(stream, "Isolate" | "Debug")
            && !(self.options.include_vm_output && matches!(stream, "Stdout" | "Stderr"))
        {
            return Ok(());
        }
        if self.events.len() >= 32 || serde_json::to_vec(&notification).unwrap().len() > 32768 {
            self.gaps.insert("event_packet_budget_exhausted".into());
            return Ok(());
        }
        self.events.push(notification);
        Ok(())
    }
    fn drain_pending_events(&mut self) -> Result<()> {
        for drained in 0..129 {
            let next = self
                .client
                .as_mut()
                .and_then(transport::Client::try_receive);
            match next {
                None => break,
                Some(Ok(mut value)) => {
                    if drained == 128 {
                        self.lost = true;
                        return Err(Error::new(
                            "DART_EVENT_BOUNDS",
                            "Pending VM event flood fenced another request",
                        ));
                    }
                    self.endpoint.redact(&mut value);
                    if value.get("id").is_some() {
                        self.lost = true;
                        return Err(Error::new(
                            "DART_PROTOCOL",
                            "Unexpected late reply before another request; no reuse",
                        ));
                    }
                    self.event(value)?;
                }
                Some(Err(error)) => {
                    self.lost = true;
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    async fn request(&mut self, method: &str, params: Value, cleanup: bool) -> Result<Value> {
        if self.lost {
            return Err(Error::new(
                "DART_CONNECTION_LOST",
                "VM connection is no longer reusable",
            ));
        }
        self.drain_pending_events()?;
        if self.changed {
            return Err(Error::new(
                "DART_GENERATION_CHANGED",
                "VM/isolate changed; select a fresh generation without replay",
            ));
        }
        let deadline = if cleanup {
            tokio::time::Instant::now() + Duration::from_millis(500)
        } else {
            self.deadline()?
        };
        self.calls += 1;
        let client = self
            .client
            .as_mut()
            .ok_or_else(|| Error::new("DART_NOT_CONNECTED", "VM Service is unavailable"))?;
        let id = match client.send(method, params, deadline).await {
            Ok(id) => id,
            Err(e) => {
                self.lost = true;
                return Err(e);
            }
        };
        let expected_id = id.to_string();
        for _ in 0..4096 {
            let mut value = match self.client.as_mut().unwrap().receive(deadline).await {
                Ok(v) => v,
                Err(e) => {
                    self.lost = true;
                    return Err(e);
                }
            };
            self.endpoint.redact(&mut value);
            if value.get("id").is_some() {
                if value["id"].as_str() != Some(expected_id.as_str()) {
                    self.lost = true;
                    return Err(Error::new(
                        "DART_PROTOCOL",
                        "Unexpected VM reply identity; connection is not reused",
                    ));
                }
                if value.get("error").is_some() {
                    if let Some(code) = value["error"]["code"].as_i64() {
                        self.gaps.insert(format!("{method}:rpc_code:{code}"));
                    }
                    return Err(Error::new(
                        "DART_METHOD_UNAVAILABLE",
                        "Selected VM/extension rejected this typed read; no fallback or enabling mutation was attempted",
                    ));
                }
                let result = value
                    .get("result")
                    .filter(|v| v.is_object())
                    .ok_or_else(|| Error::new("DART_PROTOCOL", "Expected a VM response object"))?
                    .clone();
                if result["type"] == "Sentinel" {
                    return Err(Error::new(
                        "DART_OBJECT_UNAVAILABLE",
                        "Selected isolate/object is collected, expired or unavailable; IDs were not substituted",
                    ));
                }
                if self.changed {
                    return Err(Error::new(
                        "DART_GENERATION_CHANGED",
                        "Observed reload/exit/pause fenced this response",
                    ));
                }
                return Ok(result);
            }
            self.event(value)?;
        }
        self.lost = true;
        Err(Error::new(
            "DART_EVENT_BOUNDS",
            "VM event flood exhausted request budget; capture stops",
        ))
    }
    async fn optional(&mut self, method: &str, params: Value) -> Value {
        match self.request(method, params, false).await {
            Ok(v) => {
                if (matches!(
                    method,
                    "getObject"
                        | "getInstances"
                        | "getRetainingPath"
                        | "getInboundReferences"
                        | "getStack"
                ) || method.starts_with("ext.flutter.inspector."))
                    && serde_json::to_vec(&v).is_ok_and(|bytes| bytes.len() > 32768)
                {
                    self.gaps.insert(format!("{method}:result_exceeds_32KiB"));
                    return json!({"availability":"unavailable","reason":"DART_RESULT_BOUNDS"});
                }
                v
            }
            Err(e) => {
                self.gaps.insert(format!("{method}:{}", e.code));
                let bytes = e.details.as_ref().map(|d| d["replyBytes"].clone());
                if let Some(Value::Number(n)) = &bytes {
                    self.gaps.insert(format!("{method}:reply_bytes:{n}"));
                }
                json!({"availability":"unavailable","reason":e.code,"replyBytes":bytes})
            }
        }
    }
    async fn allocations(&mut self) -> Value {
        let mut profile = self.optional("getAllocationProfile", self.params()).await;
        if reduce_allocations(&mut profile) {
            self.gaps.insert(format!(
                "getAllocationProfile:classes_limited_to_{MAX_CLASSES}"
            ));
        }
        profile
    }
    fn params(&self) -> Value {
        json!({"isolateId":self.options.isolate_id})
    }
    fn summary(&self) -> Value {
        json!({"adapter":"dart-vm-service/1","stage":self.stage,"runtimeGeneration":self.epoch,"generationChanged":self.changed,"connectionLost":self.lost,"requests":self.calls,"sourceLoss":self.gaps,"cleanup":self.cleanup,"historyComplete":false,"nativeAssociationVerified":false})
    }
    fn packet(
        &mut self,
        kind: &str,
        mut data: Value,
        complete: bool,
    ) -> Result<(Vec<u8>, bool, Value)> {
        self.endpoint.redact(&mut data);
        if let Some(object) = data.as_object_mut() {
            object.insert(
                "isolateGroupId".into(),
                self.isolate["isolateGroupId"].clone(),
            );
            object.insert(
                "pendingEvents".into(),
                json!(std::mem::take(&mut self.events)),
            );
        }
        let mut bytes=serde_json::to_vec(&json!({"schemaVersion":1,"kind":kind,"targetId":self.options.target_id,"declaredAppId":self.options.app_id,"runtimeKind":self.options.runtime_kind,"buildMode":self.options.build_mode,"buildId":self.options.build_id,"symbolsBuildId":self.options.symbols_build_id,"vmStartTime":self.vm["startTime"],"vmPid":self.vm["pid"],"isolateId":self.options.isolate_id,"isolateStartTime":self.isolate["startTime"],"rootLibrary":self.isolate["rootLib"],"runtimeGeneration":self.epoch,"generationMeaning":"socket observation epoch; VM/isolate startTime producer-reported; native device/build association unverified","actionIds":self.actions,"observedAt":crate::project::now(),"data":data})).map_err(|_|Error::invalid("Cannot encode Dart evidence"))?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(Error::new(
                "DART_PACKET_BOUNDS",
                "Dart evidence packet exceeds 8 MiB",
            ));
        }
        bytes.push(b'\n');
        Ok((bytes, complete, self.summary()))
    }
    fn validate_isolate(&mut self, isolate: Value) -> Result<()> {
        if isolate["type"] != "Isolate"
            || isolate["id"] != json!(self.options.isolate_id)
            || isolate["startTime"].as_u64().is_none()
        {
            return Err(Error::new(
                "DART_ISOLATE_IDENTITY",
                "Selected isolate identity is unavailable",
            ));
        }
        if self
            .options
            .isolate_start_time
            .is_some_and(|n| !same_birth(&isolate["startTime"], n))
            || !self.isolate.is_null()
                && (isolate["rootLib"] != self.isolate["rootLib"]
                    || isolate["isolateGroupId"] != self.isolate["isolateGroupId"])
        {
            self.changed = true;
            return Err(Error::new(
                "DART_GENERATION_CHANGED",
                "Selected isolate generation/library changed",
            ));
        }
        // The first validated isolate names this collector's epoch; later reads only
        // recheck it, so every packet header carries one birth for comparison.
        if self.isolate.is_null() {
            self.isolate = isolate;
        }
        Ok(())
    }
    pub async fn read(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        match self.read_inner().await {
            Ok(result) => Ok(result),
            Err(error) if !self.events.is_empty() || self.changed => {
                self.gaps.insert(format!("interrupted:{}", error.code));
                self.stage = 9;
                self.packet(
                    "interruption",
                    json!({"reason":error.code,"sourceLoss":self.gaps}),
                    true,
                )
            }
            Err(error) => Err(error),
        }
    }
    async fn read_inner(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        self.deadline()?;
        if self.changed {
            return Err(Error::new(
                "DART_GENERATION_CHANGED",
                "Observed isolate reload/exit/pause fenced further reads",
            ));
        }
        if self.lost {
            return Err(Error::new(
                "DART_CONNECTION_LOST",
                "VM connection lost; collection will not reconnect",
            ));
        }
        match self.stage {
            0 => self.discover().await,
            1 => self.subscribe().await,
            2 => {
                self.stage = 3;
                let stack = self
                    .optional(
                        "getStack",
                        json!({"isolateId":self.options.isolate_id,"limit":64}),
                    )
                    .await;
                let clock = self.optional("getVMTimelineMicros", json!({})).await;
                self.origin = clock["timestamp"].as_u64();
                let memory = self.optional("getMemoryUsage", self.params()).await;
                let allocations = self.allocations().await;
                self.packet("snapshot",json!({"position":"before","vmMicros":self.origin,"stack":stack,"memory":memory,"allocations":allocations}),false)
            }
            3 => self.read_extensions().await,
            4 => self.read_objects().await,
            5 => {
                if self.changed {
                    return Err(Error::new(
                        "DART_GENERATION_CHANGED",
                        "Selected isolate changed during observation",
                    ));
                }
                if Instant::now() >= self.until.unwrap() {
                    self.stage = 6;
                } else {
                    let deadline = self.deadline()?.min(
                        tokio::time::Instant::now()
                            + self
                                .until
                                .unwrap()
                                .saturating_duration_since(Instant::now())
                                .min(Duration::from_millis(250)),
                    );
                    let result = self.client.as_mut().unwrap().receive(deadline).await;
                    match result {
                        Ok(mut notification) => {
                            self.endpoint.redact(&mut notification);
                            self.event(notification)?;
                        }
                        Err(e) if e.code == "DART_REQUEST_TIMEOUT" => {}
                        Err(e) => {
                            self.lost = true;
                            return Err(e);
                        }
                    }
                }
                let events = std::mem::take(&mut self.events);
                self.packet("events", json!({"events":events}), false)
            }
            6 => self.read_final_snapshot().await,
            _ => {
                let events = std::mem::take(&mut self.events);
                self.packet("coverage",json!({"events":events,"sourceLoss":self.gaps,"nativeNetworkCoverage":"unknown","frameworkWork":"reported Flutter build/raster/rebuild/repaint observations, not GPU execution","historyComplete":false}),true)
            }
        }
    }
    async fn discover(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        self.lease = Some(crate::lease::ControlLease::acquire(&format!(
            "dart-service:{}",
            self.endpoint.address.port()
        ))?);
        self.client = Some(transport::Client::open(&self.endpoint, self.deadline()?).await?);
        let version = self.request("getVersion", json!({}), false).await?;
        if version["major"] != 4 {
            return Err(Error::new(
                "DART_PROTOCOL_VERSION",
                "This adapter requires negotiated VM Service major 4; no alternate protocol was assumed",
            ));
        }
        self.vm = self.request("getVM", json!({}), false).await?;
        if self.vm["type"] != "VM"
            || self.vm["startTime"].as_u64().is_none()
            || self.vm["isolates"].as_array().is_none_or(|a| a.len() > 64)
        {
            return Err(Error::new(
                "DART_VM_IDENTITY",
                "VM identity/isolate inventory is unavailable or exceeds 64 isolates",
            ));
        }
        if self
            .options
            .vm_start_time
            .is_some_and(|n| !same_birth(&self.vm["startTime"], n))
        {
            return Err(Error::new(
                "DART_GENERATION_CHANGED",
                "VM birth differs from selection; no capture reads ran",
            ));
        }
        let discover = self.options.mode == "discover";
        if discover && self.options.isolate_id.is_some() {
            let isolate = self.request("getIsolate", self.params(), false).await?;
            self.validate_isolate(isolate)?;
        }
        self.stage = 1;
        self.packet("discovery",json!({"version":version,"vm":self.vm,"isolate":self.isolate,"buildModeMeaning":"caller-declared; extension/method availability is observed separately","runtimeLimits":"Flutter release usually lacks VM Service; web uses browser evidence; no runtime/native build proof inferred"}),discover)
    }
    async fn subscribe(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        for stream in ["Isolate", "Debug", "Logging", "GC", "Extension"]
            .into_iter()
            .chain(self.options.include_vm_output.then_some("Stdout"))
            .chain(self.options.include_vm_output.then_some("Stderr"))
        {
            if self
                .request("streamListen", json!({"streamId":stream}), false)
                .await
                .is_ok()
            {
                self.subscriptions.push(stream.into());
            } else {
                self.gaps.insert(format!("stream_unavailable:{stream}"));
            }
        }
        if !self.subscriptions.iter().any(|s| s == "Isolate")
            || !self.subscriptions.iter().any(|s| s == "Debug")
        {
            return Err(Error::new(
                "DART_LIFECYCLE_UNAVAILABLE",
                "Lifecycle subscriptions are required before isolate reads",
            ));
        }
        let isolate = self.request("getIsolate", self.params(), false).await?;
        self.validate_isolate(isolate)?;
        if self.isolate["pauseEvent"]["kind"]
            .as_str()
            .is_some_and(|k| k.starts_with("Pause"))
        {
            self.gaps
                .insert("isolate_initially_paused; no automatic resume".into());
        }
        self.stage = 2;
        self.packet("isolate", json!({"isolate":self.isolate}), false)
    }
    async fn read_extensions(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        self.stage = 4;
        let mut observations = vec![];
        let extensions: BTreeSet<String> = self.isolate["extensionRPCs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        if self.options.inspector {
            let dispose = "ext.flutter.inspector.disposeGroup";
            if extensions.contains(dispose) {
                let group = format!("testrun-{}", self.epoch);
                self.group = Some(group.clone());
                for method in ["ext.flutter.inspector.getRootWidget"] {
                    if extensions.contains(method) {
                        let result = self
                            .optional(
                                method,
                                json!({"isolateId":self.options.isolate_id,"objectGroup":group}),
                            )
                            .await;
                        let id = result["result"]["valueId"].as_str().map(str::to_owned);
                        observations.push(json!({"method":method,"result":result}));
                        if let Some(id) = id.filter(|s| super::model::label(s, 256).is_ok()) {
                            let details = "ext.flutter.inspector.getDetailsSubtree";
                            if extensions.contains(details) {
                                let result=self.optional(details,json!({"isolateId":self.options.isolate_id,"objectGroup":group,"arg":id,"subtreeDepth":"1"})).await;
                                observations.push(json!({"method":details,"result":result}));
                            }
                        }
                    }
                }
            } else {
                self.gaps.insert(
                    "inspector_group_cleanup_unavailable; object-group reads skipped".into(),
                );
            }
            for method in [
                "ext.flutter.inspector.isWidgetCreationTracked",
                "ext.flutter.inspector.isWidgetTreeReady",
                "ext.flutter.inspector.trackRebuildDirtyWidgets",
                "ext.flutter.inspector.trackRepaintWidgets",
            ] {
                if extensions.contains(method) {
                    let result = self.optional(method, self.params()).await;
                    observations.push(json!({"method":method,"result":result}));
                }
            }
        }
        if self.options.network && extensions.contains("ext.dart.io.getHttpProfile") {
            let state = self
                .optional("ext.dart.io.httpEnableTimelineLogging", self.params())
                .await;
            let profile = self
                .optional("ext.dart.io.getHttpProfile", self.params())
                .await;
            observations.push(json!({"method":"ext.dart.io.getHttpProfile","state":state,"result":profile,"coverage":"existing dart:io HttpClient profiler only; never enabled/cleared; native-plugin and web traffic can be absent"}));
        } else if self.options.network {
            self.gaps.insert(
                "dart_io_network_extension_unavailable; no logging enablement attempted".into(),
            );
        }
        self.packet("extensions",json!({"observations":observations,"available":extensions,"globalInstrumentationChanges":false,"ownedInspectorGroupAllocation":self.group.is_some()}),false)
    }
    async fn read_objects(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        self.stage = 5;
        let mut observations = vec![];
        let mut objects = self.options.object_ids.clone();
        let mut automatic = 0usize;
        for class in self.options.class_ids.clone() {
            let instances=self.optional("getInstances",json!({"isolateId":self.options.isolate_id,"objectId":class,"limit":16,"includeSubclasses":false,"includeImplementers":false})).await;
            for id in instances["instances"]
                .as_array()
                .into_iter()
                .flatten()
                .take(16)
                .filter_map(|v| v["id"].as_str())
            {
                if automatic < 4
                    && !objects.iter().any(|s| s == id)
                    && super::model::label(id, 256).is_ok()
                {
                    objects.push(id.into());
                    automatic += 1;
                }
            }
            observations.push(json!({"method":"getInstances","classId":class,"result":instances}));
        }
        for object in objects {
            let value=self.optional("getObject",json!({"isolateId":self.options.isolate_id,"objectId":object,"offset":0,"count":32})).await;
            let path = self
                .optional(
                    "getRetainingPath",
                    json!({"isolateId":self.options.isolate_id,"targetId":object,"limit":32}),
                )
                .await;
            let inbound = self
                .optional(
                    "getInboundReferences",
                    json!({"isolateId":self.options.isolate_id,"targetId":object,"limit":32}),
                )
                .await;
            observations.push(json!({"method":"object_retention","objectId":object,"object":value,"retainingPath":path,"inboundReferences":inbound,"meaning":"live service object ID in this VM/isolate; can expire; not stable across captures"}));
        }
        // Exclude snapshot and object reads from the sampled window: a Debug Flutter
        // CPU reply that also covered them exceeded the 8 MiB message limit.
        let clock = self.optional("getVMTimelineMicros", json!({})).await;
        self.window = clock["timestamp"].as_u64();
        self.until = Some(Instant::now() + Duration::from_millis(self.options.sample_ms));
        self.packet("objects",json!({"windowVmMicros":self.window,"observations":observations,"heapScope":"VM isolateGroup heap; sibling-isolate objects can be included","fullBinaryHeapSnapshot":"unregistered; class snapshots and bounded live retaining-path reads are distinct"}),false)
    }
    async fn read_final_snapshot(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        self.stage = 7;
        let isolate = self.request("getIsolate", self.params(), false).await?;
        self.validate_isolate(isolate)?;
        let vm = self.request("getVM", json!({}), false).await?;
        let birth = self.vm["startTime"].as_u64().unwrap_or_default();
        if !same_birth(&vm["startTime"], birth) || vm["pid"] != self.vm["pid"] {
            self.changed = true;
            return Err(Error::new(
                "DART_GENERATION_CHANGED",
                "VM identity changed before final reads",
            ));
        }
        let clock = self.optional("getVMTimelineMicros", json!({})).await;
        let end = clock["timestamp"].as_u64();
        let mut observations = vec![];
        if let (Some(start), Some(end)) = (self.window.or(self.origin), end)
            && let Some(extent) = end.checked_sub(start)
        {
            if self.options.mode == "cpu" {
                let mut cpu=self.optional("getCpuSamples",json!({"isolateId":self.options.isolate_id,"timeOriginMicros":start,"timeExtentMicros":extent})).await;
                if reduce_cpu(&mut cpu) {
                    self.gaps
                        .insert("getCpuSamples:stacks_or_frames_limited".into());
                }
                observations.push(json!({"method":"getCpuSamples","result":cpu}));
            }
            if self.options.timeline {
                let timeline = self
                    .optional(
                        "getVMTimeline",
                        json!({"timeOriginMicros":start,"timeExtentMicros":extent}),
                    )
                    .await;
                observations.push(json!({"method":"getVMTimeline","result":timeline,"scope":"VM-wide recorded timeline; not app-isolated and recorder/streams unchanged"}));
            }
        }
        let memory = self.optional("getMemoryUsage", self.params()).await;
        let allocations = self.allocations().await;
        if self.options.process_memory {
            let memory = self.optional("getProcessMemoryUsage", json!({})).await;
            observations.push(json!({"method":"getProcessMemoryUsage","result":memory,"scope":"whole VM process; buckets can overlap"}));
        }
        self.packet("snapshot",json!({"position":"after","vmMicros":end,"memory":memory,"allocations":allocations,"observations":observations}),false)
    }
    pub async fn settle(&mut self) -> Value {
        if let Some(group) = self.group.clone()
            && !self.group_attempted
            && !self.changed
            && !self.lost
        {
            self.group_attempted = true;
            self.group_disposed = self
                .request(
                    "ext.flutter.inspector.disposeGroup",
                    json!({"isolateId":self.options.isolate_id,"objectGroup":group}),
                    true,
                )
                .await
                .is_ok();
        }
        let closed = if let Some(mut client) = self.client.take() {
            client.close().await
        } else {
            true
        };
        self.lease.take();
        self.cleanup = json!({"socketClosed":closed,"groupDisposeAcknowledged":self.group_disposed,"inspectorObjectsMayBeRetained":self.group.is_some()&&!self.group_disposed,"normalOwnedSettlement":closed&&(self.group.is_none()||self.group_disposed),"streamCleanup":"socket-owned subscriptions; connection closed, no global resets","hardManagerLossVerified":false});
        self.summary()
    }
}
