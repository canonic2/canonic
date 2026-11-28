//! Producer-reported RN evidence. No V8 heap interpretation or inferred native identity.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn decode(bytes: &[u8], index: &mut Index) -> Result<()> {
    let mut seen = BTreeSet::new();
    let lines: Vec<_> = bytes.split(|b| *b == b'\n').collect();
    for (line, raw) in lines.iter().enumerate() {
        if raw.is_empty() {
            continue;
        }
        let packet = match bounded_json(raw, MAX_RAW) {
            Ok(v) => v,
            Err(_) if line + 1 == lines.len() && !bytes.ends_with(b"\n") => {
                index
                    .unsupported
                    .push("truncated final RN packet; earlier complete packets retained".into());
                break;
            }
            Err(e) => return Err(e),
        };
        if packet["schemaVersion"] != 1 {
            return Err(invalid());
        }
        let source = format!("/lines/{line}");
        let base = json!({"targetId":packet["targetId"],"appId":null,"proxyAppId":packet["proxyAppId"],"pageId":packet["pageId"],"logicalDeviceId":packet["logicalDeviceId"],"runtimeGeneration":packet["runtimeGeneration"],"runtimeGenerationMeaning":packet["runtimeGenerationMeaning"],"executionContextId":packet["executionContextId"],"deviceAssociationVerified":false,"observedAt":packet["observedAt"],"observedAtMeaning":"collector wall time; not producer event time","causeEstablished":false});
        let actions: Vec<String> = packet["actionIds"]
            .as_array()
            .ok_or_else(invalid)?
            .iter()
            .map(|a| {
                let s = text(a)?;
                crate::diagnostics::model::label(s, 256)?;
                Ok(s.to_owned())
            })
            .collect::<Result<_>>()?;
        if actions.len() > 32 {
            return Err(invalid());
        }
        let data = &packet["data"];
        let mut output = PacketRows {
            base,
            actions,
            index,
        };
        match packet["kind"].as_str() {
            Some("events" | "coverage") => output.events(data, &source)?,
            Some("probe") => output.probe(data, &source, &mut seen)?,
            Some("profile") if !data["profile"].is_null() => {
                let profile = &data["profile"];
                match text(&data["mode"])? {
                    "cpu" => output.cpu(profile, &source)?,
                    "heap_sampling" => output.heap_sampling(data, profile, &source)?,
                    _ => return Err(invalid()),
                }
            }
            _ => output.emit(
                "event",
                packet["kind"].as_str().ok_or_else(invalid)?,
                source,
                data.clone(),
                None,
                None,
            )?,
        }
    }
    index.unsupported.push("RN bounded runtime window; unknown native device attribution, silent proxy rebinding, source-map resolution and native-network coverage".into());
    super::super::network_data::runtime_rows(index)?;
    Ok(())
}

struct PacketRows<'a> {
    base: Value,
    actions: Vec<String>,
    index: &'a mut Index,
}
impl PacketRows<'_> {
    fn emit(
        &mut self,
        kind: &str,
        name: &str,
        path: String,
        payload: Value,
        metric: Option<(f64, &str, &str)>,
        extra: Option<&str>,
    ) -> Result<()> {
        let mut details = self.base.clone();
        details["observation"] = payload;
        let mut r = row(kind, name, path, details);
        r.correlations = self.actions.clone();
        if let Some(id) = extra {
            crate::diagnostics::model::label(id, 128)?;
            if !r.correlations.iter().any(|s| s == id) {
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

    fn events(&mut self, data: &Value, source: &str) -> Result<()> {
        for (i, event) in array(&data["events"])?.iter().enumerate() {
            let method = text(&event["method"])?;
            let failure = method == "Runtime.exceptionThrown"
                || method == "Runtime.consoleAPICalled" && event["params"]["type"] == "error"
                || method == "Log.entryAdded"
                    && matches!(
                        event["params"]["entry"]["level"].as_str(),
                        Some("error" | "fatal")
                    );
            self.emit(
                if failure { "failure" } else { "event" },
                method,
                format!("{source}/data/events/{i}"),
                json!({"event":event,"failureMeaning":if failure {Some("producer-reported exception or error-level log; not proof of an uncaught exception, native crash or cause")} else {None},"stackResolution":"producer frames/async parents retained unresolved; script URL alone is not bundle identity","networkCoverage":"inspector-visible only; native traffic may be absent"}),
                None,
                None,
            )?;
        }
        self.emit(
            "event",
            "RN source coverage",
            source.to_owned(),
            json!({"sourceLoss":data["sourceLoss"],"historyComplete":false}),
            None,
            None,
        )?;
        Ok(())
    }

    fn probe(
        &mut self,
        data: &Value,
        source: &str,
        seen: &mut BTreeSet<(String, u64)>,
    ) -> Result<()> {
        self.emit(
            "event",
            "RN runtime/probe identity",
            source.to_owned(),
            json!({"hermes":data["hermes"],"hermesProperties":data["hermesProperties"],"metadata":data["probe"]["metadata"],"status":data["status"],"dropped":data["probe"]["dropped"],"identityMeaning":"app-declared build/bundle/update metadata; not authenticated native identity"}),
            None,
            None,
        )?;
        let probe = &data["probe"];
        if probe.is_null() {
            return Ok(());
        }
        if probe["schemaVersion"] != 1 {
            return Err(invalid());
        }
        let generation = text(&probe["metadata"]["runtimeGeneration"])?;
        crate::diagnostics::model::label(generation, 256)?;
        let events = array(&probe["events"])?;
        if events.len() > 64 {
            return Err(invalid());
        }
        for (i, event) in events.iter().enumerate() {
            let seq = event["sequence"]
                .as_u64()
                .filter(|v| *v > 0)
                .ok_or_else(invalid)?;
            if !seen.insert((generation.to_owned(), seq)) {
                continue;
            }
            let name = text(&event["name"])?;
            crate::diagnostics::model::label(name, 128)?;
            let (kind, metric) = match text(&event["kind"])? {
                "marker" => ("marker", None),
                "measure" => (
                    "span",
                    Some((
                        number(&event["fields"]["durationMs"])?,
                        "milliseconds",
                        "app-declared ordered same-clock measurement; no native clock alignment",
                    )),
                ),
                "react_commit" => (
                    "metric",
                    Some((
                        number(&event["fields"]["actualDuration"])?,
                        "milliseconds",
                        "React Profiler actualDuration for a reported commit; not every render attempt or user latency; development/StrictMode overhead may apply",
                    )),
                ),
                _ => return Err(invalid()),
            };
            self.emit(
                kind,
                name,
                format!("{source}/data/probe/events/{i}"),
                json!({"event":event,"probeGeneration":generation,"metadata":probe["metadata"],"clock":probe["clock"],"componentIdentity":"caller Profiler id scoped to probe installation; no renderer-global identity or why-render inference"}),
                metric,
                event["actionId"].as_str(),
            )?;
        }
        Ok(())
    }

    fn cpu(&mut self, profile: &Value, source: &str) -> Result<()> {
        let nodes = array(&profile["nodes"])?;
        let samples = array(&profile["samples"])?;
        if nodes.len() > 10000 || samples.len() > 50000 {
            return Err(invalid());
        }
        let mut map = BTreeMap::new();
        let mut frame_sizes = BTreeMap::new();
        let mut positions = BTreeMap::new();
        let mut parents = BTreeMap::new();
        for (position, node) in nodes.iter().enumerate() {
            let id = node["id"].as_u64().ok_or_else(invalid)?;
            text(&node["callFrame"]["functionName"])?;
            let size = serde_json::to_vec(&node["callFrame"])
                .map_err(|_| invalid())?
                .len();
            if size > 32768 {
                return Err(invalid());
            }
            frame_sizes.insert(id, size);
            positions.insert(id, position);
            if map.insert(id, &node["callFrame"]).is_some() {
                return Err(invalid());
            }
            if node.get("children").is_some_and(|v| !v.is_array()) {
                return Err(invalid());
            }
            for child in node["children"].as_array().into_iter().flatten() {
                if parents
                    .insert(child.as_u64().ok_or_else(invalid)?, id)
                    .is_some()
                {
                    return Err(invalid());
                }
            }
        }
        for (child, parent) in &parents {
            if !map.contains_key(child) || !map.contains_key(parent) {
                return Err(invalid());
            }
        }
        let start = number(&profile["startTime"])?;
        let end = number(&profile["endTime"])?;
        if end < start {
            return Err(invalid());
        }
        let deltas = profile["timeDeltas"].as_array();
        if deltas.is_some_and(|d| d.len() != samples.len()) {
            return Err(invalid());
        }
        let counts = cpu_sample_counts(samples, deltas.map(Vec::as_slice), &map)?;
        validate_cpu_chains(&map, &parents)?;
        for (id, (count, weight)) in counts {
            let frames = cpu_stack_frames(id, &map, &frame_sizes, &parents)?;
            self.emit(
                "stack",
                "Hermes sampled CPU stack",
                format!("{source}/data/profile/nodes/{}", positions[&id]),
                json!({"framesLeafFirst":frames,"samples":count,"sampleNodeId":id,"samplesSource":format!("{source}/data/profile/samples"),"observedDeltaMicroseconds":deltas.map(|_|weight),"startTime":start,"endTime":end,"engine":"Hermes","resolved":false}),
                Some((
                    count as f64,
                    "samples",
                    "recorded Hermes leaf samples; not CPU utilization, wall duration or native/off-CPU coverage",
                )),
                None,
            )?;
        }
        Ok(())
    }

    fn heap_sampling(&mut self, data: &Value, profile: &Value, source: &str) -> Result<()> {
        let mut pending = vec![(
            &profile["head"],
            0usize,
            format!("{source}/data/profile/head"),
        )];
        let mut ids = BTreeSet::new();
        while let Some((node, depth, path)) = pending.pop() {
            if depth > 32
                || ids.len() >= 10000
                || !ids.insert(node["id"].as_u64().ok_or_else(invalid)?)
            {
                return Err(invalid());
            }
            let size = node["selfSize"].as_u64().ok_or_else(invalid)?;
            text(&node["callFrame"]["functionName"])?;
            if serde_json::to_vec(&node["callFrame"])
                .map_err(|_| invalid())?
                .len()
                > 32768
            {
                return Err(invalid());
            }
            self.emit(
                "metric",
                "Hermes sampled allocation",
                path.clone(),
                json!({"callFrame":node["callFrame"],"nodeId":node["id"],"samplingIntervalBytes":data["samplingIntervalBytes"],"depth":depth,"engine":"Hermes","retainingGraphAvailable":false}),
                Some((
                    size as f64,
                    "bytes",
                    "producer allocation-sampling selfSize estimate; not live heap size, retained ownership, leak proof or a V8 heap snapshot",
                )),
                None,
            )?;
            for (i, child) in array(&node["children"])?.iter().enumerate().rev() {
                pending.push((child, depth + 1, format!("{path}/children/{i}")));
            }
        }
        Ok(())
    }
}

type SampleCounts = BTreeMap<u64, (u64, f64)>;
fn cpu_sample_counts(
    samples: &[Value],
    deltas: Option<&[Value]>,
    map: &BTreeMap<u64, &Value>,
) -> Result<SampleCounts> {
    let mut counts: BTreeMap<u64, (u64, f64)> = BTreeMap::new();
    for (i, sample) in samples.iter().enumerate() {
        let id = sample.as_u64().ok_or_else(invalid)?;
        if !map.contains_key(&id) {
            return Err(invalid());
        }
        let entry = counts.entry(id).or_default();
        entry.0 += 1;
        if let Some(d) = deltas {
            let delta = number(&d[i])?;
            if delta < 0.0 {
                return Err(invalid());
            }
            entry.1 += delta;
            if !entry.1.is_finite() {
                return Err(invalid());
            }
        }
    }
    Ok(counts)
}

fn validate_cpu_chains(map: &BTreeMap<u64, &Value>, parents: &BTreeMap<u64, u64>) -> Result<()> {
    // Validate all chains, including unsampled nodes, before publishing results.
    for id in map.keys() {
        let mut current = *id;
        let mut chain = BTreeSet::new();
        loop {
            if !chain.insert(current) || chain.len() > 256 {
                return Err(invalid());
            }
            match parents.get(&current) {
                Some(parent) => current = *parent,
                None => break,
            }
        }
    }
    Ok(())
}

fn cpu_stack_frames(
    id: u64,
    map: &BTreeMap<u64, &Value>,
    frame_sizes: &BTreeMap<u64, usize>,
    parents: &BTreeMap<u64, u64>,
) -> Result<Vec<Value>> {
    let mut frames = vec![];
    let mut current = id;
    let mut expanded = 0usize;
    loop {
        expanded = expanded.saturating_add(frame_sizes[&current]);
        if expanded > 32768 {
            return Err(invalid());
        }
        frames.push(map[&current].clone());
        match parents.get(&current) {
            Some(parent) => current = *parent,
            None => break,
        }
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn diagnostic_react_native_console_errors_preserve_severity_without_crash_claims() {
        let packet = json!({"schemaVersion":1,"kind":"events","targetId":"android:acme","proxyAppId":"com.example.acme","actionIds":[],"data":{"events":[
            {"method":"Runtime.consoleAPICalled","params":{"type":"info","args":[{"type":"string","value":"Acme ready"}]}},
            {"method":"Runtime.consoleAPICalled","params":{"type":"error","args":[{"type":"string","value":"Acme caught exception: Acme JavaScript fixture failure"}]}},
            {"method":"Log.entryAdded","params":{"entry":{"level":"error","text":"Acme error log"}}}
        ]}});
        let index = super::super::decode(
            "react-native-diagnostics-v1",
            &serde_json::to_vec(&packet).unwrap(),
        )
        .unwrap();
        let failures: Vec<_> = index
            .rows
            .iter()
            .filter(|row| row.kind == "failure")
            .collect();
        assert_eq!(failures.len(), 2);
        assert_eq!(
            failures[0].data["observation"]["event"]["params"]["type"],
            "error"
        );
        for row in failures {
            assert_eq!(row.data["causeEstablished"], false);
            assert_eq!(row.data["deviceAssociationVerified"], false);
            assert!(row.data["appId"].is_null());
        }
        assert!(index.rows.iter().any(|row| row.kind == "event"
            && row.data["observation"]["event"]["params"]["type"] == "info"));
    }
}
