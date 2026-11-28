//! First measure exact pinned same-origin identity selection and launch formats.
use super::*;
use client_certificates::wire;
mod installed;

async fn reference(
    fixture: &Fixture,
    engine: &str,
    mode: &str,
    keys: &wire::Identities,
    first: &wire::Site,
    second: &wire::Site,
    algorithm_cases: &Value,
) -> Value {
    let a = keys.input(&first.url, 0);
    let b = keys.input(&first.url, 1);
    let input = fixture
        .workspace
        .path()
        .join(format!("certificate-selection-{engine}-{mode}-input.json"));
    std::fs::write(&input,serde_json::to_vec(&json!({"first":first.url,"cases":{
        "AB":[a,b],"BA":[b,a],"ABA":[a,b,a],
        "mixed":[keys.encrypted_input(&first.url,0,true),keys.encrypted_input(&first.url,1,false)]
    },"algorithmCases":algorithm_cases,"formats":{
        "pfx":[keys.encrypted_input(&first.url,0,true),keys.encrypted_input(&second.url,1,true)],
        "encryptedPem":[keys.encrypted_input(&first.url,0,false),keys.encrypted_input(&second.url,1,false)]
    }})).unwrap()).unwrap();
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-client-certificate-selection-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(input)
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
            fixture.workspace.path().join(format!(
                "certificate-selection-sdk-{engine}-{mode}.{suffix}"
            )),
            bytes,
        )
        .unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ownedProcesses"], 0);
    assert!(value.get("failure").is_none(), "{value}");
    value
}
#[tokio::test]
async fn pinned_http_certificate_selection_reference_checkpoint() {
    let fixture = installed_fixture();
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
    let mut algorithm_cases = json!({});
    for (name, site) in [
        ("both", &both),
        ("rsaHint", &rsa),
        ("ecHint", &ec),
        ("rsaOnly", &rsa_only),
        ("rsaFirst", &rsa_first),
        ("ecFirst", &ec_first),
    ] {
        let a = different.input(&site.url, 0);
        let b = different.input(&site.url, 1);
        algorithm_cases[format!("{name}AB")] = json!({"url":site.url,"certificates":[a,b]});
        algorithm_cases[format!("{name}BA")] = json!({"url":site.url,"certificates":[b,a]});
        algorithm_cases[format!("{name}Mixed")] =
            json!({"url":site.url,"certificates":[different.encrypted_input(&site.url,0,true),b]});
    }
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let value = reference(
                &fixture,
                engine,
                mode,
                &keys,
                &first,
                &second,
                &algorithm_cases,
            )
            .await;
            eprintln!("Certificate selection {engine}/{mode}: {value}");
        }
    }
}
