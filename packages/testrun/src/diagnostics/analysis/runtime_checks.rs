//! Explicit normalized reports; absence of a report never establishes a clean run.
use super::model::{Index, MAX_RAW, Row};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Capture {
    schema_version: u32,
    target_id: String,
    app_id: String,
    process_generation: String,
    build_id: String,
    timing: String,
    environment: Vec<String>,
    coverage: String,
    reports: Vec<Report>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Report {
    id: String,
    tool: String,
    category: String,
    message: String,
    thread_id: Option<String>,
    stacks: Vec<Stack>,
    raw_reference: String,
    fatal: Option<bool>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Stack {
    role: String,
    thread_id: Option<String>,
    frames: Vec<String>,
}
fn invalid() -> Error {
    Error::new(
        "INVALID_RUNTIME_CHECKS",
        "Invalid normalized runtime-check evidence",
    )
}
fn label(s: &str) -> Result<()> {
    crate::diagnostics::model::label(s, 1024)
}
pub(super) fn decode(bytes: &[u8], index: &mut Index) -> Result<()> {
    let capture: Capture = serde_json::from_value(super::decode::bounded_json(bytes, MAX_RAW)?)
        .map_err(|_| invalid())?;
    if capture.schema_version != 1
        || capture.reports.len() > 1024
        || capture.environment.len() > 32
        || !matches!(capture.timing.as_str(), "instrumented" | "unknown")
    {
        return Err(invalid());
    }
    for text in [
        &capture.target_id,
        &capture.app_id,
        &capture.process_generation,
        &capture.build_id,
        &capture.coverage,
    ]
    .into_iter()
    .chain(capture.environment.iter())
    {
        label(text)?;
    }
    let mut ids = BTreeSet::new();
    for report in capture.reports {
        if !ids.insert(report.id.clone())
            || !matches!(
                report.tool.as_str(),
                "address_sanitizer"
                    | "thread_sanitizer"
                    | "undefined_behavior_sanitizer"
                    | "strict_mode"
                    | "race_detector"
                    | "lock_checker"
                    | "runtime_check"
            )
            || !matches!(
                report.category.as_str(),
                "memory"
                    | "race"
                    | "lock"
                    | "undefined_behavior"
                    | "policy_violation"
                    | "assertion"
                    | "other"
            )
            || report.message.is_empty()
            || report.message.len() > 16384
            || report.stacks.len() > 8
        {
            return Err(invalid());
        }
        for text in [&report.id, &report.raw_reference]
            .into_iter()
            .chain(report.thread_id.iter())
        {
            label(text)?;
        }
        for stack in &report.stacks {
            if !matches!(
                stack.role.as_str(),
                "fault"
                    | "allocation"
                    | "free"
                    | "conflicting_access"
                    | "lock_acquired"
                    | "thread_created"
                    | "other"
            ) || stack.frames.len() > 64
            {
                return Err(invalid());
            }
            for text in stack.frames.iter().chain(stack.thread_id.iter()) {
                label(text)?;
            }
        }
        let row = Row {
            kind: "failure".into(),
            name: report.tool.clone(),
            timestamp: None,
            clock_domain: None,
            value: None,
            unit: None,
            definition: None,
            correlations: vec![report.id.clone()],
            source: format!("runtime-checks-v1/{}", report.id),
            data: json!({"report":report,"targetId":capture.target_id,"appId":capture.app_id,
                "processGeneration":capture.process_generation,"buildId":capture.build_id,
                "timing":capture.timing,"environment":capture.environment,"coverage":capture.coverage,
                "identityProvenance":"producer_declared_not_authenticated","normalization":"producer_normalized_report_not_native_report_parser",
                "absenceProvesCleanRun":false,"rawReferenceVerified":false}),
        };
        super::decode::push(index, row)?;
    }
    index.unsupported.push("runtime-check coverage and identity are producer declarations; missing reports do not prove a clean run".into());
    Ok(())
}

/// Preserve native text as bounded blocks. No heuristic stack symbolication or
/// process attribution: compiler-runtime output is not an authenticated app ID.
pub(super) fn sanitizer_text(bytes: &[u8], index: &mut Index) -> Result<()> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut blocks: Vec<(&str, Vec<&str>)> = vec![];
    for line in text.lines() {
        if line.len() > 4096 {
            return Err(invalid());
        }
        let tool = if line.contains("ERROR: AddressSanitizer:") {
            Some("address_sanitizer")
        } else if line.contains("WARNING: ThreadSanitizer:") {
            Some("thread_sanitizer")
        } else if line.contains("SUMMARY: UndefinedBehaviorSanitizer:") {
            Some("undefined_behavior_sanitizer")
        } else {
            None
        };
        if let Some(tool) = tool {
            if blocks.len() >= 1024 {
                return Err(invalid());
            }
            blocks.push((tool, vec![]));
        }
        if let Some((_, lines)) = blocks.last_mut() {
            if lines.len() >= 256 {
                return Err(invalid());
            }
            lines.push(line);
        }
    }
    if blocks.is_empty() {
        return Err(invalid());
    }
    for (i, (tool, lines)) in blocks.into_iter().enumerate() {
        let row = Row {
            kind: "failure".into(),
            name: tool.into(),
            timestamp: None,
            clock_domain: None,
            value: None,
            unit: None,
            definition: None,
            correlations: vec![format!("report-{i}")],
            source: format!("sanitizer-report-text/{i}"),
            data: json!({"text":lines.join("\n"),"timing":"instrumented","identityProvenance":"unattributed_native_text",
                "absenceProvesCleanRun":false,"stackParsing":"raw_text_only","coverage":"unknown"}),
        };
        super::decode::push(index, row)?;
    }
    index.unsupported.push("sanitizer text is unattributed, unverified and incomplete; use runtime-checks-v1 for explicit producer identity/stacks; preamble is retained only in raw evidence".into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet() -> serde_json::Value {
        json!({"schemaVersion":1,"targetId":"android:test","appId":"com.example.acme",
            "processGeneration":"pid:42/start:3","buildId":"build-a","timing":"instrumented",
            "environment":["ASAN_OPTIONS=detect_leaks=1"],"coverage":"selected instrumented process",
            "reports":[{"id":"r1","tool":"address_sanitizer","category":"memory","message":"heap-use-after-free",
                "threadId":"T1","stacks":[{"role":"fault","threadId":"T1","frames":["#0 read /src/main.cc:4"]}],
                "rawReference":"artifact:asan-log","fatal":true}]})
    }
    #[test]
    fn diagnostic_runtime_checks_preserve_evidence_without_inventing_time_or_authority() {
        for tool in [
            "address_sanitizer",
            "thread_sanitizer",
            "undefined_behavior_sanitizer",
            "strict_mode",
            "race_detector",
            "lock_checker",
            "runtime_check",
        ] {
            let mut p = packet();
            p["reports"][0]["tool"] = json!(tool);
            let index =
                super::super::decode::decode("runtime-checks-v1", &serde_json::to_vec(&p).unwrap())
                    .unwrap();
            assert_eq!(index.rows[0].name, tool);
            assert!(index.rows[0].timestamp.is_none());
            assert_eq!(index.rows[0].data["absenceProvesCleanRun"], false);
            assert_eq!(index.rows[0].data["rawReferenceVerified"], false);
            assert!(!index.unsupported.is_empty());
        }
    }
    #[test]
    fn diagnostic_runtime_checks_refuse_ambiguous_identity_unbounded_stacks_and_unknown_fields() {
        let mut cases = vec![];
        let mut p = packet();
        p["processGeneration"] = json!("");
        cases.push(p);
        let mut p = packet();
        p["timing"] = json!("production_like");
        cases.push(p);
        let mut p = packet();
        p["reports"][0]["evaluate"] = json!("evil()");
        cases.push(p);
        let mut p = packet();
        p["reports"][0]["stacks"][0]["frames"] = json!(vec!["frame"; 65]);
        cases.push(p);
        let mut p = packet();
        let duplicate = p["reports"][0].clone();
        p["reports"].as_array_mut().unwrap().push(duplicate);
        cases.push(p);
        for p in cases {
            assert!(
                super::super::decode::decode("runtime-checks-v1", &serde_json::to_vec(&p).unwrap())
                    .is_err()
            );
        }
    }
    #[test]
    fn diagnostic_sanitizer_text_preserves_native_reports_and_refuses_unknown_or_oversized_blocks()
    {
        let text=b"preamble\n==42==ERROR: AddressSanitizer: heap-use-after-free\n #0 Acme /src/a.cc:3\nSUMMARY: AddressSanitizer: heap-use-after-free\nWARNING: ThreadSanitizer: data race\nSUMMARY: ThreadSanitizer: data race\nSUMMARY: UndefinedBehaviorSanitizer: undefined-behavior /src/b.cc:4\n";
        let index = super::super::decode::decode("sanitizer-report-text", text).unwrap();
        assert_eq!(index.rows.len(), 3);
        assert_eq!(
            index.rows[0].data["identityProvenance"],
            "unattributed_native_text"
        );
        assert!(
            index.rows[0].data["text"]
                .as_str()
                .unwrap()
                .contains("/src/a.cc:3")
        );
        assert!(super::super::decode::decode("sanitizer-report-text", b"no findings").is_err());
        let oversized = format!("ERROR: AddressSanitizer: fault\n{}", "x".repeat(4097));
        assert!(
            super::super::decode::decode("sanitizer-report-text", oversized.as_bytes()).is_err()
        );
    }
}
