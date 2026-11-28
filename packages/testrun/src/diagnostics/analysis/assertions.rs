//! Fixed predicates over retained observations; never evaluates app or host code.
use super::model::{Index, Receipt};
use crate::error::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
#[cfg(test)]
mod tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Request {
    checks: Vec<Check>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Check {
    id: String,
    domain: String,
    kind: Option<String>,
    name: Option<String>,
    correlation_id: Option<String>,
    predicate: Predicate,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Predicate {
    Count {
        min: usize,
        max: Option<usize>,
    },
    Equals {
        pointer: String,
        expected: Value,
    },
    Metric {
        unit: String,
        definition: String,
        min: Option<f64>,
        max: Option<f64>,
        aggregation: String,
    },
}
fn invalid() -> Error {
    Error::invalid(
        "Use 1..32 unique bounded assertion checks with supported domains and fixed predicates",
    )
}
pub(in crate::diagnostics) fn schema() -> Value {
    let label = json!({"type":"string","minLength":1,"maxLength":256});
    let object = |p: Value, r: &[&str]| json!({"type":"object","properties":p,"required":r,"additionalProperties":false});
    object(
        json!({"checks":{"type":"array","minItems":1,"maxItems":32,"items":object(json!({"id":label,"domain":{"enum":["ui","api","accessibility","visual","media","performance"]},"kind":label,"name":label,"correlationId":label,"predicate":{"oneOf":[
        object(json!({"type":{"const":"count"},"min":{"type":"integer","minimum":0,"maximum":20000},"max":{"type":"integer","minimum":0,"maximum":20000}}),&["type","min"]),
        object(json!({"type":{"const":"equals"},"pointer":{"type":"string","maxLength":256},"expected":{"type":["string","number","boolean","null"],"maxLength":4096}}),&["type","pointer","expected"]),
        object(json!({"type":{"const":"metric"},"unit":label,"definition":label,"min":{"type":"number"},"max":{"type":"number"},"aggregation":{"enum":["all","mean","max"]}}),&["type","unit","definition","aggregation"])
    ]}}),&["id","domain","predicate"])}}),
        &["checks"],
    )
}
pub(super) fn evaluate(receipt: &Receipt, index: &Index, request: &Request) -> Result<Value> {
    if request.checks.is_empty() || request.checks.len() > 32 {
        return Err(invalid());
    }
    let complete = receipt.capture_phase == "complete"
        && receipt.metadata.observation_complete
        && receipt.metadata.source_loss.is_empty()
        && index.unsupported.is_empty();
    let mut ids = BTreeSet::new();
    let mut results = vec![];
    for c in &request.checks {
        for s in [&c.id]
            .into_iter()
            .chain(c.kind.iter())
            .chain(c.name.iter())
            .chain(c.correlation_id.iter())
        {
            crate::diagnostics::model::label(s, 256)?;
        }
        if !ids.insert(&c.id)
            || !matches!(
                c.domain.as_str(),
                "ui" | "api" | "accessibility" | "visual" | "media" | "performance"
            )
        {
            return Err(invalid());
        }
        if c.kind.as_deref().is_some_and(|kind| {
            !matches!(
                kind,
                "event"
                    | "failure"
                    | "metric"
                    | "stack"
                    | "thread"
                    | "allocation"
                    | "retention"
                    | "span"
                    | "frame"
                    | "action"
                    | "marker"
                    | "process"
                    | "screenshot"
                    | "video"
                    | "audio"
            )
        }) {
            return Err(invalid());
        }
        let rows: Vec<_> = index
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                c.kind.as_ref().is_none_or(|k| &r.kind == k)
                    && c.name.as_ref().is_none_or(|n| &r.name == n)
                    && c.correlation_id
                        .as_ref()
                        .is_none_or(|id| r.correlations.contains(id))
            })
            .take(201)
            .collect();
        let (status, reason) = if rows.len() > 200 {
            ("unavailable", "selection_exceeds_200_rows")
        } else if rows.iter().any(|(_, r)| r.data["competingControl"] == true) {
            ("automation_failed", "competing_device_control")
        } else if rows
            .iter()
            .any(|(_, r)| r.data["test"]["outcome"] == "fixture_failed")
        {
            ("fixture_failed", "recorded_fixture_failure")
        } else if rows
            .iter()
            .any(|(_, r)| r.data["test"]["outcome"] == "automation_failed")
        {
            ("automation_failed", "recorded_automation_failure")
        } else if rows.iter().any(|(_, r)| {
            matches!(
                r.data["test"]["outcome"].as_str(),
                Some("unavailable" | "skipped" | "cancelled")
            )
        }) {
            ("unavailable", "test_did_not_establish_assertion_evidence")
        } else {
            test(&c.predicate, &rows, complete)?
        };
        results.push(json!({"id":c.id,"domain":c.domain,"outcome":status,"reason":reason,"matched":if rows.len()>200{None}else{Some(rows.len())},"matchedAtLeast":rows.len(),"selectionTruncated":rows.len()>200,"witnessOrdinals":rows.iter().take(8).map(|(i,_)|*i).collect::<Vec<_>>()}));
    }
    let outcome = [
        "fixture_failed",
        "automation_failed",
        "assertion_failed",
        "unavailable",
    ]
    .into_iter()
    .find(|s| results.iter().any(|r| r["outcome"] == *s))
    .unwrap_or("passed");
    Ok(
        json!({"outcome":outcome,"checks":results,"analysisId":receipt.id,"collectorId":receipt.collector_id,"rawSha256":receipt.raw_sha256,"indexSha256":receipt.index_sha256,"observationComplete":complete,"scope":"selected retained observations; declared domain does not establish source authority or accessibility conformance","executionPerformed":false,"private":true}),
    )
}
fn test(
    p: &Predicate,
    rows: &[(usize, &super::model::Row)],
    complete: bool,
) -> Result<(&'static str, &'static str)> {
    let pass = |ok| {
        if ok {
            ("passed", "predicate_satisfied")
        } else {
            ("assertion_failed", "predicate_contradicted")
        }
    };
    match p {
        Predicate::Count { min, max } => {
            if *min > 20000 || max.is_some_and(|n| n > 20000 || n < *min) {
                return Err(invalid());
            }
            if max.is_some_and(|m| rows.len() > m) {
                return Ok(pass(false));
            }
            if !complete && (rows.len() < *min || max.is_some()) {
                return Ok(("unavailable", "complete_observation_required_for_count"));
            }
            Ok(pass(rows.len() >= *min))
        }
        Predicate::Equals { pointer, expected } => {
            if pointer.len() > 256
                || (!pointer.is_empty() && !pointer.starts_with('/'))
                || pointer.split('/').count() > 16
                || expected.is_object()
                || expected.is_array()
                || expected.as_str().is_some_and(|s| s.len() > 4096)
            {
                return Err(invalid());
            }
            if rows.is_empty() {
                return Ok(("unavailable", "no_selected_observation"));
            }
            let values: Option<Vec<_>> = rows
                .iter()
                .map(|(_, r)| {
                    r.data
                        .pointer(pointer)
                        .filter(|v| !v.is_object() && !v.is_array())
                })
                .collect();
            Ok(match values {
                Some(v) => pass(v.iter().all(|v| *v == expected)),
                None => ("unavailable", "scalar_pointer_unavailable"),
            })
        }
        Predicate::Metric {
            unit,
            definition,
            min,
            max,
            aggregation,
        } => {
            crate::diagnostics::model::label(unit, 256)?;
            crate::diagnostics::model::label(definition, 256)?;
            if min.is_none() && max.is_none()
                || min.iter().chain(max.iter()).any(|n| !n.is_finite())
                || min.zip(*max).is_some_and(|(a, b)| a > b)
                || !matches!(aggregation.as_str(), "all" | "mean" | "max")
            {
                return Err(invalid());
            }
            if rows.is_empty() || !complete {
                return Ok(("unavailable", "complete_metric_observations_required"));
            }
            if matches!(unit.as_str(), "unknown" | "producer-unspecified")
                || definition == "unknown"
            {
                return Ok(("unavailable", "measurement_semantics_unknown"));
            }
            if rows.iter().any(|(_, r)| {
                r.unit.as_ref() != Some(unit)
                    || r.definition.as_ref() != Some(definition)
                    || r.value.is_none()
            }) {
                return Ok(("unavailable", "metric_definition_or_unit_mismatch"));
            }
            let values: Vec<_> = rows.iter().map(|(_, r)| r.value.unwrap()).collect();
            let selected = match aggregation.as_str() {
                "mean" => vec![values.iter().map(|v| v / values.len() as f64).sum()],
                "max" => vec![values.iter().copied().reduce(f64::max).unwrap()],
                _ => values,
            };
            if selected.iter().any(|v| !v.is_finite()) {
                return Ok(("unavailable", "aggregation_not_finite"));
            }
            Ok(pass(selected.iter().all(|v| {
                min.is_none_or(|m| *v >= m) && max.is_none_or(|m| *v <= m)
            })))
        }
    }
}
