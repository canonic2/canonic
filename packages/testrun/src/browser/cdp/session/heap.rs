//! Heap snapshot streams remain owned until sealed; native worker models belong to DevTools attachments.
use super::*;
use crate::browser::{artifacts::Format, heap::MAX_BYTES};
const BRIDGE: &str = include_str!("../../reading/heap.js");
struct Capture {
    id: Uuid,
    page: Reference,
    stream: super::super::events::HeapStream,
    max: usize,
    gap: bool,
    complete: bool,
    epoch: Uuid,
}
struct Domain {
    session: String,
    epoch: Uuid,
    starting: bool,
}
struct Model {
    developer: Reference,
    evidence: Uuid,
    ready: bool,
    identity: Identity,
}
enum Identity {
    Captured(Uuid),
    Imported(Option<Uuid>),
}
impl Identity {
    fn comparable(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Captured(a), Self::Captured(b)) => a == b,
            (Self::Imported(Some(a)), Self::Imported(Some(b))) => a == b,
            _ => false,
        }
    }
    fn summary(&self) -> Value {
        match self {
            Self::Captured(epoch) => {
                json!({"source":"captured-profiler-epoch","identityEpoch":epoch,"verified":true})
            }
            Self::Imported(group) => {
                json!({"source":"caller-import","identityGroup":group,"verified":false})
            }
        }
    }
}
#[derive(Default)]
pub(super) struct Heap {
    capture: Option<Capture>,
    models: BTreeMap<Uuid, Model>,
    domains: BTreeMap<Reference, Domain>,
    pub(super) busy: Option<Reference>,
}
impl Worker {
    pub(super) fn heap_reset(&mut self, event: &Event) {
        if event.method == "HeapProfiler.resetProfiles" {
            for domain in self.heap.domains.values_mut() {
                if event.session_id.as_deref() == Some(domain.session.as_str()) {
                    domain.epoch = Uuid::new_v4();
                }
            }
            if let Some(c) = self.heap.capture.as_mut()
                && self.heap.domains.get(&c.page).is_some_and(|domain| {
                    event.session_id.as_deref() == Some(domain.session.as_str())
                })
            {
                c.gap = true;
            }
        }
    }
    pub(super) fn heap_gap(&mut self) {
        // A missing reset notification invalidates identity across captures too.
        for domain in self.heap.domains.values_mut() {
            domain.epoch = Uuid::new_v4();
        }
        if let Some(c) = self.heap.capture.as_mut() {
            c.gap = true;
        }
    }
    pub(super) fn heap_fence(&mut self) {
        self.heap
            .domains
            .retain(|page, _| self.index.retained(*page, Kind::Page));
        let stale = self
            .heap
            .models
            .iter()
            .filter(|(_, model)| {
                self.developer_status(model.developer)
                    .is_ok_and(|status| status["closed"] == true)
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in stale {
            self.heap.models.remove(&id);
        }
    }
    pub(super) fn heap_status(&self) -> Value {
        json!({"capture":self.heap.capture.as_ref().map(|c|{let(bytes,truncated)=c.stream.summary();json!({"captureId":c.id,"page":c.page,"bytes":bytes,"maxBytes":c.max,"sourceLoss":c.gap,"truncated":truncated})}),"profilers":self.heap.domains.iter().map(|(page,domain)|json!({"page":page,"identityEpoch":domain.epoch,"starting":domain.starting})).collect::<Vec<_>>(),"models":self.heap.models.iter().map(|(id,m)|json!({"snapshotId":id,"developer":m.developer,"evidenceId":m.evidence,"ready":m.ready})).collect::<Vec<_>>(),"inspectionCleanupPending":self.evaluation.groups.pending_heap()})
    }
    pub(super) async fn close_heap_models(
        &mut self,
        developer: Reference,
        deadline: Instant,
    ) -> Result<(), Error> {
        let ids = self
            .heap
            .models
            .iter()
            .filter(|(_, model)| model.developer == developer)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(());
        }
        if self.developer_status(developer)?["closed"] != true {
            let session = self
                .developer_session(developer)
                .map_err(|_| Error::from("HEAP_CLEANUP_PENDING"))?;
            let expression = format!(
                "(()=>{{const owner=globalThis[Symbol.for({})];for(const id of {}){{owner?.items.get(id)?.worker.dispose();owner?.items.delete(id);}}return true;}})()",
                json!(self.heap_key()),
                json!(ids)
            );
            let reply = self
                .responsive_call(
                    Some(&session),
                    "Runtime.evaluate",
                    json!({"expression":expression,"returnByValue":true}),
                    Lane::Ordinary,
                    deadline,
                )
                .await?;
            if reply.get("exceptionDetails").is_some() || reply["result"]["value"] != true {
                return Err("HEAP_CLEANUP_PENDING".into());
            }
        }
        for id in ids {
            self.heap.models.remove(&id);
        }
        Ok(())
    }
    pub(super) fn seal_heap(&mut self) -> Result<Value, Error> {
        let Some(c) = self.heap.capture.take() else {
            return Ok(Value::Null);
        };
        let (bytes, truncated) = c.stream.finish();
        let complete = c.complete && !c.gap && !truncated;
        let format = Format {
            prefix: "heap",
            extension: if complete { "heapsnapshot" } else { "bin" },
            mime: if complete {
                "application/json"
            } else {
                "application/octet-stream"
            },
            name: if complete {
                "v8-heap-snapshot-json"
            } else {
                "v8-heap-snapshot-json-prefix"
            },
            max_bytes: MAX_BYTES,
        };
        self.developers.evidence.stage(c.id,format,bytes,json!({"page":c.page,"identityEpoch":c.epoch,"complete":complete,"sourceLoss":c.gap,"truncated":truncated,"privacy":"private-unredacted-heap","scope":"selected-page-target","instrumentation":["CDP-HeapProfiler"],"stringsIncluded":true,"profilerRetained":true}))?;
        self.developers
            .evidence
            .finalize(self.journal.directory(), c.id)
    }
    #[expect(
        clippy::too_many_lines,
        reason = "One snapshot transaction retains the profiler domain, event stream and evidence reservation until capture settlement"
    )]
    pub(super) async fn heap_capture(
        &mut self,
        id: Uuid,
        page: Reference,
        max: usize,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if !(1024..=MAX_BYTES).contains(&max) {
            return Err("HEAP_CAPTURE_BOUNDS".into());
        }
        if self.heap.capture.is_some() {
            return Err("HEAP_CAPTURE_BUSY".into());
        }
        self.drain()?;
        let session = self
            .index
            .route(page, Some(Kind::Page))
            .map_err(resource_error)?
            .session_id;
        if self
            .heap
            .domains
            .get(&page)
            .is_some_and(|domain| domain.session != session)
        {
            self.heap.domains.remove(&page);
        }
        if !self.heap.domains.contains_key(&page) {
            if self.heap.domains.len() >= 4 {
                return Err("HEAP_PROFILER_CAPACITY".into());
            }
            self.heap.domains.insert(
                page,
                Domain {
                    session: session.clone(),
                    epoch: Uuid::new_v4(),
                    starting: true,
                },
            );
            if let Err(error) = self
                .call(
                    id,
                    Some(&session),
                    "HeapProfiler.enable",
                    json!({}),
                    deadline,
                )
                .await
            {
                if error.code != "OUTCOME_UNKNOWN" {
                    self.heap.domains.remove(&page);
                }
                return Err(error);
            }
            self.heap
                .domains
                .get_mut(&page)
                .ok_or("HEAP_PROFILER_NOT_ENABLED")?
                .starting = false;
        }
        let epoch = self.heap.domains[&page].epoch;
        let capture = Uuid::new_v4();
        let stream = self
            .events
            .as_mut()
            .ok_or("BACKEND_LOST")?
            .heap_stream(session.clone(), max)
            .ok_or("HEAP_CAPTURE_BUSY")?;
        self.developers.evidence.reserve(capture, 1, max)?;
        self.heap.capture = Some(Capture {
            id: capture,
            page,
            stream,
            max,
            gap: false,
            complete: false,
            epoch,
        });
        let result: Result<(), Error> = async {
            self.call(
                id,
                Some(&session),
                "HeapProfiler.takeHeapSnapshot",
                json!({"reportProgress":false,"captureNumericValue":true,"exposeInternals":false}),
                deadline,
            )
            .await?;
            self.drain()?;
            self.heap
                .capture
                .as_mut()
                .ok_or("HEAP_CAPTURE_LOST")?
                .complete = true;
            Ok(())
        }
        .await;
        if result
            .as_ref()
            .err()
            .is_some_and(|error| error.code == "OUTCOME_UNKNOWN")
        {
            let mut error = result.unwrap_err();
            error.partial = Some(
                json!({"captureId":capture,"page":page,"captureRetained":true,"replayed":false}),
            );
            return Err(error);
        }
        let complete = self
            .heap
            .capture
            .as_ref()
            .is_some_and(|c| c.complete && !c.gap && !c.stream.summary().1);
        let reports = self.seal_heap()?;
        if let Err(mut error) = result {
            error.partial = Some(json!({"captureId":capture,"reports":reports,"replayed":false}));
            return Err(error);
        }
        if !complete {
            let mut error = Error::from("HEAP_CAPTURE_INCOMPLETE");
            error.partial = Some(reports);
            return Err(error);
        }
        Ok(reports)
    }
    pub(super) async fn heap_stop(
        &mut self,
        page: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self.heap.capture.is_some() {
            return Err("HEAP_CAPTURE_BUSY".into());
        }
        let domain = self
            .heap
            .domains
            .get(&page)
            .ok_or("HEAP_PROFILER_NOT_ENABLED")?;
        if domain.starting {
            return Err("HEAP_CAPTURE_BUSY".into());
        }
        let session = domain.session.clone();
        let epoch = domain.epoch;
        if let Err(error) = self
            .cleanup_call(Some(&session), "HeapProfiler.disable", json!({}), deadline)
            .await
            && self.index.retained(page, Kind::Page)
        {
            return Err(error);
        }
        self.heap.domains.remove(&page);
        Ok(json!({"page":page,"stopped":true,"identityEpochEnded":epoch,"evidencePreserved":true}))
    }
    fn heap_key(&self) -> String {
        format!(
            "testrun:heap:{}",
            self.shared.state.lock().expect("Native owner").connection
        )
    }
    async fn heap_bridge(
        &mut self,
        id: Uuid,
        developer: Reference,
        action: &str,
        snapshot: Uuid,
        argument: Value,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let session = self.developer_session(developer)?;
        let purpose = if action == "release" {
            execution_groups::Purpose::HeapCleanup
        } else {
            execution_groups::Purpose::HeapInspection
        };
        let target = self.developer_target(developer)?;
        let group =
            self.evaluation
                .groups
                .begin_private(developer, target, session.clone(), purpose)?;
        let expression = format!(
            "({BRIDGE})({},{},{},{})",
            json!(self.heap_key()),
            json!(action),
            json!(snapshot),
            argument
        );
        let reply = self
            .execution_group_call(
                id,
                &group,
                "Runtime.evaluate",
                json!({"expression":expression,"awaitPromise":true,"returnByValue":true,"objectGroup":group.1}),
                deadline,
            )
            .await;
        self.release_execution_groups(deadline).await;
        let reply = reply?;
        if self.developer_session(developer).ok().as_ref() != Some(&session) {
            return Err("DEVELOPER_SESSION_LOST".into());
        }
        if reply.get("exceptionDetails").is_some() {
            let text = reply["exceptionDetails"]["exception"]["description"]
                .as_str()
                .unwrap_or("");
            let code = [
                "HEAP_MODEL_UNAVAILABLE",
                "HEAP_MODEL_CAPACITY",
                "HEAP_RESULT_BOUNDS",
                "HEAP_CLASS_SELECTOR_CAPACITY",
                "UNKNOWN_HEAP_CLASS",
                "UNKNOWN_HEAP_DIFF_CLASS",
                "UNKNOWN_HEAP_NODE",
                "UNKNOWN_HEAP_SNAPSHOT",
                "HEAP_MODEL_PENDING",
            ]
            .into_iter()
            .find(|code| text.starts_with(&format!("Error: canonic:{code}")))
            .unwrap_or("HEAP_MODEL_FAILED");
            return Err(code.into());
        }
        let value = reply["result"]["value"].clone();
        if matches!(action, "initialize" | "append" | "release") {
            if value != true {
                return Err("HEAP_MODEL_FAILED".into());
            }
        } else if value.get("data").is_none() || !value["truncated"].is_boolean() {
            return Err("HEAP_MODEL_FAILED".into());
        }
        if value.to_string().len() > 65536 {
            return Err("HEAP_RESULT_BOUNDS".into());
        }
        Ok(value)
    }
    fn heap_model(&self, developer: Reference, snapshot: Uuid) -> Result<(), Error> {
        let model = self
            .heap
            .models
            .get(&snapshot)
            .ok_or("UNKNOWN_HEAP_SNAPSHOT")?;
        if model.developer != developer {
            return Err("FOREIGN_HEAP_SNAPSHOT".into());
        }
        if !model.ready {
            return Err("HEAP_MODEL_PENDING".into());
        }
        Ok(())
    }
    #[expect(
        clippy::too_many_lines,
        reason = "The typed heap router retains model allocation and release around each awaited native worker operation"
    )]
    pub(super) async fn heap_operation(
        &mut self,
        id: Uuid,
        request: Request,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.heap_fence();
        match request {
            Request::HeapImport {
                developer,
                path,
                max_bytes,
                identity_group,
            } => {
                crate::browser::heap::validate_import(&path, max_bytes)?;
                self.developer_session(developer)?;
                let owner = Uuid::new_v4();
                self.developers.evidence.reserve(owner, 1, max_bytes)?;
                let result = (|| {
                    let bytes = crate::browser::heap::read_import(&path, max_bytes)?;
                    crate::browser::heap::validate_snapshot(&bytes)?;
                    let format = Format {
                        prefix: "heap-import",
                        extension: "heapsnapshot",
                        mime: "application/json",
                        name: "v8-heap-snapshot-input",
                        max_bytes: MAX_BYTES,
                    };
                    self.developers.evidence.stage(owner,format,bytes,json!({"analysisDeveloper":developer,"identityGroup":identity_group,"identityVerified":false,"source":"caller-provided-private-file","validation":"pending-native-parser","privacy":"private-unredacted-heap"}))?;
                    self.developers
                        .evidence
                        .finalize(self.journal.directory(), owner)
                })();
                self.developers.evidence.release(owner);
                result
            }
            Request::HeapCapture { page, max_bytes } => {
                self.heap_capture(id, page, max_bytes, deadline).await
            }
            Request::HeapLoad {
                developer,
                evidence_id,
            } => {
                if self.heap.models.len() >= 2 {
                    return Err("HEAP_MODEL_CAPACITY".into());
                }
                let status = self.developer_status(developer)?;
                let artifact = self.developers.evidence.get(evidence_id)?;
                let identity = if artifact.summary["format"] == "v8-heap-snapshot-input" {
                    if artifact.summary["analysisDeveloper"] != json!(developer) {
                        return Err("FOREIGN_HEAP_SNAPSHOT".into());
                    }
                    Identity::Imported(
                        serde_json::from_value(artifact.summary["identityGroup"].clone())
                            .map_err(|_| Error::from("HEAP_EVIDENCE_FORMAT"))?,
                    )
                } else {
                    if artifact.summary["format"] != "v8-heap-snapshot-json"
                        || artifact.summary["complete"] != true
                    {
                        return Err("HEAP_EVIDENCE_FORMAT".into());
                    }
                    if status["page"] != artifact.summary["page"] {
                        return Err("FOREIGN_HEAP_SNAPSHOT".into());
                    }
                    Identity::Captured(
                        serde_json::from_value(artifact.summary["identityEpoch"].clone())
                            .map_err(|_| Error::from("HEAP_EVIDENCE_FORMAT"))?,
                    )
                };
                let identity_summary = identity.summary();
                let bytes = String::from_utf8(artifact.read()?)
                    .map_err(|_| Error::from("HEAP_EVIDENCE_FORMAT"))?;
                let snapshot = Uuid::new_v4();
                self.heap.models.insert(
                    snapshot,
                    Model {
                        developer,
                        evidence: evidence_id,
                        ready: false,
                        identity,
                    },
                );
                self.heap.busy = Some(developer);
                let result: Result<Value, Error> = async {
                    self.heap_bridge(id, developer, "initialize", snapshot, Value::Null, deadline)
                        .await?;
                    let mut start = 0;
                    while start < bytes.len() {
                        let mut end = (start + 65536).min(bytes.len());
                        while !bytes.is_char_boundary(end) {
                            end -= 1;
                        }
                        self.heap_bridge(
                            id,
                            developer,
                            "append",
                            snapshot,
                            json!(&bytes[start..end]),
                            deadline,
                        )
                        .await?;
                        start = end;
                    }
                    self.heap_bridge(id, developer, "finish", snapshot, Value::Null, deadline)
                        .await
                }
                .await;
                self.heap.busy = None;
                match result {
                    Ok(summary) => {
                        self.heap
                            .models
                            .get_mut(&snapshot)
                            .ok_or("UNKNOWN_HEAP_SNAPSHOT")?
                            .ready = true;
                        Ok(
                            json!({"snapshotId":snapshot,"developer":developer,"evidenceId":evidence_id,"identity":identity_summary,"summary":summary,"backend":"bundled-native-DevTools-heap-worker"}),
                        )
                    }
                    Err(mut error) => {
                        error.partial = Some(
                            json!({"snapshotId":snapshot,"developer":developer,"evidenceId":evidence_id,"modelRetained":true,"nextAction":"heap_release"}),
                        );
                        Err(error)
                    }
                }
            }
            Request::HeapQuery {
                developer,
                snapshot_id,
                query,
                max_name_length,
            } => {
                query.validate()?;
                crate::browser::heap::name_length(max_name_length)?;
                self.heap_model(developer, snapshot_id)?;
                self.heap.busy = Some(developer);
                let mut options = json!(query);
                options["maxNameLength"] = json!(max_name_length);
                let result = self
                    .heap_bridge(id, developer, "query", snapshot_id, options, deadline)
                    .await;
                self.heap.busy = None;
                result.map(|result|json!({"snapshotId":snapshot_id,"result":result,"private":true,"backend":"bundled-native-DevTools-heap-worker"}))
            }
            Request::HeapCompare {
                developer,
                snapshot_id,
                base_snapshot_id,
                class_index,
                max_name_length,
                offset,
                limit,
            } => {
                crate::browser::heap::name_length(max_name_length)?;
                crate::browser::heap::page(offset, limit)?;
                self.heap_model(developer, snapshot_id)?;
                self.heap_model(developer, base_snapshot_id)?;
                if !self.heap.models[&snapshot_id]
                    .identity
                    .comparable(&self.heap.models[&base_snapshot_id].identity)
                {
                    return Err("HEAP_INCOMPARABLE".into());
                }
                let identity = self.heap.models[&snapshot_id].identity.summary();
                self.heap.busy = Some(developer);
                let result = self
                    .heap_bridge(
                        id,
                        developer,
                        "compare",
                        snapshot_id,
                        json!({"base":base_snapshot_id,"classIndex":class_index,"maxNameLength":max_name_length,"offset":offset,"limit":limit}),
                        deadline,
                    )
                    .await;
                self.heap.busy = None;
                result.map(|result|json!({"snapshotId":snapshot_id,"baseSnapshotId":base_snapshot_id,"comparisonIdentity":identity,"result":result,"private":true}))
            }
            Request::HeapRelease {
                developer,
                snapshot_id,
            } => {
                let model = self
                    .heap
                    .models
                    .get(&snapshot_id)
                    .ok_or("UNKNOWN_HEAP_SNAPSHOT")?;
                if model.developer != developer {
                    return Err("FOREIGN_HEAP_SNAPSHOT".into());
                }
                if let Err(error) = self
                    .heap_bridge(id, developer, "release", snapshot_id, Value::Null, deadline)
                    .await
                    && error.code != "UNKNOWN_HEAP_SNAPSHOT"
                {
                    return Err(error);
                }
                self.heap.models.remove(&snapshot_id);
                Ok(json!({"snapshotId":snapshot_id,"released":true,"evidencePreserved":true}))
            }
            _ => Err("INVALID_SCOPE".into()),
        }
    }
}
