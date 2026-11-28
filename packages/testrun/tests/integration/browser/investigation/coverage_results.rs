//! Installed SDK coverage: private sources, navigation epochs and UTF-16 ranges.
use super::*;

fn report(value: &Value, format: &str) -> Value {
    let report = value["reports"]
        .as_array()
        .unwrap()
        .iter()
        .find(|report| report["format"] == format)
        .unwrap_or_else(|| panic!("Missing {format}: {value}"));
    assert_eq!(report["complete"], true, "{report}");
    assert_eq!(report["sourceTextsIncluded"], true, "{report}");
    serde_json::from_slice(&std::fs::read(report["attachment"]["path"].as_str().unwrap()).unwrap())
        .unwrap()
}
async fn seed(mcp: &mut Mcp, target: &str, page: &Value, label: &str) {
    let script = format!(
        "/* 😀 Acme {label} */ function acmeNamed() {{ return 42; }} acmeNamed();\n//# sourceURL=https://example.com/acme-{label}.js?token=Acme"
    );
    let css = format!(
        "/* 😀 Acme {label} */ #acmeCoverageProbe {{ color: red; }} .acmeUnused {{ display: none; }}\n/*# sourceURL=https://example.com/acme-{label}.css?token=Acme */"
    );
    let expression = format!(
        "()=>{{new Function({})(); const style=document.createElement('style');style.textContent={};document.head.append(style);document.body.innerHTML='<div id=acmeCoverageProbe>Acme</div>';return getComputedStyle(document.querySelector('#acmeCoverageProbe')).color;}}",
        json!(script),
        json!(css)
    );
    operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}),
    )
    .await;
}
#[expect(
    clippy::too_many_lines,
    reason = "One installed workflow verifies navigation policy, sources, SDK ranges, CPU coexistence and cleanup together"
)]
pub(super) async fn run(fixture: &Fixture, mcp: &mut Mcp, target: &str, page: &Value, base: &str) {
    for reset in [true, false] {
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":base}})).await;
        let mut options = json!({"cpu":true,"javascriptCoverage":true,"cssCoverage":true,"coverageFormat":"playwright"});
        if !reset {
            options["resetOnNavigation"] = json!(false);
        }
        let request =
            json!({"operation":"profile_start","page":page,"options":options}).to_string();
        let started = std::time::Instant::now();
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &request])
            .await;
        let profile = completed(mcp, target, submitted).await;
        let setup_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(profile["sourceTextsIncluded"], true);
        assert_eq!(
            profile["navigation"],
            if reset {
                "reset-on-navigation"
            } else {
                "cumulative"
            }
        );
        seed(mcp, target, page, "before").await;
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{base}/other?coverage={}",uuid::Uuid::new_v4())}})).await;
        seed(mcp, target, page, "after").await;
        let taking = std::time::Instant::now();
        let first = operation(
            mcp,
            target,
            json!({"operation":"profile_take_coverage","page":page}),
        )
        .await;
        let take_ms = taking.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(first["complete"], true, "{first}");
        let js = report(&first, "playwright-javascript-coverage-delta");
        let entries = js["entries"].as_array().unwrap();
        assert!(
            entries
                .iter()
                .all(|entry| entry["url"].as_str().is_some_and(|url| !url.is_empty()))
        );
        let before = entries
            .iter()
            .find(|entry| entry["url"] == "https://example.com/acme-before.js?token=Acme");
        if reset {
            assert!(before.is_none(), "{js}");
        }
        if let Some(before) = before {
            assert!(before["source"].as_str().unwrap().contains("Acme before"));
        }
        let after = entries
            .iter()
            .find(|entry| entry["url"] == "https://example.com/acme-after.js?token=Acme")
            .unwrap_or_else(|| panic!("{js}"));
        assert!(after["source"].as_str().unwrap().contains("😀 Acme after"));
        assert!(
            after["functions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|function| function["functionName"] == "acmeNamed"
                    && function["ranges"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|range| range["count"].as_u64().unwrap() > 0))
        );
        let css = report(&first, "playwright-css-coverage-delta");
        let entries = css["entries"].as_array().unwrap();
        assert_eq!(
            entries.iter().any(|entry| entry["text"]
                .as_str()
                .is_some_and(|text| text.contains("Acme before"))),
            !reset,
            "{css}"
        );
        let after = entries
            .iter()
            .find(|entry| {
                entry["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("Acme after"))
            })
            .unwrap_or_else(|| panic!("{css}"));
        assert!(!after["ranges"].as_array().unwrap().is_empty(), "{after}");
        let units = after["text"].as_str().unwrap().encode_utf16().count() as u64;
        let text = after["text"].as_str().unwrap();
        let start = text[..text.find("#acmeCoverageProbe").unwrap()]
            .encode_utf16()
            .count();
        let end = text[..=text.find('}').unwrap()].encode_utf16().count();
        assert!(
            after["ranges"]
                .as_array()
                .unwrap()
                .iter()
                .any(|range| *range == json!({"start":start,"end":end})),
            "{after}"
        );
        for range in after["ranges"].as_array().unwrap() {
            assert!(
                range["start"].as_u64().unwrap() < range["end"].as_u64().unwrap()
                    && range["end"].as_u64().unwrap() <= units
            );
        }
        seed(mcp, target, page, "final").await;
        let stopping = std::time::Instant::now();
        let stopped = operation(mcp, target, json!({"operation":"profile_stop","page":page})).await;
        let stop_ms = stopping.elapsed().as_secs_f64() * 1000.0;
        let js = report(&stopped, "playwright-javascript-coverage");
        assert!(
            js["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["url"] == "https://example.com/acme-final.js?token=Acme")
        );
        report(&stopped, "playwright-css-coverage");
        assert!(
            stopped["reports"]
                .as_array()
                .unwrap()
                .iter()
                .any(|report| report["format"] == "v8-cpu-profile")
        );
        for result in [&first, &stopped] {
            for report in result["reports"].as_array().unwrap() {
                operation(mcp,target,json!({"operation":"developer_evidence_remove","evidenceId":report["evidenceId"]})).await;
            }
        }
        eprintln!(
            "SDK coverage reset={reset}: {}",
            json!({"freshCliSetupMs":setup_ms,"retainedCoverageTakeMs":take_ms,"retainedStopMs":stop_ms,"workflowIncludingEvidenceRemovalMs":started.elapsed().as_secs_f64()*1000.0})
        );
    }
}

#[tokio::test]
async fn installed_step_four_coverage_results_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut cleanup_failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready().await;
            let result=std::panic::AssertUnwindSafe(async {
                let page=operation(&mut mcp,target,json!({"operation":"page_create","context":ready["browser"]["selectedContext"],"select":false})).await["page"].clone();
                if engine=="chrome" { run(&fixture,&mut mcp,target,&page,&site.url).await; }
                else { super::collectors::refused(&mut mcp,target,json!({"operation":"profile_start","page":page,"options":{"javascriptCoverage":true,"cssCoverage":true,"coverageFormat":"playwright"}}),"NATIVE_DEVELOPER_UNSUPPORTED",true).await; }
                operation(&mut mcp,target,json!({"operation":"page_close","page":page})).await;
                eprintln!("Coverage result finished {engine} {mode}");
            }).catch_unwind().await;
            let disconnected = mcp
                .request(
                    "tools/call",
                    json!({"name":"disconnect","arguments":{"targetId":target}}),
                )
                .await;
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("coverage-disconnect-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(&disconnected).unwrap(),
            )
            .unwrap();
            let released = disconnected["result"]["structuredContent"]["ok"] == true;
            if !released {
                // Preserve the first failure while allowing the same owned cleanup
                // to finish. A later success does not turn this run green.
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                cleanup_failures.push(json!({"engine":engine,"mode":mode,"response":disconnected}));
            }
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(
        cleanup_failures.is_empty(),
        "Coverage disconnects: {cleanup_failures:?}"
    );
}
