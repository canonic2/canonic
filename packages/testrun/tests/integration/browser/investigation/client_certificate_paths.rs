//! Measure pinned caller-file precedence before implementing the transport.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use client_certificates::wire;
mod installed;

#[tokio::test]
async fn pinned_http_certificate_paths_reference_checkpoint() {
    let fixture = installed_fixture();
    let keys = wire::Identities::new().await;
    let site = wire::Site::start(&keys).await;
    let encrypted = keys.encrypted_input(&site.url, 0, false);
    let pfx = keys.encrypted_input(&site.url, 0, true);
    let input = fixture
        .workspace
        .path()
        .join("certificate-paths-input.json");
    std::fs::write(
        &input,
        serde_json::to_vec(&json!({
            "url":site.url,"certA":STANDARD.encode(keys.certs[0].as_bytes()),
            "certB":STANDARD.encode(keys.certs[1].as_bytes()),
            "key":STANDARD.encode(keys.key.as_bytes()),"pfx":pfx["pfx"],
            "encryptedKey":encrypted["key"],"passphrase":encrypted["passphrase"]
        }))
        .unwrap(),
    )
    .unwrap();
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            let output = Command::new(
                fixture
                    .cache
                    .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
            )
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/browser-client-certificate-paths-baseline.cjs"
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
                        .join(format!("certificate-paths-sdk-{engine}-{mode}.{suffix}")),
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
            assert!(value.get("failure").is_none(), "{value}");
            assert_eq!(value["ownedProcesses"], 0, "{value}");
            for case in [
                "absolutePem",
                "relativePem",
                "pfx",
                "encryptedPem",
                "symlinkPem",
                "inlineWins",
            ] {
                assert_eq!(value["cases"][case]["ok"], true, "{case}: {value}");
                assert_eq!(value["cases"][case]["identity"], "A", "{case}: {value}");
            }
            assert_eq!(value["cases"]["missing"]["ok"], false, "{value}");
            assert_eq!(value["cases"]["missing"]["stage"], "create", "{value}");
            for (name, expected) in [
                ("originalAfterReplace", "A"),
                ("newAfterReplace", "B"),
                ("originalAfterDelete", "A"),
                ("newAfterDelete", "B"),
            ] {
                assert_eq!(value["lifetime"][name], expected, "{name}: {value}");
            }
            if engine != "chrome" {
                assert_eq!(value["lifetime"]["associatedAfterDelete"], "A", "{value}");
                assert_eq!(
                    value["lifetime"]["browserAfterDelete"],
                    if engine == "webkit" { "none" } else { "A" },
                    "{value}"
                );
            }
            eprintln!("Certificate paths {engine}/{mode}: {value}");
        }
    }
}
