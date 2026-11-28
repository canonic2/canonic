//! Installed caller paths, precedence and immutable context inheritance.
use super::*;
use std::os::unix::fs::PermissionsExt;

fn write_files(root: &std::path::Path, keys: &wire::Identities, url: &str) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let pfx = keys.encrypted_input(url, 0, true);
    let encrypted = keys.encrypted_input(url, 0, false);
    for (name, bytes) in [
        ("Acme certificate.pem", keys.certs[0].as_bytes().to_vec()),
        ("Acme key.pem", keys.key.as_bytes().to_vec()),
        (
            "Acme archive.pfx",
            STANDARD.decode(pfx["pfx"].as_str().unwrap()).unwrap(),
        ),
        (
            "Acme encrypted.pem",
            STANDARD.decode(encrypted["key"].as_str().unwrap()).unwrap(),
        ),
    ] {
        let path = root.join(name);
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let link = root.join("Acme linked.pem");
    if !link.exists() {
        std::os::unix::fs::symlink("Acme certificate.pem", link).unwrap();
    }
}
fn relative(fixture: &Fixture, root: &std::path::Path, name: &str) -> String {
    root.join(name)
        .strip_prefix(fixture.workspace.path())
        .unwrap()
        .to_str()
        .unwrap()
        .into()
}
async fn client(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    mut options: Value,
    context: Option<&Value>,
    engine: &str,
    cli: bool,
) -> Value {
    if let Some(context) = context {
        options["browserContext"] = context.clone();
        options["allowUnknownCookiePartitions"] = json!(engine == "firefox");
    }
    let request = json!({"operation":"http_context_create","options":options});
    let created = if cli {
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &request.to_string()])
            .await;
        completed(mcp, target, submitted).await
    } else {
        operation(mcp, target, request).await
    };
    created["httpContext"].clone()
}
async fn remove(mcp: &mut Mcp, target: &str, http: &Value) {
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":http}),
    )
    .await;
}
async fn observed(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    http: &Value,
    site: &wire::Site,
    expected: &str,
    via: usize,
) {
    let before = site.requests.lock().unwrap().len();
    let request = json!({"url":site.url,"readBody":true});
    let input = json!({"operation":"http_fetch","httpContext":http,"request":request});
    let response = match via % 3 {
        0 => {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &input.to_string()])
                .await;
            completed(mcp, target, submitted).await
        }
        1 => operation(mcp, target, input).await,
        _ => {
            let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"http_fetch","httpContext":http,"request":request}]})).await;
            completed(mcp, target, submitted).await["results"][0].clone()
        }
    };
    assert_eq!(response["status"], 200);
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
async fn exercise(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    browser: &Value,
    engine: &str,
    root: &std::path::Path,
    keys: &wire::Identities,
    site: &wire::Site,
) {
    if engine != "chrome" {
        std::fs::remove_file(root.join("Acme archive.pfx")).unwrap();
        let inherited = client(
            fixture,
            mcp,
            target,
            json!({}),
            Some(browser),
            engine,
            false,
        )
        .await;
        observed(fixture, mcp, target, &inherited, site, "A", 2).await;
        remove(mcp, target, &inherited).await;
        let context=operation(mcp,target,json!({"operation":"context_create","options":{"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"ignoreHTTPSErrors":true,"clientCertificates":[{"origin":site.url,"certPath":relative(fixture,root,"Acme certificate.pem"),"keyPath":relative(fixture,root,"Acme encrypted.pem"),"passphrase":"Acme fixture passphrase"}]}})).await["context"].clone();
        std::fs::remove_file(root.join("Acme certificate.pem")).unwrap();
        std::fs::remove_file(root.join("Acme encrypted.pem")).unwrap();
        let inherited = client(
            fixture,
            mcp,
            target,
            json!({}),
            Some(&context),
            engine,
            false,
        )
        .await;
        observed(fixture, mcp, target, &inherited, site, "A", 1).await;
        remove(mcp, target, &inherited).await;
        let page = operation(
            mcp,
            target,
            json!({"operation":"page_create","context":context,"select":false}),
        )
        .await["page"]
            .clone();
        operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/Acme-certificate-file-page",site.url),"waitUntil":"load"}})).await;
        assert_eq!(
            site.requests
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|row| row["path"] == "/Acme-certificate-file-page")
                .unwrap()["identity"],
            if engine == "webkit" { "none" } else { "A" }
        );
        operation(
            mcp,
            target,
            json!({"operation":"context_close","context":context}),
        )
        .await;
    }
    for context in [None, Some(browser)] {
        for index in 0..6 {
            write_files(root, keys, &site.url);
            let material = match index {
                0 => {
                    json!({"origin":site.url,"certPath":root.join("Acme certificate.pem"),"keyPath":root.join("Acme key.pem")})
                }
                1 => {
                    json!({"origin":site.url,"certPath":relative(fixture,root,"Acme certificate.pem"),"keyPath":relative(fixture,root,"Acme key.pem")})
                }
                2 => {
                    json!({"origin":site.url,"pfxPath":relative(fixture,root,"Acme archive.pfx"),"passphrase":"Acme fixture passphrase"})
                }
                3 => {
                    json!({"origin":site.url,"certPath":relative(fixture,root,"Acme certificate.pem"),"keyPath":relative(fixture,root,"Acme encrypted.pem"),"passphrase":"Acme fixture passphrase"})
                }
                4 => {
                    json!({"origin":site.url,"certPath":relative(fixture,root,"Acme linked.pem"),"keyPath":relative(fixture,root,"Acme key.pem")})
                }
                _ => {
                    let mut inline = keys.input(&site.url, 0);
                    inline["certPath"] =
                        json!(relative(fixture, root, "Acme absent certificate.pem"));
                    inline["keyPath"] = json!(relative(fixture, root, "Acme absent key.pem"));
                    inline
                }
            };
            let http = client(
                fixture,
                mcp,
                target,
                json!({"ignoreHttpsErrors":true,"clientCertificates":[material]}),
                context,
                engine,
                index == 1,
            )
            .await;
            observed(fixture, mcp, target, &http, site, "A", index).await;
            remove(mcp, target, &http).await;
        }
    }
    write_files(root, keys, &site.url);
    let options = json!({"ignoreHttpsErrors":true,"clientCertificates":[{"origin":site.url,"certPath":relative(fixture,root,"Acme certificate.pem"),"keyPath":relative(fixture,root,"Acme key.pem")}]});
    let original = client(fixture, mcp, target, options.clone(), None, engine, true).await;
    std::fs::write(root.join("Acme certificate.pem"), &keys.certs[1]).unwrap();
    observed(fixture, mcp, target, &original, site, "A", 0).await;
    let changed = client(fixture, mcp, target, options, None, engine, false).await;
    observed(fixture, mcp, target, &changed, site, "B", 1).await;
    std::fs::remove_file(root.join("Acme certificate.pem")).unwrap();
    std::fs::remove_file(root.join("Acme key.pem")).unwrap();
    observed(fixture, mcp, target, &original, site, "A", 2).await;
    observed(fixture, mcp, target, &changed, site, "B", 0).await;
    remove(mcp, target, &original).await;
    remove(mcp, target, &changed).await;
    let before = site.requests.lock().unwrap().len();
    collectors::refused(mcp,target,json!({"operation":"http_context_create","options":{"clientCertificates":[{"origin":site.url,"certPath":relative(fixture,root,"Acme certificate.pem"),"keyPath":relative(fixture,root,"Acme key.pem")}]}}),"HTTP_CLIENT_CERTIFICATE_FILE_IO",true).await;
    write_files(root, keys, &site.url);
    collectors::refused(mcp,target,json!({"operation":"http_context_create","options":{"clientCertificates":[{"origin":site.url,"cert":"","certPath":relative(fixture,root,"Acme certificate.pem"),"keyPath":relative(fixture,root,"Acme key.pem")}]}}),"HTTP_CLIENT_CERTIFICATE_INVALID",false).await;
    assert_eq!(site.requests.lock().unwrap().len(), before);
    assert_eq!(
        operation(mcp, target, json!({"operation":"http_context_list"})).await["contexts"],
        json!([])
    );
    let evidence = operation(mcp, target, json!({"operation":"http_evidence_list"})).await;
    assert_eq!(evidence["evidence"], json!([]));
    assert_eq!(evidence["pending"], json!([]));
}
#[tokio::test]
async fn installed_step_four_http_certificate_paths_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let keys = wire::Identities::new().await;
    let site = wire::Site::start(&keys).await;
    let mut failures = vec![];
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let root = fixture
                .workspace
                .path()
                .join(format!("Acme certificate files {engine} {mode}"));
            write_files(&root, &keys, &site.url);
            let mut args = json!({"engine":engine,"mode":mode});
            if engine != "chrome" {
                args["context"] = json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"ignoreHTTPSErrors":true,"clientCertificates":[{"origin":site.url,"pfxPath":relative(&fixture,&root,"Acme archive.pfx"),"passphrase":"Acme fixture passphrase"}]});
            }
            let connected = mcp.tool("browser_connect", args).await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let browser = &ready["browser"]["selectedContext"];
            let result = std::panic::AssertUnwindSafe(exercise(
                &fixture, &mut mcp, target, browser, engine, &root, &keys, &site,
            ))
            .catch_unwind()
            .await;
            std::fs::write(fixture.workspace.path().join(format!("certificate-files-{engine}-{mode}.json")),serde_json::to_vec_pretty(&json!({"exerciseSucceeded":result.is_ok(),"requests":*site.requests.lock().unwrap()})).unwrap()).unwrap();
            let closed = performance_lifecycle::disconnect_observed(
                &fixture, &mut mcp, target, engine, mode, browser,
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
                "Certificate files {engine}/{mode}: selected files, inline precedence, immutable snapshots and inherited context bytes; original disconnect observed"
            );
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

#[tokio::test]
async fn installed_step_four_http_certificate_file_launch_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let keys = wire::Identities::new().await;
    let site = wire::Site::start(&keys).await;
    let mut failures = vec![];
    for engine in ["firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            for encrypted in [false, true] {
                let format = if encrypted { "encrypted-pem" } else { "pem" };
                let root = fixture
                    .workspace
                    .path()
                    .join(format!("Acme initial files {engine} {mode} {format}"));
                write_files(&root, &keys, &site.url);
                let key_name = if encrypted {
                    "Acme encrypted.pem"
                } else {
                    "Acme key.pem"
                };
                let mut material = json!({"origin":site.url,"certPath":relative(&fixture,&root,"Acme certificate.pem"),"keyPath":relative(&fixture,&root,key_name)});
                if encrypted {
                    material["passphrase"] = json!("Acme fixture passphrase");
                }
                let tool = if encrypted {
                    "connect"
                } else {
                    "browser_connect"
                };
                let connected = mcp.tool(tool,json!({"engine":engine,"mode":mode,"context":{"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"ignoreHTTPSErrors":true,"clientCertificates":[material]}})).await;
                let target = connected["targetId"].as_str().unwrap();
                let ready = fixture.ready_target(target).await;
                let browser = &ready["browser"]["selectedContext"];
                let result = std::panic::AssertUnwindSafe(async {
                    std::fs::remove_file(root.join("Acme certificate.pem")).unwrap();
                    std::fs::remove_file(root.join(key_name)).unwrap();
                    let http = client(&fixture, &mut mcp, target, json!({}), Some(browser), engine, false).await;
                    observed(&fixture, &mut mcp, target, &http, &site, "A", 2).await;
                    remove(&mut mcp, target, &http).await;
                    let path = format!("/Acme-initial-file-{format}");
                    operation(&mut mcp,target,json!({"operation":"page_navigate","page":ready["browser"]["selectedPage"],"navigation":{"kind":"goto","url":format!("{}{path}",site.url),"waitUntil":"load"}})).await;
                    assert_eq!(site.requests.lock().unwrap().iter().rev().find(|row|row["path"]==path).unwrap()["identity"],if engine=="webkit"{"none"}else{"A"});
                    assert_eq!(operation(&mut mcp,target,json!({"operation":"http_context_list"})).await["contexts"],json!([]));
                    let evidence = operation(&mut mcp,target,json!({"operation":"http_evidence_list"})).await;
                    assert_eq!(evidence["evidence"], json!([]));
                    assert_eq!(evidence["pending"], json!([]));
                }).catch_unwind().await;
                let closed = performance_lifecycle::disconnect_observed(
                    &fixture,
                    &mut mcp,
                    target,
                    engine,
                    &format!("{mode}-{format}"),
                    browser,
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
                    "Certificate file launch {engine}/{mode}/{format}: initial snapshot survives deletion; original disconnect observed"
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
