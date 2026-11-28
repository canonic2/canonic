//! Bounded RN/Expo inspector sessions, separate runtime generations and owned cleanup.
use super::inspector as discovery;
#[cfg(test)]
mod tests;
use crate::{
    browser::cdp::{Connection, EventItem, Events, Lane},
    error::{Error, Result},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};
use tokio::sync::watch;

pub(crate) fn mode() -> String {
    "discover".into()
}
pub(crate) fn sample_ms() -> u64 {
    1000
}
pub(crate) const PROBE: &str = crate::browser::cdp::REACT_NATIVE_PROBE;
pub(crate) struct Options {
    pub endpoint: String,
    pub target: String,
    pub app: String,
    pub page: Option<String>,
    pub logical: Option<String>,
    pub mode: String,
    pub network: bool,
    pub scripts: bool,
    pub probe: bool,
    pub takeover: bool,
    pub profiler: bool,
    pub sample_ms: u64,
}
pub(crate) struct Capture {
    options: Options,
    base: url::Url,
    stage: u8,
    selected: Value,
    connection: Option<Connection>,
    events: Option<Events>,
    context: Option<i64>,
    context_unique: Option<String>,
    scripts: std::collections::BTreeMap<String, String>,
    epoch: String,
    changed: bool,
    probe_generation: Option<String>,
    engine: bool,
    profile: Option<&'static str>,
    stop_attempted: bool,
    cleanup: Value,
    lease: Option<crate::lease::ControlLease>,
    until: Option<Instant>,
    control: Option<(watch::Receiver<Option<String>>, Instant)>,
    gaps: BTreeSet<String>,
    pending: Vec<Value>,
    event_count: u64,
    pub(super) actions: Vec<String>,
}
impl Capture {
    pub fn new(options: Options) -> Result<Self> {
        let base = discovery::endpoint(&options.endpoint)?;
        super::model::label(&options.target, 256)?;
        super::model::label(&options.app, 255)?;
        if !["android:", "device:", "simulator:"].iter().any(|prefix| {
            options
                .target
                .strip_prefix(prefix)
                .is_some_and(|v| !v.is_empty())
        }) || !options.app.contains('.')
            || !options
                .app
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || !["discover", "logs", "cpu", "heap_sampling"].contains(&options.mode.as_str())
            || !(100..=10000).contains(&options.sample_ms)
        {
            return Err(Error::invalid(
                "Select exact native target/app, RN mode discover/logs/cpu/heap_sampling and sampleMs 100..10000",
            ));
        }
        if options.mode != "discover" && (options.page.is_none() || options.logical.is_none()) {
            return Err(Error::invalid(
                "Capture requires explicit pageId and logicalDeviceId from discovery",
            ));
        }
        for s in [&options.page, &options.logical].into_iter().flatten() {
            super::model::label(s, 256)?;
        }
        if matches!(options.mode.as_str(), "cpu" | "heap_sampling")
            && (!options.probe || !options.profiler)
        {
            return Err(Error::invalid(
                "Profiling requires probe:true and allowProfilerControl:true; coordinate runtime-global profilers with other clients",
            ));
        }
        Ok(Self {options,base,stage:0,selected:Value::Null,connection:None,events:None,context:None,context_unique:None,scripts:std::collections::BTreeMap::new(),epoch:uuid::Uuid::new_v4().to_string(),changed:false,probe_generation:None,engine:false,profile:None,stop_attempted:false,cleanup:Value::Null,lease:None,until:None,control:None,gaps:BTreeSet::from(["partial_runtime_window; native device association and silent proxy rebinding remain unverified".into()]),pending:vec![],event_count:0,actions:vec![]})
    }
    pub fn control(&mut self, stopped: watch::Receiver<Option<String>>, budget: Duration) {
        self.control = Some((stopped, Instant::now() + budget));
    }
    fn budget(&self) -> Result<Duration> {
        let mut budget = Duration::from_secs(3);
        if let Some((stopped, deadline)) = &self.control {
            if stopped.borrow().is_some() {
                return Err(Error::new(
                    "CANCELLED",
                    "RN capture stopped before another runtime request",
                ));
            }
            budget = budget.min(deadline.saturating_duration_since(Instant::now()));
        }
        if budget.is_zero() {
            return Err(Error::new("TIMEOUT", "RN capture duration expired"));
        }
        Ok(budget)
    }
    fn summary(&self) -> Value {
        json!({"adapter":"react-native-inspector/1","stage":self.stage,"runtimeGeneration":self.epoch,"contextObserved":self.context.is_some(),"events":self.event_count,"sourceLoss":self.gaps,"historyComplete":false,"deviceAssociationVerified":false,"cleanup":self.cleanup,"profilerMayBeActive":self.profile.is_some(),"probeAutomaticallyInstalled":false})
    }
    fn event(&mut self, item: EventItem) {
        let EventItem::Event(event) = item else {
            self.gaps.insert("inspector_event_loss".into());
            self.changed = true;
            return;
        };
        self.event_count += 1;
        match event.method.as_str() {
            "Runtime.executionContextCreated" => {
                let id = event.params["context"]["id"]
                    .as_i64()
                    .filter(|id| *id > 0 && *id <= i32::MAX as i64);
                let unique = event.params["context"]["uniqueId"]
                    .as_str()
                    .map(str::to_owned);
                if self.context.is_some() && self.context_unique != unique {
                    self.changed = true;
                }
                if id.is_none() || self.context.is_some_and(|old| Some(old) != id) {
                    self.changed = true;
                } else if self.context.is_none() {
                    self.context = id;
                    self.context_unique = unique;
                }
            }
            "Debugger.scriptParsed" => {
                if let (Some(id), Some(hash)) = (
                    event.params["scriptId"].as_str(),
                    event.params["hash"].as_str(),
                ) {
                    if self.scripts.len() < 256 {
                        if self
                            .scripts
                            .insert(id.into(), hash.into())
                            .is_some_and(|old| old != hash)
                        {
                            self.changed = true;
                            self.gaps
                                .insert("script_identity_replaced; fresh capture required".into());
                        }
                    } else {
                        self.gaps
                            .insert("script_identity_tracking_budget_exhausted".into());
                    }
                }
            }
            "Runtime.executionContextsCleared"
            | "Runtime.executionContextDestroyed"
            | "Inspector.detached" => self.changed = true,
            "Debugger.paused" => {
                self.gaps
                    .insert("runtime_paused; no automatic resume or stepping".into());
                self.changed = true;
            }
            _ => {}
        }
        let size = serde_json::to_vec(&event.params)
            .map(|v| v.len())
            .unwrap_or(usize::MAX);
        if self.pending.len() >= 32 || size > 32768 {
            self.gaps
                .insert("native_event_omitted_by_packet_budget".into());
            return;
        }
        self.pending
            .push(json!({"method":event.method,"params":event.params}));
    }
    fn drain(&mut self) {
        while let Some(item) = self.events.as_mut().and_then(Events::try_next) {
            self.event(item);
        }
        if self.connection.as_ref().is_some_and(|c| !c.status().open) {
            self.changed = true;
            self.gaps.insert("inspector_connection_lost".into());
        }
    }
    async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.drain();
        if self.changed {
            return Err(Error::new(
                "RN_RUNTIME_CHANGED",
                "Runtime/context changed; select a fresh capture without reconnecting or replaying",
            ));
        }
        let deadline = tokio::time::Instant::now() + self.budget()?;
        let connection = self
            .connection
            .as_ref()
            .ok_or_else(|| Error::new("RN_NOT_CONNECTED", "Inspector transport is unavailable"))?;
        let result = connection
            .call(None, method, params, Lane::Ordinary, deadline)
            .await;
        self.drain();
        result.map_err(|e|Error::new(if matches!(e,crate::browser::cdp::Error::Remote(_)){"RN_METHOD_REJECTED"}else{"RN_METHOD_UNAVAILABLE"},"Selected runtime rejected, lost or timed out the requested method; it was not retried"))
    }
    async fn probe(&mut self) -> Value {
        let Some(context) = self.context else {
            return json!({"status":"context_unavailable"});
        };
        let result = self
            .call(
                "Runtime.evaluate",
                json!({"expression":PROBE,"returnByValue":true,"contextId":context}),
            )
            .await;
        let parsed = result
            .ok()
            .filter(|v| v.get("exceptionDetails").is_none())
            .and_then(|v| v["result"]["value"].as_str().map(str::to_owned))
            .and_then(|text| super::analysis::apple_json(text.as_bytes()).ok());
        let Some(value) = parsed.filter(|v| v["schemaVersion"] == 1) else {
            self.gaps
                .insert("fixed_runtime_probe_unavailable; no probe installation attempted".into());
            return json!({"status":"unavailable"});
        };
        self.engine = value["hermes"] == true;
        let metadata = &value["probe"]["metadata"];
        if !metadata.is_null()
            && (metadata["appId"] != self.options.app
                || metadata["targetId"] != self.options.target)
        {
            self.changed = true;
            self.engine = false;
            self.gaps
                .insert("app_probe_scope_mismatch; no profiler start".into());
        }
        let generation = value
            .pointer("/probe/metadata/runtimeGeneration")
            .and_then(Value::as_str);
        if let (Some(old), Some(new)) = (&self.probe_generation, generation)
            && old != new
        {
            self.changed = true;
            self.gaps.insert("app_probe_generation_changed".into());
        }
        if self.probe_generation.is_none() {
            self.probe_generation = generation.map(str::to_owned);
        }
        value
    }
    fn packet(
        &mut self,
        kind: &str,
        data: Value,
        complete: bool,
    ) -> Result<(Vec<u8>, bool, Value)> {
        let mut bytes=serde_json::to_vec(&json!({"schemaVersion":1,"kind":kind,"targetId":self.options.target,"proxyAppId":self.options.app,"pageId":self.options.page,"logicalDeviceId":self.options.logical,"runtimeGeneration":self.epoch,"runtimeGenerationMeaning":"collector socket epoch fenced by observed context events; not a process birth or proof against silent proxy rebinding","executionContextId":self.context,"deviceAssociationVerified":false,"actionIds":self.actions,"observedAt":crate::project::now(),"data":data})).map_err(|_|Error::new("RN_FORMAT","Cannot encode runtime evidence"))?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(Error::new(
                "RN_PACKET_BOUNDS",
                "Runtime packet exceeds 8 MiB",
            ));
        }
        bytes.push(b'\n');
        Ok((bytes, complete, self.summary()))
    }
    pub async fn read(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        self.budget()?;
        match self.stage {
            0 => self.discover().await,
            1 => {
                let version = discovery::get(&self.base, "json/version", self.budget()?)
                    .await
                    .unwrap_or_else(|_| json!({"availability":"unknown"}));
                self.stage = 2;
                self.packet("version", version, false)
            }
            2 => self.attach().await,
            3 => {
                let deadline = tokio::time::Instant::now() + self.budget()?;
                while self.context.is_none() && !self.changed {
                    let item =
                        tokio::time::timeout_at(deadline, self.events.as_mut().unwrap().next())
                            .await;
                    match item {
                        Ok(Some(item)) => self.event(item),
                        _ => break,
                    }
                }
                self.drain();
                if self.context.is_none() {
                    return Err(Error::new(
                        "RN_CONTEXT_UNAVAILABLE",
                        "Runtime did not report a usable execution context; no profiling/probe ran",
                    ));
                }
                self.stage = 4;
                let events = std::mem::take(&mut self.pending);
                self.packet("events", json!({"events":events}), false)
            }
            4 => {
                self.stage = 5;
                let data = if self.options.probe {
                    self.probe().await
                } else {
                    json!({"status":"not_requested"})
                };
                self.packet("probe", data, false)
            }
            5 => self.start_profile().await,
            6 => self.read_events().await,
            7 => {
                self.stage = 8;
                let profile = self.stop_profile().await;
                self.packet("profile",json!({"mode":self.options.mode,"profile":profile,"samplingIntervalBytes":if self.options.mode=="heap_sampling"{Some(32768)}else{None},"sourceLoss":self.gaps}),false)
            }
            8 => {
                self.stage = 9;
                let data = if self.options.probe {
                    self.probe().await
                } else {
                    json!({"status":"not_requested"})
                };
                self.packet("probe", data, false)
            }
            _ => {
                let list = discovery::get(&self.base, "json/list", self.budget()?).await;
                let unchanged = list
                    .ok()
                    .and_then(|v| discovery::pages(&self.base, &v, &self.options.app).ok())
                    .is_some_and(|pages| pages.iter().any(|p| p == &self.selected));
                if !unchanged {
                    self.gaps
                        .insert("selected_proxy_target_changed_or_unavailable".into());
                }
                self.stage = 10;
                let events = std::mem::take(&mut self.pending);
                self.packet("coverage",json!({"events":events,"sourceLoss":self.gaps,"historyComplete":false,"nativeTargetVerified":false,"nativeHeapOwnership":"unavailable; allocation sampling is not a retaining graph","sourceMaps":"script URLs/hashes and probe declarations only; no automatic download or source-map resolution"}),true)
            }
        }
    }
    async fn discover(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        let list = discovery::get(&self.base, "json/list", self.budget()?).await?;
        let pages = discovery::pages(&self.base, &list, &self.options.app)?;
        if self.options.mode == "discover" {
            self.stage = 10;
            return self.packet("discovery",json!({"pages":pages,"nativeTargetAssociation":"caller_declared; logicalDeviceId is not an OS device serial","noMatchesMeaning":"inspector unavailable/not connected/build restricted; does not establish absence of native evidence"}),true);
        }
        let selected: Vec<_> = pages
            .into_iter()
            .filter(|p| {
                p["id"] == json!(self.options.page)
                    && p["logicalDeviceId"] == json!(self.options.logical)
            })
            .collect();
        if selected.len() != 1 {
            return Err(Error::new(
                "RN_TARGET_UNAVAILABLE",
                "Exact app/page/logical device was not uniquely discovered; do not choose another page automatically",
            ));
        }
        self.selected = selected[0].clone();
        if self.selected["capabilities"]["supportsMultipleDebuggers"] != true
            && !self.options.takeover
        {
            return Err(Error::new(
                "RN_INSPECTOR_CONTENTION",
                "This target does not advertise debugger multiplexing; allowDebuggerTakeover:true explicitly permits replacing its existing debugger",
            ));
        }
        self.stage = 1;
        self.packet("discovery", json!({"selected":self.selected}), false)
    }
    async fn attach(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        let lease_key = format!(
            "rn-inspector:{}:{}:{}",
            self.base.port().unwrap(),
            self.options.logical.as_deref().unwrap(),
            self.options.page.as_deref().unwrap()
        );
        self.lease = Some(crate::lease::ControlLease::acquire(&lease_key)?);
        let socket = self.selected["webSocketDebuggerUrl"]
            .as_str()
            .unwrap()
            .to_string();
        let deadline = tokio::time::Instant::now() + self.budget()?;
        let tcp = tokio::time::timeout_at(
            deadline,
            tokio::net::TcpStream::connect(discovery::address(&self.base)?),
        )
        .await
        .map_err(|_| Error::new("RN_CONNECT_TIMEOUT", "Inspector connection timed out"))?
        .map_err(|_| Error::new("RN_CONNECT_FAILED", "Inspector socket could not be reached"))?;
        let (connection, events) = Connection::open_react_native(tcp, &socket, deadline)
            .await
            .map_err(|_| Error::new("RN_HANDSHAKE_FAILED", "Inspector WebSocket upgrade failed"))?;
        self.connection = Some(connection);
        self.events = Some(events);
        self.call("Runtime.enable", json!({})).await?;
        for method in [
            Some("Log.enable"),
            self.options.network.then_some("Network.enable"),
            self.options.scripts.then_some("Debugger.enable"),
        ]
        .into_iter()
        .flatten()
        {
            if self.call(method, json!({})).await.is_err() {
                self.gaps.insert(format!("unsupported:{method}"));
            }
        }
        self.stage = 3;
        self.packet("attachment",json!({"status":"connected","legacyTakeoverAllowed":self.options.takeover,"networkCoverage":"inspector-visible requests only; native libraries/images/modules may be absent","nativeAppIdentity":"proxy reported; installed build/device association unverified"}),false)
    }
    async fn start_profile(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        if self.changed {
            return Err(Error::new(
                "RN_RUNTIME_CHANGED",
                "Runtime changed before sampling",
            ));
        }
        let command = match self.options.mode.as_str() {
            "cpu" => Some(("Profiler.start", "Profiler.stop", json!({}))),
            "heap_sampling" => Some((
                "HeapProfiler.startSampling",
                "HeapProfiler.stopSampling",
                json!({"samplingInterval":32768,"stackDepth":32}),
            )),
            _ => None,
        };
        if let Some((start, stop, params)) = command {
            if !self.engine {
                self.gaps
                    .insert("profiling_unavailable_without_observed_Hermes_probe".into());
            } else {
                self.profile = Some(stop);
                match self.call(start, params).await {
                    Ok(_) => {}
                    Err(e) => {
                        if e.code == "RN_METHOD_REJECTED" {
                            self.profile = None;
                            self.gaps.insert(
                                "profiler_start_rejected; borrowed profiler not stopped".into(),
                            );
                        } else {
                            self.gaps.insert("profiler_start_unconfirmed; stop attempted once under explicit profiler control".into());
                        }
                    }
                }
            }
        }
        self.until = Some(Instant::now() + Duration::from_millis(self.options.sample_ms));
        self.stage = 6;
        self.packet("sampling",json!({"mode":self.options.mode,"requestedMs":self.options.sample_ms,"hermesObserved":self.engine,"globalProfilerControlAllowed":self.options.profiler}),false)
    }
    async fn read_events(&mut self) -> Result<(Vec<u8>, bool, Value)> {
        self.drain();
        if self.changed {
            self.gaps.insert(
                "runtime_changed; no reconnect, resume or new-runtime profiler stop".into(),
            );
            self.stage = 9;
        } else if Instant::now() >= self.until.unwrap() {
            self.stage = 7;
        } else {
            let wait = self
                .until
                .unwrap()
                .saturating_duration_since(Instant::now())
                .min(self.budget()?)
                .min(Duration::from_millis(500));
            let mut stopped = self.control.as_ref().unwrap().0.clone();
            let item = tokio::select! {_=stopped.changed()=>return Err(Error::new("CANCELLED","RN sample stopped")),item=tokio::time::timeout(wait,self.events.as_mut().unwrap().next())=>item};
            match item {
                Ok(Some(item)) => self.event(item),
                Ok(None) => {
                    self.changed = true;
                    self.gaps.insert("inspector_connection_lost".into());
                }
                Err(_) => {}
            }
        }
        let events = std::mem::take(&mut self.pending);
        self.packet("events", json!({"events":events}), false)
    }
    async fn stop_profile(&mut self) -> Value {
        self.drain();
        if self.stop_attempted {
            return Value::Null;
        }
        let Some(method) = self.profile else {
            return Value::Null;
        };
        self.stop_attempted = true;
        if self.changed {
            self.gaps
                .insert("profiler_cleanup_unverified_after_runtime_change".into());
            return Value::Null;
        }
        let Some(connection) = &self.connection else {
            return Value::Null;
        };
        match connection
            .call(
                None,
                method,
                json!({}),
                Lane::Control,
                tokio::time::Instant::now() + Duration::from_secs(2),
            )
            .await
        {
            Ok(value) => {
                self.profile = None;
                value.get("profile").cloned().unwrap_or(Value::Null)
            }
            Err(_) => {
                self.gaps
                    .insert("profiler_stop_unconfirmed; not retried".into());
                Value::Null
            }
        }
    }
    pub async fn settle(&mut self) -> Value {
        let _ = self.stop_profile().await;
        let closed = if let Some(connection) = self.connection.take() {
            connection
                .close(tokio::time::Instant::now() + Duration::from_secs(2))
                .await
                .is_ok()
        } else {
            true
        };
        self.events.take();
        self.lease.take();
        self.cleanup = json!({"socketClosed":closed,"profilerStopAcknowledged":self.stop_attempted&&self.profile.is_none(),"nativeProfilerState":if self.profile.is_some(){"unknown; inspect original runtime before further profiling"}else{"not_active_by_this_capture"},"normalOwnedSettlement":closed&&self.profile.is_none(),"hardManagerLossVerified":false});
        self.summary()
    }
}
