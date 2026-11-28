//! OTLP JSON signal models, resource/schema scope and exact trace/exemplar witnesses.
use super::*;
fn any(v: &Value, depth: usize) -> Result<Value> {
    if depth > 16 {
        return Err(invalid());
    }
    if v.as_object().is_none_or(|m| m.len() > 1) {
        return Err(invalid());
    }
    if let Some(s) = v["stringValue"].as_str() {
        if s.len() > 8192 {
            return Err(invalid());
        }
        return Ok(json!(s));
    }
    if let Some(v) = v.get("intValue") {
        let n = v
            .as_i64()
            .or_else(|| v.as_str()?.parse().ok())
            .ok_or_else(invalid)?;
        return Ok(json!(n));
    }
    if let Some(v) = v.get("doubleValue") {
        return Ok(v.clone());
    }
    if let Some(b) = v["boolValue"].as_bool() {
        return Ok(json!(b));
    }
    if let Some(s) = v["bytesValue"].as_str() {
        if s.len() > 8192 {
            return Err(invalid());
        }
        return Ok(json!({"base64":s}));
    }
    if let Some(a) = v.get("arrayValue") {
        return Ok(Value::Array(
            optional_array(&a["values"], 128)?
                .iter()
                .map(|v| any(v, depth + 1))
                .collect::<Result<_>>()?,
        ));
    }
    if let Some(a) = v.get("kvlistValue") {
        return attrs(&a["values"], depth + 1);
    }
    if v.as_object().is_some_and(|m| m.is_empty()) {
        return Ok(Value::Null);
    }
    Err(invalid())
}
fn attrs(v: &Value, depth: usize) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for a in optional_array(v, 128)? {
        let key = text(&a["key"])?;
        if out.insert(key.into(), any(&a["value"], depth)?).is_some() {
            return Err(invalid());
        }
    }
    Ok(Value::Object(out))
}
fn nanos(v: &Value) -> Result<Option<u64>> {
    if v.is_null() {
        return Ok(None);
    }
    let n = v
        .as_u64()
        .or_else(|| v.as_str()?.parse().ok())
        .ok_or_else(invalid)?;
    Ok((n != 0).then_some(n))
}
fn stamp(row: &mut Row, v: &Value) -> Result<()> {
    if let Some(n) = nanos(v)? {
        time(row, Some(n as f64 / 1e9));
        row.data["timeUnixNano"] = json!(n.to_string());
        row.data["numericTimePrecision"] = json!(
            "floating seconds for ordering; original nanosecond string authoritative; clocks not proven synchronized"
        );
    }
    Ok(())
}
fn context(resource: &Value, scope: &Value, schema: &Value) -> Result<Value> {
    let attributes = attrs(&resource["attributes"], 0)?;
    Ok(
        json!({"resource":attributes,"resourceDroppedAttributesCount":resource["droppedAttributesCount"],"instrumentationScope":scope,"schemaUrl":schema,"serviceName":attributes["service.name"],"releaseId":attributes["service.version"],"imageId":attributes["container.image.id"],"environment":attributes["deployment.environment.name"]}),
    )
}
fn ids(row: &mut Row, v: &Value) -> Result<()> {
    if !v["traceId"].is_null() && v["traceId"] != "" {
        let id = hex(&v["traceId"], 32)?;
        let span = if !v["spanId"].is_null() && v["spanId"] != "" {
            Some(hex(&v["spanId"], 16)?)
        } else {
            None
        };
        trace(row, &id, span.as_deref());
    } else if !v["spanId"].is_null() && v["spanId"] != "" {
        row.data["spanIdWithoutTrace"] = v["spanId"].clone();
    }
    Ok(())
}
fn release(row: &mut Row) {
    if let Some(id) = row.data["imageId"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() < 240)
    {
        row.correlations.push(format!("image:{id}"));
    }
    if let (Some(service), Some(release)) = (
        row.data["serviceName"].as_str(),
        row.data["releaseId"].as_str(),
    ) && service.len() + release.len() < 240
    {
        row.correlations
            .push(format!("release:{service}:{release}"));
    }
}
pub(super) fn decode(v: &Value, index: &mut Index) -> Result<()> {
    if ["resourceSpans", "resourceLogs", "resourceMetrics"]
        .iter()
        .all(|k| v.get(k).is_none())
    {
        return Err(invalid());
    }
    decode_spans(v, index)?;
    decode_logs(v, index)?;
    decode_metrics(v, index)?;
    Ok(())
}

fn decode_spans(v: &Value, index: &mut Index) -> Result<()> {
    for (r, resource) in optional_array(&v["resourceSpans"], 64)?.iter().enumerate() {
        for (s, scope) in optional_array(&resource["scopeSpans"], 64)?
            .iter()
            .enumerate()
        {
            let base = context(
                &resource["resource"],
                &scope["scope"],
                &resource["schemaUrl"],
            )?;
            for (n, span) in array(&scope["spans"], 2000)?.iter().enumerate() {
                let id = hex(&span["traceId"], 32)?;
                let span_id = hex(&span["spanId"], 16)?;
                let name = text(&span["name"])?;
                let mut data = base.clone();
                data["attributes"] = attrs(&span["attributes"], 0)?;
                for key in [
                    "kind",
                    "status",
                    "flags",
                    "droppedAttributesCount",
                    "droppedEventsCount",
                    "droppedLinksCount",
                    "links",
                ] {
                    data[key] = span[key].clone();
                }
                data["scopeSchemaUrl"] = scope["schemaUrl"].clone();
                optional_array(&span["links"], 128)?;
                let parent = if !span["parentSpanId"].is_null()
                    && span["parentSpanId"] != ""
                    && span["parentSpanId"] != "0000000000000000"
                {
                    Some(hex(&span["parentSpanId"], 16)?)
                } else {
                    None
                };
                data["parentSpanId"] = json!(parent);
                let source = format!("/resourceSpans/{r}/scopeSpans/{s}/spans/{n}");
                let mut row = decode::row("span", name, source.clone(), data);
                trace(&mut row, &id, Some(&span_id));
                stamp(&mut row, &span["startTimeUnixNano"])?;
                if let (Some(start), Some(end)) = (
                    nanos(&span["startTimeUnixNano"])?,
                    nanos(&span["endTimeUnixNano"])?,
                ) {
                    let duration = end
                        .checked_sub(start)
                        .filter(|n| *n <= 1 << 53)
                        .ok_or_else(invalid)?;
                    row.value = Some(duration as f64);
                    row.unit = Some("nanoseconds".into());
                    row.definition=Some("OTLP recorded span end minus start; overlapping client/server spans are not additive or causal proof".into());
                    row.data["endTimeUnixNano"] = json!(end.to_string());
                }
                release(&mut row);
                let correlations = row.correlations.clone();
                decode::push(index, row)?;
                if span["status"]["code"] == 2 || span["status"]["code"] == "STATUS_CODE_ERROR" {
                    let mut row = decode::row(
                        "failure",
                        "OTLP recorded span error",
                        format!("{source}/status"),
                        json!({"traceId":id,"spanId":span_id,"status":span["status"],"resource":base["resource"],"causeEstablished":false}),
                    );
                    row.correlations = correlations.clone();
                    decode::push(index, row)?;
                }
                for (e, event) in optional_array(&span["events"], 128)?.iter().enumerate() {
                    let mut data = base.clone();
                    data["attributes"] = attrs(&event["attributes"], 0)?;
                    let mut row = decode::row(
                        "event",
                        text(&event["name"])?,
                        format!("{source}/events/{e}"),
                        data,
                    );
                    trace(&mut row, &id, Some(&span_id));
                    stamp(&mut row, &event["timeUnixNano"])?;
                    decode::push(index, row)?;
                }
            }
        }
    }
    Ok(())
}

fn decode_logs(v: &Value, index: &mut Index) -> Result<()> {
    for (r, resource) in optional_array(&v["resourceLogs"], 64)?.iter().enumerate() {
        for (s, scope) in optional_array(&resource["scopeLogs"], 64)?
            .iter()
            .enumerate()
        {
            let base = context(
                &resource["resource"],
                &scope["scope"],
                &resource["schemaUrl"],
            )?;
            for (n, log) in array(&scope["logRecords"], 2000)?.iter().enumerate() {
                let mut data = base.clone();
                data["body"] = if log["body"].is_null() {
                    Value::Null
                } else {
                    any(&log["body"], 0)?
                };
                data["attributes"] = attrs(&log["attributes"], 0)?;
                for key in [
                    "severityNumber",
                    "severityText",
                    "flags",
                    "observedTimeUnixNano",
                    "droppedAttributesCount",
                ] {
                    data[key] = log[key].clone();
                }
                let mut row = decode::row(
                    if log["severityNumber"].as_u64().is_some_and(|n| n >= 17) {
                        "failure"
                    } else {
                        "event"
                    },
                    "OTLP log record",
                    format!("/resourceLogs/{r}/scopeLogs/{s}/logRecords/{n}"),
                    data,
                );
                ids(&mut row, log)?;
                stamp(&mut row, &log["timeUnixNano"])?;
                release(&mut row);
                decode::push(index, row)?;
            }
        }
    }
    Ok(())
}

fn decode_metrics(v: &Value, index: &mut Index) -> Result<()> {
    for (r, resource) in optional_array(&v["resourceMetrics"], 64)?
        .iter()
        .enumerate()
    {
        for (s, scope) in optional_array(&resource["scopeMetrics"], 64)?
            .iter()
            .enumerate()
        {
            let base = context(
                &resource["resource"],
                &scope["scope"],
                &resource["schemaUrl"],
            )?;
            for (m, metric) in array(&scope["metrics"], 256)?.iter().enumerate() {
                let kinds = [
                    "gauge",
                    "sum",
                    "histogram",
                    "exponentialHistogram",
                    "summary",
                ]
                .into_iter()
                .filter(|k| metric.get(*k).is_some())
                .collect::<Vec<_>>();
                if kinds.len() != 1 {
                    return Err(invalid());
                }
                let kind = kinds[0];
                let unit = metric["unit"].as_str().unwrap_or("");
                let name = text(&metric["name"])?;
                for (n, point) in array(&metric[kind]["dataPoints"], 2000)?.iter().enumerate() {
                    let mut data = base.clone();
                    data["metricKind"] = json!(kind);
                    data["point"] = point.clone();
                    data["attributes"] = attrs(&point["attributes"], 0)?;
                    data["aggregationTemporality"] = metric[kind]["aggregationTemporality"].clone();
                    data["isMonotonic"] = metric[kind]["isMonotonic"].clone();
                    data["description"] = metric["description"].clone();
                    data["otlpUnit"] = json!(unit);
                    data["scopeSchemaUrl"] = scope["schemaUrl"].clone();
                    let mut row = decode::row(
                        "metric",
                        name,
                        format!(
                            "/resourceMetrics/{r}/scopeMetrics/{s}/metrics/{m}/{kind}/dataPoints/{n}"
                        ),
                        data,
                    );
                    if matches!(kind, "gauge" | "sum") {
                        if point.get("asDouble").is_some() && point.get("asInt").is_some() {
                            return Err(invalid());
                        }
                        row.value = if let Some(v) = point.get("asInt") {
                            v.as_i64()
                                .or_else(|| v.as_str()?.parse::<i64>().ok())
                                .filter(|n| n.unsigned_abs() <= 9007199254740992)
                                .map(|n| n as f64)
                        } else {
                            point.get("asDouble").and_then(number)
                        };
                    }
                    row.unit = Some(
                        if unit.is_empty() {
                            "producer-unspecified"
                        } else {
                            unit
                        }
                        .into(),
                    );
                    row.definition = Some(format!(
                        "OTLP {kind} recorded datapoint; temporality/monotonicity explicit; no interpolation, rate conversion or percentile inference"
                    ));
                    stamp(&mut row, &point["timeUnixNano"])?;
                    for exemplar in optional_array(&point["exemplars"], 8)? {
                        if !exemplar["traceId"].is_null() && exemplar["traceId"] != "" {
                            let id = hex(&exemplar["traceId"], 32)?;
                            row.correlations.push(format!("trace:{id}"));
                            row.data["correlationMeaning"] = json!(
                                "exemplar link only; entire aggregate not attributed to trace"
                            );
                        }
                    }
                    release(&mut row);
                    decode::push(index, row)?;
                }
            }
        }
    }
    Ok(())
}
