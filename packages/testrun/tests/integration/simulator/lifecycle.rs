//! Shipping lifecycle requests against an isolated simulator command boundary.
//!
//! The fake `xcrun` boots and shuts down a simulator that has no framebuffer,
//! so each connect reaches the owned boot and then fails its first
//! observation; the owner's cleanup is what these cases check.
use super::*;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};
#[cfg(feature = "acceptance-ios-simulator")]
mod native;

struct Fixture {
    client: Client,
    root: tempfile::TempDir,
    set: PathBuf,
}
impl Fixture {
    async fn start() -> Self {
        let root = tempfile::tempdir().unwrap();
        let set = root.path().join("Acme device set");
        std::fs::create_dir(&set).unwrap();
        let set = set.canonicalize().unwrap();
        // Every command names the session's device set, whichever process
        // runs it.
        std::fs::write(root.path().join("set-path"), set.to_str().unwrap()).unwrap();
        std::fs::write(root.path().join("state"), "Shutdown").unwrap();
        std::fs::write(root.path().join("stamp"), "2026-10-19T10:00:00Z").unwrap();
        let tool = root.path().join("xcrun");
        std::fs::write(&tool, r#"#!/bin/sh
set -eu
cd "$(dirname "$0")"
printf '%s\n' "$*" >> calls
[ "$1" = simctl ]; shift
[ "$1" = --set ]; shift
[ "$1" = "$(cat set-path)" ]; shift
case "$1" in
list) printf '{"devices":{"iOS-Acme":[{"udid":"AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE","name":"Testrun-QA-Acme","state":"%s","isAvailable":true,"lastBootedAt":"%s"}]}}\n' "$(cat state)" "$(cat stamp)" ;;
boot) if [ -f uncertain-before ]; then exit 7; fi; echo Booted > state; echo 2026-10-19T11:00:00Z > stamp; if [ -f uncertain-boot ]; then exit 7; fi; if [ -f uncertain-boot-now ]; then date -u +%Y-%m-%dT%H:%M:%SZ > stamp; exit 7; fi ;;
bootstatus) echo started > bootstatus; while [ -f hold ]; do sleep 0.02; done ;;
shutdown) if [ -f fail-close ]; then exit 7; fi; echo Shutdown > state ;;
*) exit 9 ;;
esac
"#).unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
        let client = Client::start_environment(
            root.path(),
            Duration::from_secs(60),
            None,
            &[
                ("TESTRUN_SIMULATOR_DEVICE_SET", set.as_os_str()),
                ("TESTRUN_XCRUN", tool.as_os_str()),
            ],
        )
        .await;
        Self { client, root, set }
    }
    async fn arguments(&mut self, boot: bool) -> Value {
        let list = self
            .client
            .call("target_list", json!({"kind":"simulator"}))
            .await;
        assert_eq!(list["ok"], true, "{list}");
        let target = list["data"]["targets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|target| target["name"] == "Testrun-QA-Acme")
            .expect("custom device set missing");
        assert_eq!(target["deviceSetPath"], self.set.to_str().unwrap());
        assert_eq!(target["platform"], "ios");
        assert!(target["targetId"].as_str().unwrap().matches(':').count() == 2);
        json!({"targetId":target["targetId"],"boot":boot})
    }
    async fn open(&mut self, boot: bool) -> Value {
        let args = self.arguments(boot).await;
        self.client.call("connect", args).await
    }
    fn state(&self) -> String {
        std::fs::read_to_string(self.root.path().join("state"))
            .unwrap()
            .trim()
            .to_owned()
    }
    fn calls(&self, command: &str) -> usize {
        std::fs::read_to_string(self.root.path().join("calls"))
            .unwrap()
            .lines()
            .filter(|line| line.contains(&format!(" {command} ")))
            .count()
    }
    async fn until_state(&self, expected: &str) {
        tokio::time::timeout(Duration::from_secs(30), async {
            while self.state() != expected {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("simulator did not reach {expected}"));
    }
    async fn finish(mut self) {
        let closed = self.client.call("disconnect", json!({})).await;
        assert_eq!(closed["ok"], true, "{closed}");
        drop(self.client.input);
        assert!(self.client.child.wait().await.unwrap().success());
    }
}

#[tokio::test]
async fn cancelled_boot_keeps_status_responsive_and_cleans_up_after_startup_settles() {
    let mut fixture = Fixture::start().await;
    std::fs::write(fixture.root.path().join("hold"), "held").unwrap();
    let args = fixture.arguments(true).await;
    let id = fixture
        .client
        .begin_request("tools/call", json!({"name":"connect","arguments":args}))
        .await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while !fixture.root.path().join("bootstatus").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(fixture.client.request("ping", json!({})).await["result"].is_object());
    fixture.client.input.write_all(format!("{}\n", json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}})).as_bytes()).await.unwrap();
    assert_eq!(
        fixture.client.request("ping", json!({})).await["result"],
        json!({})
    );
    let status = tokio::time::timeout(
        Duration::from_secs(5),
        fixture.client.call("status", json!({})),
    )
    .await
    .expect("status waits for a held boot");
    assert_eq!(status["ok"], true, "{status}");
    assert_ne!(status["data"]["state"], "ready", "{status}");
    assert!(fixture.client.request("ping", json!({})).await["result"].is_object());
    std::fs::remove_file(fixture.root.path().join("hold")).unwrap();
    // Startup settles without a framebuffer; the owner shuts down its own boot.
    fixture.until_state("Shutdown").await;
    assert_eq!(fixture.calls("boot"), 1, "the boot was replayed");
    fixture.finish().await;
}

#[tokio::test]
async fn failed_shutdown_is_retryable_and_replacement_boot_is_preserved() {
    let mut fixture = Fixture::start().await;
    std::fs::write(fixture.root.path().join("fail-close"), "fault").unwrap();
    let opened = fixture.open(true).await;
    assert_eq!(opened["ok"], false, "{opened}");
    assert_eq!(fixture.state(), "Booted");
    // Connect's cleanup may retry the failing shutdown; each attempt is the owned boot.
    let attempts = fixture.calls("shutdown");
    assert!(attempts >= 1);
    std::fs::remove_file(fixture.root.path().join("fail-close")).unwrap();
    // A different boot stamp cannot be shut down by the old owner's retry.
    std::fs::write(fixture.root.path().join("stamp"), "2026-10-19T12:00:00Z").unwrap();
    let closed = fixture.client.call("disconnect", json!({})).await;
    assert_eq!(closed["ok"], true, "{closed}");
    assert_eq!(fixture.state(), "Booted");
    assert_eq!(fixture.calls("shutdown"), attempts);
    fixture.finish().await;
}

#[tokio::test]
async fn boot_side_effect_without_completion_proof_remains_uncertain_without_replay() {
    for fault in ["uncertain-boot", "uncertain-before"] {
        let mut fixture = Fixture::start().await;
        std::fs::write(fixture.root.path().join(fault), "fault").unwrap();
        let opened = fixture.open(true).await;
        assert_eq!(opened["ok"], false, "{opened}");
        assert!(
            opened.to_string().contains("SIMULATOR_BOOT_UNCERTAIN"),
            "{opened}"
        );
        let status = fixture.client.call("status", json!({})).await;
        assert_ne!(status["data"]["state"], "ready", "{status}");
        assert_eq!(fixture.calls("boot"), 1, "{fault}: the boot was replayed");
        assert_eq!(
            fixture.calls("shutdown"),
            0,
            "{fault}: an unproven boot was shut down"
        );
        drop(fixture.client.input);
        let _ = fixture.client.child.wait().await;
    }
}

#[tokio::test]
async fn an_uncertain_boot_that_happened_right_after_dispatch_is_shut_down_once() {
    let mut fixture = Fixture::start().await;
    std::fs::write(fixture.root.path().join("uncertain-boot-now"), "fault").unwrap();
    let opened = fixture.open(true).await;
    assert_eq!(opened["ok"], false, "{opened}");
    assert!(
        opened.to_string().contains("SIMULATOR_BOOT_UNCERTAIN"),
        "{opened}"
    );
    // Its boot time follows the dispatch, so cleanup knows Testrun booted it.
    fixture.until_state("Shutdown").await;
    assert_eq!(fixture.calls("boot"), 1, "the boot was replayed");
    assert_eq!(fixture.calls("shutdown"), 1);
    drop(fixture.client.input);
    let _ = fixture.client.child.wait().await;
}

#[tokio::test]
async fn custom_set_boot_is_explicit_and_owned_shutdown_preserves_external_boots() {
    let mut fixture = Fixture::start().await;
    let refused = fixture.open(false).await;
    assert_eq!(refused["code"], "TARGET_NOT_READY", "{refused}");
    assert_eq!(fixture.calls("boot"), 0);
    let owned = fixture.open(true).await;
    assert_eq!(owned["ok"], false, "{owned}");
    assert_eq!(fixture.calls("boot"), 1);
    fixture.until_state("Shutdown").await;
    assert_eq!(
        fixture.client.call("disconnect", json!({})).await["ok"],
        true
    );
    std::fs::write(fixture.root.path().join("state"), "Booted").unwrap();
    let external = fixture.open(true).await;
    assert_eq!(external["ok"], false, "{external}");
    assert_eq!(fixture.calls("boot"), 1, "an external boot was repeated");
    assert_eq!(
        fixture.client.call("disconnect", json!({})).await["ok"],
        true
    );
    assert_eq!(fixture.state(), "Booted");
    assert_eq!(fixture.calls("shutdown"), 1);
    fixture.finish().await;
}
