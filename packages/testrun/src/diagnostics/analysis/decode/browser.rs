//! Index browser-owned results; native heap/trace semantics stay with their analyzers.
use super::{bounded_json, invalid, push, row};
use crate::diagnostics::analysis::model::*;
use crate::error::Result;
use serde_json::{Value, json};

pub(super) fn decode(bytes: &[u8], index: &mut Index) -> Result<()> {
    let packet = bounded_json(bytes, MAX_RAW)?;
    if packet["schemaVersion"] != 1
        || packet["operation"]["settled"] != true
        || !matches!(
            packet["engine"].as_str(),
            Some("chrome" | "firefox" | "webkit")
        )
    {
        return Err(invalid());
    }
    let target = packet["targetId"].as_str().ok_or_else(invalid)?;
    crate::browser::runtime::connection(target)?;
    let operation = packet["operation"]["id"].as_str().ok_or_else(invalid)?;
    uuid::Uuid::parse_str(operation).map_err(|_| invalid())?;
    let request = packet["request"]["operation"]
        .as_str()
        .ok_or_else(invalid)?;
    crate::diagnostics::model::label(request, 128)?;
    let actions = packet["actionIds"].as_array().cloned().unwrap_or_default();
    if actions.len() > 32 {
        return Err(invalid());
    }
    let mut correlations = Vec::new();
    for action in actions {
        let action = action.as_str().ok_or_else(invalid)?;
        crate::diagnostics::model::label(action, 256)?;
        correlations.push(action.to_string());
    }
    let base = json!({"targetId":target,"engine":packet["engine"],"backend":packet["backend"],
        "sessionId":packet["sessionId"],"sessionGeneration":packet["sessionGeneration"],"browserIdentity":packet["browserIdentity"],
        "operationId":operation,"requestOperation":request,"requestWitness":"/request","scopes":packet["scopes"],
        "appId":null,"processGeneration":null,"rendererProcessId":null,
        "identityProvenance":"retained packet producer; imported packets are not authenticated",
        "causeEstablished":false,"nativeArtifactOwnership":"browser owner; no file/model pin or copy"});
    let mut summary = base.clone();
    summary["outcome"] = packet["operation"]["outcome"].clone();
    summary["code"] = packet["operation"]["code"].clone();
    emit(
        index,
        if matches!(summary["outcome"].as_str(), Some("failed" | "unknown")) {
            "failure"
        } else {
            "event"
        },
        "Browser operation outcome",
        "/operation".into(),
        summary,
        &correlations,
    )?;
    let result = &packet["operation"]["result"];
    if let Some(fields) = result.as_object() {
        for (key, value) in fields {
            let source = format!(
                "/operation/result/{}",
                key.replace('~', "~0").replace('/', "~1")
            );
            if let Some(items) = value.as_array() {
                for (i, item) in items.iter().enumerate() {
                    let kind = if matches!(request, "diagnostics_read" | "diagnostics_wait")
                        && key == "events"
                        && matches!(item["kind"].as_str(), Some("page_error" | "request_failed"))
                    {
                        "failure"
                    } else {
                        "event"
                    };
                    result_row(
                        index,
                        kind,
                        &format!("{source}/{i}"),
                        item,
                        &base,
                        &correlations,
                    )?;
                }
            } else {
                result_row(index, "event", &source, value, &base, &correlations)?;
            }
        }
    } else if !result.is_null() {
        result_row(
            index,
            "event",
            "/operation/result",
            result,
            &base,
            &correlations,
        )?;
    }
    index
        .unsupported
        .push("browser_operation_result_is_not_complete_browser_history".into());
    index.unsupported.push("native_heap_trace_profile_and_artifact_queries_keep_browser_owners_and_limits; no shared raw decoder inferred".into());
    super::super::network_data::browser_body(&packet, index)?;
    Ok(())
}
fn result_row(
    index: &mut Index,
    kind: &str,
    source: &str,
    value: &Value,
    base: &Value,
    correlations: &[String],
) -> Result<()> {
    let size = serde_json::to_vec(value).map_err(|_| invalid())?.len();
    let mut data = base.clone();
    if size > 32768 {
        data["gap"] = json!(
            "native_result_field_exceeds_shared_row_budget; read raw witness or select a smaller native query"
        );
        data["encodedBytes"] = json!(size);
        index
            .unsupported
            .push(format!("oversized_native_field:{source}"));
    } else {
        data["nativeResult"] = value.clone();
    }
    emit(
        index,
        kind,
        "Browser native result",
        source.into(),
        data,
        correlations,
    )
}
fn emit(
    index: &mut Index,
    kind: &str,
    name: &str,
    source: String,
    data: Value,
    correlations: &[String],
) -> Result<()> {
    let mut r = row(kind, name, source, data);
    r.correlations = correlations.to_vec();
    push(index, r)
}
