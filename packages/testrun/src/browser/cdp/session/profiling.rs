//! Owned CDP CPU and JavaScript/CSS coverage collectors and immutable results.
use super::*;
use crate::browser::{artifacts::Format, profiling::Options};
mod coverage;
mod sources;

#[derive(Clone)]
struct Profile {
    id: Uuid,
    page: Reference,
    session: String,
    options: Options,
    state: &'static str,
    cpu: bool,
    javascript: bool,
    javascript_read: bool,
    css: bool,
    profiler: bool,
    dom: bool,
    css_enabled: bool,
    debugger: bool,
    sources: sources::Sources,
    sheets: BTreeMap<String, Value>,
    sheet_loss: bool,
    interrupted: bool,
    reports: Vec<Value>,
    pending: Vec<(&'static str, &'static str, Value)>,
    delta: Option<coverage::Capture>,
}
#[derive(Default)]
pub(super) struct Profiling {
    items: BTreeMap<Reference, Profile>,
    /// The last summarized CPU profile, keyed by evidence ID and digest.
    summary: Option<(
        Uuid,
        Value,
        std::sync::Arc<crate::browser::profiling::cpu::Summary>,
    )>,
}
impl Worker {
    /// A page of function timings from a saved CPU profile. The evidence is
    /// looked up on every call; only its parsed summary is reused.
    pub(super) fn profile_query(
        &mut self,
        evidence_id: Uuid,
        query: &crate::browser::profiling::cpu::Query,
    ) -> Result<Value, Error> {
        let artifact = self.developers.evidence.get(evidence_id)?;
        if artifact.summary["format"] != "v8-cpu-profile" {
            return Err("PROFILE_EVIDENCE_FORMAT".into());
        }
        let digest = artifact.summary["attachment"]["sha256"].clone();
        let summary = match &self.profiling.summary {
            Some((id, cached, summary)) if *id == evidence_id && *cached == digest => {
                summary.clone()
            }
            _ => {
                let profile: Value = serde_json::from_slice(&artifact.read()?)
                    .map_err(|_| Error::from("PROFILE_EVIDENCE_FORMAT"))?;
                let summary =
                    std::sync::Arc::new(crate::browser::profiling::cpu::Summary::parse(&profile)?);
                self.profiling.summary = Some((evidence_id, digest, summary.clone()));
                summary
            }
        };
        Ok(
            json!({"evidenceId":evidence_id,"complete":artifact.summary["complete"],"private":true,"result":summary.page(query)}),
        )
    }
    pub(super) fn profile_gap(&mut self) {
        for p in self.profiling.items.values_mut() {
            p.sheet_loss = true;
            p.sources.gap();
        }
    }
    pub(super) fn profile_status(&self, page: Reference) -> Result<Value, Error> {
        self.index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?;
        Ok(self.profiling.items.get(&page).map(|p|json!({"page":p.page,"profileId":p.id,"state":p.state,"coverageCaptureId":p.delta.as_ref().map(|delta|delta.id),"options":p.options,"reports":p.reports,"instrumentation":["CDP-Profiler","CDP-CSS"],"scope":"selected-page-target","navigation":if p.options.reset(){"reset-on-navigation"}else{"cumulative"},"sourceTextsIncluded":p.options.sources(),"coverageSourceComplete":p.sources.complete(),"coverageSourceCount":p.sources.len()})).unwrap_or_else(||json!({"page":page,"state":"idle"})))
    }
    pub(super) fn profile_snapshots(&self) -> Value {
        json!(
            self.profiling
                .items
                .keys()
                .filter_map(|page| self.profile_status(*page).ok())
                .collect::<Vec<_>>()
        )
    }
    #[expect(
        clippy::too_many_lines,
        reason = "One collector startup transaction records each acknowledged domain before cleanup can consume its ownership flags"
    )]
    pub(super) async fn profile_start(
        &mut self,
        id: Uuid,
        page: Reference,
        options: Options,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options.validate()?;
        if self.profiling.items.contains_key(&page) {
            return Err("PROFILE_CONFLICT".into());
        }
        if options.debugger() && self.debugger_owned(page) {
            return Err("COVERAGE_DEBUGGER_CONFLICT".into());
        }
        if self.debugger_owned(page) {
            return Err("DEBUGGER_PROFILE_CONFLICT".into());
        }
        if self.profiling.items.len() >= 4 {
            return Err("PROFILE_CAPACITY".into());
        }
        let count = usize::from(options.cpu)
            + usize::from(options.javascript_coverage)
            + usize::from(options.css_coverage);
        let profile_id = Uuid::new_v4();
        let route = self
            .index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?;
        let session = route.session_id;
        self.developers
            .evidence
            .reserve(profile_id, count, count * 8 * 1024 * 1024)?;
        self.profiling.items.insert(
            page,
            Profile {
                id: profile_id,
                page,
                session: session.clone(),
                options: options.clone(),
                state: "starting",
                cpu: false,
                javascript: false,
                javascript_read: false,
                css: false,
                profiler: false,
                dom: false,
                css_enabled: false,
                debugger: false,
                sources: sources::Sources::default(),
                sheets: BTreeMap::new(),
                sheet_loss: false,
                interrupted: false,
                reports: Vec::new(),
                pending: Vec::new(),
                delta: None,
            },
        );
        let result: Result<(), Error> = async {
            if options.cpu || options.javascript_coverage {
                self.call(id, Some(&session), "Profiler.enable", json!({}), deadline)
                    .await?;
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .profiler = true;
            }
            if options.javascript_coverage {
                self.call(
                    id,
                    Some(&session),
                    "Profiler.startPreciseCoverage",
                    json!({"callCount":options.javascript_call_count,"detailed":options.javascript_detailed,"allowTriggeredUpdates":false}),
                    deadline,
                )
                .await?;
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .javascript = true;
            }
            if options.debugger() {
                // Claim before dispatch: uncertain enable/skip replies retain
                // the cleanup owner and fence an application debugger.
                self.profiling.items.get_mut(&page).expect("Profile owner").debugger=true;
                self.call(id,Some(&session),"Debugger.enable",json!({"maxScriptsCacheSize":8*1024*1024}),deadline).await?;
                self.call(id,Some(&session),"Debugger.setSkipAllPauses",json!({"skip":true}),deadline).await?;
            }
            if options.cpu {
                self.call(
                    id,
                    Some(&session),
                    "Profiler.setSamplingInterval",
                    json!({"interval":options.sampling_interval_us}),
                    deadline,
                )
                .await?;
                self.call(id, Some(&session), "Profiler.start", json!({}), deadline)
                    .await?;
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .cpu = true;
            }
            if options.css_coverage {
                self.call(id, Some(&session), "DOM.enable", json!({}), deadline)
                    .await?;
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .dom = true;
                self.call(id, Some(&session), "CSS.enable", json!({}), deadline)
                    .await?;
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .css_enabled = true;
                self.call(
                    id,
                    Some(&session),
                    "CSS.startRuleUsageTracking",
                    json!({}),
                    deadline,
                )
                .await?;
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .css = true;
            }
            Ok(())
        }
        .await;
        if let Err(mut error) = result {
            if error.code == "OUTCOME_UNKNOWN" {
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .state = "uncertain";
                return Err(error);
            }
            let cleanup = self
                .profile_stop_inner(page, Instant::now() + Duration::from_secs(5))
                .await;
            error.partial = Some(
                json!({"page":page,"cleanup":cleanup.ok(),"cleanupComplete":!self.profiling.items.contains_key(&page)}),
            );
            return Err(error);
        }
        self.profiling
            .items
            .get_mut(&page)
            .expect("Profile owner")
            .state = "recording";
        self.profile_status(page)
    }
    pub(super) fn profile_owns_debugger(&self, page: Reference) -> bool {
        self.profiling
            .items
            .get(&page)
            .is_some_and(|p| p.options.debugger())
    }
    pub(super) fn profile_active(&self, page: Reference) -> bool {
        self.profiling.items.contains_key(&page)
    }
    async fn profile_result(
        &mut self,
        page: Reference,
        mut value: Value,
        css: bool,
        deadline: Instant,
    ) -> Value {
        while self.profiling.items[&page].sources.pending() && Instant::now() < deadline {
            if self
                .observation_tick(deadline.min(Instant::now() + Duration::from_millis(10)))
                .await
                .is_err()
            {
                self.profiling.items[&page].sources.gap();
                break;
            }
        }
        let p = &self.profiling.items[&page];
        let mut entries = p.sources.snapshot(css);
        if css {
            if p.options.sdk() {
                return crate::browser::profiling::css_result(
                    &value,
                    entries,
                    &p.sources.order(true),
                    p.options.sources(),
                );
            }
            value["styleSheets"] = json!(p.sheets);
            value["styleSheetIdentityLoss"] = json!(p.sheet_loss);
            if p.options.reset() {
                for field in ["ruleUsage", "coverage"] {
                    if let Some(ranges) = value[field].as_array_mut() {
                        ranges.retain(|range| {
                            range["styleSheetId"]
                                .as_str()
                                .is_some_and(|id| entries.contains_key(id))
                        });
                    }
                }
            }
            if p.options.sources() {
                value["sources"] = json!(
                    entries
                        .iter()
                        .map(|(id, (url, text))| (id, json!({"url":url,"text":text})))
                        .collect::<BTreeMap<_, _>>()
                );
            }
        } else {
            value = crate::browser::profiling::javascript_result(value, &p.options);
            if let Some(scripts) = value["result"].as_array_mut() {
                if p.options.reset() || p.options.sdk() {
                    scripts.retain(|entry| {
                        entry["scriptId"]
                            .as_str()
                            .is_some_and(|id| entries.contains_key(id))
                    });
                }
                if p.options.sources() {
                    for script in scripts {
                        if let Some(source) = script["scriptId"]
                            .as_str()
                            .and_then(|id| entries.get_mut(id))
                            .and_then(|(_, text)| text.take())
                            .filter(|source| !source.is_empty())
                        {
                            script["source"] = Value::String(source);
                        }
                    }
                }
            }
            if p.options.sdk() {
                value = crate::browser::profiling::javascript_entries(value);
            }
        }
        value
    }
    pub(super) async fn profile_stop(
        &mut self,
        page: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self
            .profiling
            .items
            .get(&page)
            .is_some_and(|p| matches!(p.state, "starting" | "finalizing" | "capturing-coverage"))
        {
            return Err("PROFILE_BUSY".into());
        }
        self.profile_stop_inner(page, deadline).await
    }
    #[expect(
        clippy::too_many_lines,
        reason = "One collector shutdown preserves partial CPU, JavaScript and CSS reports while releasing their acknowledged domains"
    )]
    async fn profile_stop_inner(
        &mut self,
        page: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let coverage_capture = if self
            .profiling
            .items
            .get(&page)
            .is_some_and(|profile| profile.delta.is_some())
        {
            Some(self.coverage_finalize(page)?)
        } else {
            None
        };
        let profile = self
            .profiling
            .items
            .get_mut(&page)
            .ok_or("PROFILE_NOT_ACTIVE")?;
        profile.state = "finalizing";
        let profile = profile.clone();
        let result: Result<(), Error> = async {
            if profile.cpu {
                let reply = self
                    .cleanup_call(Some(&profile.session), "Profiler.stop", json!({}), deadline)
                    .await?;
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .cpu = false;
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .pending
                    .push(("cpuprofile", "v8-cpu-profile", reply["profile"].clone()));
            }
            if profile.javascript {
                if !profile.javascript_read {
                    let reply = self
                        .cleanup_call(
                            Some(&profile.session),
                            "Profiler.takePreciseCoverage",
                            json!({}),
                            deadline,
                        )
                        .await?;
                    // A known drain is owned before awaiting source work.
                    self.profiling
                        .items
                        .get_mut(&page)
                        .expect("Profile owner")
                        .javascript_read = true;
                    let reply = self.profile_result(page, reply, false, deadline).await;
                    let p = self.profiling.items.get_mut(&page).expect("Profile owner");
                    p.pending.push((
                        "json",
                        if p.options.sdk() {
                            "playwright-javascript-coverage"
                        } else {
                            "v8-precise-javascript-coverage"
                        },
                        reply,
                    ));
                    p.javascript_read = true;
                }
                self.cleanup_call(
                    Some(&profile.session),
                    "Profiler.stopPreciseCoverage",
                    json!({}),
                    deadline,
                )
                .await?;
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .javascript = false;
            }
            if profile.css {
                let reply = self
                    .cleanup_call(
                        Some(&profile.session),
                        "CSS.stopRuleUsageTracking",
                        json!({}),
                        deadline,
                    )
                    .await?;
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .css = false;
                let reply = self.profile_result(page, reply, true, deadline).await;
                let format = if self.profiling.items[&page].options.sdk() {
                    "playwright-css-coverage"
                } else {
                    "cdp-css-rule-usage"
                };
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .pending
                    .push(("json", format, reply));
            }
            for (enabled, method) in [
                (profile.profiler, "Profiler.disable"),
                (profile.css_enabled, "CSS.disable"),
                (profile.dom, "DOM.disable"),
                (profile.debugger, "Debugger.disable"),
            ] {
                if enabled {
                    self.cleanup_call(Some(&profile.session), method, json!({}), deadline)
                        .await?;
                    let p = self.profiling.items.get_mut(&page).expect("Profile owner");
                    match method {
                        "Profiler.disable" => p.profiler = false,
                        "CSS.disable" => p.css_enabled = false,
                        "Debugger.disable" => p.debugger = false,
                        _ => p.dom = false,
                    };
                }
            }
            while let Some((extension, format, value)) = self
                .profiling
                .items
                .get(&page)
                .expect("Profile owner")
                .pending
                .first()
                .cloned()
            {
                self.profile_artifact(page, extension, format, &value)?;
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .pending
                    .remove(0);
            }
            Ok(())
        }
        .await;
        let p = self.profiling.items.get(&page).expect("Profile owner");
        let value = json!({"page":page,"profileId":p.id,"state":if result.is_ok(){"complete"}else{"finalization_pending"},"reports":p.reports,"coverageCapture":coverage_capture,"sourceTextsIncluded":p.options.sources(),"coverageSourceComplete":p.sources.complete(),"scope":"selected-page-target","navigation":if p.options.reset(){"reset-on-navigation"}else{"cumulative"}});
        match result {
            Ok(()) => {
                self.developers.evidence.release(p.id);
                self.profiling.items.remove(&page);
                Ok(value)
            }
            Err(mut error) => {
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .state = "finalization_pending";
                error.partial = Some(value);
                Err(error)
            }
        }
    }
    fn profile_artifact(
        &mut self,
        page: Reference,
        extension: &'static str,
        format: &'static str,
        value: &Value,
    ) -> Result<(), Error> {
        let p = self
            .profiling
            .items
            .get(&page)
            .ok_or("PROFILE_NOT_ACTIVE")?;
        let coverage = format != "v8-cpu-profile" && format != "native-profile-capture-status-json";
        let (bytes, format, within_bound) = if coverage {
            crate::browser::profiling::encode_coverage(value, format)?
        } else {
            (
                serde_json::to_vec(value).map_err(|_| Error::from("BACKEND_PROTOCOL"))?,
                format,
                true,
            )
        };
        if !within_bound {
            p.sources.gap();
        }
        let artifact=Format{prefix:"profile",extension,mime:"application/json",name:format,max_bytes:8*1024*1024}.write(self.journal.directory(),&bytes,json!({"page":page,"profileId":p.id,"private":true,"redacted":false,"complete":!p.interrupted&&(!format.contains("css")||!p.sheet_loss)&&(!coverage||p.sources.complete()),"sourceInterrupted":p.interrupted,"sourceTextsIncluded":coverage&&within_bound&&p.options.sources(),"coverageSourceComplete":coverage.then(||p.sources.complete())}))?;
        let report = self.developers.evidence.insert_for(p.id, artifact)?;
        self.profiling
            .items
            .get_mut(&page)
            .expect("Profile owner")
            .reports
            .push(report);
        Ok(())
    }
    pub(super) fn profile_event(&mut self, event: &Event) {
        if self.profiling.items.is_empty()
            || !matches!(
                event.method.as_str(),
                "Runtime.executionContextsCleared"
                    | "Debugger.scriptParsed"
                    | "CSS.styleSheetAdded"
            )
        {
            return;
        }
        let connection = self.connection.clone();
        for p in self
            .profiling
            .items
            .values_mut()
            .filter(|p| Some(p.session.as_str()) == event.session_id.as_deref())
        {
            if event.method == "Runtime.executionContextsCleared" && p.options.reset() {
                p.sources.reset();
                p.sheets.clear();
            }
            if event.method == "Debugger.scriptParsed"
                && p.options.debugger()
                && let (Some(connection), Some(id), Some(url)) = (
                    connection.as_ref(),
                    event.params["scriptId"].as_str(),
                    event.params["url"].as_str(),
                )
                && (!url.is_empty() || p.options.anonymous())
            {
                p.sources
                    .observe(connection, &p.session, false, id, url, p.options.sources());
            }
            if event.method != "CSS.styleSheetAdded" {
                continue;
            }
            let header = &event.params["header"];
            if let Some(id) = header["styleSheetId"].as_str() {
                if (p.options.sources() || p.options.reset() || p.options.sdk())
                    && let (Some(connection), Some(url)) =
                        (connection.as_ref(), header["sourceURL"].as_str())
                {
                    p.sources
                        .observe(connection, &p.session, true, id, url, p.options.sources());
                }
                if p.sheets.len() >= 512 && !p.sheets.contains_key(id) {
                    p.sheet_loss = true;
                    continue;
                }
                p.sheets.insert(id.to_owned(),json!({"url":header["sourceURL"].as_str().map(crate::browser::diagnostics::url),"startLine":header["startLine"],"startColumn":header["startColumn"],"isInline":header["isInline"],"length":header["length"]}));
            }
        }
    }
    pub(super) async fn close_profiles(&mut self, deadline: Instant) {
        for page in self.profiling.items.keys().copied().collect::<Vec<_>>() {
            let _ = self.profile_stop_inner(page, deadline).await;
        }
    }
    pub(super) fn clear_profiles(&mut self) {
        for p in self.profiling.items.values() {
            self.developers.evidence.release(p.id);
            if let Some(delta) = &p.delta {
                self.developers.evidence.release(delta.id);
            }
        }
        self.profiling.items.clear();
    }
    pub(super) fn finalize_profiles_after_exit(&mut self) -> Result<(), Error> {
        for page in self.profiling.items.keys().copied().collect::<Vec<_>>() {
            if self.profiling.items[&page].delta.is_some() {
                self.coverage_finalize(page)?;
            }
            let p = self.profiling.items.get_mut(&page).expect("Profile owner");
            p.interrupted = true;
            p.state = "finalization_pending";
            if p.pending.is_empty() && p.reports.is_empty() {
                p.pending.push(("json","native-profile-capture-status-json",json!({"profileId":p.id,"page":page,"options":p.options,"complete":false,"reason":"owned-browser-exited-before-profile-result","profileDataAvailable":false})));
            }
            while let Some((extension, format, value)) =
                self.profiling.items[&page].pending.first().cloned()
            {
                self.profile_artifact(page, extension, format, &value)?;
                self.profiling
                    .items
                    .get_mut(&page)
                    .expect("Profile owner")
                    .pending
                    .remove(0);
            }
            self.developers
                .evidence
                .release(self.profiling.items[&page].id);
            self.profiling.items.remove(&page);
        }
        Ok(())
    }
}
