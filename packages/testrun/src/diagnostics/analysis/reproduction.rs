//! Bounded repeated-attempt evidence with explicit environment and failure retention.
use super::model::{Index, MAX_RAW, Receipt};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Run {
    schema_version: u32,
    run_id: String,
    scenario: String,
    scenario_version: String,
    build_id: String,
    target_id: String,
    app_id: String,
    runtime: String,
    os_version: String,
    hardware: String,
    mode: String,
    warm_state: String,
    warmup_attempts: u32,
    sampling: String,
    instrumentation: Vec<String>,
    contention: String,
    fixture_id: String,
    metric: Metric,
    budget: Budget,
    termination: String,
    cleanup: String,
    coverage: String,
    attempts: Vec<Attempt>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Metric {
    name: String,
    unit: String,
    definition: String,
    direction: String,
    uncertainty: Option<f64>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Budget {
    max_attempts: usize,
    max_duration_ms: u64,
    elapsed_ms: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Attempt {
    id: String,
    action_id: String,
    at_ms: u64,
    outcome: String,
    reason: String,
    value: Option<f64>,
    evidence_refs: Vec<String>,
    fault: Option<Fault>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Fault {
    kind: String,
    owner: String,
    scope: String,
    before: String,
    applied: String,
    after: String,
    cleanup: String,
    receipt_ref: String,
}
fn invalid() -> Error {
    Error::new(
        "INVALID_REPRODUCTION",
        "Invalid or unbounded reproduction evidence",
    )
}
fn label(s: &str) -> Result<()> {
    crate::diagnostics::model::label(s, 256)
}
pub(super) fn decode(bytes: &[u8], index: &mut Index) -> Result<()> {
    let mut run: Run = serde_json::from_value(super::decode::bounded_json(bytes, MAX_RAW)?)
        .map_err(|_| invalid())?;
    validate_run(&run)?;
    let mut ids = BTreeSet::new();
    let mut last = 0;
    for attempt in &run.attempts {
        for s in [&attempt.id, &attempt.action_id, &attempt.reason]
            .into_iter()
            .chain(attempt.evidence_refs.iter())
        {
            label(s)?;
        }
        if !ids.insert(&attempt.id)
            || attempt.at_ms < last
            || attempt.at_ms > run.budget.elapsed_ms
            || attempt.evidence_refs.len() > 16
            || !matches!(
                attempt.outcome.as_str(),
                "passed" | "failed" | "invalid" | "unavailable" | "cancelled"
            )
            || attempt
                .value
                .is_some_and(|v| !v.is_finite() || !(0.0..=1e15).contains(&v))
        {
            return Err(invalid());
        }
        last = attempt.at_ms;
        if let Some(f) = &attempt.fault {
            for s in [
                &f.kind,
                &f.owner,
                &f.scope,
                &f.before,
                &f.applied,
                &f.after,
                &f.receipt_ref,
            ] {
                label(s)?;
            }
            if !matches!(
                f.cleanup.as_str(),
                "verified" | "failed" | "unknown" | "not_reversible"
            ) || !matches!(
                f.kind.as_str(),
                "offline"
                    | "delay"
                    | "response"
                    | "lifecycle"
                    | "permission"
                    | "pressure"
                    | "fixture"
            ) {
                return Err(invalid());
            }
        }
    }
    // Instrumentation ordering is not a difference; duplicate declarations refuse.
    run.instrumentation.sort();
    if run.instrumentation.windows(2).any(|p| p[0] == p[1]) {
        return Err(invalid());
    }
    let attempts = std::mem::take(&mut run.attempts);
    let mut context = serde_json::to_value(&run).map_err(|_| invalid())?;
    context.as_object_mut().unwrap().remove("attempts");
    context["identityProvenance"] = json!("producer_declared_not_authenticated");
    context["receiptReferencesVerified"] = json!(false);
    super::decode::push(
        index,
        super::decode::row("process", "reproduction_run", "/run".into(), context),
    )?;
    for (n, attempt) in attempts.into_iter().enumerate() {
        let mut row = super::decode::row(
            if attempt.outcome == "passed" {
                "metric"
            } else {
                "failure"
            },
            &run.metric.name,
            format!("/attempts/{n}"),
            json!({"runId":run.run_id,"attempt":attempt,"observationOnly":true}),
        );
        if let Some(value) = attempt.value {
            row.value = Some(value);
            row.unit = Some(run.metric.unit.clone());
            row.definition = Some(run.metric.definition.clone());
        }
        row.correlations = vec![attempt.id.clone(), attempt.action_id.clone()];
        super::decode::push(index, row)?;
    }
    Ok(())
}
fn context(index: &Index) -> Option<&Value> {
    (index.format == "reproduction-v1")
        .then(|| index.rows.first())
        .flatten()
        .filter(|r| r.name == "reproduction_run")
        .map(|r| &r.data)
}
fn summary(index: &Index) -> Value {
    let attempts: Vec<_> = index.rows.iter().skip(1).collect();
    let mut values: Vec<_> = attempts
        .iter()
        .filter(|r| r.data["attempt"]["outcome"] == "passed")
        .filter_map(|r| r.value)
        .collect();
    values.sort_by(f64::total_cmp);
    let n = values.len();
    let median = if n == 0 {
        None
    } else {
        Some(values[(n - 1) / 2] / 2.0 + values[n / 2] / 2.0)
    };
    let failures = attempts
        .iter()
        .filter(|r| r.data["attempt"]["outcome"] != "passed" || r.value.is_none())
        .count();
    let fault_gaps = attempts
        .iter()
        .filter(|r| {
            let f = &r.data["attempt"]["fault"];
            f.is_object() && (f["cleanup"] != "verified" || f["before"] != f["after"])
        })
        .count();
    json!({"attempts":attempts.len(),"validSamples":n,"failedInvalidUnavailableOrMissingValue":failures,"faultCleanupGaps":fault_gaps,
        "median":median,"min":values.first(),"max":values.last(),"mean":if n==0{None}else{Some(values.iter().map(|v|v/n as f64).sum::<f64>())},
        "sampleDefinition":"one recorded passed attempt; failed/invalid/unavailable/missing measurements are retained and disqualify improvement claims"})
}
pub(super) fn compare(a: &Receipt, ai: &Index, b: &Receipt, bi: &Index) -> Value {
    let mut reasons = vec![];
    let ac = context(ai);
    let bc = context(bi);
    let sa = summary(ai);
    let sb = summary(bi);
    if let (Some(ac), Some(bc)) = (ac, bc) {
        compare_contexts(ac, bc, &mut reasons);
        let schedule = |index: &Index| {
            index.rows.iter().skip(1).map(|r|{
            let f=&r.data["attempt"]["fault"];
            json!({"actionId":r.data["attempt"]["actionId"],"fault":if f.is_object(){json!([f["kind"],f["owner"],f["scope"],f["before"],f["applied"]])}else{Value::Null}})
        }).collect::<Vec<_>>()
        };
        if schedule(ai) != schedule(bi) {
            reasons.push("action_or_fault_schedule");
        }
    } else {
        reasons.push("format");
    }
    for (r, index, c, s) in [(a, ai, ac, &sa), (b, bi, bc, &sb)] {
        if r.phase != "complete"
            || r.capture_phase != "complete"
            || !index.unsupported.is_empty()
            || !r.metadata.observation_complete
            || !r.metadata.source_loss.is_empty()
        {
            reasons.push("capture_or_observation_incomplete");
        }
        if let Some(c) = c {
            for key in ["targetId", "appId", "buildId"] {
                if !r.binding[key].is_null() && r.binding[key] != c[key] {
                    reasons.push("investigation_binding_conflict");
                }
            }
            if c["budget"]["elapsedMs"].as_u64() > c["budget"]["maxDurationMs"].as_u64()
                || s["attempts"].as_u64() > c["budget"]["maxAttempts"].as_u64()
            {
                reasons.push("declared_budget_exceeded");
            }
            if ["unit", "definition"].iter().any(|key| {
                matches!(
                    c["metric"][*key].as_str(),
                    Some("unknown" | "producer-unspecified")
                )
            }) {
                reasons.push("measurement_unknown");
            }
        }
        if s["validSamples"].as_u64().unwrap_or(0) < 2 {
            reasons.push("insufficient_samples");
        }
        if s["failedInvalidUnavailableOrMissingValue"] != 0 || s["faultCleanupGaps"] != 0 {
            reasons.push("failed_attempt_or_fault_cleanup");
        }
    }
    reasons.sort_unstable();
    reasons.dedup();
    let delta = if reasons.is_empty() {
        sa["median"]
            .as_f64()
            .zip(sb["median"].as_f64())
            .map(|(x, y)| y - x)
    } else {
        None
    };
    let uncertainty = if reasons.is_empty() {
        ac.and_then(|c| c["metric"]["uncertainty"].as_f64())
            .zip(bc.and_then(|c| c["metric"]["uncertainty"].as_f64()))
            .map(|(x, y)| x + y)
    } else {
        None
    };
    json!({"compatible":reasons.is_empty(),"incompatible":reasons,"method":"reproduction-observations/1","baseline":{"analysisId":a.id,"rawSha256":a.raw_sha256,"context":ac,"samples":sa},"candidate":{"analysisId":b.id,"rawSha256":b.raw_sha256,"context":bc,"samples":sb},"candidateMinusBaselineMedian":delta,"medianDifferenceUncertainty":uncertainty,"uncertaintyMeaning":"sum of producer-declared absolute per-sample bounds in metric units; null means unknown/incompatible; not a statistical confidence interval",
        "buildPolicy":"different exact declared buildIds allowed; scenario/environment definitions must match; identities are not authenticated",
        "conclusion":"observed differences only; no causal, statistical significance or tail-latency guarantee; query every retained attempt","private":true})
}

fn validate_run(run: &Run) -> Result<()> {
    if run.schema_version != 1
        || !(1..=1024).contains(&run.budget.max_attempts)
        || run.attempts.len() > 1024
        || !(1..=86400000).contains(&run.budget.max_duration_ms)
        || run.budget.elapsed_ms > 86400000
        || run.warmup_attempts > 1024
        || run.instrumentation.len() > 32
        || run
            .metric
            .uncertainty
            .is_some_and(|v| !v.is_finite() || !(0.0..=1e15).contains(&v))
        || !matches!(
            run.metric.direction.as_str(),
            "lower" | "higher" | "neutral"
        )
        || !matches!(run.warm_state.as_str(), "cold" | "warm" | "hot" | "unknown")
        || !matches!(
            run.hardware.as_str(),
            "physical" | "simulator" | "emulator" | "browser" | "host" | "unknown"
        )
        || !matches!(
            run.termination.as_str(),
            "completed"
                | "cancelled"
                | "failure"
                | "budget_exhausted"
                | "cleanup_failed"
                | "source_lost"
        )
        || !matches!(run.cleanup.as_str(), "verified" | "failed" | "unknown")
        || !matches!(run.coverage.as_str(), "complete" | "partial" | "unknown")
    {
        return Err(invalid());
    }
    for s in [
        &run.run_id,
        &run.scenario,
        &run.scenario_version,
        &run.build_id,
        &run.target_id,
        &run.app_id,
        &run.runtime,
        &run.os_version,
        &run.mode,
        &run.sampling,
        &run.contention,
        &run.fixture_id,
        &run.metric.name,
        &run.metric.unit,
        &run.metric.definition,
    ]
    .into_iter()
    .chain(run.instrumentation.iter())
    {
        label(s)?;
    }
    Ok(())
}

fn compare_contexts(ac: &Value, bc: &Value, reasons: &mut Vec<&'static str>) {
    for field in [
        "scenario",
        "scenarioVersion",
        "targetId",
        "appId",
        "runtime",
        "osVersion",
        "hardware",
        "mode",
        "warmState",
        "warmupAttempts",
        "sampling",
        "instrumentation",
        "contention",
        "fixtureId",
        "metric",
    ] {
        if ac[field] != bc[field] {
            reasons.push(field);
        }
    }
    for c in [ac, bc] {
        if c["warmState"] == "unknown"
            || c["hardware"] == "unknown"
            || c["contention"] == "unknown"
            || c["sampling"] == "unknown"
        {
            reasons.push("environment_unknown");
        }
        if c["coverage"] != "complete"
            || c["cleanup"] != "verified"
            || c["termination"] != "completed"
        {
            reasons.push("incomplete_run_or_cleanup");
        }
        if [
            "buildId",
            "targetId",
            "appId",
            "runtime",
            "osVersion",
            "mode",
            "fixtureId",
        ]
        .iter()
        .any(|f| c[*f] == "unknown")
        {
            reasons.push("identity_or_fixture_unknown");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(build: &str, values: [f64; 2]) -> Value {
        json!({"schemaVersion":1,"runId":build,"scenario":"checkout","scenarioVersion":"v1","buildId":build,"targetId":"android:Acme","appId":"com.example.acme","runtime":"ART","osVersion":"API36","hardware":"emulator","mode":"debug","warmState":"warm","warmupAttempts":2,"sampling":"per attempt elapsed","instrumentation":["markers"],"contention":"isolated fixture","fixtureId":"data-v1","metric":{"name":"checkout","unit":"milliseconds","definition":"request to rendered result","direction":"lower"},"budget":{"maxAttempts":2,"maxDurationMs":10000,"elapsedMs":2000},"termination":"completed","cleanup":"verified","coverage":"complete","attempts":[
            {"id":"a1","actionId":"checkout","atMs":100,"outcome":"passed","reason":"result observed","value":values[0],"evidenceRefs":["capture-1"]},
            {"id":"a2","actionId":"checkout","atMs":1000,"outcome":"passed","reason":"result observed","value":values[1],"evidenceRefs":["capture-2"]}
        ]})
    }
    fn index(v: Value) -> Index {
        super::super::decode::decode("reproduction-v1", &serde_json::to_vec(&v).unwrap()).unwrap()
    }
    fn receipt() -> Receipt {
        serde_json::from_value(json!({"version":1,"id":"analysis-a","collectorId":"collector-a","signature":"s","owner":"fixture","phase":"complete","rawSha256":"00","rawBytes":1,"format":"reproduction-v1","capturePhase":"complete","captureReason":null,"binding":{},"metadata":{"observationComplete":true},"indexSha256":"00","reason":null})).unwrap()
    }
    #[test]
    fn diagnostic_reproduction_comparison_allows_exact_different_builds_and_reports_observed_units()
    {
        let a = index(run("build-a", [100.0, 120.0]));
        let b = index(run("build-b", [70.0, 90.0]));
        let r = compare(&receipt(), &a, &receipt(), &b);
        assert_eq!(r["compatible"], true, "{r}");
        assert_eq!(r["candidateMinusBaselineMedian"], -30.0);
        assert_eq!(r["baseline"]["samples"]["validSamples"], 2);
        assert_eq!(a.rows[0].data["receiptReferencesVerified"], false);
        assert!(a.rows.iter().all(|r| r.timestamp.is_none()));
        assert!(r["medianDifferenceUncertainty"].is_null());
        let mut v = run("a", [100.0, 120.0]);
        v["metric"]["uncertainty"] = json!(1.0);
        let measured = index(v);
        assert_eq!(
            compare(&receipt(), &measured, &receipt(), &measured)["medianDifferenceUncertainty"],
            2.0
        );
    }
    #[test]
    fn diagnostic_reproduction_failed_attempts_environment_and_cleanup_never_disappear() {
        let a = index(run("build-a", [100.0, 120.0]));
        let mut cases = vec![];
        let mut v = run("b", [70.0, 90.0]);
        v["budget"]["elapsedMs"] = json!(10001);
        cases.push(v);
        let mut v = run("b", [70.0, 90.0]);
        v["budget"]["maxAttempts"] = json!(1);
        cases.push(v);
        let mut v = run("b", [70.0, 90.0]);
        v["warmState"] = json!("cold");
        cases.push(v);
        let mut v = run("b", [70.0, 90.0]);
        v["attempts"][0]["outcome"] = json!("invalid");
        cases.push(v);
        let mut v = run("b", [70.0, 90.0]);
        v["cleanup"] = json!("failed");
        v["termination"] = json!("cleanup_failed");
        cases.push(v);
        let mut v = run("b", [70.0, 90.0]);
        v["attempts"][0]["fault"] = json!({"kind":"offline","owner":"network_set","scope":"selected target","before":"online","applied":"offline","after":"offline","cleanup":"failed","receiptRef":"network-1"});
        cases.push(v);
        for v in cases {
            let b = index(v);
            let r = compare(&receipt(), &a, &receipt(), &b);
            assert_eq!(b.rows.len(), 3);
            assert_eq!(r["compatible"], false, "{r}");
            assert!(r["candidateMinusBaselineMedian"].is_null());
        }
    }
    #[test]
    fn diagnostic_reproduction_rejects_duplicate_attempts_unbounded_records_and_arbitrary_execution()
     {
        let mut cases = vec![];
        let mut v = run("a", [1.0, 2.0]);
        v["attempts"][1]["id"] = json!("a1");
        cases.push(v);
        let mut v = run("a", [1.0, 2.0]);
        v["budget"]["maxAttempts"] = json!(1025);
        cases.push(v);
        let mut v = run("a", [1.0, 2.0]);
        v["budget"]["elapsedMs"] = json!(86400001);
        cases.push(v);
        let mut v = run("a", [1.0, 2.0]);
        v["command"] = json!(["must-not-run"]);
        cases.push(v);
        let mut v = run("a", [1.0, 2.0]);
        v["metric"]["uncertainty"] = json!(-1);
        cases.push(v);
        for v in cases {
            assert!(
                super::super::decode::decode("reproduction-v1", &serde_json::to_vec(&v).unwrap())
                    .is_err()
            );
        }
    }
}
