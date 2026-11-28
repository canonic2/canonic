//! Same-origin selection and initial-format inheritance through installed transports.
use super::*;
fn native_observations(closed: &Value) {
    let owners = &closed["result"]["structuredContent"]["data"]["cleanup"]["driverShutdown"]["lifecycle"]
        ["ownedChildren"];
    assert_eq!(owners["invalidObservations"], 0);
    assert_eq!(owners["sourceUnavailable"], false);
    assert_eq!(owners["capacityExceeded"], false);
    let children = owners["children"].as_array().unwrap();
    assert_eq!(children.len(), 1);
    let child = &children[0];
    assert!(child["observingNativeState"]["exitStatusKnown"].is_boolean());
    assert!(child["observingNativeState"]["nativeHandlePresent"].is_boolean());
    assert_eq!(child["observingNativeState"]["closesNeeded"], 5);
    assert_eq!(child["nativeCloseState"]["exitStatusKnown"], true);
    assert_eq!(child["nativeCloseState"]["nativeHandlePresent"], false);
    assert_eq!(
        child["nativeCloseState"]["closesGot"],
        child["nativeCloseState"]["closesNeeded"]
    );
    assert!(child["nativeCloseElapsedMs"].is_u64());
}

async fn client(
    mcp: &mut Mcp,
    target: &str,
    mut options: Value,
    context: Option<&Value>,
    engine: &str,
) -> Value {
    if let Some(context) = context {
        options["browserContext"] = context.clone();
        options["allowUnknownCookiePartitions"] = json!(engine == "firefox");
    }
    operation(
        mcp,
        target,
        json!({"operation":"http_context_create","options":options}),
    )
    .await["httpContext"]
        .clone()
}
async fn observed(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    client: &Value,
    site: &wire::Site,
    url: &str,
    expected: &str,
    via: usize,
) {
    let before = site.requests.lock().unwrap().len();
    let request = json!({"url":url,"readBody":true});
    let input = json!({"operation":"http_fetch","httpContext":client,"request":request});
    let response = match via % 3 {
        0 => {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &input.to_string()])
                .await;
            completed(mcp, target, submitted).await
        }
        1 => operation(mcp, target, input).await,
        _ => {
            let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"http_fetch","httpContext":client,"request":request}]})).await;
            completed(mcp, target, submitted).await["results"][0].clone()
        }
    };
    assert_eq!(response["status"], 200, "{response}");
    assert_eq!(response["replayed"], false);
    assert_eq!(response["retryCount"], 0);
    let records = site.requests.lock().unwrap();
    assert_eq!(records.len(), before + 1);
    assert_eq!(records.last().unwrap()["identity"], expected);
    drop(records);
    let body=operation(mcp,target,json!({"operation":"http_evidence_read","evidenceId":response["body"]["evidenceId"],"format":"text"})).await;
    assert_eq!(body["value"], expected);
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
async fn remove(mcp: &mut Mcp, target: &str, client: &Value) {
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":client}),
    )
    .await;
}
async fn selection(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    browser: &Value,
    engine: &str,
    keys: &wire::Identities,
    first: &wire::Site,
    algorithm_cases: &Value,
    sites: &[(&str, &wire::Site)],
    expected: &Value,
) {
    let a = keys.input(&first.url, 0);
    let b = keys.input(&first.url, 1);
    let ordinary = [
        ("AB", json!([a, b])),
        ("BA", json!([b, a])),
        ("ABA", json!([a, b, a])),
        (
            "mixed",
            json!([keys.encrypted_input(&first.url, 0, true), b]),
        ),
    ];
    for context in [None, Some(browser)] {
        for (index, (name, certificates)) in ordinary.iter().enumerate() {
            let http = client(
                mcp,
                target,
                json!({"ignoreHttpsErrors":true,"clientCertificates":certificates}),
                context,
                engine,
            )
            .await;
            observed(
                fixture,
                mcp,
                target,
                &http,
                first,
                &first.url,
                expected["cases"][*name].as_str().unwrap(),
                index,
            )
            .await;
            remove(mcp, target, &http).await;
        }
        for (index, (name, input)) in algorithm_cases.as_object().unwrap().iter().enumerate() {
            let site = sites
                .iter()
                .find(|(prefix, _)| name.starts_with(*prefix))
                .unwrap()
                .1;
            let http = client(
                mcp,
                target,
                json!({"ignoreHttpsErrors":true,"clientCertificates":input["certificates"]}),
                context,
                engine,
            )
            .await;
            observed(
                fixture,
                mcp,
                target,
                &http,
                site,
                input["url"].as_str().unwrap(),
                expected["algorithmCases"][name].as_str().unwrap(),
                index,
            )
            .await;
            remove(mcp, target, &http).await;
        }
    }
}
#[tokio::test]
async fn installed_step_four_http_certificate_selection_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    let keys = wire::Identities::new().await;
    let first = wire::Site::start(&keys).await;
    let second = wire::Site::start(&keys).await;
    first.redirect_to(&second.url);
    let different = wire::Identities::different_algorithms().await;
    let both = wire::Site::start(&different).await;
    let rsa = wire::Site::start_with_hint(&different, Some(0)).await;
    let ec = wire::Site::start_with_hint(&different, Some(1)).await;
    let rsa_only = wire::Site::start_rsa_only(&different).await;
    let rsa_first = wire::Site::start_signature_order(&different, true).await;
    let ec_first = wire::Site::start_signature_order(&different, false).await;
    let sites = [
        ("both", &both),
        ("rsaHint", &rsa),
        ("ecHint", &ec),
        ("rsaOnly", &rsa_only),
        ("rsaFirst", &rsa_first),
        ("ecFirst", &ec_first),
    ];
    let mut algorithm_cases = json!({});
    for (name, site) in sites {
        let a = different.input(&site.url, 0);
        let b = different.input(&site.url, 1);
        algorithm_cases[format!("{name}AB")] = json!({"url":site.url,"certificates":[a,b]});
        algorithm_cases[format!("{name}BA")] = json!({"url":site.url,"certificates":[b,a]});
        algorithm_cases[format!("{name}Mixed")] =
            json!({"url":site.url,"certificates":[different.encrypted_input(&site.url,0,true),b]});
    }
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let expected = reference(
                &fixture,
                engine,
                mode,
                &keys,
                &first,
                &second,
                &algorithm_cases,
            )
            .await;
            let formats = if engine == "chrome" {
                vec!["none"]
            } else {
                vec!["pfx", "encryptedPem"]
            };
            for (index, format) in formats.into_iter().enumerate() {
                let mut arguments = json!({"engine":engine,"mode":mode});
                if engine != "chrome" {
                    arguments["context"] = json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"ignoreHTTPSErrors":true,
                    "clientCertificates":[keys.encrypted_input(&first.url,0,format=="pfx"),keys.encrypted_input(&second.url,1,format=="pfx")]});
                }
                let connected = mcp.tool("browser_connect", arguments).await;
                let target = connected["targetId"].as_str().unwrap();
                let ready = fixture.ready_target(target).await;
                let browser = &ready["browser"]["selectedContext"];
                let result=std::panic::AssertUnwindSafe(async {
                if index==0 {selection(&fixture,&mut mcp,target,browser,engine,&keys,&first,&algorithm_cases,&sites,&expected).await;}
                if engine!="chrome" {
                    if engine=="firefox" {
                        collectors::refused(&mut mcp,target,json!({"operation":"http_context_create","options":{"browserContext":browser}}),"HTTP_COOKIE_PARTITION_IDENTITY_UNAVAILABLE",true).await;
                    }
                    let http=client(&mut mcp,target,json!({}),Some(browser),engine).await;
                    let row=&expected["initial"][format];
                    observed(&fixture,&mut mcp,target,&http,&first,&first.url,row["first"].as_str().unwrap(),0).await;
                    let before=first.requests.lock().unwrap().len();
                    observed(&fixture,&mut mcp,target,&http,&second,&format!("{}/redirect",first.url),row["redirect"].as_str().unwrap(),2).await;
                    assert_eq!(first.requests.lock().unwrap().len(),before+1);
                    remove(&mut mcp,target,&http).await;
                    operation(&mut mcp,target,json!({"operation":"page_navigate","page":ready["browser"]["selectedPage"],"navigation":{"kind":"goto","url":format!("{}/Acme-initial-page",first.url),"waitUntil":"load"}})).await;
                    assert_eq!(first.requests.lock().unwrap().iter().rev().find(|request|request["path"]=="/Acme-initial-page").unwrap()["identity"],row["page"]);
                }
                assert_eq!(operation(&mut mcp,target,json!({"operation":"http_context_list"})).await["contexts"],json!([]));
                let evidence=operation(&mut mcp,target,json!({"operation":"http_evidence_list"})).await;
                assert_eq!(evidence["evidence"],json!([]));assert_eq!(evidence["pending"],json!([]));
            }).catch_unwind().await;
                std::fs::write(fixture.workspace.path().join(format!("certificate-selection-installed-{engine}-{mode}-{format}.json")),serde_json::to_vec_pretty(&json!({"exerciseSucceeded":result.is_ok(),"reference":expected,"first":*first.requests.lock().unwrap(),"second":*second.requests.lock().unwrap()})).unwrap()).unwrap();
                let closed = performance_lifecycle::disconnect_observed(
                    &fixture,
                    &mut mcp,
                    target,
                    engine,
                    &format!("{mode}-{format}"),
                    browser,
                )
                .await;
                let result =
                    if closed["result"]["structuredContent"]["ok"] == true && engine != "chrome" {
                        result.and_then(|()| {
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                native_observations(&closed)
                            }))
                        })
                    } else {
                        result
                    };
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
                    "Certificate selection {engine}/{mode}/{format}: exact same-origin/key/format selection, source inheritance and original disconnect observed"
                );
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original disconnect failures: {failures:?}"
    );
}
