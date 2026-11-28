//! Retained provider/OTLP observations and witnessed joins; no remote dispatch.
use super::{decode, model::*};
use crate::{
    diagnostics::observability::{Scope, Window, utc},
    error::{Error, Result},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
mod otlp;
mod providers;
#[cfg(test)]
mod tests;
fn invalid() -> Error {
    Error::invalid(
        "Select a supported bounded observability export with exact incident scope/window and native signal semantics",
    )
}
pub(super) fn array(v: &Value, max: usize) -> Result<&Vec<Value>> {
    v.as_array().filter(|a| a.len() <= max).ok_or_else(invalid)
}
pub(super) fn optional_array(v: &Value, max: usize) -> Result<&[Value]> {
    if v.is_null() {
        Ok(&[])
    } else {
        Ok(array(v, max)?.as_slice())
    }
}
pub(super) fn text(v: &Value) -> Result<&str> {
    let s = v.as_str().ok_or_else(invalid)?;
    crate::diagnostics::model::label(s, 1024)?;
    Ok(s)
}
pub(super) fn number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.parse().ok())
        .filter(|n| n.is_finite())
}
pub(super) fn hex(v: &Value, len: usize) -> Result<String> {
    let s = v.as_str().ok_or_else(invalid)?;
    if s.len() != len || !s.bytes().all(|b| b.is_ascii_hexdigit()) || s.bytes().all(|b| b == b'0') {
        return Err(invalid());
    }
    Ok(s.to_ascii_lowercase())
}
pub(super) fn clock(index: &mut Index) {
    if !index.clocks.iter().any(|c| c.id == "reported-unix-seconds") {
        index.clocks.push(Clock {
            id: "reported-unix-seconds".into(),
            unit: "seconds".into(),
            reference_domain: None,
            max_drift_ppm: None,
            anchors: vec![],
        });
    }
}
pub(super) fn time(row: &mut Row, seconds: Option<f64>) {
    if let Some(n) = seconds.filter(|n| n.is_finite()) {
        row.timestamp = Some(n);
        row.clock_domain = Some("reported-unix-seconds".into());
    }
}
pub(super) fn trace(row: &mut Row, id: &str, span: Option<&str>) {
    row.correlations.push(format!("trace:{id}"));
    row.data["traceId"] = json!(id);
    if let Some(span) = span {
        row.correlations.push(format!("span:{id}:{span}"));
        row.data["spanId"] = json!(span);
    }
}
pub(super) fn decode(format: &str, bytes: &[u8], index: &mut Index) -> Result<()> {
    if format == "observability-evidence-v1" {
        return envelope(bytes, index);
    }
    let value = decode::bounded_json(bytes, MAX_RAW)?;
    clock(index);
    match format {
        "otlp-json" => otlp::decode(&value, index)?,
        "prometheus-json" => providers::prometheus(&value, index)?,
        "loki-json" => providers::loki(&value, index)?,
        "sentry-json" => providers::sentry(&value, index)?,
        "jaeger-json" => providers::jaeger(&value, index)?,
        "cloudwatch-json" => providers::cloudwatch(&value, index)?,
        "crashlytics-json" => providers::crashlytics(&value, index)?,
        "play-vitals-json" => providers::vitals(&value, index)?,
        "deployment-evidence-v1" => deployment(&value, index)?,
        _ => return Err(invalid()),
    }
    index.unsupported.push("retained provider/producer observations; sampling, retention, permissions, query completeness and clock agreement are not inferred; zero rows cannot establish health".into());
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Envelope {
    schema_version: u32,
    provider: String,
    source_id: String,
    scope: Scope,
    window: Window,
    authorization_owner: String,
    config_sha256: Option<String>,
    request: Value,
    format: String,
    unit: Option<String>,
    definition: Option<String>,
    pages: Vec<Page>,
    coverage: String,
    limits: Limits,
    scan: Scan,
    #[serde(default)]
    links: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Page {
    payload: Value,
    response_sha256: Option<String>,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Limits {
    max_records: usize,
    max_bytes: usize,
    timeout_ms: u32,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Scan {
    bytes: Option<u64>,
    cost: Option<f64>,
    meaning: String,
    #[serde(default)]
    max_bytes: Option<u64>,
    #[serde(default)]
    max_cost: Option<f64>,
}
fn allowed(provider: &str, format: &str) -> bool {
    match provider {
        "prometheus" => format == "prometheus-json",
        "loki" => format == "loki-json",
        "sentry" => format == "sentry-json",
        "crashlytics" => format == "crashlytics-json",
        "play_vitals" => format == "play-vitals-json",
        "apple" => matches!(format, "apple-metrickit" | "apple-ips"),
        "cloudwatch" => format == "cloudwatch-json",
        "jaeger" => format == "jaeger-json",
        "tempo" => format == "otlp-json",
        "grafana_cloud" => matches!(
            format,
            "otlp-json" | "prometheus-json" | "loki-json" | "jaeger-json"
        ),
        "datadog" | "new_relic" | "honeycomb" | "otlp" => format == "otlp-json",
        _ => false,
    }
}
fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn envelope(bytes: &[u8], index: &mut Index) -> Result<()> {
    let e: Envelope =
        serde_json::from_value(decode::bounded_json(bytes, MAX_RAW)?).map_err(|_| invalid())?;
    validate_envelope(&e)?;
    let links = e
        .links
        .iter()
        .map(|s| {
            let mut u = url::Url::parse(s).map_err(|_| invalid())?;
            if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() {
                return Err(invalid());
            }
            u.set_query(None);
            u.set_fragment(None);
            Ok(u.to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    let context = json!({"provider":e.provider,"sourceId":e.source_id,"scope":e.scope,"window":e.window,"authorizationOwner":e.authorization_owner,"configSha256":e.config_sha256,"request":e.request,"coverage":e.coverage,"scan":e.scan,"limits":e.limits,"providerLinks":links,"identityProvenance":"selected/exported query scope; IAM, native resource identity, release association and cost are not authenticated by parsing","causalityEstablished":false});
    decode::push(
        index,
        decode::row(
            "event",
            "Observability query provenance",
            "/request".into(),
            context.clone(),
        ),
    )?;
    let mut count = 0;
    for (page, p) in e.pages.into_iter().enumerate() {
        if p.response_sha256.as_ref().is_some_and(|s| !digest(s)) {
            return Err(invalid());
        }
        let raw = serde_json::to_vec(&p.payload).map_err(|_| invalid())?;
        if raw.len() > e.limits.max_bytes {
            return Err(Error::new(
                "OBSERVABILITY_BOUNDS",
                "Exported provider page exceeds selected byte budget",
            ));
        }
        let child = decode::decode(&e.format, &raw)?;
        count += child.rows.len();
        if count > e.limits.max_records {
            return Err(Error::new(
                "OBSERVABILITY_BOUNDS",
                "Export exceeds selected decoded-record budget",
            ));
        }
        if index.clocks.len() + child.clocks.len() > 32 {
            return Err(invalid());
        }
        for mut c in child.clocks {
            c.id = format!("page{page}:{}", c.id);
            if let Some(r) = c.reference_domain.as_mut() {
                *r = format!("page{page}:{r}");
            }
            index.clocks.push(c);
        }
        for mut row in child.rows {
            row.source = format!("/pages/{page}/payload{}", row.source);
            if let Some(c) = row.clock_domain.as_mut() {
                *c = format!("page{page}:{c}");
            }
            if let Some(m) = row.data.as_object_mut() {
                if m.insert("observability".into(), context.clone()).is_some() {
                    return Err(invalid());
                }
            } else {
                row.data = json!({"observation":row.data,"observability":context});
            }
            row.data["providerPage"] = json!({"ordinal":page,"responseSha256":p.response_sha256});
            if e.format == "prometheus-json"
                && row.value.is_some()
                && let (Some(unit), Some(definition)) = (&e.unit, &e.definition)
            {
                crate::diagnostics::model::label(unit, 256)?;
                crate::diagnostics::model::label(definition, 256)?;
                row.unit = Some(unit.clone());
                row.definition = Some(format!("declared provider query measurement: {definition}"));
                row.data["measurementSemantics"] =
                    json!("caller/preset declared; not authenticated by provider result");
            }
            decode::push(index, row)?;
        }
        for gap in child.unsupported {
            index.unsupported.push(format!("page{page}:{gap}"));
        }
    }
    index.unsupported.push("observability query/export coverage is not complete incident history; continuation, retention, sample/series suppression and late arrival may omit observations".into());
    Ok(())
}
fn deployment(v: &Value, index: &mut Index) -> Result<()> {
    if v["schemaVersion"] != 1 {
        return Err(invalid());
    }
    for (n, event) in array(&v["records"], 1024)?.iter().enumerate() {
        let kind = text(&event["kind"])?;
        if !matches!(
            kind,
            "deployment" | "feature" | "configuration" | "resource_state" | "audit"
        ) {
            return Err(invalid());
        }
        let id = text(&event["id"])?;
        let resource = text(&event["resource"])?;
        let release = text(&event["release"])?;
        for key in ["artifactSha256", "configSha256"] {
            if let Some(s) = event[key].as_str()
                && !digest(s)
            {
                return Err(invalid());
            }
        }
        let mut row = decode::row(
            "marker",
            "Declared deployed-system observation",
            format!("/records/{n}"),
            json!({"kind":kind,"id":id,"resource":resource,"release":release,"imageId":event["imageId"],"artifactSha256":event["artifactSha256"],"configSha256":event["configSha256"],"details":event["details"],"provenance":"exported deployment/audit observation; current local source is not incident-time deployment proof"}),
        );
        time(&mut row, Some(utc(text(&event["time"])?)?));
        row.correlations.push(format!("deployment:{id}"));
        row.correlations
            .push(format!("release:{resource}:{release}"));
        for (key, prefix) in [
            ("imageId", "image"),
            ("artifactSha256", "artifact"),
            ("configSha256", "config"),
        ] {
            if let Some(id) = event[key].as_str() {
                crate::diagnostics::model::label(id, 256)?;
                row.correlations.push(format!("{prefix}:{id}"));
            }
        }
        decode::push(index, row)?;
    }
    Ok(())
}
pub(super) fn joins(rows: &[(usize, &Row)], offset: usize, limit: usize) -> Result<Value> {
    let mut traces: BTreeMap<String, Vec<(usize, &Row)>> = BTreeMap::new();
    for (n, row) in rows {
        if let Some(trace) = row.data["traceId"].as_str() {
            traces.entry(trace.into()).or_default().push((*n, *row));
        } else {
            let mut seen = BTreeSet::new();
            for id in &row.correlations {
                if let Some(id) = id.strip_prefix("trace:")
                    && seen.insert(id)
                {
                    traces.entry(id.into()).or_default().push((*n, *row));
                }
            }
        }
    }
    let matched = traces.len();
    if matched > MAX_ROWS {
        return Err(Error::new(
            "ANALYSIS_BOUNDS",
            "Trace groups exceed the selected query budget; filter retained rows",
        ));
    }
    let items=traces.into_iter().skip(offset).take(limit).map(|(id,rows)|{
        let spans=rows.iter().filter(|(_,r)|r.kind=="span").collect::<Vec<_>>();let ids=spans.iter().filter_map(|(_,r)|r.data["spanId"].as_str()).collect::<BTreeSet<_>>();
        let missing=spans.iter().filter_map(|(n,r)|r.data["parentSpanId"].as_str().filter(|p|!ids.contains(p)).map(|p|json!({"ordinal":n,"parentSpanId":p}))).take(16).collect::<Vec<_>>();
        json!({"traceId":id,"spanObservations":spans.len(),"distinctSpanIds":ids.len(),"duplicateSpanIds":spans.len()>ids.len(),"missingParents":missing,"witnessOrdinals":rows.iter().take(16).map(|(n,_)|*n).collect::<Vec<_>>(),"witnessesTruncated":rows.len()>16,"logOrEventObservations":rows.iter().filter(|(_,r)|matches!(r.kind.as_str(),"event"|"failure")).count(),"metricExemplarObservations":rows.iter().filter(|(_,r)|r.kind=="metric").count(),"association":"exact reported trace/span IDs; metric exemplar is not attribution of the entire aggregate; missing parents can reflect sampling/retention; no causal proof"})
    }).collect::<Vec<_>>();
    Ok(
        json!({"view":"observability","traces":items,"matched":matched,"nextOffset":if offset+items.len()<matched{Some(offset+items.len())}else{None},"coverage":"selected retained observations only","sourceAuthentication":"not_established","causalityEstablished":false}),
    )
}

fn validate_envelope(e: &Envelope) -> Result<()> {
    e.scope.validate()?;
    e.window.validate(86400)?;
    if e.schema_version != 1
        || !allowed(&e.provider, &e.format)
        || e.pages.is_empty()
        || e.pages.len() > 8
        || !(1..=2000).contains(&e.limits.max_records)
        || !(1..=MAX_RAW).contains(&e.limits.max_bytes)
        || !(1..=10000).contains(&e.limits.timeout_ms)
        || !matches!(e.coverage.as_str(), "complete" | "partial" | "unknown")
        || e.config_sha256.as_ref().is_some_and(|s| !digest(s))
        || e.links.len() > 16
    {
        return Err(invalid());
    }
    for s in [&e.source_id, &e.authorization_owner, &e.scan.meaning] {
        crate::diagnostics::model::label(s, 256)?;
    }
    for c in [e.scan.cost, e.scan.max_cost].into_iter().flatten() {
        if !c.is_finite() || c < 0.0 {
            return Err(invalid());
        }
    }
    if e.scan
        .bytes
        .zip(e.scan.max_bytes)
        .is_some_and(|(a, b)| a > b)
        || e.scan.cost.zip(e.scan.max_cost).is_some_and(|(a, b)| a > b)
    {
        return Err(Error::new(
            "OBSERVABILITY_BUDGET",
            "Reported provider scan/cost exceeded selected budget; inspect retained raw evidence",
        ));
    }
    Ok(())
}
