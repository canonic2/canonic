//! Runner-owned results and capture/attachment references; no runner execution.
mod junit;
#[cfg(test)]
mod tests;
use super::model::{Index, MAX_RAW};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Report {
    schema_version: u32,
    run_id: String,
    runner: String,
    invocation_ref: Option<String>,
    control: String,
    build_id: String,
    target_id: String,
    coverage: String,
    cases: Vec<Case>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Case {
    id: String,
    attempt: u32,
    name: String,
    outcome: String,
    message: String,
    duration_ms: Option<f64>,
    action_ids: Vec<String>,
    attachments: Vec<Attachment>,
    capture: Capture,
    checks: Vec<Check>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Attachment {
    kind: String,
    name: String,
    reference: String,
    sha256: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Capture {
    strategy: String,
    before_ms: u64,
    after_ms: u64,
    limit_bytes: u64,
    lost: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Check {
    id: String,
    domain: String,
    outcome: String,
    message: String,
}
fn invalid() -> Error {
    Error::new(
        "INVALID_TEST_RESULTS",
        "Invalid, ambiguous or unsupported bounded test results",
    )
}
fn label(s: &str) -> Result<()> {
    crate::diagnostics::model::label(s, 256)
}
fn message(s: &str) -> Result<()> {
    if s.len() > 8192 {
        Err(invalid())
    } else {
        Ok(())
    }
}
pub(super) fn decode(format: &str, bytes: &[u8], index: &mut Index) -> Result<()> {
    if format == "junit-xml" {
        return junit::decode(bytes, index);
    }
    let report: Report = serde_json::from_value(super::decode::bounded_json(bytes, MAX_RAW)?)
        .map_err(|_| invalid())?;
    if report.schema_version != 1
        || report.cases.len() > 1024
        || !matches!(
            report.runner.as_str(),
            "xctest"
                | "swift_testing"
                | "android_instrumentation"
                | "espresso"
                | "ui_automator"
                | "flutter"
                | "dart"
                | "web"
                | "package_native"
                | "external"
        )
        || !matches!(
            report.control.as_str(),
            "none" | "testrun_released" | "external_owned" | "competing" | "unknown"
        )
        || !matches!(report.coverage.as_str(), "complete" | "partial" | "unknown")
    {
        return Err(invalid());
    }
    for s in [&report.run_id, &report.build_id, &report.target_id]
        .into_iter()
        .chain(report.invocation_ref.iter())
    {
        label(s)?;
    }
    let mut ids = BTreeSet::new();
    for (ordinal, case) in report.cases.into_iter().enumerate() {
        validate_case(&case, &mut ids)?;
        if case.capture.lost {
            index.unsupported.push(format!(
                "Capture loss for case {} attempt {}",
                case.id, case.attempt
            ));
        }
        let mut row = super::decode::row(
            if matches!(
                case.outcome.as_str(),
                "passed" | "skipped" | "unavailable" | "cancelled"
            ) {
                "event"
            } else {
                "failure"
            },
            &case.name,
            format!("/cases/{ordinal}"),
            json!({"test":case,"runId":report.run_id,"runner":report.runner,"invocationRef":report.invocation_ref,"control":report.control,"buildId":report.build_id,"targetId":report.target_id,"coverage":report.coverage,"identityProvenance":"producer_declared","attachmentsVerified":false,"captureBoundsVerified":false,"competingControl":report.control=="competing"}),
        );
        row.correlations = vec![
            report.run_id.clone(),
            case.id.clone(),
            format!("attempt:{}", case.attempt),
        ];
        row.correlations.extend(case.action_ids);
        super::decode::push(index, row)?;
    }
    if report.coverage != "complete" || matches!(report.control.as_str(), "competing" | "unknown") {
        index.unsupported.push("Test coverage or exclusive control is incomplete/unknown; runner declarations are not authenticated".into());
    }
    Ok(())
}

fn validate_case(case: &Case, ids: &mut BTreeSet<(String, u32)>) -> Result<()> {
    label(&case.id)?;
    label(&case.name)?;
    message(&case.message)?;
    if !ids.insert((case.id.clone(), case.attempt))
        || case.attempt == 0
        || case.attempt > 1024
        || !matches!(
            case.outcome.as_str(),
            "passed"
                | "assertion_failed"
                | "automation_failed"
                | "fixture_failed"
                | "unavailable"
                | "skipped"
                | "cancelled"
        )
        || case
            .duration_ms
            .is_some_and(|n| !n.is_finite() || !(0.0..=86400000.0).contains(&n))
        || case.action_ids.len() > 16
        || case.attachments.len() > 16
        || case.checks.len() > 32
        || !matches!(
            case.capture.strategy.as_str(),
            "none" | "prebuffered" | "post_failure"
        )
        || case.capture.before_ms > 300000
        || case.capture.after_ms > 300000
        || case.capture.limit_bytes > 8388608
        || (case.capture.strategy != "prebuffered" && case.capture.before_ms != 0)
        || (case.capture.strategy == "none"
            && (case.capture.after_ms != 0 || case.capture.limit_bytes != 0))
    {
        return Err(invalid());
    }
    for s in &case.action_ids {
        label(s)?;
    }
    for a in &case.attachments {
        validate_attachment(a)?;
    }
    let mut check_ids = BTreeSet::new();
    for c in &case.checks {
        label(&c.id)?;
        message(&c.message)?;
        if !check_ids.insert(&c.id)
            || !matches!(
                c.domain.as_str(),
                "ui" | "api" | "accessibility" | "visual" | "media" | "performance"
            )
            || !matches!(
                c.outcome.as_str(),
                "passed" | "assertion_failed" | "unavailable"
            )
            || case.outcome == "passed" && c.outcome != "passed"
        {
            return Err(invalid());
        }
    }

    Ok(())
}
fn validate_attachment(a: &Attachment) -> Result<()> {
    label(&a.name)?;
    label(&a.reference)?;
    if !matches!(
        a.kind.as_str(),
        "log"
            | "crash"
            | "profile"
            | "screenshot"
            | "video"
            | "audio"
            | "trace"
            | "junit"
            | "xcresult"
            | "other"
    ) || a
        .sha256
        .as_ref()
        .is_some_and(|s| s.len() != 64 || !s.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err(invalid());
    }
    Ok(())
}
