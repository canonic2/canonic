//! Real intercepted uploads and private downloads using owned synthetic bytes.
use super::page_native::{execute, successful};
use super::*;
use crate::browser::{
    contract::{ContextOptions, Engine, Mode},
    page::{Action, Load, Locator, Navigation, Read},
    transfer::{Clearance, Provenance, Registration},
};
use base64::Engine as _;
const BYTES: &[u8] = b"SYNTHETIC TEST DOCUMENT\nAcme,value\nExample,42\n";
fn css(s: &str) -> Locator {
    Locator::Css { selector: s.into() }
}
async fn snapshot_until(session: &Session, select: impl Fn(&Value) -> Option<Value>) -> Value {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let changed = session.0.host.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(value) = select(&session.status()) {
                break value;
            }
            changed.await;
        }
    })
    .await
    .unwrap_or_else(|error| {
        panic!(
            "{error}: {}; {:?}",
            session.status(),
            session.private_failures()
        )
    })
}
#[tokio::test]
async fn firefox_webkit_verified_uploads_choosers_and_downloads_in_both_modes() {
    for engine in [Engine::Firefox, Engine::Webkit] {
        for mode in [Mode::Headless, Mode::Headed] {
            let root = tempfile::tempdir().unwrap();
            let private = root.path().join("state");
            crate::artifacts::directory(&private).unwrap();
            let session = Session::launch(
                super::native::bundle().await,
                Uuid::new_v4(),
                Uuid::new_v4(),
                LaunchOptions {
                    media_fixtures: Default::default(),
                    proxy: None,
                    engine,
                    mode,
                    context: ContextOptions {
                        accept_downloads: true,
                        ..ContextOptions::default()
                    },
                },
                &private,
            )
            .unwrap();
            let scenario = tokio::spawn(exercise(session.clone())).await;
            let operation = session.disconnect(30000).unwrap();
            let closed = session
                .wait(operation, Instant::now() + Duration::from_secs(35))
                .await
                .unwrap();
            assert_eq!(closed.outcome, Outcome::Succeeded, "{}", session.status());
            scenario.expect("Transfer scenario failed");
        }
    }
}
async fn exercise(session: Session) {
    let startup = session
        .wait(session.startup(), Instant::now() + Duration::from_secs(45))
        .await
        .unwrap();
    assert_eq!(startup.outcome, Outcome::Succeeded, "{}", session.status());
    let page: Reference = serde_json::from_value(startup.result.unwrap()["page"].clone()).unwrap();
    let html = r#"<input id=file type=file onchange="document.querySelector('output').textContent=Array.from(this.files).map(f=>f.name+':'+f.size).join(',')"><output>None</output><button id=choose onclick="document.querySelector('#file').click()">Choose</button><a id=download download="../../Acme.csv">Download</a><script>document.querySelector('#download').href=URL.createObjectURL(new Blob(['SYNTHETIC TEST DOCUMENT\nAcme,value\nExample,42\n'],{type:'text/csv'}));</script>"#;
    successful(
        &session,
        Request::PageNavigate {
            page,
            navigation: Navigation::SetContent {
                html: html.into(),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    let fixture = upload_fixture(&session, page).await;
    resolve_chooser(&session, page, fixture).await;
    download_fixture(&session, page).await;
    tampering_and_bounds(&session, page, fixture).await;
}

async fn upload_fixture(session: &Session, page: Reference) -> Reference {
    let fixture = successful(
        session,
        Request::FixtureRegister {
            fixture: Registration {
                name: "Acme.csv".into(),
                mime_type: "text/csv".into(),
                data_base64: base64::engine::general_purpose::STANDARD.encode(BYTES),
                provenance: Provenance::SyntheticGenerated,
                cleared: Clearance {
                    by: "Acme QA".into(),
                    method: "Synthetic fixture generated for this acceptance".into(),
                },
            },
        },
    )
    .await;
    let fixture: Reference =
        serde_json::from_value(fixture["fixture"]["reference"].clone()).unwrap();
    successful(
        session,
        Request::PageUpload {
            page,
            frame: None,
            locator: css("#file"),
            fixtures: vec![fixture],
        },
    )
    .await;
    let text = successful(
        session,
        Request::PageRead {
            page,
            frame: None,
            locator: Some(css("output")),
            read: Read::Text,
            max_bytes: 8192,
        },
    )
    .await;
    assert_eq!(text["result"]["value"], format!("Acme.csv:{}", BYTES.len()));
    successful(
        session,
        Request::PageUpload {
            page,
            frame: None,
            locator: css("#file"),
            fixtures: vec![],
        },
    )
    .await;
    fixture
}

async fn resolve_chooser(session: &Session, page: Reference, fixture: Reference) {
    successful(
        session,
        Request::PageAction {
            page,
            frame: None,
            locator: css("#choose"),
            action: Action::Click {},
            force: false,
            postcondition: None,
        },
    )
    .await;
    let chooser = snapshot_until(session, |s| {
        s["transfers"]["choosers"].as_array()?.first().cloned()
    })
    .await;
    let chooser: Reference = serde_json::from_value(chooser["reference"].clone()).unwrap();
    successful(
        session,
        Request::ChooserResolve {
            chooser,
            fixtures: vec![fixture],
        },
    )
    .await;
    let duplicate = execute(
        session,
        Request::ChooserResolve {
            chooser,
            fixtures: vec![fixture],
        },
        1000,
    )
    .await;
    assert_eq!(duplicate.code.as_deref(), Some("STALE_REFERENCE"));
}

async fn download_fixture(session: &Session, page: Reference) {
    successful(
        session,
        Request::PageAction {
            page,
            frame: None,
            locator: css("#download"),
            action: Action::Click {},
            force: false,
            postcondition: None,
        },
    )
    .await;
    let download = snapshot_until(session, |s| {
        s["transfers"]["downloads"]
            .as_array()?
            .iter()
            .find(|d| d["state"] == "complete" || d["state"] == "partial_or_failed")
            .cloned()
    })
    .await;
    assert_eq!(
        download["state"],
        "complete",
        "{download}; {:?}",
        session.private_failures()
    );
    let path = Path::new(download["artifact"]["path"].as_str().unwrap());
    assert!(
        path.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("download-")
    );
    assert_eq!(std::fs::read(path).unwrap(), BYTES);
    assert_eq!(
        download["artifact"]["sha256"],
        format!("{:x}", Sha256::digest(BYTES))
    );
    let download: Reference = serde_json::from_value(download["reference"].clone()).unwrap();
    successful(session, Request::DownloadDelete { download }).await;
    assert!(!path.exists());
    successful(session, Request::DownloadDelete { download }).await;
}

async fn tampering_and_bounds(session: &Session, page: Reference, fixture: Reference) {
    let stored = session.0.host.transfers.lock().unwrap().fixtures[&fixture]
        .path
        .clone();
    std::fs::write(stored, b"changed fixture").unwrap();
    let changed = execute(session, Request::FixtureVerify { fixture }, 1000).await;
    assert_eq!(changed.code.as_deref(), Some("FIXTURE_CHANGED"));
    successful(session, Request::PageNavigate { page, navigation: Navigation::SetContent {
        html: r#"<a id=large download=large.bin>Download</a><script>document.querySelector('a').href=URL.createObjectURL(new Blob([new Uint8Array(9*1024*1024)]));</script>"#.into(), wait_until: Load::Load,
    }}).await;
    successful(
        session,
        Request::PageAction {
            page,
            frame: None,
            locator: css("#large"),
            action: Action::Click {},
            force: false,
            postcondition: None,
        },
    )
    .await;
    let oversized = snapshot_until(session, |s| {
        s["transfers"]["downloads"]
            .as_array()?
            .iter()
            .find(|d| d["state"] == "partial_or_failed")
            .cloned()
    })
    .await;
    assert_eq!(oversized["code"], "DOWNLOAD_BOUNDS");
    assert!(oversized.get("artifact").is_none());
    let oversized = serde_json::from_value(oversized["reference"].clone()).unwrap();
    successful(
        session,
        Request::DownloadDelete {
            download: oversized,
        },
    )
    .await;
}
