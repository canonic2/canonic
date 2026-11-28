//! Native producer projections. No off-CPU inference from samples or symbol guesses.
use super::*;
use crate::diagnostics::native_trace::{
    ACTIVITY_MONITOR, ACTIVITY_MONITOR_FORMAT, ACTIVITY_MONITOR_TABLE,
};
use base64::Engine;
use std::collections::{BTreeMap, BTreeSet};
pub(crate) fn artifact_bytes(raw: &[u8]) -> Result<(Vec<u8>, String)> {
    let bytes = if raw.starts_with(&[0x1f, 0x8b]) {
        let mut bytes = vec![];
        flate2::read::MultiGzDecoder::new(raw)
            .take((MAX_RAW + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        if bytes.len() > MAX_RAW {
            return Err(Error::new(
                "ANALYSIS_BOUNDS",
                "Expanded native envelope exceeds 8 MiB",
            ));
        }
        bytes
    } else {
        raw.to_vec()
    };
    artifact(&bounded_json(&bytes, MAX_RAW)?)
}
mod xml;
pub(crate) use xml::{activity_monitor_export, recording_summary};
pub(crate) fn memory_tables(bytes: &[u8]) -> Result<Vec<String>> {
    xml::memory_tables(bytes)
}

pub(crate) fn artifact(value: &Value) -> Result<(Vec<u8>, String)> {
    if value["schemaVersion"] != 1
        || !matches!(
            value["backend"].as_str(),
            Some("perfetto" | "simpleperf" | "xctrace" | "stack_sample")
        )
    {
        return Err(invalid());
    }
    let a = &value["artifact"];
    let encoding = text(&a["encoding"])?;
    if !matches!(encoding, "identity" | "tar")
        && !(encoding == "tar.gz"
            && value["backend"] == "xctrace"
            && value["report"]["template"] == ACTIVITY_MONITOR)
    {
        return Err(invalid());
    }
    let encoded = text(&a["base64"])?;
    if encoded.len() > 6 * 1024 * 1024 {
        return Err(invalid());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| invalid())?;
    if bytes.len() > crate::diagnostics::native_trace::MAX_ARTIFACT
        || encoding == "tar.gz" && !bytes.starts_with(&[0x1f, 0x8b, 8])
        || crate::diagnostics::digest(&bytes) != text(&a["sha256"])?
    {
        return Err(invalid());
    }
    Ok((bytes, encoding.into()))
}
#[expect(
    clippy::too_many_arguments,
    reason = "A trace row keeps its index, envelope, identity, location, time, measurement and observation explicit at the emission boundary"
)]
fn emit(
    index: &mut Index,
    envelope: &Value,
    kind: &str,
    name: &str,
    path: String,
    time: Option<f64>,
    measurement: Option<(f64, &str, &str)>,
    observation: Value,
) -> Result<()> {
    let (pointer, location) = if let Some((prefix, record)) = path.split_once("/text/") {
        (format!("{prefix}/text"), Some(record.to_owned()))
    } else {
        (path.clone(), None)
    };
    let mut r = row(
        kind,
        name,
        pointer,
        json!({"appId":null,"declaredAppId":envelope["declaredAppId"],"targetId":envelope["targetId"],"backend":envelope["backend"],"identity":envelope["report"]["identity"],"observation":observation,"resolved":false,"causeEstablished":false}),
    );
    if let Some(location) = location {
        r.data["producerTextRecord"] = json!({"locator":location,"ordinalBase":0,"meaning":"record/line inside the source text, not an additional JSON path"});
    }
    if let Some(time) = time {
        r.timestamp = Some(time);
        r.clock_domain = Some(format!("native:{}", envelope["backend"].as_str().unwrap()));
    }
    r.correlations =
        serde_json::from_value(envelope.get("actionIds").cloned().unwrap_or(json!([])))
            .map_err(|_| invalid())?;
    if let Some((v, u, d)) = measurement {
        if !v.is_finite() || v < 0.0 {
            return Err(invalid());
        }
        r.value = Some(v);
        r.unit = Some(u.into());
        r.definition = Some(d.into());
    }
    push(index, r)
}
// RFC 4180 quoting including embedded newlines; no truncation or split(',') loss.
fn csv(input: &str) -> Result<Vec<BTreeMap<String, String>>> {
    let mut rows = vec![];
    let mut fields = vec![];
    let mut field = String::new();
    let mut chars = input.chars().peekable();
    let mut quoted = false;
    let mut closed = false;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                field.push(c);
            }
        } else {
            match c {
                '"' if field.is_empty() && !closed => quoted = true,
                ',' => {
                    fields.push(std::mem::take(&mut field));
                    closed = false;
                }
                '\n' => {
                    fields.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut fields));
                    closed = false;
                }
                '\r' if chars.peek() == Some(&'\n') => {}
                _ if closed => return Err(invalid()),
                _ => field.push(c),
            }
        }
        if field.len() > 32768 || fields.len() > 32 || rows.len() > 4098 {
            return Err(invalid());
        }
    }
    if quoted {
        return Err(invalid());
    }
    if !field.is_empty() || !fields.is_empty() || closed {
        fields.push(field);
        rows.push(fields);
    }
    if rows.is_empty() {
        return Err(invalid());
    }
    let headers = rows.remove(0);
    if headers.iter().collect::<BTreeSet<_>>().len() != headers.len() {
        return Err(invalid());
    }
    rows.into_iter()
        .filter(|r| r != &vec![String::new()])
        .map(|r| {
            if r.len() != headers.len() {
                Err(invalid())
            } else {
                Ok(headers.iter().cloned().zip(r).collect())
            }
        })
        .collect()
}
fn n(row: &BTreeMap<String, String>, key: &str) -> Result<f64> {
    row.get(key)
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .ok_or_else(invalid)
}
fn s<'a>(row: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str> {
    row.get(key).map(String::as_str).ok_or_else(invalid)
}
pub(super) fn decode(bytes: &[u8], index: &mut Index) -> Result<()> {
    let envelope = bounded_json(bytes, MAX_RAW)?;
    artifact(&envelope)?;
    let backend = text(&envelope["backend"])?;
    index.clocks.push(Clock {
        id: format!("native:{backend}"),
        unit: "nanoseconds".into(),
        reference_domain: None,
        max_drift_ppm: None,
        anchors: vec![],
    });
    index.unsupported.push("Native evidence is partial and caller/producer scoped; missing rows do not prove absence; app/device association and external symbol resolution are not authenticated".into());
    if let Some(gaps) = envelope["report"]["gaps"].as_array() {
        for gap in gaps.iter().take(32) {
            index.unsupported.push(gap.to_string());
        }
    }
    let sections = array(&envelope["sections"])?;
    if sections.len() > 16 {
        return Err(invalid());
    }
    let mut tables = BTreeMap::new();
    for (i, section) in sections.iter().enumerate() {
        let name = text(&section["name"])?;
        let content = text(&section["text"])?;
        if content.len() > 1024 * 1024 {
            return Err(invalid());
        }
        match section["format"].as_str() {
            Some("csv") if backend == "perfetto" => {
                if tables.insert(name, (i, csv(content)?)).is_some() {
                    return Err(invalid());
                }
            }
            Some("simpleperf-text") if backend == "simpleperf" => {
                simpleperf(index, &envelope, content, i)?
            }
            Some("xml") if backend == "xctrace" => xml::decode(index, &envelope, section, i)?,
            Some(ACTIVITY_MONITOR_FORMAT)
                if backend == "xctrace" && name == ACTIVITY_MONITOR_TABLE =>
            {
                xml::decode_activity_monitor(index, &envelope, content, i)?;
            }
            Some("sample-text") if backend == "stack_sample" => {
                sample(index, &envelope, content, i)?
            }
            _ => return Err(invalid()),
        }
    }
    if backend == "perfetto" {
        perfetto(index, &envelope, &tables)?;
    }
    if sections.is_empty() {
        index
            .unsupported
            .push("No structured exports were available; native artifact remains retained".into());
    }
    Ok(())
}
type Tables<'a> = BTreeMap<&'a str, (usize, Vec<BTreeMap<String, String>>)>;
type Lookup<'a> = BTreeMap<String, &'a BTreeMap<String, String>>;
fn perfetto(index: &mut Index, e: &Value, tables: &Tables<'_>) -> Result<()> {
    let lookup = |name| -> Result<Lookup<'_>> {
        let mut out = BTreeMap::new();
        if let Some((_, rows)) = tables.get(name) {
            for row in rows {
                let id = s(row, "id")?.to_owned();
                if out.insert(id, row).is_some() {
                    return Err(invalid());
                }
            }
        }
        Ok(out)
    };
    let callsites = lookup("callsites")?;
    let frame_map = lookup("frames")?;
    for (name, (section, rows)) in tables {
        let limit = match *name {
            "processes" | "threads" => 2048,
            "loss" => 1024,
            _ => 4096,
        };
        if rows.len() > limit {
            index.unsupported.push(format!(
                "Perfetto {name} export reached its {limit} row limit; remainder not indexed"
            ));
        }
        for (i, r) in rows.iter().take(limit).enumerate() {
            let path = format!("/sections/{section}/text/row/{i}");
            let data = json!({"fields":r,"table":name,"traceProcessorScope":"system_or_user_data_as_recorded; UTID/UPID identify trace lifetimes, not OS PID alone"});
            match *name {
                "processes" => emit(
                    index,
                    e,
                    "process",
                    r.get("name").map(String::as_str).unwrap_or("unnamed"),
                    path,
                    None,
                    None,
                    data,
                )?,
                "threads" => emit(
                    index,
                    e,
                    "thread",
                    r.get("name").map(String::as_str).unwrap_or("unnamed"),
                    path,
                    None,
                    None,
                    data,
                )?,
                "scheduling" | "slices" => perfetto_interval(index, e, name, r, path, data, i)?,
                "actual_frames" | "expected_frames" => {
                    perfetto_frame(index, e, name, r, path, data, i)?
                }
                "loss" => perfetto_loss(index, e, r, path, data)?,
                "bounds" => emit(index, e, "event", "trace_bounds", path, None, None, data)?,
                "cpu_samples" => perfetto_cpu_sample(index, e, r, path, &callsites, &frame_map)?,
                "callsites" | "frames" => {}
                _ => return Err(invalid()),
            }
        }
    }
    Ok(())
}

fn perfetto_interval(
    index: &mut Index,
    e: &Value,
    name: &str,
    r: &BTreeMap<String, String>,
    path: String,
    data: Value,
    i: usize,
) -> Result<()> {
    let dur = n(r, "dur")?;
    let time = n(r, "ts")?;
    if dur < 0.0 {
        index
            .unsupported
            .push(format!("unfinished {name} interval retained raw at {i}"));
        return Ok(());
    }
    let (label, meaning) = if name == "scheduling" {
        (
            s(r, "state")?,
            "recorded thread-state duration; Running=on CPU, R/R+=runnable, S/D=blocked state; io_wait/blocked_function are producer hints, not proven wait causes",
        )
    } else {
        (
            s(r, "name")?,
            "recorded user/system slice duration; overlapping/nested slices are not additive; names alone do not prove locks, IPC, network, disk or GC causality",
        )
    };
    emit(
        index,
        e,
        "span",
        if label.is_empty() {
            "unnamed interval"
        } else {
            label
        },
        path,
        Some(time),
        Some((dur, "nanoseconds", meaning)),
        data,
    )?;
    Ok(())
}

fn perfetto_frame(
    index: &mut Index,
    e: &Value,
    name: &str,
    r: &BTreeMap<String, String>,
    path: String,
    data: Value,
    i: usize,
) -> Result<()> {
    let dur = n(r, "dur")?;
    let time = n(r, "ts")?;
    if dur < 0.0 {
        index
            .unsupported
            .push(format!("unfinished {name} frame retained raw at {i}"));
        return Ok(());
    }
    emit(
        index,
        e,
        "frame",
        name,
        path,
        Some(time),
        Some((
            dur,
            "nanoseconds",
            if name == "actual_frames" {
                "recorded Android FrameTimeline actual interval; producer jank/present type and layer/UPID preserved; not GPU execution or an independently verified displayed frame"
            } else {
                "recorded Android FrameTimeline expected interval; per-frame producer budget, not actual work duration"
            },
        )),
        data,
    )?;
    Ok(())
}

fn perfetto_loss(
    index: &mut Index,
    e: &Value,
    r: &BTreeMap<String, String>,
    path: String,
    data: Value,
) -> Result<()> {
    let value = n(r, "value")?;
    if r.get("severity")
        .is_some_and(|v| v == "error" || v == "data_loss")
    {
        index.unsupported.push(format!(
            "Perfetto loss/error statistic {}={value}",
            s(r, "name")?
        ));
    }
    emit(
        index,
        e,
        "metric",
        s(r, "name")?,
        path,
        None,
        Some((
            value,
            "count",
            "producer trace statistic; inspect severity/name; nonzero is not always data loss",
        )),
        data,
    )?;
    Ok(())
}

fn perfetto_cpu_sample(
    index: &mut Index,
    e: &Value,
    r: &BTreeMap<String, String>,
    path: String,
    callsites: &Lookup<'_>,
    frame_map: &Lookup<'_>,
) -> Result<()> {
    let mut frames = vec![];
    let mut current = s(r, "callsite_id")?.to_owned();
    let mut seen = BTreeSet::new();
    while current != "[NULL]" && !current.is_empty() {
        if frames.len() >= 128 || !seen.insert(current.clone()) {
            return Err(invalid());
        }
        let call = callsites.get(&current);
        let Some(call) = call else {
            index
                .unsupported
                .push("Missing/truncated Perfetto callsite; raw sample retained".into());
            frames.push(json!({"name":"unavailable callsite","resolved":false,"incomplete":true}));
            break;
        };
        let frame = call.get("frame_id").and_then(|id| frame_map.get(id));
        let Some(frame) = frame else {
            index
                .unsupported
                .push("Missing/truncated Perfetto frame; raw sample retained".into());
            frames.push(json!({"name":"unavailable frame","resolved":false,"incomplete":true}));
            break;
        };
        frames.push(json!({"name":frame.get("name"),"address":frame.get("rel_pc"),"image":frame.get("mapping_name"),"buildId":frame.get("build_id"),"resolved":false}));
        current = s(call, "parent_id")?.into();
    }
    frames.reverse();
    emit(
        index,
        e,
        "stack",
        "native CPU sample",
        path,
        Some(n(r, "ts")?),
        Some((
            1.0,
            "samples",
            "recorded native CPU sample; not elapsed time or off-CPU evidence",
        )),
        json!({"fields":r,"framesRootFirst":frames,"sampleKind":"perf_sample","weightUnit":"samples","kernelCoverage":"producer_defined"}),
    )?;
    Ok(())
}
fn simpleperf(index: &mut Index, e: &Value, input: &str, section: usize) -> Result<()> {
    let mut fields = BTreeMap::new();
    let mut frames: Vec<Value> = vec![];
    let mut frame = serde_json::Map::new();
    let mut sample = false;
    let mut ordinal = 0;
    fn finish(
        index: &mut Index,
        e: &Value,
        fields: &BTreeMap<String, String>,
        frames: &[Value],
        ordinal: usize,
        section: usize,
    ) -> Result<()> {
        if fields.is_empty() {
            return Ok(());
        }
        let time = n(fields, "time")?;
        let period = n(fields, "event_count")?;
        if period < 0.0 || frames.len() > 128 {
            return Err(invalid());
        }
        let mut frames = frames.to_vec();
        frames.reverse();
        emit(
            index,
            e,
            "stack",
            "simpleperf recorded sample",
            format!("/sections/{section}/text/sample/{ordinal}"),
            Some(time),
            Some((
                1.0,
                "samples",
                "one recorded simpleperf sample; event_count is separately retained, not off-CPU or wall time",
            )),
            json!({"fields":fields,"framesRootFirst":frames,"eventPeriod":period,"eventType":fields.get("event_type"),"sampleKind":"simpleperf","symbolProvenance":"producer names; no external symbol identity verified","weightUnit":"samples"}),
        )
    }
    for line in input.lines().chain(std::iter::once("__end__")) {
        if !line.starts_with(' ') {
            if sample {
                if !frame.is_empty() {
                    frames.push(Value::Object(std::mem::take(&mut frame)));
                }
                finish(index, e, &fields, &frames, ordinal, section)?;
                ordinal += 1;
            }
            fields.clear();
            frames.clear();
            sample = line == "sample:";
            if ordinal > 4096 {
                return Err(Error::new(
                    "ANALYSIS_BOUNDS",
                    "simpleperf exceeds 4096 samples",
                ));
            }
            if line == "lost_situation:" {
                index.unsupported.push("simpleperf lost_situation retained in producer report; sampling never proves complete coverage".into());
            }
            continue;
        }
        if sample {
            if let Some((key, value)) = line.trim().split_once(": ") {
                if key == "vaddr_in_file" && !frame.is_empty() {
                    frames.push(Value::Object(std::mem::take(&mut frame)));
                }
                if matches!(key, "vaddr_in_file" | "file" | "symbol" | "execution_type") {
                    let key = match key {
                        "symbol" => "name",
                        "file" => "image",
                        other => other,
                    };
                    frame.insert(key.into(), json!(value));
                } else {
                    fields.insert(key.to_owned(), value.to_owned());
                }
                if frames.len() > 128 {
                    return Err(invalid());
                }
            }
        } else if line.trim().starts_with("lost_count:") {
            emit(
                index,
                e,
                "event",
                "simpleperf loss",
                format!("/sections/{section}/text/loss"),
                None,
                None,
                json!({"producer":line.trim()}),
            )?;
        }
    }
    if ordinal == 0 {
        index
            .unsupported
            .push("No recognized simpleperf sample records; report remains raw".into());
    }
    Ok(())
}
fn sample(index: &mut Index, e: &Value, input: &str, section: usize) -> Result<()> {
    // sample's indented call graph counts are inclusive. Preserve paths/counts
    // without pretending each line is an independent CPU sample.
    let mut count = 0;
    for (line, text) in input.lines().enumerate() {
        if text.contains("Call graph:")
            || text.contains("Thread_")
            || text.contains("Sampling process")
            || text.contains("Analysis of sampling")
            || text.contains("Sort by top of stack")
            || text.contains("Process:")
            || text.contains("Code Type:")
            || text.contains("Sampling interval:")
            || text.contains("Total number in stack")
            || text.contains("Binary Images:")
        {
            emit(
                index,
                e,
                "event",
                "hang sample context",
                format!("/sections/{section}/text/line/{line}"),
                None,
                None,
                json!({"text":text,"pauseStatus":e["report"]["identity"]["pausedBeforeAttach"],"freezeEstablished":false,"attachMayPerturb":true}),
            )?;
        }
        let trimmed = text.trim_start_matches([' ', '+', '!', '|', ':']);
        if let Some((value, frame)) = trimmed.split_once(' ')
            && let Ok(value) = value.parse::<u64>()
            && !frame.trim().is_empty()
        {
            emit(
                index,
                e,
                "stack",
                "hang call graph observation",
                format!("/sections/{section}/text/line/{line}"),
                None,
                Some((
                    value as f64,
                    "count",
                    "inclusive producer sample call-graph count; rows overlap and cannot be summed as independent samples",
                )),
                json!({"text":text,"sampleKind":"inclusive_hang_callgraph","framesRootFirst":null,"resolved":false,"freezeEstablished":false}),
            )?;
            count += 1;
        }
    }
    if count == 0 {
        index
            .unsupported
            .push("No recognized sample call graph; native text remains raw".into());
    }
    Ok(())
}
