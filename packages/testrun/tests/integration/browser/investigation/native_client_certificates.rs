//! Exact SDK certificate-context behavior on owned, deliberately untrusted TLS peers.
use super::*;
use super::{certificate_fixture as certificates, wire_fixture as wire};
mod installed;
fn inputs(keys: &certificates::Identities, selected: &str) -> Value {
    json!({"pem":[keys.input(selected,0)],"pfx":[keys.encrypted_input(selected,0,true)],
        "encryptedPem":[keys.encrypted_input(selected,0,false)],
        "duplicateFormats":[keys.input(selected,1),keys.encrypted_input(selected,0,true)]})
}

fn file_inputs(
    fixture: &Fixture,
    keys: &certificates::Identities,
    selected: &str,
    tag: &str,
) -> Value {
    use base64::Engine as _;
    let mut values = inputs(keys, selected);
    for (name, entries) in values.as_object_mut().unwrap() {
        for (index, entry) in entries.as_array_mut().unwrap().iter_mut().enumerate() {
            for (inline, path, suffix) in [
                ("cert", "certPath", "pem"),
                ("key", "keyPath", "key"),
                ("pfx", "pfxPath", "pfx"),
            ] {
                let Some(encoded) = entry.as_object_mut().unwrap().remove(inline) else {
                    continue;
                };
                let filename = format!("Acme native {tag} {name} {index} {suffix}");
                let destination = fixture.workspace.path().join(&filename);
                std::fs::write(
                    &destination,
                    base64::engine::general_purpose::STANDARD
                        .decode(encoded.as_str().unwrap())
                        .unwrap(),
                )
                .unwrap();
                std::fs::set_permissions(
                    &destination,
                    std::os::unix::fs::PermissionsExt::from_mode(0o600),
                )
                .unwrap();
                entry[path] = json!(filename);
            }
        }
    }
    values
}
fn remove_files(fixture: &Fixture, entries: &Value) {
    for entry in entries.as_array().unwrap() {
        for path in ["certPath", "keyPath", "pfxPath"] {
            if let Some(path) = entry[path].as_str() {
                std::fs::remove_file(fixture.workspace.path().join(path)).unwrap();
            }
        }
    }
}
async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    reference_with_options(fixture, engine, mode, false, false).await
}
async fn upstream_proxy(
    selected: &str,
    other: &str,
    plain: &str,
) -> super::http_proxy::tunnel::Proxy {
    let ports = [selected, other, plain]
        .into_iter()
        .map(|url| url::Url::parse(url).unwrap().port().unwrap())
        .collect();
    super::http_proxy::tunnel::Proxy::start_forwarding(
        ports,
        Some(super::context_proxy::basic("Acme upstream", "Acme example")),
    )
    .await
}
fn proxy_options(proxy: &super::http_proxy::tunnel::Proxy) -> Value {
    json!({"server":proxy.url,"username":"Acme upstream","password":"Acme example"})
}
fn assert_proxy(
    proxy: &super::http_proxy::tunnel::Proxy,
    selected: &str,
    other: &str,
    plain: &str,
) {
    let records = proxy.requests();
    assert!(!records.is_empty());
    for url in [selected, other, plain] {
        let port = url::Url::parse(url).unwrap().port().unwrap();
        assert!(
            records.iter().any(|row| row["target"]
                .as_str()
                .is_some_and(|target| target.ends_with(&format!(":{port}")))),
            "Missing selected proxy destination: {records:?}"
        );
    }
    let authorization = super::context_proxy::basic("Acme upstream", "Acme example");
    assert!(
        records.iter().all(|row| row["method"] == "CONNECT"
            && row["headers"]["proxy-authorization"] == authorization),
        "{records:?}"
    );
}
async fn reference_with_options(
    fixture: &Fixture,
    engine: &str,
    mode: &str,
    use_proxy: bool,
    use_files: bool,
) -> Value {
    let keys = certificates::Identities::new().await;
    let selected = certificates::Site::start(&keys).await;
    let other = certificates::Site::start(&keys).await;
    let plain = wire::Site::start().await;
    let proxy = if use_proxy {
        Some(upstream_proxy(&selected.url, &other.url, &plain.url).await)
    } else {
        None
    };
    let material = if use_files {
        file_inputs(
            fixture,
            &keys,
            &selected.url,
            &format!("sdk {engine} {mode}"),
        )
    } else {
        inputs(&keys, &selected.url)
    };
    let mut input =
        json!({"selected":selected.url,"other":other.url,"plain":plain.url,"cases":material});
    if let Some(proxy) = &proxy {
        input["proxy"] = proxy_options(proxy);
    }
    let input_path = fixture
        .workspace
        .path()
        .join(format!("native-certificates-input-{engine}-{mode}.json"));
    std::fs::write(&input_path, serde_json::to_vec(&input).unwrap()).unwrap();
    std::fs::set_permissions(
        &input_path,
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )
    .unwrap();
    // Stream reference output to private files so phase markers survive a stall.
    let log_path = |suffix: &str| {
        fixture
            .workspace
            .path()
            .join(format!("native-certificates-sdk-{engine}-{mode}.{suffix}"))
    };
    let private_log = |suffix: &str| {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(log_path(suffix))
            .unwrap()
    };
    let status = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-native-certificates-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(fixture.cache.join("browsers"))
    .arg(&input_path)
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PW_LANG_NAME", "javascript")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::from(private_log("json")))
    .stderr(std::process::Stdio::from(private_log("stderr")))
    .status()
    .await
    .unwrap();
    let output = std::process::Output {
        status,
        stdout: std::fs::read(log_path("json")).unwrap(),
        stderr: std::fs::read(log_path("stderr")).unwrap(),
    };
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ownedProcesses"], 0, "{report}");
    assert_eq!(report["cases"].as_object().unwrap().len(), 4, "{report}");
    for row in report["cases"].as_object().unwrap().values() {
        assert_eq!(row["plainStatus"], 200, "{report}");
        assert_eq!(row["httpStatus"], 200, "{report}");
        assert_eq!(row["httpIdentity"], "A", "{report}");
    }
    if let Some(proxy) = &proxy {
        assert_proxy(proxy, &selected.url, &other.url, &plain.url);
        std::fs::write(
            fixture.workspace.path().join(format!(
                "native-certificates-sdk-proxy-{engine}-{mode}.json"
            )),
            serde_json::to_vec_pretty(&proxy.requests()).unwrap(),
        )
        .unwrap();
    }
    report
}
#[tokio::test]
async fn pinned_native_client_certificates_reference_checkpoint() {
    let fixture = installed_fixture();
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let report = reference(&fixture, engine, mode).await;
            eprintln!(
                "Native certificate reference {engine}/{mode}: {}",
                report["cases"]
            );
        }
    }
}
