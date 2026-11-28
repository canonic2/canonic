//! Selected Android packet and text decoders; failures are candidates, never causes.
use super::{bounded_json, invalid, push, row};
use crate::diagnostics::{analysis::model::*, android::parse};
use crate::error::{Error, Result};
use base64::Engine;
use serde_json::{Value, json};
use std::io::Read;

fn millis(text: &str) -> Option<f64> {
    parse::time_key(text).map(|(s, n)| s as f64 * 1000.0 + n as f64 / 1e6)
}
pub(super) fn decode(format: &str, bytes: &[u8], index: &mut Index) -> Result<()> {
    index.clocks.push(Clock {
        id: "android-epoch".into(),
        unit: "milliseconds".into(),
        reference_domain: None,
        max_drift_ppm: None,
        anchors: vec![],
    });
    if format != "android-diagnostics-v1" {
        let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
        return report(text, format, "/text", index);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let lines: Vec<_> = text.lines().collect();
    if lines.is_empty() {
        return Err(invalid());
    }
    for (n, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let packet = match bounded_json(line.as_bytes(), MAX_RAW) {
            Ok(value) => value,
            Err(_) if n + 1 == lines.len() && !text.ends_with('\n') && !index.rows.is_empty() => {
                index
                    .unsupported
                    .push("trailing_android_packet_truncated".into());
                break;
            }
            Err(e) => return Err(e),
        };
        if packet["schemaVersion"] != 1 || !packet["appId"].as_str().is_some_and(parse::package) {
            return Err(invalid());
        }
        let source = format!("line:{n}");
        let base = json!({"appId":packet["appId"],"userId":packet["userId"],"uid":packet["uid"],"bootId":packet["bootId"],"identityProvenance":"packet_producer_reported"});
        let observed = packet["observedAt"].as_str().and_then(millis);
        let data = &packet["data"];
        let kind = packet["kind"].as_str().ok_or_else(invalid)?;
        let first_row = index.rows.len();
        match kind {
            "state" => packet_state(&packet, &source, base, observed, index)?,
            "logs" => packet_logs(data, &source, base, index)?,
            "exits" => packet_exits(data, &source, base, index)?,
            "processes" => packet_processes(data, &source, base, observed, index)?,
            "installed" => packet_installed(data, &source, base, index)?,
            "symbols" => packet_symbols(data, &source, base, index)?,
            "services" => packet_services(&packet, &source, base, index)?,
            "report" => {
                if !packet_report(data, &source, base, index)? {
                    continue;
                }
            }
            "window" => packet_window(data, &source, base, index)?,
            "readiness" => push(
                index,
                row(
                    "event",
                    "Android transport readiness",
                    source.clone(),
                    base.clone(),
                ),
            )?,
            _ => gap(index, &source, "unknown_android_packet_kind", base.clone())?,
        }
        packet_correlations(&packet, first_row, index)?;
    }
    Ok(())
}
fn packet_state(
    packet: &Value,
    source: &str,
    mut base: Value,
    observed: Option<f64>,
    index: &mut Index,
) -> Result<()> {
    let data = &packet["data"];
    base["stateSource"] = json!(true);
    base["targetId"] = packet["targetId"].clone();
    base["buildIdentity"] = packet["buildIdentity"].clone();
    base["label"] = packet["label"].clone();
    base["processGeneration"] = data["processGeneration"].clone();
    base["pid"] = data["pid"].clone();
    base["group"] = data["group"].clone();
    base["scope"] = data["scope"].clone();
    base["counterWindow"] = data["counterWindow"].clone();
    base["observationTimestampMeaning"] =
        json!("cycle-start device epoch; reads are sequential; frame clocks remain in frame data");
    if data["scope"] == "device" {
        base["systemContext"] = json!(true);
        base["relatedAppId"] = packet["appId"].clone();
        base["appId"] = Value::Null;
    }
    state_snapshot(data, &base, observed, source, index)?;
    state_metrics(data, &base, observed, source, index)?;
    for (i, thread) in data["readings"]["threads"]["threads"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let mut details = base.clone();
        details["thread"] = thread.clone();
        push(
            index,
            row(
                "thread",
                "Android process thread state",
                format!("{source}/data/readings/threads/threads/{i}"),
                details,
            ),
        )?;
    }
    if data["status"] != "sampled" {
        gap(
            index,
            source,
            "android_state_source_unavailable_or_unrecognized",
            base.clone(),
        )?;
    }
    gap(
        index,
        source,
        "android_state_is_sequential_bounded_observation_not_retention_or_causality",
        base.clone(),
    )?;
    Ok(())
}

fn packet_logs(data: &Value, source: &str, base: Value, index: &mut Index) -> Result<()> {
    if let Some(entries) = data["entries"].as_array() {
        for (i, record) in entries.iter().enumerate() {
            let entry: parse::Log =
                serde_json::from_value(record["entry"].clone()).map_err(|_| invalid())?;
            let classification = classify(&entry);
            let mut details = base.clone();
            details["message"] = json!(entry.message);
            details["tag"] = json!(entry.tag);
            details["pid"] = json!(entry.pid);
            details["tid"] = json!(entry.tid);
            details["rawTimestamp"] = json!(entry.time);
            details["processGeneration"] = record["processGeneration"].clone();
            details["association"] = record["association"].clone();
            details["systemContext"] = record["systemContext"].clone();
            details["relatedAppId"] = record["relatedAppId"].clone();
            details["causeEstablished"] = json!(false);
            let frame = entry.tag == "DEBUG" && entry.message.trim().starts_with('#');
            if frame {
                details["sampleKind"] = json!("backtrace_observation; not CPU sampling");
            }
            if let Some(classification) = classification {
                details["classification"] = json!(classification);
            }
            let mut r = row(
                if classification.is_some() {
                    "failure"
                } else if frame {
                    "stack"
                } else {
                    "event"
                },
                classification.unwrap_or("Android log"),
                format!("{source}/data/entries/{i}"),
                details,
            );
            r.timestamp = millis(&entry.time);
            r.clock_domain = r.timestamp.map(|_| "android-epoch".into());
            push(index, r)?;
        }
    }
    if data["status"] != "available" && data["status"] != "not_requested" {
        gap(index, source, "log_source_unavailable", base.clone())?;
    }
    if data["complete"] != true {
        gap(
            index,
            source,
            "log_history_and_ring_delivery_incomplete",
            base.clone(),
        )?;
    }
    Ok(())
}

fn packet_exits(data: &Value, source: &str, base: Value, index: &mut Index) -> Result<()> {
    if let Some(entries) = data["entries"].as_array() {
        for (i, entry) in entries.iter().enumerate() {
            let mut details = base.clone();
            details["exitInfo"] = entry.clone();
            if let Some(info) = details["exitInfo"].as_object_mut() {
                info.remove("raw");
            }
            details["classification"] = entry["classification"].clone();
            details["causeEstablished"] = json!(false);
            details["processGeneration"] = Value::Null;
            push(
                index,
                row(
                    "failure",
                    entry["classification"].as_str().unwrap_or("unknown_exit"),
                    format!("{source}/data/entries/{i}"),
                    details,
                ),
            )?;
        }
    }
    if data["status"] != "available" {
        gap(
            index,
            source,
            "exit_history_unavailable_or_unknown",
            base.clone(),
        )?;
    }
    gap(
        index,
        source,
        "historical_absence_and_process_generation_not_proven",
        base.clone(),
    )?;
    Ok(())
}

fn packet_processes(
    data: &Value,
    source: &str,
    base: Value,
    observed: Option<f64>,
    index: &mut Index,
) -> Result<()> {
    for (field, name) in [
        ("processes", "process observed"),
        ("firstObserved", "process first observed"),
        ("noLongerObserved", "process no longer observed"),
    ] {
        if let Some(processes) = data[field].as_array() {
            for (i, p) in processes.iter().enumerate() {
                let mut details = base.clone();
                details["process"] = p.clone();
                details["processGeneration"] = p["generation"].clone();
                details["deathProven"] = json!(false);
                let mut r = row(
                    "process",
                    name,
                    format!("{source}/data/{field}/{i}"),
                    details,
                );
                r.timestamp = observed;
                r.clock_domain = observed.map(|_| "android-epoch".into());
                push(index, r)?;
            }
        }
    }
    Ok(())
}

fn packet_installed(data: &Value, source: &str, mut base: Value, index: &mut Index) -> Result<()> {
    base["installed"] = data["identity"].clone();
    push(
        index,
        row(
            "event",
            "installed Android application",
            format!("{source}/data/identity"),
            base.clone(),
        ),
    )?;
    Ok(())
}

fn packet_symbols(data: &Value, source: &str, base: Value, index: &mut Index) -> Result<()> {
    if let Some(libraries) = data["libraries"].as_array() {
        for (i, library) in libraries.iter().enumerate() {
            let mut details = base.clone();
            details["library"] = library.clone();
            if let Some(inspection) = details["library"]["inspection"].as_object_mut() {
                inspection.remove("raw");
            }
            details["buildId"] = library["inspection"]["buildId"].clone();
            details["architecture"] = library["inspection"]["architecture"].clone();
            details["resolved"] = json!(false);
            push(
                index,
                row(
                    "event",
                    "installed native library symbol inspection",
                    format!("{source}/data/libraries/{i}"),
                    details,
                ),
            )?;
        }
    }
    gap(
        index,
        source,
        "symbol_inventory_is_bounded_and_not_symbolication",
        base.clone(),
    )?;
    Ok(())
}

fn packet_services(packet: &Value, source: &str, base: Value, index: &mut Index) -> Result<()> {
    let data = &packet["data"];
    if let Some(text) = data["text"].as_str() {
        for (i, line) in text
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains("ServiceRecord{"))
        {
            let user = format!(" u{} ", packet["userId"].as_u64().unwrap_or(u64::MAX));
            if !line.contains(&user) {
                continue;
            }
            let mut details = base.clone();
            details["serviceRecord"] = json!(line.chars().take(2048).collect::<String>());
            push(
                index,
                row(
                    "process",
                    "Android service record",
                    format!("{source}/data/text:L{i}"),
                    details,
                ),
            )?;
        }
    }
    if data["status"] != "available" {
        gap(index, source, "service_topology_unavailable", base.clone())?;
    }
    Ok(())
}

fn packet_report(data: &Value, source: &str, base: Value, index: &mut Index) -> Result<bool> {
    if let Some(encoded) = data["dataBase64"].as_str() {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| invalid())?;
        if bytes.len() > 256 * 1024 {
            return Err(Error::new(
                "ANALYSIS_BOUNDS",
                "Android report exceeds 256 KiB",
            ));
        }
        let expanded;
        let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
            let mut decoded = Vec::new();
            let result = flate2::read::MultiGzDecoder::new(bytes.as_slice())
                .take(256 * 1024 + 1)
                .read_to_end(&mut decoded);
            if result.is_err() || decoded.len() > 256 * 1024 {
                gap(
                    index,
                    source,
                    "android_report_decompression_failed_or_limited",
                    base.clone(),
                )?;
                return Ok(false);
            }
            expanded = decoded;
            expanded.as_slice()
        } else {
            bytes.as_slice()
        };
        if let Ok(text) = std::str::from_utf8(bytes) {
            report(
                text,
                "android-report-text",
                &format!("{source}/data/dataBase64:decoded"),
                index,
            )?;
        } else {
            gap(
                index,
                source,
                "binary_android_report_decoder_unavailable",
                base.clone(),
            )?;
        }
    } else if data["status"] != "not_requested" {
        gap(
            index,
            source,
            "android_report_restricted_or_unavailable",
            base.clone(),
        )?;
    }
    Ok(true)
}

fn packet_window(data: &Value, source: &str, mut base: Value, index: &mut Index) -> Result<()> {
    base["sourceLoss"] = data["sourceLoss"].clone();
    gap(
        index,
        source,
        "android_window_is_a_bounded_sample",
        base.clone(),
    )?;
    Ok(())
}

fn packet_correlations(packet: &Value, first_row: usize, index: &mut Index) -> Result<()> {
    if let Some(actions) = packet["actionIds"].as_array() {
        if actions.len() > 32 {
            return Err(invalid());
        }
        let actions: Vec<String> = actions
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or_else(invalid))
            .collect::<Result<_>>()?;
        for action in &actions {
            crate::diagnostics::model::label(action, 256)?;
        }
        for r in &mut index.rows[first_row..] {
            let before = serde_json::to_vec(r).map_err(|_| invalid())?.len();
            r.correlations = actions.clone();
            r.data["correlationProvenance"] = json!(
                "caller-declared investigation action context; no event causation or reproduction completion established"
            );
            r.validate()?;
            let after = serde_json::to_vec(r).map_err(|_| invalid())?.len();
            index.model_bytes = index
                .model_bytes
                .saturating_sub(before)
                .saturating_add(after);
            if after > 65536 || index.model_bytes > MAX_INDEX - 65536 {
                return Err(Error::new(
                    "ANALYSIS_BOUNDS",
                    "Correlated Android rows exceed the row/index budget",
                ));
            }
        }
    }
    Ok(())
}

fn state_snapshot(
    data: &Value,
    base: &Value,
    observed: Option<f64>,
    source: &str,
    index: &mut Index,
) -> Result<()> {
    let mut snapshot = base.clone();
    snapshot["status"] = data["status"].clone();
    snapshot["fields"] = data["fields"].clone();
    snapshot["media"] = data["media"].clone();
    snapshot["readings"] = data["readings"]
        .as_object()
        .map(|readings| {
            readings
                .iter()
                .map(|(name, value)| {
                    (
                        name.clone(),
                        json!({"status":value["status"],"code":value["code"]}),
                    )
                })
                .collect::<serde_json::Map<String, Value>>()
        })
        .map_or(Value::Null, Value::Object);
    snapshot["complete"] = json!(false);
    let mut event = row(
        "event",
        "Android state snapshot",
        format!("{source}/data"),
        snapshot,
    );
    event.timestamp = observed;
    event.clock_domain = observed.map(|_| "android-epoch".into());
    push(index, event)?;
    Ok(())
}

fn state_metrics(
    data: &Value,
    base: &Value,
    observed: Option<f64>,
    source: &str,
    index: &mut Index,
) -> Result<()> {
    if let Some(metrics) = data["metrics"].as_array() {
        if metrics.len() > 512 {
            return Err(Error::new(
                "ANALYSIS_BOUNDS",
                "At most 512 metrics per Android state packet",
            ));
        }
        for (i, m) in metrics.iter().enumerate() {
            let name = m["name"].as_str().ok_or_else(invalid)?;
            let mut details = base.clone();
            details["measurement"] = m.clone();
            let mut r = row(
                "metric",
                name,
                format!("{source}/data/metrics/{i}"),
                details,
            );
            r.value = Some(
                m["value"]
                    .as_f64()
                    .filter(|n| n.is_finite())
                    .ok_or_else(invalid)?,
            );
            r.unit = Some(m["unit"].as_str().ok_or_else(invalid)?.into());
            r.definition = Some(m["definition"].as_str().ok_or_else(invalid)?.into());
            r.timestamp = observed;
            r.clock_domain = observed.map(|_| "android-epoch".into());
            push(index, r)?;
        }
    }
    Ok(())
}
fn gap(index: &mut Index, source: &str, reason: &str, data: Value) -> Result<()> {
    if !index.unsupported.iter().any(|v| v == reason) {
        index.unsupported.push(reason.into());
    }
    push(index, row("event", reason, source.into(), data))
}
fn classify(entry: &parse::Log) -> Option<&'static str> {
    let message = entry.message.to_ascii_lowercase();
    if entry.tag == "AndroidRuntime" && message.contains("fatal exception") {
        Some("java_crash_candidate")
    } else if entry.tag == "libc" && message.contains("fatal signal")
        || entry.tag == "DEBUG" && message.starts_with("signal ")
    {
        Some("native_signal_candidate")
    } else if message.contains("anr in ") || entry.tag == "am_anr" {
        Some("anr_candidate")
    } else if entry.tag == "am_kill" || message.starts_with("killing ") {
        Some(
            if message.contains("low memory") || message.contains("lmk") {
                "low_memory_kill_candidate"
            } else {
                "system_kill_candidate"
            },
        )
    } else if message.contains("watchdog") {
        Some("watchdog_context")
    } else if message.contains("foregroundservicestartnotallowedexception")
        || message.contains("unable to start service")
        || message.contains("startforegroundservice") && message.contains("did not")
    {
        Some("service_start_failure_candidate")
    } else if message.contains("background execution not allowed")
        || message.contains("background start not allowed")
    {
        Some("background_policy_candidate")
    } else if message.contains("package_replaced")
        || message.contains("package replaced")
        || message.contains("force stopping") && message.contains("install")
    {
        Some("package_replacement_context")
    } else {
        None
    }
}
pub(super) fn report(text: &str, format: &str, source: &str, index: &mut Index) -> Result<()> {
    let tombstone =
        text.contains("*** *** ***") && text.contains("signal ") && text.contains("pid:");
    let anr = text.contains("ANR in ")
        || text.contains("DALVIK THREADS")
        || text.contains("----- pid ") && text.contains("Cmd line:") && text.contains(" tid=");
    let broad = format == "android-bugreport-text";
    if !broad && !tombstone && !anr {
        return gap(
            index,
            source,
            "unrecognized_android_report_text",
            json!({"format":format}),
        );
    }
    let mut app = None;
    let mut apps = std::collections::BTreeSet::new();
    let mut pid = None;
    let mut abi = None;
    for line in text.lines() {
        if let Some(value) = line.trim().strip_prefix("ABI:") {
            abi = Some(value.trim().trim_matches('\'').to_string());
        }
        if let Some(value) = line
            .trim()
            .strip_prefix("Cmdline:")
            .or_else(|| line.trim().strip_prefix("Cmd line:"))
        {
            let value = value.trim();
            if parse::package(value.split(':').next().unwrap_or(value)) {
                app = Some(value.split(':').next().unwrap_or(value).to_string());
                apps.insert(app.clone().unwrap());
            }
        }
        if let Some((_, value)) = line.split_once(">>> ")
            && let Some(value) = value.split(" <<<").next()
        {
            let package = value.split(':').next().unwrap_or(value);
            if parse::package(package) {
                app = Some(package.into());
                apps.insert(package.to_string());
            }
        }
        if let Some((_, value)) = line.split_once("pid:") {
            pid = value
                .trim()
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .and_then(|v| v.parse::<u32>().ok());
        }
    }
    if broad || apps.len() != 1 {
        app = None;
        pid = None;
    }
    let data = json!({"appId":app,"pid":pid,"architecture":abi,"processGeneration":null,"classification":if tombstone{"native_crash_candidate"}else if anr{"anr_trace_candidate"}else{"unclassified_report"},"causeEstablished":false,"format":format,"scope":if broad{"broad_system_artifact"}else{"report_scope; process generation may be unknown"}});
    push(
        index,
        row(
            if tombstone || anr { "failure" } else { "event" },
            if tombstone {
                "native tombstone"
            } else if anr {
                "ANR evidence"
            } else {
                "Android report"
            },
            source.into(),
            data,
        ),
    )?;
    report_lines(
        text,
        source,
        broad,
        if broad { None } else { app },
        abi,
        index,
    )?;
    gap(
        index,
        source,
        "report_completeness_and_symbolication_not_established",
        json!({"format":format}),
    )
}

fn report_lines(
    text: &str,
    source: &str,
    broad: bool,
    mut current_app: Option<String>,
    abi: Option<String>,
    index: &mut Index,
) -> Result<()> {
    for (n, line) in text.lines().enumerate() {
        if line.starts_with("----- pid ") || line.starts_with("----- end ") {
            current_app = None;
        }
        if let Some(value) = line
            .trim()
            .strip_prefix("Cmd line:")
            .or_else(|| line.trim().strip_prefix("Cmdline:"))
        {
            let value = value.trim().split(':').next().unwrap_or_default();
            current_app = parse::package(value).then(|| value.to_string());
        }
        let trimmed = line.trim();
        let kind = if trimmed.starts_with('#')
            && trimmed.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
        {
            Some("stack")
        } else if trimmed.starts_with('"')
            && (trimmed.contains("prio=") || trimmed.contains(" tid="))
        {
            Some("thread")
        } else if broad && (trimmed.contains("FATAL EXCEPTION") || trimmed.contains("ANR in ")) {
            Some("failure")
        } else {
            None
        };
        if let Some(kind) = kind {
            if line.len() > 16384 {
                return Err(Error::new(
                    "ANALYSIS_BOUNDS",
                    "Android report line exceeds 16 KiB",
                ));
            }
            push(
                index,
                row(
                    kind,
                    if kind == "stack" {
                        "native backtrace frame"
                    } else if kind == "thread" {
                        "ANR thread"
                    } else {
                        "bugreport failure candidate"
                    },
                    format!("{source}:L{n}"),
                    json!({"appId":if broad{None}else{current_app.clone()},"line":line,"architecture":abi,"processGeneration":null,"resolved":false,"sampleKind":"report_observation; not CPU sampling","causeEstablished":false}),
                ),
            )?;
        }
    }
    Ok(())
}
