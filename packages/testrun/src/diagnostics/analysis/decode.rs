//! Bounded decoders for selected public formats; unsupported binaries are refused.
mod android;
mod apple;
mod browser;
mod dart;
pub(crate) mod native_trace;
mod react_native;
use super::model::*;
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::io::Read;

fn invalid() -> Error {
    Error::new(
        "INVALID_DIAGNOSTIC_FORMAT",
        "Artifact does not match the selected diagnostic format",
    )
}
fn array(value: &Value) -> Result<&Vec<Value>> {
    value.as_array().ok_or_else(invalid)
}
fn number(value: &Value) -> Result<f64> {
    value.as_f64().filter(|v| v.is_finite()).ok_or_else(invalid)
}
fn text(value: &Value) -> Result<&str> {
    value.as_str().ok_or_else(invalid)
}

// Scan before allocating a JSON tree. Strings are bounded by the byte limit;
// depth and structural tokens bound object overhead independently of bytes.
pub fn bounded_json(bytes: &[u8], max: usize) -> Result<Value> {
    bounded_json_tokens(bytes, max, 300_000)
}
/// Like [`bounded_json`] with a caller-selected structural token budget.
pub fn bounded_json_tokens(bytes: &[u8], max: usize, budget: usize) -> Result<Value> {
    if bytes.len() > max {
        return Err(Error::new(
            "ANALYSIS_BOUNDS",
            "Diagnostic input exceeds its byte budget",
        ));
    }
    let (mut depth, mut tokens, mut string, mut escape) = (0u32, 0usize, false, false);
    for &b in bytes {
        if string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                string = false;
            }
        } else {
            match b {
                b'"' => {
                    string = true;
                    tokens += 1;
                }
                b'{' | b'[' => {
                    depth += 1;
                    tokens += 1;
                }
                b'}' | b']' => {
                    depth = depth.saturating_sub(1);
                }
                b',' | b':' => tokens += 1,
                _ => {}
            }
        }
        if depth > 64 || tokens > budget {
            return Err(Error::new(
                "ANALYSIS_BOUNDS",
                "Diagnostic JSON exceeds depth or object budget",
            ));
        }
    }
    serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
        .map_err(|_| invalid())
}
pub fn decode(format: &str, raw: &[u8]) -> Result<Index> {
    if raw.len() > MAX_RAW {
        return Err(Error::new("ANALYSIS_BOUNDS", "Raw evidence exceeds 8 MiB"));
    }
    if !SUPPORTED_FORMATS.contains(&format) {
        return Err(Error::new(
            "UNSUPPORTED_DIAGNOSTIC_FORMAT",
            "Select a format from diagnostics_capabilities; native binary formats require their own decoder",
        ));
    }
    let expanded;
    let bytes = if raw.starts_with(&[0x1f, 0x8b]) {
        let mut decoder = flate2::read::MultiGzDecoder::new(raw).take((MAX_RAW + 1) as u64);
        let mut output = Vec::new();
        decoder.read_to_end(&mut output).map_err(|_| invalid())?;
        if output.len() > MAX_RAW {
            return Err(Error::new(
                "ANALYSIS_BOUNDS",
                "Expanded diagnostic evidence exceeds 8 MiB",
            ));
        }
        expanded = output;
        &expanded
    } else {
        raw
    };
    let mut index = Index {
        model_bytes: 0,
        version: 1,
        parser: PARSER.into(),
        format: format.into(),
        clocks: vec![],
        rows: vec![],
        unsupported: vec![],
    };
    decode_into(format, bytes, &mut index)?;
    for r in &index.rows {
        if let Some(domain) = &r.clock_domain
            && !index.clocks.iter().any(|c| &c.id == domain)
        {
            return Err(Error::new(
                "CLOCK_UNAVAILABLE",
                "Record timestamp references an undeclared clock",
            ));
        }
    }
    let output = serde_json::to_vec(&index).map_err(|_| invalid())?;
    if output.len() > MAX_INDEX {
        return Err(Error::new(
            "ANALYSIS_BOUNDS",
            "Derived index exceeds 16 MiB",
        ));
    }
    Ok(index)
}

const SUPPORTED_FORMATS: &[&str] = &[
    "testrun-events-v1",
    "chrome-trace",
    "har",
    "speedscope",
    "android-diagnostics-v1",
    "android-tombstone",
    "android-anr",
    "android-bugreport-text",
    "apple-diagnostics-v1",
    "apple-ips",
    "apple-crash-text",
    "apple-jetsam",
    "apple-metrickit",
    "apple-sysdiagnose-text",
    "browser-operation-v1",
    "react-native-diagnostics-v1",
    "dart-diagnostics-v1",
    "flutter-app-size-v1",
    "native-trace-v1",
    "java-hprof",
    "memory-graph-v1",
    "memory-series-v1",
    "ui-performance-v1",
    "package-size-v1",
    "network-evidence-v1",
    "data-snapshot-v1",
    "pcap",
    "scenario-context-v1",
    "runtime-checks-v1",
    "sanitizer-report-text",
    "reproduction-v1",
    "test-results-v1",
    "junit-xml",
    "framework-evidence-v1",
    "runtime-samples-v1",
    "embedded-cdp-v1",
    "service-evidence-v1",
    "observability-evidence-v1",
    "otlp-json",
    "prometheus-json",
    "loki-json",
    "sentry-json",
    "jaeger-json",
    "cloudwatch-json",
    "crashlytics-json",
    "play-vitals-json",
    "deployment-evidence-v1",
    "host-process-v1",
    "box-log-v1",
    "js-cpu-profile",
    "jfr-json",
    "pprof",
    "stackprof-json",
    "xdebug-cachegrind",
    "windows-minidump",
];

fn decode_into(format: &str, bytes: &[u8], index: &mut Index) -> Result<()> {
    if matches!(
        format,
        "observability-evidence-v1"
            | "otlp-json"
            | "prometheus-json"
            | "loki-json"
            | "sentry-json"
            | "jaeger-json"
            | "cloudwatch-json"
            | "crashlytics-json"
            | "play-vitals-json"
            | "deployment-evidence-v1"
    ) {
        super::observability::decode(format, bytes, index)?;
    } else if format == "service-evidence-v1" {
        super::services::decode(bytes, index)?;
    } else if matches!(
        format,
        "host-process-v1"
            | "box-log-v1"
            | "js-cpu-profile"
            | "jfr-json"
            | "pprof"
            | "stackprof-json"
            | "xdebug-cachegrind"
            | "windows-minidump"
    ) {
        super::services::artifact(format, bytes, index)?;
    } else if format == "framework-evidence-v1" {
        super::frameworks::decode(bytes, index)?;
    } else if format == "runtime-samples-v1" {
        super::frameworks::samples(bytes, index)?;
    } else if format == "embedded-cdp-v1" {
        super::frameworks::embedded(bytes, index)?;
    } else if matches!(format, "test-results-v1" | "junit-xml") {
        super::test_results::decode(format, bytes, index)?;
    } else if format == "reproduction-v1" {
        super::reproduction::decode(bytes, index)?;
    } else if format == "sanitizer-report-text" {
        super::runtime_checks::sanitizer_text(bytes, index)?;
    } else if format == "runtime-checks-v1" {
        super::runtime_checks::decode(bytes, index)?;
    } else if format == "scenario-context-v1" {
        super::scenario::decode(bytes, index)?;
    } else if matches!(format, "network-evidence-v1" | "data-snapshot-v1" | "pcap") {
        super::network_data::decode(format, bytes, index)?;
    } else if matches!(format, "ui-performance-v1" | "package-size-v1") {
        super::performance::decode(format, bytes, index)?;
    } else if matches!(
        format,
        "java-hprof" | "memory-graph-v1" | "memory-series-v1"
    ) {
        super::memory::decode(format, bytes, index)?;
    } else if format == "native-trace-v1" {
        native_trace::decode(bytes, index)?;
    } else if format == "dart-diagnostics-v1" {
        dart::decode(bytes, index)?;
    } else if format == "flutter-app-size-v1" {
        dart::app_size(bytes, index)?;
    } else if format == "react-native-diagnostics-v1" {
        react_native::decode(bytes, index)?;
    } else if format == "browser-operation-v1" {
        browser::decode(bytes, index)?;
    } else if format.starts_with("apple-") {
        apple::decode(format, bytes, index)?;
    } else if format.starts_with("android-") {
        android::decode(format, bytes, index)?;
    } else {
        let value = bounded_json(bytes, MAX_RAW)?;
        match format {
            "testrun-events-v1" => {
                decode_events(&value, index)?;
            }
            "chrome-trace" => chrome(&value, index)?,
            "har" => har(&value, index)?,
            "speedscope" => speedscope(&value, index)?,
            _ => unreachable!(),
        }
    }
    Ok(())
}

fn decode_events(value: &Value, index: &mut Index) -> Result<()> {
    if value["schemaVersion"] != 1 {
        return Err(invalid());
    }
    index.clocks =
        serde_json::from_value(value.get("clocks").cloned().unwrap_or_else(|| json!([])))
            .map_err(|_| invalid())?;
    Metadata {
        clocks: index.clocks.clone(),
        ..Metadata::default()
    }
    .validate()?;
    for (i, v) in array(&value["records"])?.iter().enumerate() {
        let mut row: Row = serde_json::from_value(v.clone()).map_err(|_| invalid())?;
        row.source = format!("/records/{i}");
        push(index, row)?;
    }
    Ok(())
}
pub(super) fn push(index: &mut Index, row: Row) -> Result<()> {
    row.validate()?;
    let size = serde_json::to_vec(&row).map_err(|_| invalid())?.len();
    if size > 65536 || index.model_bytes.saturating_add(size) > MAX_INDEX - 65536 {
        return Err(Error::new(
            "ANALYSIS_BOUNDS",
            "Decoded row or cumulative index exceeds its byte budget",
        ));
    }
    if index.rows.len() >= MAX_ROWS {
        return Err(Error::new(
            "ANALYSIS_BOUNDS",
            "Diagnostic index exceeds 20000 rows",
        ));
    }
    index.model_bytes += size;
    index.rows.push(row);
    Ok(())
}
pub(super) fn row(kind: &str, name: &str, source: String, data: Value) -> Row {
    Row {
        kind: kind.into(),
        name: name.into(),
        source,
        data,
        timestamp: None,
        clock_domain: None,
        value: None,
        unit: None,
        definition: None,
        correlations: vec![],
    }
}
fn chrome(v: &Value, index: &mut Index) -> Result<()> {
    index.clocks.push(Clock {
        id: "trace".into(),
        unit: "microseconds".into(),
        reference_domain: None,
        max_drift_ppm: None,
        anchors: vec![],
    });
    let (events, prefix) = if v.is_array() {
        (array(v)?, "")
    } else {
        (array(&v["traceEvents"])?, "/traceEvents")
    };
    for (i, e) in events.iter().enumerate() {
        let phase = text(&e["ph"])?;
        if phase.len() != 1 || !phase.as_bytes()[0].is_ascii_graphic() {
            return Err(invalid());
        }
        let kind = match phase {
            "X" => "span",
            "M" if matches!(
                e["name"].as_str(),
                Some("thread_name" | "thread_sort_index")
            ) =>
            {
                "thread"
            }
            "M" if matches!(
                e["name"].as_str(),
                Some("process_name" | "process_labels" | "process_sort_index")
            ) =>
            {
                "process"
            }
            "i" | "I" | "R" => "marker",
            _ => "event",
        };
        let mut r = row(kind, text(&e["name"])?, format!("{prefix}/{i}"), e.clone());
        if let Some(ts) = e.get("ts") {
            r.timestamp = Some(number(ts)?);
            r.clock_domain = Some("trace".into());
        }
        if phase == "X" {
            let n = number(&e["dur"])?;
            if n < 0.0 {
                return Err(invalid());
            }
            r.value = Some(n);
            r.unit = Some("microseconds".into());
            r.definition =
                Some("recorded complete-event duration; overlapping spans are not additive".into());
        }
        if let Some(id) = e.get("id") {
            r.correlations.push(format!("trace-id:{id}"));
        }
        push(index, r)?;
        if phase == "C" {
            for (key, value) in e["args"].as_object().ok_or_else(invalid)? {
                let mut r = row(
                    "metric",
                    key,
                    format!(
                        "{prefix}/{i}/args/{}",
                        key.replace('~', "~0").replace('/', "~1")
                    ),
                    json!({"pid":e["pid"],"tid":e["tid"]}),
                );
                r.timestamp = e["ts"].as_f64();
                r.clock_domain = r.timestamp.map(|_| "trace".into());
                r.value = Some(number(value)?);
                r.unit = Some("producer-unspecified".into());
                r.definition = Some("trace counter; unit was not supplied by the format".into());
                push(index, r)?;
            }
        }
        if !matches!(phase, "X" | "M" | "i" | "I" | "R" | "C")
            && !index.unsupported.iter().any(|p| p == phase)
        {
            index.unsupported.push(phase.into());
        }
    }
    Ok(())
}
fn har(v: &Value, index: &mut Index) -> Result<()> {
    if v["log"]["version"] != "1.2" {
        return Err(invalid());
    }
    for (i, e) in array(&v["log"]["entries"])?.iter().enumerate() {
        text(&e["startedDateTime"])?;
        text(&e["request"]["method"])?;
        text(&e["request"]["url"])?;
        let duration = number(&e["time"])?;
        if duration < 0.0 || e["response"]["status"].as_u64().is_none() {
            return Err(invalid());
        }
        let mut r = row(
            "span",
            "HTTP request",
            format!("/log/entries/{i}"),
            e.clone(),
        );
        r.value = Some(duration);
        r.unit = Some("milliseconds".into());
        r.definition = Some("HAR total request time; -1 timing phases remain unavailable".into());
        if let Some(id) = e.get("pageref").and_then(Value::as_str) {
            r.correlations.push(format!("har-page:{id}"));
        }
        push(index, r)?;
        super::network_data::har(e, i, index)?;
        if e["response"]["status"].as_u64().unwrap() >= 400 {
            push(
                index,
                row(
                    "failure",
                    "HTTP error response",
                    format!("/log/entries/{i}/response/status"),
                    json!({"status":e["response"]["status"]}),
                ),
            )?;
        }
    }
    index.unsupported.push("HAR wall timestamps preserved as startedDateTime; numeric cross-clock alignment unavailable".into());
    Ok(())
}
fn sample_frames(sample: &Value, frames: &[Value], frame_sizes: &[usize]) -> Result<Vec<Value>> {
    let stack = array(sample)?;
    if stack.len() > 256 {
        return Err(Error::new(
            "ANALYSIS_BOUNDS",
            "Sample stack exceeds 256 frames",
        ));
    }
    let mut resolved = Vec::new();
    let mut expanded_size = 0usize;
    for frame in stack {
        let id = frame
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(invalid)?;
        let frame = frames.get(id).ok_or_else(invalid)?;
        expanded_size = expanded_size.saturating_add(frame_sizes[id]);
        if expanded_size > 32768 {
            return Err(Error::new(
                "ANALYSIS_BOUNDS",
                "Expanded sample frames exceed 32 KiB",
            ));
        }
        resolved.push(frame.clone());
    }
    Ok(resolved)
}

fn speedscope(v: &Value, index: &mut Index) -> Result<()> {
    if v["$schema"] != "https://www.speedscope.app/file-format-schema.json" {
        return Err(invalid());
    }
    let frames = array(&v["shared"]["frames"])?;
    let frame_sizes: Vec<_> = frames
        .iter()
        .map(|frame| serde_json::to_vec(frame).unwrap().len())
        .collect();
    for frame in frames {
        text(&frame["name"])?;
    }
    let profiles = array(&v["profiles"])?;
    if profiles.len() > 32 {
        return Err(Error::new(
            "ANALYSIS_BOUNDS",
            "At most 32 speedscope profiles per analysis",
        ));
    }
    for (p, profile) in profiles.iter().enumerate() {
        let unit = text(&profile["unit"])?;
        if !matches!(
            unit,
            "bytes" | "none" | "seconds" | "milliseconds" | "microseconds" | "nanoseconds"
        ) {
            return Err(invalid());
        }
        text(&profile["name"])?;
        let start = number(&profile["startValue"])?;
        let end = number(&profile["endValue"])?;
        if end < start {
            return Err(invalid());
        }
        if scale(unit).is_some() {
            index.clocks.push(Clock {
                id: format!("speedscope:{p}"),
                unit: unit.into(),
                reference_domain: None,
                max_drift_ppm: None,
                anchors: vec![],
            });
        }
        if profile["type"] == "evented" {
            evented(profile, frames, &frame_sizes, p, start, end, unit, index)?;
            continue;
        }
        if profile["type"] != "sampled" {
            return Err(Error::new(
                "UNSUPPORTED_DIAGNOSTIC_FORMAT",
                "Select a sampled or evented speedscope profile",
            ));
        }
        let samples = array(&profile["samples"])?;
        let weights = array(&profile["weights"])?;
        if weights.len() != samples.len() {
            return Err(invalid());
        }
        let mut position = start;
        for (i, (sample, weight)) in samples.iter().zip(weights).enumerate() {
            let resolved = sample_frames(sample, frames, &frame_sizes)?;
            let w = number(weight)?;
            if w < 0.0 {
                return Err(invalid());
            }
            let mut r = row(
                "stack",
                "sampled stack",
                format!("/profiles/{p}/samples/{i}"),
                json!({"frames":resolved,"profile":p,"profileName":profile["name"],"startValue":start,"endValue":end,"symbolication":"producer_provided"}),
            );
            if scale(unit).is_some() {
                r.timestamp = Some(position);
                r.clock_domain = Some(format!("speedscope:{p}"));
            }
            r.value = Some(w);
            r.unit = Some(unit.into());
            r.definition=Some("sample weight; sum of recorded weights, not a wall-time or CPU utilization guarantee".into());
            position += w;
            if !position.is_finite() {
                return Err(invalid());
            }
            push(index, r)?;
        }
    }
    Ok(())
}
#[expect(
    clippy::too_many_arguments,
    reason = "One evented profile carries its source frame table and byte sizes, ordinal, validated bounds and unit into the shared output index"
)]
fn evented(
    profile: &Value,
    frames: &[Value],
    sizes: &[usize],
    p: usize,
    start: f64,
    end: f64,
    unit: &str,
    index: &mut Index,
) -> Result<()> {
    let mut stack = Vec::<usize>::new();
    let mut position = start;
    for (i, event) in array(&profile["events"])?.iter().enumerate() {
        let at = number(&event["at"])?;
        let frame = event["frame"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n < frames.len())
            .ok_or_else(invalid)?;
        if at < position || at > end {
            return Err(invalid());
        }
        if at > position && !stack.is_empty() {
            let mut row = row(
                "stack",
                "evented stack interval",
                format!("/profiles/{p}/events/{i}"),
                json!({"frames":stack.iter().map(|id|&frames[*id]).collect::<Vec<_>>(),"profile":p,"profileName":profile["name"],"startValue":start,"endValue":end,"symbolication":"producer_provided","intervalStart":position,"intervalEnd":at}),
            );
            if scale(unit).is_some() {
                row.timestamp = Some(position);
                row.clock_domain = Some(format!("speedscope:{p}"));
            }
            row.value = Some(at - position);
            row.unit = Some(unit.into());
            row.definition=Some("evented profile axis interval; exclusive leaf stack; not CPU utilization or GPU completion".into());
            push(index, row)?;
        }
        match event["type"].as_str() {
            Some("O") => {
                stack.push(frame);
                if stack.len() > 256 || stack.iter().map(|n| sizes[*n]).sum::<usize>() > 32768 {
                    return Err(invalid());
                }
            }
            Some("C") => {
                if stack.pop() != Some(frame) {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
        position = at;
    }
    if !stack.is_empty() {
        return Err(invalid());
    }
    Ok(())
}
