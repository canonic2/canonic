//! Dart service packets and Flutter size artifacts, never V8/HPROF models.
use super::*;
use std::collections::BTreeMap;

struct Emit<'a> {
    index: &'a mut Index,
    base: Value,
    actions: Vec<String>,
}
impl Emit<'_> {
    fn row(
        &mut self,
        kind: &str,
        name: &str,
        source: String,
        payload: Value,
        metric: Option<(f64, &str, &str)>,
        extra: Option<&str>,
    ) -> Result<()> {
        let mut data = self.base.clone();
        if serde_json::to_vec(&payload).map_err(|_| invalid())?.len() > 32768 {
            data["observation"] =
                json!({"availability":"raw_only","reason":"row_payload_budget","rawSource":source});
            if !self
                .index
                .unsupported
                .iter()
                .any(|s| s == "oversized Dart observation retained raw; indexed payload omitted")
            {
                self.index.unsupported.push(
                    "oversized Dart observation retained raw; indexed payload omitted".into(),
                );
            }
        } else {
            data["observation"] = payload;
        }
        let mut r = row(kind, name, source, data);
        r.correlations = self.actions.clone();
        if let Some(id) = extra {
            crate::diagnostics::model::label(id, 128)?;
            if !r.correlations.iter().any(|v| v == id) {
                if r.correlations.len() == 32 {
                    return Err(invalid());
                }
                r.correlations.push(id.into());
            }
        }
        if let Some((value, unit, definition)) = metric {
            if !value.is_finite() || value < 0.0 {
                return Err(invalid());
            }
            r.value = Some(value);
            r.unit = Some(unit.into());
            r.definition = Some(definition.into());
        }
        push(self.index, r)
    }
    fn events(&mut self, events: &Value, path: &str) -> Result<()> {
        for (i, n) in array(events)?.iter().enumerate() {
            let event = &n["params"]["event"];
            let kind = text(&event["kind"])?;
            let source = format!("{path}/{i}/params/event");
            let selected = event["isolate"]["id"] == self.base["isolateId"]
                && !event["isolate"]["id"].is_null();
            self.base["observedIsolateId"] = event["isolate"]["id"].clone();
            self.base["eventScope"] = json!(if selected {
                "selected_isolate"
            } else if event["isolate"]["id"].is_null() {
                "VM_or_unattributed_context"
            } else {
                "neighbor_isolate_context"
            });
            let extension = event["extensionKind"].as_str();
            if extension == Some("Flutter.Frame") && selected {
                for (field, name, definition) in [
                    (
                        "build",
                        "flutter_build_duration",
                        "reported Flutter framework build duration; development overhead can apply; not native/GPU time",
                    ),
                    (
                        "raster",
                        "flutter_raster_duration",
                        "reported Flutter raster thread duration; not GPU execution or displayed-frame latency",
                    ),
                    (
                        "elapsed",
                        "flutter_frame_span",
                        "reported engine frame total span; pipelined build/raster overlap is not additive",
                    ),
                    (
                        "vsyncOverhead",
                        "flutter_vsync_overhead",
                        "reported Flutter vsync overhead",
                    ),
                ] {
                    if let Some(value) = event["extensionData"].get(field) {
                        self.row(
                            "frame",
                            name,
                            format!("{source}/extensionData/{field}"),
                            event.clone(),
                            Some((number(value)?, "microseconds", definition)),
                            None,
                        )?;
                    }
                }
            } else if extension == Some("Testrun.Diagnostics") && selected {
                let data = &event["extensionData"];
                if data["schemaVersion"] != 1 {
                    return Err(invalid());
                }
                let kind = text(&data["kind"])?;
                let name = text(&data["name"])?;
                let metric = if kind == "measure" {
                    Some((
                        number(&data["durationMicros"])?,
                        "microseconds",
                        "app-declared same-clock interval around an owned operation; not native-plugin causality",
                    ))
                } else {
                    None
                };
                self.row(if metric.is_some(){"span"}else{"marker"},name,source,json!({"event":event,"probeIdentity":"app-declared; no automatic installation or OS association","nativeFailureEstablished":false}),metric,data["actionId"].as_str())?;
            } else {
                let failure = selected
                    && (kind == "PauseException"
                        || kind == "Logging"
                            && (event["logRecord"]["level"]
                                .as_u64()
                                .is_some_and(|n| n >= 1000)
                                || event["logRecord"]["error"]["valueAsString"]
                                    .as_str()
                                    .is_some_and(|s| !s.is_empty())
                                // An attached error object (not the Null instance), such as a
                                // StateError reference without a printable value.
                                || event["logRecord"]["error"]["kind"]
                                    .as_str()
                                    .is_some_and(|kind| kind != "Null"))
                        || extension == Some("Flutter.Error"));
                self.row(
                    if failure { "failure" } else { "event" },
                    kind,
                    source,
                    event.clone(),
                    None,
                    None,
                )?;
            }
        }
        self.base
            .as_object_mut()
            .unwrap()
            .remove("observedIsolateId");
        self.base.as_object_mut().unwrap().remove("eventScope");
        Ok(())
    }
    fn snapshot(&mut self, data: &Value, path: &str) -> Result<()> {
        self.base["heapScope"] = json!("isolate_group_shared");
        self.base["isolateGroupId"] = data["isolateGroupId"].clone();
        self.base["vmMicros"] = data["vmMicros"].clone();
        self.base["snapshotPosition"] = data["position"].clone();
        for (field, definition) in [
            (
                "heapUsage",
                "observed Dart heap usage; not process RSS or retained size",
            ),
            ("heapCapacity", "observed Dart heap capacity in bytes"),
            (
                "externalUsage",
                "embedder-reported external memory associated with Dart objects; separate from Dart heap and not all native memory",
            ),
        ] {
            if let Some(value) = data["memory"].get(field) {
                self.row(
                    "metric",
                    field,
                    format!("{path}/memory/{field}"),
                    json!({"memory":data["memory"],"gcRequested":false}),
                    Some((number(value)?, "bytes", definition)),
                    None,
                )?;
            }
        }
        let allocations = &data["allocations"];
        if let Some(members) = allocations["members"].as_array() {
            if members.len() > 10000 {
                return Err(invalid());
            }
            for (i, class) in members.iter().enumerate() {
                for field in [
                    "bytesCurrent",
                    "instancesCurrent",
                    "accumulatedSize",
                    "instancesAccumulated",
                ] {
                    if let Some(value) = class.get(field)
                        && number(value)? < 0.0
                    {
                        return Err(invalid());
                    }
                }
                self.base["dartClassSnapshot"] = json!(true);
                self.base["classIdentity"] =
                    json!({"id":class["class"]["id"],"name":class["class"]["name"]});
                self.base["accumulatorReset"] = allocations["dateLastAccumulatorReset"].clone();
                let metric=class.get("bytesCurrent").map(|v|number(v).map(|value|(value,"bytes","producer-reported current class allocation bytes; no GC/retention proof"))).transpose()?;
                self.row("allocation","bytesCurrent",format!("{path}/allocations/members/{i}"),json!({"statistics":class,"units":{"bytesCurrent":"bytes","instancesCurrent":"objects","accumulatedSize":"bytes","instancesAccumulated":"objects"},"dateLastServiceGC":allocations["dateLastServiceGC"]}),metric,None)?;
                self.base
                    .as_object_mut()
                    .unwrap()
                    .remove("dartClassSnapshot");
                self.base.as_object_mut().unwrap().remove("classIdentity");
                self.base
                    .as_object_mut()
                    .unwrap()
                    .remove("accumulatorReset");
            }
        }
        self.base.as_object_mut().unwrap().remove("heapScope");
        if let Some(stack) = data.get("stack") {
            self.row("stack","Dart current/async stack",format!("{path}/stack"),json!({"stack":stack,"meaning":"bounded current/async causal stack; not CPU sampling or native failure proof","resolved":false}),None,None)?;
        }
        for (i, observation) in data["observations"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            self.observation(observation, &format!("{path}/observations/{i}"))?;
        }
        Ok(())
    }
    fn observation(&mut self, v: &Value, path: &str) -> Result<()> {
        let result = &v["result"];
        match v["method"].as_str() {
            Some("ext.dart.io.getHttpProfile") if result["requests"].is_array() => {
                for (i, request) in array(&result["requests"])?.iter().enumerate() {
                    self.row("event","Dart HTTP observation",format!("{path}/result/requests/{i}"),json!({"request":request,"coverage":v["coverage"],"loggingState":v["state"],"timeMeaning":"producer fields; HTTP wall timestamps are not VM/native clock alignment"}),None,None)?;
                }
            }
            Some("getCpuSamples") if result["samples"].is_array() => {
                let samples = array(&result["samples"])?;
                let functions = array(&result["functions"])?;
                if samples.len() > 50000 || functions.len() > 10000 {
                    return Err(invalid());
                }
                let sizes: Vec<usize> = functions
                    .iter()
                    .map(|v| serde_json::to_vec(v).unwrap().len())
                    .collect();
                let mut groups: BTreeMap<Vec<usize>, (usize, u64)> = BTreeMap::new();
                for (i, sample) in samples.iter().enumerate() {
                    let stack = array(&sample["stack"])?;
                    if stack.len() > 128 {
                        return Err(invalid());
                    }
                    let mut ids = vec![];
                    let mut expanded = 0usize;
                    for frame in stack {
                        let id = frame
                            .as_u64()
                            .and_then(|n| usize::try_from(n).ok())
                            .ok_or_else(invalid)?;
                        expanded = expanded.saturating_add(*sizes.get(id).ok_or_else(invalid)?);
                        if expanded > 32768 {
                            return Err(invalid());
                        }
                        ids.push(id);
                    }
                    // A collector-reduced sample carries the count of identical stacks.
                    let count = match sample.get("count") {
                        None => 1,
                        Some(n) => n.as_u64().filter(|n| *n > 0).ok_or_else(invalid)?,
                    };
                    let entry = groups.entry(ids).or_insert((i, 0));
                    entry.1 = entry.1.saturating_add(count);
                }
                for (ids, (first, count)) in groups {
                    let frames: Vec<_> = ids.iter().map(|id| functions[*id].clone()).collect();
                    self.row("stack","Dart sampled CPU stack",format!("{path}/result/samples/{first}"),json!({"framesProducerOrder":frames,"functionIndices":ids,"samples":count,"samplePeriod":result["samplePeriod"],"timeOriginMicros":result["timeOriginMicros"],"timeExtentMicros":result["timeExtentMicros"],"resolved":false,"meaning":"producer Dart/native function attribution; no AOT symbol resolution or OS CPU scheduling inference"}),Some((count as f64,"samples","recorded VM CPU samples grouped by exact stack; profiler settings unchanged; not wall time or CPU utilization")),None)?;
                }
            }
            Some("getVMTimeline") if result["traceEvents"].is_array() => {
                for (i, event) in array(&result["traceEvents"])?.iter().enumerate() {
                    let metric = if event["ph"] == "X" {
                        Some((
                            number(&event["dur"])?,
                            "microseconds",
                            "producer complete timeline event; VM-wide, overlapping spans not additive, not GPU execution",
                        ))
                    } else {
                        None
                    };
                    self.row(if metric.is_some(){"span"}else{"event"},event["name"].as_str().unwrap_or("Dart timeline event"),format!("{path}/result/traceEvents/{i}"),json!({"event":event,"scope":"VM-wide; selected isolate attribution not inferred","clock":"producer VM monotonic microseconds, unaligned to native clocks"}),metric,None)?;
                }
            }
            _ => self.row(
                "event",
                v["method"].as_str().unwrap_or("Dart service observation"),
                path.into(),
                v.clone(),
                None,
                None,
            )?,
        }
        Ok(())
    }
}
pub(super) fn decode(bytes: &[u8], index: &mut Index) -> Result<()> {
    let lines: Vec<_> = bytes.split(|b| *b == b'\n').collect();
    for (line, raw) in lines.iter().enumerate() {
        if raw.is_empty() {
            continue;
        }
        let p = match bounded_json(raw, MAX_RAW) {
            Ok(v) => v,
            Err(_) if line + 1 == lines.len() && !bytes.ends_with(b"\n") => {
                index.unsupported.push(
                    "clipped final Dart packet omitted; earlier complete packets retained".into(),
                );
                break;
            }
            Err(e) => return Err(e),
        };
        if p["schemaVersion"] != 1 {
            return Err(invalid());
        }
        let actions: Vec<String> = array(&p["actionIds"])?
            .iter()
            .map(|a| {
                let s = text(a)?;
                crate::diagnostics::model::label(s, 256)?;
                Ok(s.into())
            })
            .collect::<Result<_>>()?;
        if actions.len() > 32 {
            return Err(invalid());
        }
        let source = format!("/lines/{line}");
        let data = &p["data"];
        let base = json!({"targetId":p["targetId"],"appId":null,"declaredAppId":p["declaredAppId"],"runtimeKind":p["runtimeKind"],"buildMode":p["buildMode"],"buildId":p["buildId"],"symbolsBuildId":p["symbolsBuildId"],"vmStartTime":p["vmStartTime"],"vmPid":p["vmPid"],"isolateId":p["isolateId"],"isolateStartTime":p["isolateStartTime"],"rootLibrary":p["rootLibrary"],"runtimeGeneration":p["runtimeGeneration"],"observedAt":p["observedAt"],"causeEstablished":false,"nativeAssociationVerified":false});
        let mut emit = Emit {
            index,
            base,
            actions,
        };
        if let Some(events) = data.get("pendingEvents") {
            emit.events(events, &format!("{source}/data/pendingEvents"))?;
        }
        match p["kind"].as_str() {
            Some("events" | "coverage") => {
                emit.events(&data["events"], &format!("{source}/data/events"))?;
                emit.row(
                    "event",
                    "Dart source coverage",
                    source,
                    json!({"sourceLoss":data["sourceLoss"],"historyComplete":false}),
                    None,
                    None,
                )?;
            }
            Some("snapshot") => emit.snapshot(data, &format!("{source}/data"))?,
            Some("objects") => {
                emit.base["heapScope"] =
                    json!("VM isolateGroup heap; sibling-isolate objects can be included");
                emit.base["isolateGroupId"] = data["isolateGroupId"].clone();
                for (i, v) in array(&data["observations"])?.iter().enumerate() {
                    emit.row(if v["method"]=="object_retention"{"retention"}else{"allocation"},v["method"].as_str().unwrap_or("Dart object observation"),format!("{source}/data/observations/{i}"),json!({"query":v,"identityMeaning":"temporary live VM/isolate object IDs; no cross-snapshot identity matching","retentionMeaning":"producer bounded path/inbound references; truncated paths and Collected/Expired sentinels remain explicit; no automatic leak conclusion"}),None,None)?;
                }
            }
            Some("extensions") => {
                for (i, v) in array(&data["observations"])?.iter().enumerate() {
                    emit.observation(v, &format!("{source}/data/observations/{i}"))?;
                }
            }
            _ => emit.row("event", text(&p["kind"])?, source, data.clone(), None, None)?,
        }
    }
    index.unsupported.push("Dart/Flutter partial service evidence; native association/source resolution and binary heap dominators unavailable; no absence/leak/causal proof".into());
    Ok(())
}
pub(super) fn app_size(bytes: &[u8], index: &mut Index) -> Result<()> {
    let value = bounded_json(bytes, MAX_RAW)?;
    let wrapped = value["schemaVersion"] == 1;
    let root = if wrapped {
        &value["sizeAnalysis"]
    } else {
        &value
    };
    if !matches!(
        root["type"].as_str(),
        Some("apk" | "aab" | "ios" | "macos" | "windows" | "linux")
    ) {
        return Err(invalid());
    }
    let mut pending = vec![(
        root,
        if wrapped {
            "/sizeAnalysis".to_owned()
        } else {
            String::new()
        },
        0usize,
    )];
    let mut count = 0;
    while let Some((node, path, depth)) = pending.pop() {
        count += 1;
        if count > 10000 || depth > 48 {
            return Err(invalid());
        }
        let name = text(&node["n"])?;
        let size = number(&node["value"])?;
        if size < 0.0 {
            return Err(invalid());
        }
        let mut r = row(
            "metric",
            "Flutter artifact size",
            format!("{path}/value"),
            json!({"appId":null,"name":name,"artifactType":root["type"],"buildId":if wrapped{value["buildId"].clone()}else{Value::Null},"architecture":if wrapped{value["architecture"].clone()}else{Value::Null},"sizeScope":"producer subtree inclusive size; ancestor/descendant totals must not be added; package estimate is not installed/download size","resolved":false,"precompilerTrace":"retained raw; call-graph attribution not synthesized"}),
        );
        r.value = Some(size);
        r.unit = Some("bytes".into());
        r.definition=Some("producer Flutter analysis subtree byte size; compressed/uncompressed meaning follows artifact producer".into());
        push(index, r)?;
        if let Some(children) = node.get("children") {
            for (i, child) in array(children)?.iter().enumerate().rev() {
                pending.push((child, format!("{path}/children/{i}"), depth + 1));
            }
        }
    }
    index.unsupported.push("Flutter size identity producer-declared; symbols require exact retained build/architecture; no AOT symbolication or whole-package parsing".into());
    Ok(())
}
