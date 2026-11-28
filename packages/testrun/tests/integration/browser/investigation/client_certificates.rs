//! Owned TLS peers compare identity scope and inheritance with the pinned SDK.
pub(super) use super::certificate_fixture as wire;
use super::*;

async fn reference(
    fixture: &Fixture,
    engine: &str,
    mode: &str,
    keys: &wire::Identities,
    first: &wire::Site,
    second: &wire::Site,
) -> Value {
    let input = fixture
        .workspace
        .path()
        .join(format!("client-cert-input-{engine}-{mode}.json"));
    std::fs::write(&input,serde_json::to_vec(&json!({"first":first.url,"second":second.url,
        "certificates":[keys.input(&format!("{}/Acme-normalized-path",first.url),0),keys.input(&second.url,1)],
        "formats":{
            "encryptedPem":[keys.encrypted_input(&first.url,0,false),keys.encrypted_input(&second.url,1,true)],
            "pfx":[keys.encrypted_input(&first.url,0,true),keys.encrypted_input(&second.url,1,false)]}
        })).unwrap()).unwrap();
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-client-certificates-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(&input)
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PW_LANG_NAME", "javascript")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .output()
    .await
    .unwrap();
    for (suffix, bytes) in [("json", &output.stdout), ("stderr", &output.stderr)] {
        std::fs::write(
            fixture
                .workspace
                .path()
                .join(format!("client-cert-sdk-{engine}-{mode}.{suffix}")),
            bytes,
        )
        .unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ownedProcesses"], 0);
    for (name, identity) in [
        ("multiA", "A"),
        ("multiB", "B"),
        ("multiRedirect", "B"),
        ("singleRedirect", "none"),
        ("empty", "none"),
    ] {
        assert_eq!(
            report["cases"][name],
            json!({"status":200,"identity":identity})
        );
    }
    if engine != "chrome" {
        assert_eq!(report["cases"]["inheritedA"]["identity"], "A");
        assert_eq!(report["cases"]["inheritedRedirect"]["identity"], "B");
    }
    for format in ["encryptedPem", "pfx"] {
        assert_eq!(report["cases"][format!("{format}A")]["identity"], "A");
        assert_eq!(
            report["cases"][format!("{format}Redirect")]["identity"],
            "B"
        );
    }
    report
}
#[tokio::test]
async fn pinned_client_certificates_reference_checkpoint() {
    let fixture = installed_fixture();
    let keys = wire::Identities::new().await;
    let first = wire::Site::start(&keys).await;
    let second = wire::Site::start(&keys).await;
    first.redirect_to(&format!("{}/destination", second.url));
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            reference(&fixture, engine, mode, &keys, &first, &second).await;
        }
    }
}
async fn create(mcp: &mut Mcp, target: &str, options: Value) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"http_context_create","options":options}),
    )
    .await["httpContext"]
        .clone()
}
async fn observed(
    mcp: &mut Mcp,
    target: &str,
    http: &Value,
    url: &str,
    expected: &str,
    first: &wire::Site,
    second: &wire::Site,
    shared: bool,
) {
    let before = [
        first.requests.lock().unwrap().len(),
        second.requests.lock().unwrap().len(),
    ];
    let request = json!({"url":url,"readBody":true});
    let response = if shared {
        let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"http_fetch","httpContext":http,"request":request}]})).await;
        completed(mcp, target, submitted).await["results"][0].clone()
    } else {
        operation(
            mcp,
            target,
            json!({"operation":"http_fetch","httpContext":http,"request":request}),
        )
        .await
    };
    assert_eq!(response["status"], 200);
    assert_eq!(response["replayed"], false);
    assert_eq!(response["retryCount"], 0);
    let (site, index) = if url.ends_with("/redirect") || url.starts_with(&second.url) {
        (second, 1)
    } else {
        (first, 0)
    };
    assert_eq!(site.requests.lock().unwrap().len(), before[index] + 1);
    assert_eq!(
        site.requests.lock().unwrap().last().unwrap()["identity"],
        expected
    );
    if url.ends_with("/redirect") {
        assert_eq!(first.requests.lock().unwrap().len(), before[0] + 1);
    }
    let body=operation(mcp,target,json!({"operation":"http_evidence_read","evidenceId":response["body"]["evidenceId"],"format":"text"})).await;
    assert_eq!(
        body["value"], expected,
        "The retained body must agree with both the actual TLS peer and the SDK reference"
    );
    operation(
        mcp,
        target,
        json!({"operation":"http_evidence_remove","evidenceId":response["body"]["evidenceId"]}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"http_response_remove","requestId":response["requestId"]}),
    )
    .await;
}
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    initial: &Value,
    keys: &wire::Identities,
    first: &wire::Site,
    second: &wire::Site,
) -> Value {
    let certificates = json!([
        keys.input(&format!("{}/Acme-normalized-path", first.url), 0),
        keys.input(&second.url, 1)
    ]);
    if engine != "chrome" {
        let inherited = create(
            mcp,
            target,
            json!({"browserContext":initial,"allowUnknownCookiePartitions":engine=="firefox"}),
        )
        .await;
        observed(
            mcp, target, &inherited, &first.url, "A", first, second, false,
        )
        .await;
        observed(
            mcp,
            target,
            &inherited,
            &format!("{}/redirect", first.url),
            "B",
            first,
            second,
            true,
        )
        .await;
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":inherited}),
        )
        .await;
    }
    let before = std::time::Instant::now();
    let submitted=fixture.cli(&["browser","send","--target",target,&json!({"operation":"http_context_create","options":{"clientCertificates":certificates,"ignoreHttpsErrors":true}}).to_string()]).await;
    let multi = completed(mcp, target, submitted).await["httpContext"].clone();
    let fresh_ms = before.elapsed().as_secs_f64() * 1000.0;
    observed(mcp, target, &multi, &first.url, "A", first, second, true).await;
    observed(mcp, target, &multi, &second.url, "B", first, second, false).await;
    observed(
        mcp,
        target,
        &multi,
        &format!("{}/redirect", first.url),
        "B",
        first,
        second,
        true,
    )
    .await;
    let single = create(
        mcp,
        target,
        json!({"clientCertificates":[keys.input(&first.url,0)],"ignoreHttpsErrors":true}),
    )
    .await;
    observed(
        mcp,
        target,
        &single,
        &format!("{}/redirect", first.url),
        "none",
        first,
        second,
        false,
    )
    .await;
    let empty = create(
        mcp,
        target,
        json!({"clientCertificates":[],"ignoreHttpsErrors":true}),
    )
    .await;
    observed(
        mcp, target, &empty, &first.url, "none", first, second, false,
    )
    .await;
    for http in [&multi, &single, &empty] {
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":http}),
        )
        .await;
    }
    let mut format_metrics = Vec::new();
    for pfx in [false, true] {
        let selected = json!([
            keys.encrypted_input(&first.url, 0, pfx),
            keys.encrypted_input(&second.url, 1, !pfx)
        ]);
        let preparation_started = std::time::Instant::now();
        let context = create(
            mcp,
            target,
            json!({"clientCertificates":selected,"ignoreHttpsErrors":true}),
        )
        .await;
        format_metrics.push(json!({"format":if pfx {"pfx"} else {"encryptedPem"},
            "retainedCreateMs":preparation_started.elapsed().as_secs_f64()*1000.0}));
        observed(mcp, target, &context, &first.url, "A", first, second, false).await;
        observed(
            mcp,
            target,
            &context,
            &format!("{}/redirect", first.url),
            "B",
            first,
            second,
            true,
        )
        .await;
        let mut wrong = selected.clone();
        wrong[0]["passphrase"] = json!("incorrect");
        let before_contexts =
            operation(mcp, target, json!({"operation":"http_context_list"})).await;
        let before_requests = (
            first.requests.lock().unwrap().len(),
            second.requests.lock().unwrap().len(),
        );
        collectors::refused(
            mcp,
            target,
            json!({"operation":"http_context_create","options":{
            "clientCertificates":wrong,"ignoreHttpsErrors":true}}),
            if pfx {
                "HTTP_CLIENT_CERTIFICATE_PFX_INVALID"
            } else {
                "HTTP_CLIENT_CERTIFICATE_KEY_UNSUPPORTED"
            },
            true,
        )
        .await;
        assert_eq!(
            operation(mcp, target, json!({"operation":"http_context_list"})).await,
            before_contexts
        );
        assert_eq!(
            (
                first.requests.lock().unwrap().len(),
                second.requests.lock().unwrap().len()
            ),
            before_requests
        );
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":context}),
        )
        .await;
        if engine != "chrome" {
            let browser = operation(mcp,target,json!({"operation":"context_create","options":{
                "viewport":{"width":640,"height":480},"deviceScaleFactor":1,"ignoreHTTPSErrors":true,
                "clientCertificates":selected}})).await["context"].clone();
            let inherited = create(
                mcp,
                target,
                json!({"browserContext":browser,
                "allowUnknownCookiePartitions":engine=="firefox"}),
            )
            .await;
            observed(
                mcp, target, &inherited, &first.url, "A", first, second, false,
            )
            .await;
            observed(
                mcp,
                target,
                &inherited,
                &format!("{}/redirect", first.url),
                "B",
                first,
                second,
                true,
            )
            .await;
            operation(
                mcp,
                target,
                json!({"operation":"http_context_remove","httpContext":inherited}),
            )
            .await;
            operation(
                mcp,
                target,
                json!({"operation":"context_close","context":browser}),
            )
            .await;
        }
    }
    let mut context_options = json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"clientCertificates":certificates});
    if engine != "chrome" {
        context_options["ignoreHTTPSErrors"] = json!(true);
    }
    if engine == "chrome" {
        // Roadmap 5.1: native Chrome accepts ignoreHTTPSErrors with client
        // certificates, as the pinned SDK does.
        for value in [true, false] {
            let mut accepted = context_options.clone();
            accepted["ignoreHTTPSErrors"] = json!(value);
            let created = operation(
                mcp,
                target,
                json!({"operation":"context_create","options":accepted}),
            )
            .await;
            operation(
                mcp,
                target,
                json!({"operation":"context_close","context":created["context"]}),
            )
            .await;
        }
        let context = operation(
            mcp,
            target,
            json!({"operation":"context_create","options":context_options}),
        )
        .await["context"]
            .clone();
        let inherited = create(
            mcp,
            target,
            json!({"browserContext":context,"ignoreHttpsErrors":true}),
        )
        .await;
        observed(
            mcp, target, &inherited, &first.url, "A", first, second, false,
        )
        .await;
        observed(
            mcp,
            target,
            &inherited,
            &second.url,
            "B",
            first,
            second,
            true,
        )
        .await;
        operation(
            mcp,
            target,
            json!({"operation":"http_context_remove","httpContext":inherited}),
        )
        .await;
        operation(
            mcp,
            target,
            json!({"operation":"context_close","context":context}),
        )
        .await;
    } else {
        let context = operation(
            mcp,
            target,
            json!({"operation":"context_create","options":context_options}),
        )
        .await["context"]
            .clone();
        if engine == "firefox" {
            collectors::refused(
                mcp,
                target,
                json!({"operation":"http_context_create","options":{"browserContext":context}}),
                "HTTP_COOKIE_PARTITION_IDENTITY_UNAVAILABLE",
                true,
            )
            .await;
        }
        let inherited = create(
            mcp,
            target,
            json!({"browserContext":context,"allowUnknownCookiePartitions":engine=="firefox"}),
        )
        .await;
        observed(
            mcp, target, &inherited, &first.url, "A", first, second, false,
        )
        .await;
        observed(
            mcp,
            target,
            &inherited,
            &format!("{}/redirect", first.url),
            "B",
            first,
            second,
            true,
        )
        .await;
        let disabled=create(mcp,target,json!({"browserContext":context,"clientCertificates":[],"allowUnknownCookiePartitions":engine=="firefox"})).await;
        observed(
            mcp, target, &disabled, &first.url, "none", first, second, false,
        )
        .await;
        let null=create(mcp,target,json!({"browserContext":context,"clientCertificates":null,"allowUnknownCookiePartitions":engine=="firefox"})).await;
        observed(mcp, target, &null, &first.url, "A", first, second, true).await;
        let override_identity=create(mcp,target,json!({"browserContext":context,"clientCertificates":[keys.input(&first.url,1)],"allowUnknownCookiePartitions":engine=="firefox"})).await;
        observed(
            mcp,
            target,
            &override_identity,
            &first.url,
            "B",
            first,
            second,
            false,
        )
        .await;
        observed(
            mcp,
            target,
            &override_identity,
            &format!("{}/redirect", first.url),
            "none",
            first,
            second,
            true,
        )
        .await;
        operation(
            mcp,
            target,
            json!({"operation":"context_close","context":context}),
        )
        .await;
        for http in [inherited, disabled, null, override_identity] {
            let before = [
                first.requests.lock().unwrap().len(),
                second.requests.lock().unwrap().len(),
            ];
            let submitted = mcp
                .tool(
                    "browser_send",
                    json!({"targetId":target,"request":{"operation":"http_fetch","httpContext":http,"request":{"url":first.url}}}),
                )
                .await;
            let status = mcp
                .tool(
                    "browser_status",
                    json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
                )
                .await;
            assert_eq!(status["operation"]["settled"], true, "{status}");
            assert_eq!(status["operation"]["outcome"], "failed", "{status}");
            assert_eq!(status["operation"]["code"], "HTTP_CONTEXT_DISPOSED");
            assert_eq!(status["operation"]["result"]["disposed"], true);
            assert_eq!(status["operation"]["result"]["replayed"], false);
            assert_eq!(status["operation"]["result"]["httpContext"], http);
            assert_eq!(
                [
                    first.requests.lock().unwrap().len(),
                    second.requests.lock().unwrap().len(),
                ],
                before
            );
            operation(
                mcp,
                target,
                json!({"operation":"http_context_remove","httpContext":http}),
            )
            .await;
        }
    }
    let evidence = operation(mcp, target, json!({"operation":"http_evidence_list"})).await;
    assert!(evidence["pending"].as_array().unwrap().is_empty());
    assert!(evidence["evidence"].as_array().unwrap().is_empty());
    json!({"freshCliCreateMs":fresh_ms,"formatCreateMs":format_metrics})
}
#[tokio::test]
async fn installed_step_four_client_certificates_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    let keys = wire::Identities::new().await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let first = wire::Site::start(&keys).await;
            let second = wire::Site::start(&keys).await;
            first.redirect_to(&format!("{}/destination", second.url));
            let reference = reference(&fixture, engine, mode, &keys, &first, &second).await;
            let mut arguments = json!({"engine":engine,"mode":mode});
            if engine != "chrome" {
                arguments["context"] = json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"ignoreHTTPSErrors":true,
            "clientCertificates":[keys.input(&format!("{}/Acme-normalized-path",first.url),0),keys.input(&second.url,1)]});
            }
            let connected = mcp
                .tool(
                    if mode == "headless" {
                        "browser_connect"
                    } else {
                        "connect"
                    },
                    arguments,
                )
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture,
                &mut mcp,
                target,
                engine,
                &ready["browser"]["selectedContext"],
                &keys,
                &first,
                &second,
            ))
            .catch_unwind()
            .await;
            std::fs::write(fixture.workspace.path().join(format!("client-cert-wires-{engine}-{mode}.json")),serde_json::to_vec_pretty(&json!({"reference":reference,"first":*first.requests.lock().unwrap(),"second":*second.requests.lock().unwrap(),"cellResult":result.as_ref().ok()})).unwrap()).unwrap();
            let closed = performance_lifecycle::disconnect_observed(
                &fixture,
                &mut mcp,
                target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                failures.push(closed);
            }
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            eprintln!(
                "Client certificates {engine}/{mode}: exact origins, redirect reselection, explicit absence and context lifetime"
            );
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
