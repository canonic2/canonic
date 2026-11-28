//! Owned initial/later contexts compare certificate behavior through public clients.
use super::*;
fn context_options(certificates: &Value) -> Value {
    json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"clientCertificates":certificates})
}
async fn dispatch(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    input: Value,
    via: usize,
) -> Value {
    match via % 3 {
        0 => {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &input.to_string()])
                .await;
            completed(mcp, target, submitted).await
        }
        1 => operation(mcp, target, input).await,
        _ => {
            let mut action = input;
            let verb = action.as_object_mut().unwrap().remove("operation").unwrap();
            action["action"] = verb;
            let submitted = mcp
                .tool("act", json!({"targetId":target,"actions":[action]}))
                .await;
            completed(mcp, target, submitted).await["results"][0].clone()
        }
    }
}
async fn navigate(mcp: &mut Mcp, target: &str, page: &Value, url: &str) -> Value {
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":url,"waitUntil":"load"}})).await
}
async fn identity(mcp: &mut Mcp, target: &str, page: &Value) -> Value {
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"()=>document.body.innerText"})).await["value"]["value"].clone()
}
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    engine: &str,
    context: &Value,
    expected: &Value,
    selected: &certificates::Site,
    other: &certificates::Site,
    plain: &wire::Site,
    via: usize,
) -> Value {
    let before = [
        selected.requests.lock().unwrap().len(),
        other.requests.lock().unwrap().len(),
        plain.records.lock().unwrap().len(),
    ];
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context}),
    )
    .await["page"]
        .clone();
    let mut result = json!({});
    result["plainStatus"] =
        navigate(mcp, target, &page, &format!("{}/plain", plain.url)).await["response"]["status"]
            .clone();
    result["selectedStatus"] = navigate(mcp, target, &page, &format!("{}/selected", selected.url))
        .await["response"]["status"]
        .clone();
    result["selectedIdentity"] = if result["selectedStatus"] == 200 {
        identity(mcp, target, &page).await
    } else {
        Value::Null
    };
    let client=dispatch(fixture,mcp,target,json!({"operation":"http_context_create","options":{"browserContext":context,"allowUnknownCookiePartitions":engine=="firefox"}}),via).await["httpContext"].clone();
    let response=dispatch(fixture,mcp,target,json!({"operation":"http_fetch","httpContext":client,"request":{"url":format!("{}/inherited",selected.url),"readBody":true,"ignoreHttpsErrors":true}}),via+1).await;
    result["httpStatus"] = response["status"].clone();
    result["httpIdentity"]=operation(mcp,target,json!({"operation":"http_evidence_read","evidenceId":response["body"]["evidenceId"],"format":"text"})).await["value"].clone();
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
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":client}),
    )
    .await;
    result["otherStatus"] =
        navigate(mcp, target, &page, &format!("{}/other", other.url)).await["response"]["status"]
            .clone();
    result["otherIdentity"] = identity(mcp, target, &page).await;
    let isolated =
        operation(mcp, target, json!({"operation":"context_create"})).await["context"].clone();
    let isolated_page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":isolated}),
    )
    .await["page"]
        .clone();
    let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"page_navigate","page":isolated_page,"navigation":{"kind":"goto","url":format!("{}/isolated",other.url),"waitUntil":"load"}}})).await;
    let refused = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(refused["operation"]["settled"], true, "{refused}");
    assert_eq!(refused["operation"]["outcome"], "failed", "{refused}");
    assert_eq!(
        refused["operation"]["code"],
        if engine == "chrome" {
            "NAVIGATION_FAILED"
        } else {
            "BROWSER_METHOD_FAILED"
        },
        "{refused}"
    );
    result["isolatedRejected"] = json!(true);
    operation(
        mcp,
        target,
        json!({"operation":"context_close","context":isolated}),
    )
    .await;
    assert_eq!(&result, expected, "{engine}: {result}");
    let selected_records = selected.requests.lock().unwrap().clone();
    let other_records = other.requests.lock().unwrap().clone();
    let plain_records = plain.records.lock().unwrap().clone();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("native-certificates-last-wire-{engine}.json")),
        serde_json::to_vec_pretty(
            &json!({"selected":selected_records,"other":other_records,"plain":plain_records}),
        )
        .unwrap(),
    )
    .unwrap();
    let selected_records = &selected_records[before[0]..];
    let other_records = &other_records[before[1]..];
    let plain_records = &plain_records[before[2]..];
    let inherited = selected_records
        .iter()
        .filter(|row| row["path"] == "/inherited")
        .collect::<Vec<_>>();
    assert_eq!(inherited.len(), 1);
    assert_eq!(inherited[0]["identity"], "A");
    if engine == "webkit" {
        assert_eq!(
            selected_records
                .iter()
                .filter(|row| row["path"] == "/selected" && row["identity"] == "none")
                .count(),
            1
        );
        assert!(
            selected_records.iter().all(|row| matches!(
                row["path"].as_str(),
                Some("/selected" | "/inherited" | "/favicon.ico")
            )),
            "{selected_records:?}"
        );
    } else {
        assert_eq!(
            selected_records.len(),
            1,
            "Only the explicitly trusted host HTTP copy reaches the configured origin: {selected_records:?}"
        );
    }
    assert_eq!(
        other_records
            .iter()
            .filter(|row| row["path"] == "/other")
            .count(),
        1,
        "{other_records:?}"
    );
    assert!(
        other_records.iter().all(|row| row["identity"] == "none"
            && matches!(row["path"].as_str(), Some("/other" | "/favicon.ico"))),
        "The plain sibling must not issue an untrusted HTTP request: {other_records:?}"
    );
    assert_eq!(
        plain_records
            .iter()
            .filter(|row| row["url"] == "/plain")
            .count(),
        1,
        "{plain_records:?}"
    );
    assert!(
        plain_records
            .iter()
            .all(|row| matches!(row["url"].as_str(), Some("/plain" | "/favicon.ico"))),
        "{plain_records:?}"
    );
    result
}

#[tokio::test]
async fn installed_step_four_native_client_certificates_checkpoint() {
    run(false, false).await;
}
#[tokio::test]
async fn installed_step_four_native_certificate_proxy_checkpoint() {
    run(true, false).await;
}
#[tokio::test]
async fn installed_step_four_native_certificate_files_checkpoint() {
    run(false, true).await;
}
async fn run(use_proxy: bool, use_files: bool) {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    let mut cells = 0;
    for engine in if use_proxy || use_files {
        vec!["chrome"]
    } else {
        vec!["chrome", "firefox", "webkit"]
    } {
        for mode in ["headless", "headed"] {
            let expected =
                reference_with_options(&fixture, engine, mode, use_proxy, use_files).await;
            let keys = certificates::Identities::new().await;
            let selected = certificates::Site::start(&keys).await;
            let other = certificates::Site::start(&keys).await;
            let plain = wire::Site::start().await;
            let proxy = if use_proxy {
                Some(upstream_proxy(&selected.url, &other.url, &plain.url).await)
            } else {
                None
            };
            let material = inputs(&keys, &selected.url);
            let initials = if engine == "chrome" && !use_proxy {
                vec!["pem", "pfx", "encryptedPem", "duplicateFormats"]
            } else {
                vec!["pem"]
            };
            for initial_format in initials {
                let material = if use_files {
                    file_inputs(
                        &fixture,
                        &keys,
                        &selected.url,
                        &format!("installed {engine} {mode} {initial_format}"),
                    )
                } else {
                    material.clone()
                };
                let mut options = context_options(&material[initial_format]);
                if let Some(proxy) = &proxy {
                    options["proxy"] = proxy_options(proxy);
                }
                let connected = mcp
                    .tool(
                        "browser_connect",
                        json!({"engine":engine,"mode":mode,"context":options}),
                    )
                    .await;
                let target = connected["targetId"].as_str().unwrap();
                let ready = fixture.ready_target(target).await;
                let initial = ready["browser"]["selectedContext"].clone();
                if use_files {
                    remove_files(&fixture, &material[initial_format]);
                }
                let result=std::panic::AssertUnwindSafe(async {
                let doctor=mcp.tool("browser_doctor",json!({"engine":engine})).await;
                assert_eq!(doctor["capabilities"]["httpContexts"]["clientCertificates"]["browserInheritance"],true,"{doctor}");
                if engine=="chrome" {
                    assert_eq!(doctor["capabilities"]["clientCertificateTunnels"]["available"],true,"{doctor}");
                    assert_eq!(doctor["capabilities"]["clientCertificateTunnels"]["upstreamTlsVerification"],true);
                    assert_eq!(doctor["capabilities"]["clientCertificateTunnels"]["upstreamTlsVerificationScope"],"configured-origins");
                    assert_eq!(doctor["capabilities"]["clientCertificateTunnels"]["browserTlsOverride"],"owned-context-as-pinned-internal-proxy-requirement");
                    assert_eq!(doctor["capabilities"]["clientCertificateTunnels"]["fullSdkParity"],false);
                }
                let mut reports=json!({});
                reports[initial_format]=cell(&fixture,&mut mcp,target,engine,&initial,&expected["cases"][initial_format],&selected,&other,&plain,0).await;
                if initial_format=="pem" {
                    for (index,name) in ["pfx","encryptedPem","duplicateFormats"].into_iter().enumerate() {
                        let mut options=context_options(&material[name]);
                        if let Some(proxy)=&proxy {options["proxy"]=proxy_options(proxy);}
                        let context=dispatch(&fixture,&mut mcp,target,json!({"operation":"context_create","options":options}),index).await["context"].clone();
                        if use_files {remove_files(&fixture,&material[name]);}
                        reports[name]=cell(&fixture,&mut mcp,target,engine,&context,&expected["cases"][name],&selected,&other,&plain,index+1).await;
                        operation(&mut mcp,target,json!({"operation":"context_close","context":context})).await;
                        operation(&mut mcp,target,json!({"operation":"context_close","context":context})).await;
                    }
                }
                if let Some(proxy)=&proxy {
                    assert_proxy(proxy,&selected.url,&other.url,&plain.url);
                    std::fs::write(fixture.workspace.path().join(format!("native-certificates-installed-proxy-{engine}-{mode}.json")),serde_json::to_vec_pretty(&proxy.requests()).unwrap()).unwrap();
                }
                std::fs::write(fixture.workspace.path().join(format!("native-certificates-installed-{engine}-{mode}-{initial_format}.json")),serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
            }).catch_unwind().await;
                std::fs::write(fixture.workspace.path().join(format!("native-certificates-wire-{engine}-{mode}-{initial_format}.json")),serde_json::to_vec_pretty(&json!({"succeeded":result.is_ok(),"selected":*selected.requests.lock().unwrap_or_else(|error|error.into_inner()),"other":*other.requests.lock().unwrap_or_else(|error|error.into_inner()),"plain":*plain.records.lock().unwrap_or_else(|error|error.into_inner())})).unwrap()).unwrap();
                let closed = performance_lifecycle::disconnect_observed(
                    &fixture, &mut mcp, target, engine, mode, &initial,
                )
                .await;
                std::fs::write(
                    fixture.workspace.path().join(format!(
                        "native-certificates-disconnect-{engine}-{mode}-{initial_format}.json"
                    )),
                    serde_json::to_vec_pretty(&closed).unwrap(),
                )
                .unwrap();
                if closed["result"]["structuredContent"]["ok"] != true {
                    failures.push(closed);
                }
                if let Err(panic) = result {
                    mcp.stop().await;
                    std::panic::resume_unwind(panic);
                }
                cells += 1;
                eprintln!(
                    "Native client certificates {engine}/{mode}/{initial_format}: exact SDK cases, private-origin refusal, inherited identities, sibling isolation and original disconnect"
                );
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert_eq!(
        cells,
        if use_proxy {
            2
        } else if use_files {
            8
        } else {
            12
        }
    );
    assert!(failures.is_empty(), "{failures:?}");
}
