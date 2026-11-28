//! Selected public JSON exports; continuation and native semantics remain visible.
use super::*;
fn metric(
    index: &mut Index,
    name: &str,
    source: String,
    value: &Value,
    at: Option<f64>,
    data: Value,
) -> Result<()> {
    let mut r = decode::row("metric", name, source, data);
    r.value = number(value);
    r.unit = Some("producer-unspecified".into());
    r.definition=Some("provider-reported sample; query, aggregation and unit require producer evidence; nonfinite values remain raw".into());
    time(&mut r, at);
    decode::push(index, r)
}
pub(super) fn prometheus(v: &Value, index: &mut Index) -> Result<()> {
    if v["status"] != "success" {
        return Err(invalid());
    }
    let kind = text(&v["data"]["resultType"])?;
    if !matches!(kind, "matrix" | "vector") {
        return Err(invalid());
    }
    for (n, s) in array(&v["data"]["result"], 2000)?.iter().enumerate() {
        let name = s["metric"]["__name__"]
            .as_str()
            .unwrap_or("Prometheus sample");
        let single;
        let points = if kind == "vector" {
            single = vec![s["value"].clone()];
            &single
        } else {
            array(&s["values"], 2000)?
        };
        for (p, point) in points.iter().enumerate() {
            if array(point, 2)?.len() != 2 {
                return Err(invalid());
            }
            metric(
                index,
                name,
                format!(
                    "/data/result/{n}/{}{}",
                    if kind == "vector" { "value" } else { "values/" },
                    if kind == "vector" {
                        String::new()
                    } else {
                        p.to_string()
                    }
                ),
                &point[1],
                number(&point[0]),
                json!({"labels":s["metric"],"sample":point,"warnings":v["warnings"],"infos":v["infos"]}),
            )?;
        }
    }
    index.unsupported.push("Prometheus series/sample limits, warnings, stale/nonfinite values and query semantics are not full incident coverage".into());
    Ok(())
}
pub(super) fn loki(v: &Value, index: &mut Index) -> Result<()> {
    if v["status"] != "success" || v["data"]["resultType"] != "streams" {
        return Err(invalid());
    }
    for (n, s) in array(&v["data"]["result"], 2000)?.iter().enumerate() {
        for (p, point) in array(&s["values"], 2000)?.iter().enumerate() {
            if array(point, 3)?.len() < 2 {
                return Err(invalid());
            }
            let ns = text(&point[0])?.parse::<u64>().map_err(|_| invalid())?;
            let mut r = decode::row(
                "event",
                "Loki log record",
                format!("/data/result/{n}/values/{p}"),
                json!({"labels":s["stream"],"timeUnixNano":point[0],"text":point[1],"metadata":point.get(2),"stats":v["data"]["stats"]}),
            );
            time(&mut r, Some(ns as f64 / 1e9));
            decode::push(index, r)?;
        }
    }
    index
        .unsupported
        .push("Loki limit, dropped streams, retention and backend scan cost may omit logs".into());
    Ok(())
}
pub(super) fn sentry(v: &Value, index: &mut Index) -> Result<()> {
    let id = text(&v["eventID"])?;
    let mut r = decode::row(
        if matches!(v["level"].as_str(), Some("error" | "fatal")) {
            "failure"
        } else {
            "event"
        },
        "Sentry recorded event",
        "/".into(),
        json!({"eventId":id,"release":v["release"],"tags":v["tags"],"level":v["level"],"platform":v["platform"],"title":v["title"],"contexts":v["contexts"]}),
    );
    if let Some(at) = v["dateCreated"].as_str() {
        time(&mut r, Some(utc(at)?));
    }
    let t = &v["contexts"]["trace"];
    if !t["trace_id"].is_null() {
        let tid = hex(&t["trace_id"], 32)?;
        let sid = if t["span_id"].is_null() {
            None
        } else {
            Some(hex(&t["span_id"], 16)?)
        };
        trace(&mut r, &tid, sid.as_deref());
    }
    r.correlations.push(format!("sentry:{id}"));
    decode::push(index, r)
}
pub(super) fn jaeger(v: &Value, index: &mut Index) -> Result<()> {
    for (t, tr) in array(&v["data"], 128)?.iter().enumerate() {
        let tid = hex(&tr["traceID"], 32)?;
        for (n, s) in array(&tr["spans"], 2000)?.iter().enumerate() {
            if hex(&s["traceID"], 32)? != tid {
                return Err(invalid());
            }
            let sid = hex(&s["spanID"], 16)?;
            let mut r = decode::row(
                "span",
                text(&s["operationName"])?,
                format!("/data/{t}/spans/{n}"),
                json!({"tags":s["tags"],"references":s["references"],"process":tr["processes"][s["processID"].as_str().unwrap_or("")],"warnings":s["warnings"]}),
            );
            trace(&mut r, &tid, Some(&sid));
            for p in optional_array(&s["references"], 128)? {
                if p["refType"] == "CHILD_OF" && hex(&p["traceID"], 32)? == tid {
                    r.data["parentSpanId"] = json!(hex(&p["spanID"], 16)?);
                    break;
                }
            }
            time(&mut r, number(&s["startTime"]).map(|n| n / 1e6));
            r.value = number(&s["duration"]).filter(|n| *n >= 0.0);
            r.unit = Some("microseconds".into());
            r.definition =
                Some("Jaeger recorded span duration; overlapping spans are not additive".into());
            decode::push(index, r)?;
        }
    }
    Ok(())
}
pub(super) fn cloudwatch(v: &Value, index: &mut Index) -> Result<()> {
    if ["events", "results", "MetricDataResults"]
        .iter()
        .all(|k| v.get(k).is_none())
    {
        return Err(invalid());
    }
    for (n, e) in optional_array(&v["events"], 2000)?.iter().enumerate() {
        let mut r = decode::row(
            "event",
            "CloudWatch log event",
            format!("/events/{n}"),
            e.clone(),
        );
        time(&mut r, number(&e["timestamp"]).map(|n| n / 1000.0));
        decode::push(index, r)?;
    }
    for (n, e) in optional_array(&v["results"], 2000)?.iter().enumerate() {
        decode::push(
            index,
            decode::row(
                "event",
                "CloudWatch Logs Insights row",
                format!("/results/{n}"),
                json!({"fields":e,"status":v["status"],"statistics":v["statistics"]}),
            ),
        )?;
    }
    for (n, m) in optional_array(&v["MetricDataResults"], 128)?
        .iter()
        .enumerate()
    {
        let times = array(&m["Timestamps"], 2000)?;
        let values = array(&m["Values"], 2000)?;
        if times.len() != values.len() {
            return Err(invalid());
        }
        for (p, (t, value)) in times.iter().zip(values).enumerate() {
            metric(
                index,
                m["Label"].as_str().unwrap_or("CloudWatch metric"),
                format!("/MetricDataResults/{n}/Values/{p}"),
                value,
                Some(utc(text(t)?)?),
                json!({"id":m["Id"],"statusCode":m["StatusCode"],"messages":m["Messages"]}),
            )?;
        }
    }
    index.unsupported.push(format!("CloudWatch selected page only; continuation present: {}; query status {} does not prove complete incident coverage",!v["nextToken"].is_null()||!v["NextToken"].is_null(),v["status"]));
    Ok(())
}
pub(super) fn crashlytics(v: &Value, index: &mut Index) -> Result<()> {
    let rows = if v.is_array() {
        array(v, 2000)?
    } else {
        array(&v["rows"], 2000)?
    };
    for (n, e) in rows.iter().enumerate() {
        text(&e["event_id"])?;
        let mut r = decode::row(
            "failure",
            "Crashlytics exported event",
            if v.is_array() {
                format!("/{n}")
            } else {
                format!("/rows/{n}")
            },
            json!({"eventId":e["event_id"],"eventType":e["event_type"],"issue":e["issue"],"application":e["application"],"eventTimestamp":e["event_timestamp"],"provenance":"selected BigQuery JSON event projection; crash classification is producer reported"}),
        );
        if let Some(at) = e["event_timestamp"].as_str() {
            time(&mut r, Some(utc(at)?));
        }
        decode::push(index, r)?;
    }
    Ok(())
}
pub(super) fn vitals(v: &Value, index: &mut Index) -> Result<()> {
    for (n, row) in array(&v["rows"], 2000)?.iter().enumerate() {
        for (m, value) in array(&row["metrics"], 128)?.iter().enumerate() {
            metric(
                index,
                text(&value["metric"])?,
                format!("/rows/{n}/metrics/{m}"),
                &value["decimalValue"]["value"],
                None,
                json!({"startTime":row["startTime"],"dimensions":row["dimensions"],"metric":value,"aggregationPeriod":row["aggregationPeriod"]}),
            )?;
        }
    }
    index.unsupported.push("Play reporting aggregation, suppression, timezone DateTime and nextPageToken preserved in raw export; no crash-per-device or exact UTC instant inferred".into());
    Ok(())
}
