//! Concrete runtime exports; no code, pickle, Marshal or vendor binary execution.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
fn array(v: &Value, max: usize) -> Result<&Vec<Value>> {
    v.as_array().filter(|a| a.len() <= max).ok_or_else(invalid)
}
fn number(v: &Value) -> Result<f64> {
    v.as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
        .ok_or_else(invalid)
}
pub(super) fn javascript(bytes: &[u8], index: &mut Index) -> Result<()> {
    let v = decode::bounded_json(bytes, MAX_RAW)?;
    let start = number(&v["startTime"])?;
    let end = number(&v["endTime"])?;
    if end < start {
        return Err(invalid());
    }
    let mut nodes = BTreeMap::new();
    let mut parents = BTreeMap::new();
    for node in array(&v["nodes"], 10000)? {
        let id = node["id"].as_u64().filter(|n| *n > 0).ok_or_else(invalid)?;
        if nodes.insert(id, node).is_some() || node["callFrame"]["functionName"].as_str().is_none()
        {
            return Err(invalid());
        }
        if let Some(children) = node.get("children") {
            for child in array(children, 10000)? {
                let child = child.as_u64().ok_or_else(invalid)?;
                if parents.insert(child, id).is_some() {
                    return Err(invalid());
                }
            }
        }
    }
    if parents.keys().any(|n| !nodes.contains_key(n)) {
        return Err(invalid());
    }
    // Validate every chain, including unreferenced cyclic nodes.
    for id in nodes.keys() {
        let mut seen = BTreeSet::new();
        let mut current = *id;
        loop {
            if !seen.insert(current) || seen.len() > 256 {
                return Err(invalid());
            }
            let Some(parent) = parents.get(&current) else {
                break;
            };
            current = *parent;
        }
    }
    let samples = array(&v["samples"], 10000)?;
    let deltas = v.get("timeDeltas").map(|a| array(a, 10000)).transpose()?;
    if deltas.is_some_and(|d| d.len() != samples.len()) {
        return Err(invalid());
    }
    index.clocks.push(Clock {
        id: "js-profile".into(),
        unit: "microseconds".into(),
        reference_domain: None,
        max_drift_ppm: None,
        anchors: vec![],
    });
    let mut position = start;
    for (n, sample) in samples.iter().enumerate() {
        let mut id = sample.as_u64().ok_or_else(invalid)?;
        let mut frames = vec![];
        loop {
            let node = nodes.get(&id).ok_or_else(invalid)?;
            frames.push(node["callFrame"].clone());
            let Some(parent) = parents.get(&id) else {
                break;
            };
            id = *parent;
        }
        frames.reverse();
        let delta = deltas.map(|a| number(&a[n])).transpose()?;
        let mut row = decode::row(
            "stack",
            "JavaScript CPU sample",
            format!("/samples/{n}"),
            json!({"frames":frames,"nodeId":sample,"profile":0,"startTime":start,"endTime":end,"timeDelta":delta,"symbolication":"producer names; transpiled source positions; maps not fetched","engine":"producer export; Bun compatibility does not imply V8 engine identity"}),
        );
        row.value = Some(1.0);
        row.unit = Some("samples".into());
        row.definition = Some(
            "recorded JavaScript sampled stack count; not CPU utilization or native/off-CPU time"
                .into(),
        );
        if let Some(delta) = delta {
            position += delta;
            if !position.is_finite() {
                return Err(invalid());
            }
            row.timestamp = Some(position);
            row.clock_domain = Some("js-profile".into());
        }
        decode::push(index, row)?;
    }
    index.unsupported.push("sampled JS profile; missing samples/native work/event-loop latency/source-map resolution remain unavailable".into());
    Ok(())
}
pub(super) fn jfr(bytes: &[u8], index: &mut Index) -> Result<()> {
    let v = decode::bounded_json(bytes, MAX_RAW)?;
    for (n, e) in array(&v["recording"]["events"], 10000)?.iter().enumerate() {
        let name = e["type"].as_str().ok_or_else(invalid)?;
        label(name)?;
        let values = e["values"].as_object().ok_or_else(invalid)?;
        let mut data = json!({"eventType":name,"values":values,"timeMeaning":"producer UTC strings and ISO durations; no numeric cross-clock alignment","symbolication":"producer method/class names; no JFR binary or symbol tool executed"});
        let mut kind = "event";
        let sampled = matches!(name, "jdk.ExecutionSample" | "jdk.NativeMethodSample");
        if let Some(trace) = values.get("stackTrace").filter(|t| !t.is_null()) {
            let mut frames = vec![];
            for frame in array(&trace["frames"], 256)? {
                frames.push(json!({"name":frame["method"]["name"],"class":frame["method"]["type"]["name"],"descriptor":frame["method"]["descriptor"],"line":frame["lineNumber"],"bytecodeIndex":frame["bytecodeIndex"],"type":frame["type"]}));
            }
            frames.reverse();
            data["frames"] = json!(frames);
            data["stackTruncated"] = trace["truncated"].clone();
            if sampled {
                kind = "stack";
            }
        }
        let mut row = decode::row(kind, name, format!("/recording/events/{n}"), data);
        if name.len() <= 240 {
            row.correlations.push(format!("event-type:{name}"));
        }
        if kind == "stack" {
            row.value = Some(1.0);
            row.unit = Some("samples".into());
            row.definition = Some(format!(
                "JFR {name} recorded event count; not event duration or CPU utilization"
            ));
        }
        if let Some(thread) = e["values"]["sampledThread"]["javaThreadId"]
            .as_u64()
            .or_else(|| e["values"]["eventThread"]["javaThreadId"].as_u64())
        {
            row.data["profile"] = json!(thread);
            row.correlations.push(format!("java-thread:{thread}"));
        }
        decode::push(index, row)?;
    }
    index.unsupported.push("selected jfr print --json export; GC/locks/allocation/I/O/async fields remain event-specific raw values; no complete recording, common heap or inferred duration".into());
    Ok(())
}
pub(super) fn ruby(bytes: &[u8], index: &mut Index) -> Result<()> {
    let v = decode::bounded_json(bytes, MAX_RAW)?;
    let mode = v["mode"].as_str().ok_or_else(invalid)?;
    if v["version"].as_f64() != Some(1.0) || !matches!(mode, "cpu" | "wall" | "object" | "custom") {
        return Err(invalid());
    }
    let frames = v["frames"]
        .as_object()
        .filter(|a| a.len() <= 10000)
        .ok_or_else(invalid)?;
    number(&v["samples"])?;
    for (id, frame) in frames {
        label(id)?;
        let name = frame["name"].as_str().ok_or_else(invalid)?;
        if name.len() > 1024 {
            return Err(invalid());
        }
        let mut row = decode::row(
            "metric",
            if name.is_empty() {
                "anonymous Ruby frame"
            } else {
                name
            },
            format!("/frames/{id}"),
            json!({"frameId":id,"mode":mode,"frame":frame,"interval":v["interval"],"missedSamples":v["missed_samples"],"scope":"exclusive frame weight; call graph edges are not reconstructed full stacks"}),
        );
        row.value = Some(number(&frame["samples"])?);
        row.unit = Some(
            if mode == "object" {
                "sampled_objects"
            } else {
                "samples"
            }
            .into(),
        );
        row.definition = Some(format!(
            "StackProf {mode} exclusive frame samples; inclusive totals and edges overlap"
        ));
        decode::push(index, row)?;
        if let Some(edges) = frame.get("edges") {
            for (callee, weight) in edges
                .as_object()
                .filter(|a| a.len() <= 10000)
                .ok_or_else(invalid)?
            {
                if !frames.contains_key(callee) {
                    return Err(invalid());
                }
                number(weight)?;
            }
        }
    }
    index.unsupported.push("StackProf public JSON data only; Marshal is not decoded; sampled frame/edge weights do not establish wall duration, allocation bytes or complete stacks".into());
    Ok(())
}
fn compressed(text: &str, names: &mut BTreeMap<String, String>) -> Result<String> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix('(') {
        let (id, name) = rest.split_once(')').ok_or_else(invalid)?;
        if !id.bytes().all(|c| c.is_ascii_digit()) || id.is_empty() {
            return Err(invalid());
        }
        if name.trim().is_empty() {
            return names.get(id).cloned().ok_or_else(invalid);
        }
        label(id)?;
        crate::diagnostics::model::label(name.trim(), 1024)?;
        names.insert(id.into(), name.trim().into());
        return Ok(name.trim().into());
    }
    crate::diagnostics::model::label(text, 1024)?;
    Ok(text.into())
}
pub(super) fn php(bytes: &[u8], index: &mut Index) -> Result<()> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut costs = Cachegrind::default();
    let mut files = BTreeMap::new();
    let mut functions = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.len() > 8192 {
            return Err(invalid());
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(v) = line.strip_prefix("events:") {
            if !costs.events.is_empty() {
                return Err(invalid());
            }
            costs.events = v.split_whitespace().map(str::to_owned).collect();
            if costs.events.is_empty() || costs.events.len() > 8 {
                return Err(invalid());
            }
            continue;
        }
        if let Some(v) = line.strip_prefix("positions:") {
            if v.trim() != "line" {
                return Err(invalid());
            }
            continue;
        }
        if let Some(v) = line.strip_prefix("fl=") {
            costs.file = compressed(v, &mut files)?;
            continue;
        }
        if let Some(v) = line.strip_prefix("fn=") {
            if costs.calls.is_some() {
                return Err(invalid());
            }
            costs.function = compressed(v, &mut functions)?;
            costs.callee.clear();
            continue;
        }
        if let Some(v) = line.strip_prefix("cfn=") {
            costs.callee = compressed(v, &mut functions)?;
            continue;
        }
        if let Some(v) = line
            .strip_prefix("cfl=")
            .or_else(|| line.strip_prefix("cfi="))
        {
            compressed(v, &mut files)?;
            continue;
        }
        if let Some(v) = line.strip_prefix("calls=") {
            if costs.calls.is_some() || costs.callee.is_empty() {
                return Err(invalid());
            }
            costs.calls = Some(
                v.split_whitespace()
                    .next()
                    .ok_or_else(invalid)?
                    .parse::<u64>()
                    .map_err(|_| invalid())?,
            );
            continue;
        }
        if line.contains(':') {
            continue;
        } // Header metadata remains in raw evidence.
        if line.starts_with("ob=") || line.starts_with("cob=") {
            continue;
        }
        if line.contains('=') {
            return Err(invalid());
        }
        costs.record(line, n, index)?;
    }
    if costs.events.is_empty() || costs.calls.is_some() {
        return Err(invalid());
    }
    index.unsupported.push("selected Cachegrind/Callgrind line-position dialect; cycles are not expanded into stacks; event units remain explicit; no PHP execution or profiler activation".into());
    Ok(())
}

#[derive(Default)]
struct Cachegrind {
    events: Vec<String>,
    file: String,
    function: String,
    callee: String,
    calls: Option<u64>,
    last_line: i64,
}
impl Cachegrind {
    fn record(&mut self, line: &str, n: usize, index: &mut Index) -> Result<()> {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.is_empty()
            || fields.len() > self.events.len() + 1
            || self.events.is_empty()
            || self.function.is_empty()
        {
            return Err(invalid());
        }
        self.last_line = match fields[0] {
            "*" => self.last_line,
            s if s.starts_with('+') || s.starts_with('-') => self
                .last_line
                .checked_add(s.parse::<i64>().map_err(|_| invalid())?)
                .ok_or_else(invalid)?,
            s => s.parse::<i64>().map_err(|_| invalid())?,
        };
        if self.last_line < 0 {
            return Err(invalid());
        }
        for (column, event) in self.events.iter().enumerate() {
            let count = fields
                .get(column + 1)
                .map(|s| s.parse::<i64>().map_err(|_| invalid()))
                .transpose()?
                .unwrap_or(0);
            if count.unsigned_abs() > 1 << 53 || count < 0 && event != "Memory_(bytes)" {
                return Err(invalid());
            }
            let mut row = decode::row(
                "metric",
                event,
                format!("/lines/{}", n + 1),
                json!({"file":self.file,"function":self.function,"line":self.last_line,"callee":if self.calls.is_some(){Some(&self.callee)}else{None},"calls":self.calls,"costKind":if self.calls.is_some(){"inclusive_call_cost"}else{"exclusive_self_cost"},"nativeEvent":event}),
            );
            row.value = Some(count as f64);
            row.unit = Some(
                match event.as_str() {
                    "Time_(10ns)" => "10_nanoseconds",
                    "Memory_(bytes)" => "bytes",
                    _ => "producer_event_count",
                }
                .into(),
            );
            row.definition = Some(format!(
                "Xdebug/Callgrind {event} {}; call and self costs are not additive",
                if self.calls.is_some() {
                    "inclusive call"
                } else {
                    "exclusive self"
                }
            ));
            decode::push(index, row)?;
        }
        self.calls = None;
        Ok(())
    }
}
