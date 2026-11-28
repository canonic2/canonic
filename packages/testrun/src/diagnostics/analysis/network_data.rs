//! Bounded network/data evidence and exact declared correlations; no traffic or DB mutation.
mod data;
mod pcap;
#[cfg(test)]
mod tests;
use super::model::Index;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
fn invalid() -> Error {
    Error::new(
        "INVALID_NETWORK_DATA_ARTIFACT",
        "Invalid, ambiguous or unsupported network/data evidence",
    )
}
fn label(s: &str) -> Result<()> {
    crate::diagnostics::model::label(s, 256)
}
fn number(n: f64) -> Result<()> {
    if !n.is_finite() || !(0.0..=9007199254740991.0).contains(&n) {
        return Err(invalid());
    }
    Ok(())
}
fn emit(index: &mut Index, kind: &str, name: &str, source: String, data: Value) -> Result<()> {
    super::decode::push(
        index,
        super::decode::row(
            kind,
            name,
            source,
            json!({"appId":null,"networkData":data,"identityProvenance":"source/producer declared; not authenticated by parsing","causeEstablished":false}),
        ),
    )
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Identity {
    scope: String,
    request_id: Option<String>,
    trace_id: Option<String>,
    span_id: Option<String>,
    parent_span_id: Option<String>,
}
fn hex(s: &str, width: usize) -> bool {
    s.len() == width
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && s.bytes().any(|b| b != b'0')
}
impl Identity {
    fn validate(&self) -> Result<()> {
        label(&self.scope)?;
        if let Some(s) = &self.request_id {
            label(s)?;
        }
        if self.trace_id.as_ref().is_some_and(|s| !hex(s, 32))
            || self.span_id.as_ref().is_some_and(|s| !hex(s, 16))
            || self.parent_span_id.as_ref().is_some_and(|s| !hex(s, 16))
            || (self.span_id.is_some() || self.parent_span_id.is_some()) && self.trace_id.is_none()
        {
            return Err(invalid());
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Network {
    schema_version: u32,
    format: String,
    source: String,
    coverage: String,
    records: Vec<Record>,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Record {
    #[serde(rename_all = "camelCase")]
    Request {
        id: String,
        identity: Identity,
        protocol: String,
        method: String,
        address: String,
        status: Option<u16>,
        grpc_status: Option<u8>,
        cancelled: Option<bool>,
        cache: Option<String>,
        retry_of: Option<String>,
        phases: BTreeMap<String, Option<f64>>,
    },
    #[serde(rename_all = "camelCase")]
    Message {
        id: String,
        request: String,
        direction: String,
        sequence: u32,
        bytes: u64,
        encoding: String,
    },
    #[serde(rename_all = "camelCase")]
    Span {
        id: String,
        identity: Identity,
        service: String,
        operation: String,
        duration_micros: Option<f64>,
        sampled: Option<bool>,
    },
    #[serde(rename_all = "camelCase")]
    Log {
        id: String,
        identity: Identity,
        message: String,
    },
}
#[expect(
    clippy::too_many_lines,
    reason = "strict bounded record dispatch keeps protocol-specific admission and exact source projection together"
)]
pub(super) fn decode(format: &str, bytes: &[u8], index: &mut Index) -> Result<()> {
    if format == "pcap" {
        return pcap::decode(bytes, index);
    }
    let value = super::decode::bounded_json(bytes, super::model::MAX_RAW)?;
    if format == "data-snapshot-v1" {
        return data::decode(value, index, "");
    }
    let input: Network = serde_json::from_value(value).map_err(|_| invalid())?;
    if input.schema_version != 1
        || input.format != "network-evidence-v1"
        || input.records.len() > 4096
    {
        return Err(invalid());
    }
    label(&input.source)?;
    label(&input.coverage)?;
    let requests: BTreeSet<_> = input
        .records
        .iter()
        .filter_map(|r| {
            if let Record::Request { id, .. } = r {
                Some(id.clone())
            } else {
                None
            }
        })
        .collect();
    let mut ids = BTreeSet::new();
    let mut sequences = BTreeSet::new();
    let mut retries = BTreeMap::new();
    for (i, r) in input.records.into_iter().enumerate() {
        let source = format!("/records/{i}");
        let (kind, name, mut observation) = match r {
            Record::Request {
                id,
                identity,
                protocol,
                method,
                address,
                status,
                grpc_status,
                cancelled,
                cache,
                retry_of,
                phases,
            } => {
                unique(&mut ids, &id)?;
                identity.validate()?;
                label(&method)?;
                crate::diagnostics::model::label(&address, 4096)?;
                if !matches!(
                    protocol.as_str(),
                    "http1" | "http2" | "http3" | "websocket" | "grpc" | "stream" | "other"
                ) || status.is_some_and(|s| s > 599)
                    || grpc_status.is_some_and(|s| s > 16)
                    || grpc_status.is_some() && protocol != "grpc"
                {
                    return Err(invalid());
                }
                if let Some(cache) = &cache {
                    label(cache)?;
                }
                if let Some(previous) = &retry_of {
                    if previous == &id || !requests.contains(previous) {
                        return Err(invalid());
                    }
                    retries.insert(id.clone(), previous.clone());
                }
                for (phase, value) in &phases {
                    if !matches!(
                        phase.as_str(),
                        "dns" | "connect" | "tls" | "send" | "wait" | "receive" | "total"
                    ) {
                        return Err(invalid());
                    }
                    if let Some(n) = value {
                        number(*n)?;
                    }
                }
                (
                    "event",
                    "request",
                    json!({"category":"requests","id":id,"identity":identity,"protocol":protocol,"method":method,"address":address,"status":status,"grpcStatus":grpc_status,"cancelled":cancelled,"cache":cache,"retryOf":retry_of,"phasesMicros":phases,"phaseMeaning":"producer intervals; absent/null is unavailable, zero is observed zero; intervals may overlap and are not summed"}),
                )
            }
            Record::Message {
                id,
                request,
                direction,
                sequence,
                bytes,
                encoding,
            } => {
                unique(&mut ids, &id)?;
                label(&encoding)?;
                if !requests.contains(&request)
                    || !matches!(direction.as_str(), "sent" | "received")
                    || bytes > 9007199254740991
                    || !sequences.insert((request.clone(), direction.clone(), sequence))
                {
                    return Err(invalid());
                }
                (
                    "event",
                    "stream message",
                    json!({"category":"messages","id":id,"request":request,"direction":direction,"sequence":sequence,"bytes":bytes,"encoding":encoding,"meaning":"observed message metadata; no body, wire decoding, HTTP/3 visibility or delivery guarantee"}),
                )
            }
            Record::Span {
                id,
                identity,
                service,
                operation,
                duration_micros,
                sampled,
            } => {
                unique(&mut ids, &id)?;
                identity.validate()?;
                label(&service)?;
                label(&operation)?;
                if let Some(n) = duration_micros {
                    number(n)?;
                }
                (
                    "span",
                    "service span",
                    json!({"category":"spans","id":id,"identity":identity,"service":service,"operation":operation,"durationMicros":duration_micros,"sampled":sampled}),
                )
            }
            Record::Log {
                id,
                identity,
                message,
            } => {
                unique(&mut ids, &id)?;
                identity.validate()?;
                crate::diagnostics::model::label(&message, 4096)?;
                (
                    "event",
                    "service log",
                    json!({"category":"logs","id":id,"identity":identity,"message":message}),
                )
            }
        };
        observation["source"] = json!(input.source);
        observation["coverage"] = json!(input.coverage);
        emit(index, kind, name, source, observation)?;
    }
    for id in retries.keys() {
        let mut chain = BTreeSet::new();
        let mut current = id;
        while let Some(next) = retries.get(current) {
            if !chain.insert(current) || chain.len() > 4096 {
                return Err(invalid());
            }
            current = next;
        }
    }
    index.unsupported.push("Network exchange preserves producer coverage, retry/cache/cancellation and sampling claims; IDs do not authenticate services or prove causality; phase clocks and encrypted bodies are not inferred".into());
    Ok(())
}
fn unique(ids: &mut BTreeSet<String>, id: &str) -> Result<()> {
    label(id)?;
    if !ids.insert(id.into()) {
        return Err(invalid());
    }
    Ok(())
}
// Strict version-00 W3C traceparent support. Unknown versions remain unavailable.
fn traceparent(value: &str) -> Option<(String, String, bool)> {
    let p: Vec<_> = value.split('-').collect();
    if p.len() != 4
        || p[0] != "00"
        || !hex(p[1], 32)
        || !hex(p[2], 16)
        || p[3].len() != 2
        || !p[3]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    Some((
        p[1].into(),
        p[2].into(),
        u8::from_str_radix(p[3], 16).ok()? & 1 != 0,
    ))
}
pub(super) fn har(entry: &Value, i: usize, index: &mut Index) -> Result<()> {
    let mut phases = BTreeMap::new();
    for name in [
        "blocked", "dns", "connect", "ssl", "send", "wait", "receive",
    ] {
        if entry["timings"]
            .get(name)
            .is_some_and(|v| !v.is_null() && !v.is_number())
        {
            return Err(invalid());
        }
        let value = entry["timings"].get(name).and_then(Value::as_f64);
        if let Some(n) = value
            && (!n.is_finite() || n < 0.0 && n != -1.0)
        {
            return Err(invalid());
        }
        phases.insert(name, value.filter(|n| *n >= 0.0));
    }
    let headers = entry["request"]["headers"].as_array();
    let parents: Vec<_> = headers
        .into_iter()
        .flatten()
        .filter(|h| {
            h["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case("traceparent"))
        })
        .collect();
    let trace = if parents.len() == 1 {
        parents[0]["value"].as_str().and_then(traceparent)
    } else {
        None
    };
    emit(
        index,
        "event",
        "HAR request phases",
        format!("/log/entries/{i}"),
        json!({"category":"requests","id":format!("har:{i}"),"identity":{"scope":"HAR import","traceId":trace.as_ref().map(|p|&p.0),"spanId":trace.as_ref().map(|p|&p.1)},"protocol":entry["request"]["httpVersion"],"address":entry["request"]["url"],"method":entry["request"]["method"],"status":entry["response"]["status"],"phasesMillis":phases,"traceSampled":trace.as_ref().map(|p|p.2),"traceContextAvailable":trace.is_some(),"phaseMeaning":"HAR -1/missing is unavailable; ssl is included in connect; blocked/connect/TLS may overlap; no additive phase total, cache/retry/cancellation inference or wall-clock alignment"}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Query {
    operation: String,
    id: Option<String>,
    identity_scope: Option<String>,
    trace_id: Option<String>,
    request_id: Option<String>,
    collection: Option<String>,
    key: Option<String>,
    equals: Option<data::Equals>,
}
pub(super) fn query(index: &Index, q: &Query, offset: usize, limit: usize) -> Result<Value> {
    if matches!(q.operation.as_str(), "data" | "schema" | "query_plan") {
        return data::query(index, q, offset, limit);
    }
    if !matches!(
        q.operation.as_str(),
        "requests" | "phases" | "messages" | "spans" | "logs" | "packets" | "correlate"
    ) || q.collection.is_some()
        || q.key.is_some()
        || q.equals.is_some()
    {
        return Err(Error::invalid(
            "Select a network operation or data/schema/query_plan with its own selectors",
        ));
    }
    if let Some(id) = &q.id {
        label(id)?;
    }
    if let Some(scope) = &q.identity_scope {
        label(scope)?;
    }
    if q.trace_id.as_ref().is_some_and(|s| !hex(s, 32)) {
        return Err(Error::invalid("Use an exact lowercase nonzero traceId"));
    }
    if let Some(id) = &q.request_id {
        label(id)?;
    }
    if q.trace_id.is_some() && q.request_id.is_some()
        || q.request_id.is_some() && q.identity_scope.is_none()
    {
        return Err(Error::invalid(
            "Choose traceId or an exact requestId with identityScope",
        ));
    }
    if q.operation == "correlate"
        && (q.trace_id.is_none() && q.request_id.is_none() || q.id.is_some())
    {
        return Err(Error::invalid(
            "Correlation requires traceId or scoped requestId; no fuzzy ID/URL/time joins",
        ));
    }
    let selected: Vec<_> = index
        .rows
        .iter()
        .filter(|r| {
            let n = &r.data["networkData"];
            let category = n["category"].as_str();
            (if q.operation == "correlate" {
                matches!(category, Some("requests" | "spans" | "logs"))
            } else if q.operation == "phases" {
                category == Some("requests")
            } else {
                category == Some(q.operation.as_str())
            }) && q.id.as_ref().is_none_or(|id| n["id"] == *id)
                && q.trace_id
                    .as_ref()
                    .is_none_or(|id| n["identity"]["traceId"] == *id)
                && q.request_id
                    .as_ref()
                    .is_none_or(|id| n["identity"]["requestId"] == *id)
                && q.identity_scope
                    .as_ref()
                    .is_none_or(|id| n["identity"]["scope"] == *id)
        })
        .map(|r| json!({"source":r.source,"observation":r.data["networkData"]}))
        .collect();
    let mut result = page(selected, offset, limit);
    result["operation"] = json!(q.operation);
    result["unsupported"] = json!(index.unsupported);
    result["interpretation"] = json!(
        "exact declared identifiers only; trace matches are association candidates, request IDs are scope-local; missing server spans can reflect missing propagation/sampling/coverage; no cross-clock latency, body decryption or causal proof"
    );
    Ok(result)
}
fn page(rows: Vec<Value>, offset: usize, limit: usize) -> Value {
    let matched = rows.len();
    let next = offset.saturating_add(limit).min(matched);
    json!({"rows":rows.into_iter().skip(offset).take(limit).collect::<Vec<_>>(),"matched":matched,"nextOffset":if next<matched{Some(next)}else{None}})
}
pub(super) fn compare(
    a: &super::model::Receipt,
    ai: &Index,
    b: &super::model::Receipt,
    bi: &Index,
) -> Value {
    data::compare(a, ai, b, bi)
}
/// A selected settled HTTP body read can carry a configured broker's typed export.
/// It borrows existing authenticated HTTP access and never sends or retries a query.
pub(super) fn browser_body(packet: &Value, index: &mut Index) -> Result<()> {
    if packet["request"]["operation"] != "http_evidence_read"
        || packet["request"]["format"] != "json"
        || packet["operation"]["outcome"] != "succeeded"
        || packet["operation"]["result"]["format"] != "json"
    {
        return Ok(());
    }
    let value = &packet["operation"]["result"]["value"];
    if value["format"] == "data-snapshot-v1" {
        data::decode(value.clone(), index, "/operation/result/value")?;
    }
    Ok(())
}
/// Existing RN/CDP observations keep request epochs; event phases are not guessed durations.
pub(super) fn runtime_rows(index: &mut Index) -> Result<()> {
    let original = index.rows.len();
    for i in 0..original {
        let row = &index.rows[i];
        let event = &row.data["observation"]["event"];
        let Some(method) = event["method"].as_str() else {
            continue;
        };
        if !matches!(
            method,
            "Network.requestWillBeSent"
                | "Network.responseReceived"
                | "Network.loadingFinished"
                | "Network.loadingFailed"
                | "Network.webSocketFrameSent"
                | "Network.webSocketFrameReceived"
        ) {
            continue;
        }
        let p = &event["params"];
        let Some(id) = p["requestId"].as_str() else {
            continue;
        };
        label(id)?;
        let epoch = json!([
            row.data["targetId"],
            row.data["pageId"],
            row.data["runtimeGeneration"],
            row.data["executionContextId"]
        ]);
        let scope = format!(
            "runtime:{}",
            crate::diagnostics::digest(epoch.to_string().as_bytes())
        );
        let data = json!({"category":if method.contains("webSocketFrame"){"messages"}else{"requests"},"id":id,"identity":{"scope":scope,"requestId":id},"event":method,"producerTimestamp":p["timestamp"],"timestampMeaning":"CDP producer timestamp preserved; no native/collector clock alignment","address":p["request"]["url"],"method":p["request"]["method"],"status":p["response"]["status"],"protocol":p["response"]["protocol"],"fromDiskCache":p["response"]["fromDiskCache"],"fromServiceWorker":p["response"]["fromServiceWorker"],"cancelled":p["canceled"],"encodedDataLength":p["encodedDataLength"],"errorText":p["errorText"],"redirectResponse":p["redirectResponse"],"websocketOpcode":p["response"]["opcode"],"payloadIncluded":false,"coverage":"inspector-visible runtime events; native traffic and earlier/dropped events may be absent"});
        let source = row.source.clone();
        emit(index, "event", "Runtime network lifecycle", source, data)?;
    }
    Ok(())
}
