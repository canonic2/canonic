//! Stage recording, readiness parsing and bounded readiness polling.
use super::*;
use crate::error::Error;
use serde_json::json;

#[test]
fn readiness_arguments_are_bounded_and_validated_before_dispatch() {
    assert!(Readiness::parse(&json!({})).unwrap().is_none());
    let foreground = Readiness::parse(&json!({"readiness":{}})).unwrap().unwrap();
    assert_eq!(foreground.condition(), "foreground");
    assert_eq!(foreground.timeout, DEFAULT_READINESS);
    let element = Readiness::parse(
        &json!({"readiness":{"element":{"identifier":"acme.home"},"timeoutMs":500}}),
    )
    .unwrap()
    .unwrap();
    assert_eq!(element.condition(), "foreground_element");
    assert!(element.needs_tree());
    for invalid in [
        json!({"readiness":true}),
        json!({"readiness":{"timeoutMs":50}}),
        json!({"readiness":{"timeoutMs":30001}}),
        json!({"readiness":{"element":{}}}),
        json!({"readiness":{"element":{"label":{"text":"(","mode":"regex"}}}}),
        json!({"readiness":{"pid":1}}),
    ] {
        assert_eq!(
            Readiness::parse(&invalid).err().unwrap().code,
            "INVALID_ARGUMENT",
            "{invalid}"
        );
    }
}

#[tokio::test]
async fn readiness_waits_for_foreground_and_the_requested_element() {
    let readiness = Readiness::parse(
        &json!({"readiness":{"element":{"identifier":"acme.home"},"timeoutMs":2000}}),
    )
    .unwrap()
    .unwrap();
    let mut calls = 0;
    let report = wait(&readiness, || {
        calls += 1;
        let call = calls;
        async move {
            match call {
                1 => Err(Error::new("PLATFORM_FAILED", "transient read")),
                2 => Ok(Observed {
                    foreground: false,
                    evidence: json!({"focusedPackage":"com.example.launcher"}),
                    root: None,
                }),
                3 => Ok(Observed {
                    foreground: true,
                    evidence: json!({"focusedPackage":"com.example.acme"}),
                    root: Some(json!({"children":[{"identifier":"acme.splash"}]})),
                }),
                _ => Ok(Observed {
                    foreground: true,
                    evidence: json!({"focusedPackage":"com.example.acme"}),
                    root: Some(json!({"children":[{"identifier":"acme.home","label":"Home"}]})),
                }),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(report["state"], "ready");
    assert_eq!(report["attempts"], 4);
    assert_eq!(report["element"]["identifier"], "acme.home");
}

#[tokio::test]
async fn unmet_readiness_reports_the_last_observation_after_its_deadline() {
    let readiness = Readiness::parse(&json!({"readiness":{"timeoutMs":300}}))
        .unwrap()
        .unwrap();
    let started = std::time::Instant::now();
    let error = wait(&readiness, || async {
        Ok(Observed {
            foreground: false,
            evidence: json!({"focusedPackage":"com.example.launcher"}),
            root: None,
        })
    })
    .await
    .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(error.code, "APP_NOT_READY");
    let report = &error.details.unwrap()["readiness"];
    assert_eq!(report["state"], "not_ready");
    assert_eq!(
        report["lastObserved"]["focusedPackage"],
        "com.example.launcher"
    );
}

#[test]
fn restart_failure_keeps_completed_stages_and_the_cause_code() {
    let mut stages = Stages::new(true, true);
    stages
        .record("terminate", Ok(json!({"wasRunning":true})))
        .unwrap();
    let error = stages
        .record(
            "launch",
            Err(Error::new(
                "TIMEOUT",
                "Platform operation exceeded its deadline",
            )),
        )
        .unwrap_err();
    assert_eq!(error.code, "TIMEOUT");
    assert!(error.message.contains("completed: terminate"), "{error}");
    let details = error.details.unwrap();
    assert_eq!(details["replayed"], false);
    assert_eq!(details["stages"]["terminate"]["state"], "completed");
    assert_eq!(details["stages"]["terminate"]["wasRunning"], true);
    assert_eq!(details["stages"]["launch"]["state"], "failed");
    assert_eq!(details["stages"]["readiness"]["state"], "not_started");

    let mut launch = Stages::new(false, true);
    launch.record("launch", Ok(json!({"pid":42}))).unwrap();
    let error = launch
        .record(
            "readiness",
            Err(Error::new("APP_NOT_READY", "not ready")
                .details(json!({"readiness":{"state":"not_ready","attempts":3}}))),
        )
        .unwrap_err();
    let details = error.details.unwrap();
    assert!(details["stages"].get("terminate").is_none());
    assert_eq!(details["stages"]["launch"]["state"], "completed");
    assert_eq!(details["stages"]["readiness"]["attempts"], 3);
}
