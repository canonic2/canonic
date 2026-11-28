//! Bounded Apple logs, IPS/crash/Jetsam/MetricKit and explicit sysdiagnose-text observations.
use super::{bounded_json, invalid, push, row};
use crate::{
    diagnostics::{analysis::model::*, apple::reports},
    error::{Error, Result},
};
use base64::Engine;
use serde_json::{Value, json};
pub(super) fn decode(format: &str, bytes: &[u8], index: &mut Index) -> Result<()> {
    if format == "apple-diagnostics-v1" {
        let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
        for (line, text) in text.lines().enumerate() {
            let packet = match bounded_json(text.as_bytes(), MAX_RAW) {
                Ok(v) => v,
                Err(_) if !bytes.ends_with(b"\n") && !index.rows.is_empty() => {
                    index.unsupported.push("truncated_apple_packet".into());
                    break;
                }
                Err(e) => return Err(e),
            };
            if packet["schemaVersion"] != 1 {
                return Err(invalid());
            }
            let source = format!("line:{line}");
            let data = &packet["data"];
            let base = json!({"appId":packet["appId"],"targetId":packet["targetId"],"route":packet["route"],"transport":packet["transport"],"observationTimeMs":packet["observedAt"],"observationClock":"host wall time of packet retention; not report/event occurrence time","causeEstablished":false,"identityProvenance":"packet_producer_reported"});
            let first = index.rows.len();
            match packet["kind"].as_str() {
                Some("report") => packet_report(&packet, &source, index)?,
                Some("logs") => packet_logs(data, &packet, base, &source, index)?,
                Some("resources") => packet_resources(data, base, &source, index)?,
                Some("processes") => packet_processes(data, base, &source, index)?,
                _ => {
                    let mut d = base.clone();
                    d["observation"] = data.clone();
                    push(
                        index,
                        row(
                            "event",
                            packet["kind"].as_str().unwrap_or("Apple source gap"),
                            source.clone(),
                            d,
                        ),
                    )?;
                }
            }
            if let Some(actions) = packet["actionIds"].as_array() {
                if actions.len() > 32 {
                    return Err(invalid());
                }
                let actions: Vec<String> = actions
                    .iter()
                    .map(|a| {
                        let s = a.as_str().ok_or_else(invalid)?;
                        crate::diagnostics::model::label(s, 256)?;
                        Ok(s.into())
                    })
                    .collect::<Result<_>>()?;
                for r in &mut index.rows[first..] {
                    let before = serde_json::to_vec(r).unwrap().len();
                    r.correlations = actions.clone();
                    let after = serde_json::to_vec(r).unwrap().len();
                    index.model_bytes = index
                        .model_bytes
                        .saturating_sub(before)
                        .saturating_add(after);
                    if after > 65536 || index.model_bytes > MAX_INDEX - 65536 {
                        return Err(Error::new(
                            "ANALYSIS_BOUNDS",
                            "Apple correlated model exceeds bounds",
                        ));
                    }
                }
            }
        }
        index
            .unsupported
            .push("apple_native_source_is_a_bounded_partial_observation".into());
        return Ok(());
    }
    report(format, bytes, index, "/report")
}

fn packet_report(packet: &Value, source: &str, index: &mut Index) -> Result<()> {
    let data = &packet["data"];
    if let Some(encoded) = data["dataBase64"].as_str() {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| invalid())?;
        if raw.len() > MAX_RAW {
            return Err(invalid());
        }
        report(
            data["format"].as_str().ok_or_else(invalid)?,
            &raw,
            index,
            &format!("{source}/data/dataBase64:decoded"),
        )?;
    } else {
        push(
            index,
            row(
                "event",
                "Apple report unavailable",
                source.to_owned(),
                json!({"status":data["status"],"appId":packet["appId"]}),
            ),
        )?;
    }
    Ok(())
}

fn packet_logs(
    data: &Value,
    packet: &Value,
    base: Value,
    source: &str,
    index: &mut Index,
) -> Result<()> {
    for (i, entry) in data["entries"].as_array().into_iter().flatten().enumerate() {
        let message = entry["message"].as_str().unwrap_or_default();
        let class = classify(message);
        let mut details = base.clone();
        details["entry"] = entry.clone();
        details["processGeneration"] = entry["processGeneration"].clone();
        details["systemContext"] = entry["systemContext"].clone();
        details["relatedAppId"] = packet["appId"].clone();
        details["classification"] = json!(class);
        push(
            index,
            row(
                if class.is_some() { "failure" } else { "event" },
                class.unwrap_or("Apple log observation"),
                format!("{source}/data/entries/{i}"),
                details,
            ),
        )?;
    }
    Ok(())
}

fn packet_resources(data: &Value, base: Value, source: &str, index: &mut Index) -> Result<()> {
    for (i, p) in data["processes"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        for (field, name, unit, def) in [
            (
                "rssBytes",
                "rss",
                "bytes",
                "selected simulator app's host-kernel ps RSS, KiB converted to bytes; not physical-phone memory or live heap",
            ),
            (
                "cpuPercent",
                "cpu_percent_estimate",
                "percent",
                "ps %cpu estimate for the simulator process; not an interval sampler or physical device CPU",
            ),
        ] {
            if let Some(value) = p[field].as_f64().filter(|v| v.is_finite()) {
                let mut details = base.clone();
                details["process"] = p.clone();
                details["processGeneration"] = p["processGeneration"].clone();
                let mut r = row(
                    "metric",
                    name,
                    format!("{source}/data/processes/{i}/{field}"),
                    details,
                );
                r.value = Some(value);
                r.unit = Some(unit.into());
                r.definition = Some(def.into());
                push(index, r)?;
            }
        }
    }
    Ok(())
}

fn packet_processes(data: &Value, base: Value, source: &str, index: &mut Index) -> Result<()> {
    for (i, p) in data["processes"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let mut d = base.clone();
        d["process"] = p.clone();
        d["processGeneration"] = p["generation"].clone();
        push(
            index,
            row(
                "process",
                "Apple process observation",
                format!("{source}/data/processes/{i}"),
                d,
            ),
        )?;
    }
    Ok(())
}
fn classify(message: &str) -> Option<&'static str> {
    let m = message.to_ascii_lowercase();
    if m.contains("8badf00d") || m.contains("watchdog") {
        Some("watchdog_candidate")
    } else if m.contains("jetsam") || m.contains("memorystatus") {
        Some("memory_pressure_or_jetsam_context")
    } else if m.contains("thermal") {
        Some("thermal_context")
    } else if m.contains("interruption") || m.contains("audio route") {
        Some("audio_interruption_or_route_context")
    } else if m.contains("background") || m.contains("assertion") {
        Some("background_execution_context")
    } else if m.contains("exception") || m.contains("exited") || m.contains("terminated") {
        Some("exception_or_termination_candidate")
    } else {
        None
    }
}
fn report(format: &str, bytes: &[u8], index: &mut Index, source: &str) -> Result<()> {
    if matches!(format, "apple-crash-text" | "apple-sysdiagnose-text") {
        return text_report(
            format,
            std::str::from_utf8(bytes).map_err(|_| invalid())?,
            index,
            source,
        );
    }
    let (header, body) = reports::json(bytes)?;
    let app = reports::app(&header, &body);
    let provenance = json!({"appId":app,"timestampText":body["captureTime"].as_str().or_else(||header["timestamp"].as_str()),"processGeneration":null,"causeEstablished":false,"reportTimeMeaning":"producer-reported historical/field observation; not a live measurement"});
    if format == "apple-ips" && body.get("threads").is_none() && body.get("processes").is_some() {
        return jetsam(&body, index, source);
    }
    match format {
        "apple-ips" => ips_report(&body, &provenance, index, source)?,
        "apple-jetsam" => jetsam(&body, index, source)?,
        "apple-metrickit" => metrickit_report(&body, app.as_deref(), index, source)?,
        _ => return Err(invalid()),
    }
    index
        .unsupported
        .push("apple_report_completeness_and_causality_not_established".into());
    Ok(())
}

fn ips_report(body: &Value, provenance: &Value, index: &mut Index, source: &str) -> Result<()> {
    if body.get("exception").is_none()
        && body.get("termination").is_none()
        && body.get("threads").is_none()
    {
        return Err(invalid());
    }
    let summary = format!("{} {}", body["exception"], body["termination"]);
    let mut details = provenance.clone();
    details["exception"] = body["exception"].clone();
    details["termination"] = body["termination"].clone();
    details["classification"] =
        json!(classify(&summary).unwrap_or("apple_crash_or_hang_candidate"));
    details["pid"] = body["pid"].clone();
    details["bundleInfo"] = body["bundleInfo"].clone();
    push(
        index,
        row(
            "failure",
            "Apple crash/termination report",
            source.into(),
            details,
        ),
    )?;
    let images = body["usedImages"].as_array();
    if images.is_some_and(|v| v.len() > 1024) {
        return Err(Error::new(
            "ANALYSIS_BOUNDS",
            "At most 1024 Apple image entries",
        ));
    }
    for (t, thread) in body["threads"].as_array().into_iter().flatten().enumerate() {
        let mut d = provenance.clone();
        d["threadId"] = thread["id"].clone();
        d["triggered"] = thread["triggered"].clone();
        d["queue"] = thread["queue"].clone();
        push(
            index,
            row(
                "thread",
                "Apple report thread",
                format!("{source}/threads/{t}"),
                d,
            ),
        )?;
        for (f, frame) in thread["frames"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let image = frame["imageIndex"]
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| images.and_then(|v| v.get(i)));
            let mut d = provenance.clone();
            d["threadId"] = thread["id"].clone();
            d["frame"] = frame.clone();
            d["resolved"] = json!(false);
            d["sampleKind"] = json!("historical backtrace observation; not CPU sampling");
            if let Some(image) = image {
                d["buildId"] = image["uuid"].clone();
                d["architecture"] = image["arch"].clone();
                d["imageName"] = image["name"].clone();
                if let (Some(base), Some(offset)) =
                    (image["base"].as_u64(), frame["imageOffset"].as_u64())
                    && image["size"].as_u64().is_none_or(|size| offset < size)
                    && let Some(address) = base.checked_add(offset)
                {
                    d["loadAddress"] = json!(format!("0x{base:x}"));
                    d["address"] = json!(format!("0x{address:x}"));
                }
            }
            push(
                index,
                row(
                    "stack",
                    "Apple report frame",
                    format!("{source}/threads/{t}/frames/{f}"),
                    d,
                ),
            )?;
        }
    }
    Ok(())
}

fn metrickit_report(
    body: &Value,
    app: Option<&str>,
    index: &mut Index,
    source: &str,
) -> Result<()> {
    if body.get("timeStampBegin").is_none() || body.get("timeStampEnd").is_none() {
        return Err(invalid());
    }
    let window = json!({"begin":body["timeStampBegin"],"end":body["timeStampEnd"],"aggregation":"producer field-report interval; delivery time is not observation time"});
    for key in [
        "cpuMetrics",
        "memoryMetrics",
        "applicationTimeMetrics",
        "applicationExitMetrics",
        "displayMetrics",
        "cellularConditionMetrics",
        "networkTransferMetrics",
        "diskIOMetrics",
        "gpuMetrics",
        "locationActivityMetrics",
        "metaData",
    ] {
        if !body[key].is_null() {
            push(
                index,
                row(
                    "event",
                    "MetricKit field aggregate",
                    format!("{source}/{key}"),
                    json!({"appId":app,"window":window,"category":key,"payload":body[key],"units":"producer-encoded; no scalar coercion or live measurement inferred","causeEstablished":false}),
                ),
            )?;
        }
    }
    for key in [
        "crashDiagnostics",
        "hangDiagnostics",
        "cpuExceptionDiagnostics",
        "diskWriteExceptionDiagnostics",
    ] {
        for (i, entry) in body[key].as_array().into_iter().flatten().enumerate() {
            push(
                index,
                row(
                    "failure",
                    "MetricKit diagnostic candidate",
                    format!("{source}/{key}/{i}"),
                    json!({"appId":app,"window":window,"category":key,"diagnostic":entry,"causeEstablished":false,"processGeneration":null}),
                ),
            )?;
        }
    }
    if index.rows.is_empty() {
        push(
            index,
            row(
                "event",
                "MetricKit observation interval",
                source.into(),
                json!({"appId":app,"window":window}),
            ),
        )?;
    }
    Ok(())
}
fn jetsam(body: &Value, index: &mut Index, source: &str) -> Result<()> {
    let entries = body["processes"].as_array().ok_or_else(invalid)?;
    for (i, p) in entries.iter().enumerate() {
        let reason = p["reason"].clone();
        let killed = reason.as_str().is_some_and(|s| !s.is_empty()) || p["killed"] == true;
        push(
            index,
            row(
                if killed { "failure" } else { "process" },
                if killed {
                    "Jetsam termination candidate"
                } else {
                    "Jetsam process context"
                },
                format!("{source}/processes/{i}"),
                json!({"appId":null,"processName":p["name"],"pid":p["pid"],"reason":reason,"killed":p["killed"],"rpages":p["rpages"],"pageSize":body["pageSize"],"timestampText":body["date"],"processGeneration":null,"scope":"system report; name/PID alone does not establish selected bundle identity","causeEstablished":false}),
            ),
        )?;
    }
    index
        .unsupported
        .push("jetsam_report_is_system_scope_not_selected_app_or_complete_history".into());
    Ok(())
}
type TextImage = (u64, u64, String, String, String);
fn text_images(text: &str) -> Vec<TextImage> {
    text.lines()
        .filter_map(|line| {
            let w: Vec<_> = line.split_whitespace().collect();
            if w.len() < 6 || w[1] != "-" || !reports::architecture(w[4]) {
                return None;
            }
            let base = u64::from_str_radix(w[0].strip_prefix("0x")?, 16).ok()?;
            let end = u64::from_str_radix(w[2].strip_prefix("0x")?, 16).ok()?;
            reports::uuid(w[5].trim_matches(['<', '>']))?;
            Some((
                base,
                end,
                w[3].to_string(),
                w[4].to_string(),
                w[5].trim_matches(['<', '>']).to_string(),
            ))
        })
        .take(1024)
        .collect()
}

fn text_report(format: &str, text: &str, index: &mut Index, source: &str) -> Result<()> {
    let field = |key: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(key).map(str::trim))
    };
    let broad = format == "apple-sysdiagnose-text";
    if !broad
        && field("Exception Type:").is_none()
        && field("Termination Reason:").is_none()
        && field("Event:").is_none()
    {
        return Err(invalid());
    }
    let app = if broad {
        None
    } else {
        field("Identifier:").filter(|s| crate::diagnostics::apple::bundle(s))
    };
    let mut data = json!({"appId":app,"timestampText":field("Date/Time:"),"exception":field("Exception Type:"),"termination":field("Termination Reason:"),"event":field("Event:"),"processGeneration":null,"causeEstablished":false,"scope":if broad{"explicit broad system text artifact"}else{"historical report; identity producer-reported"}});
    data["classification"] =
        json!(classify(&data.to_string()).unwrap_or("apple_hang_or_crash_candidate"));
    push(
        index,
        row(
            if broad { "event" } else { "failure" },
            if broad {
                "Explicit sysdiagnose text"
            } else {
                "Apple text crash/hang report"
            },
            source.into(),
            data,
        ),
    )?;
    let images = text_images(text);
    for (i, line) in text.lines().enumerate() {
        if line.len() > 16384 {
            return Err(Error::new(
                "ANALYSIS_BOUNDS",
                "Apple report line exceeds 16 KiB",
            ));
        }
        let w: Vec<_> = line.split_whitespace().collect();
        if w.len() >= 3 && w[0].parse::<u32>().is_ok() && w[2].starts_with("0x") {
            let mut details = json!({"appId":app,"line":line,"resolved":false,"processGeneration":null,"sampleKind":"report backtrace; not CPU sampling"});
            if !broad && let Ok(address) = u64::from_str_radix(&w[2][2..], 16) {
                let matches: Vec<_> = images
                    .iter()
                    .filter(|(base, end, name, _, _)| {
                        address >= *base && address <= *end && name == w[1]
                    })
                    .collect();
                if matches.len() == 1 {
                    let (base, _, _, arch, uuid) = matches[0];
                    details["buildId"] = json!(uuid);
                    details["architecture"] = json!(arch);
                    details["loadAddress"] = json!(format!("0x{base:x}"));
                    details["address"] = json!(format!("0x{address:x}"));
                }
            }
            push(
                index,
                row(
                    "stack",
                    "Apple unresolved text frame",
                    format!("{source}:L{i}"),
                    details,
                ),
            )?;
        } else if broad && classify(line).is_some() {
            push(
                index,
                row(
                    "event",
                    "Sysdiagnose context candidate",
                    format!("{source}:L{i}"),
                    json!({"appId":null,"line":line,"causeEstablished":false}),
                ),
            )?;
        }
    }
    index
        .unsupported
        .push("apple_text_report_symbols_or_completeness_unverified".into());
    Ok(())
}
