//! Registered bytes, directory identity, chooser callbacks and collected downloads.
use super::*;
use base64::Engine as _;

pub(super) async fn fixtures_and_download(session: &Session, page: Reference) {
    let registration = super::super::transfer::Registration {
        name: "Acme.txt".into(),
        mime_type: "text/plain".into(),
        data_base64: base64::engine::general_purpose::STANDARD.encode(b"Acme upload"),
        provenance: super::super::transfer::Provenance::SyntheticGenerated,
        cleared: super::super::transfer::Clearance {
            by: "Acme QA".into(),
            method: "Generated fixture".into(),
        },
    };
    let registered = successful(
        session,
        Request::FixtureRegister {
            fixture: registration,
        },
    )
    .await;
    let fixture = serde_json::from_value(registered["fixture"]["reference"].clone()).unwrap();
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
    wait_text(session, page, "#result", "Acme.txt:Acme upload").await;
    let directory = successful(
        session,
        Request::DirectoryRegister {
            name: "Acme".into(),
            entries: vec![super::super::directory::Entry {
                path: "nested/Acme.txt".into(),
                fixture,
            }],
        },
    )
    .await;
    let directory = serde_json::from_value(directory["fixture"]["reference"].clone()).unwrap();
    successful(session, Request::FixtureVerify { fixture: directory }).await;
    successful(
        session,
        Request::PageUpload {
            page,
            frame: None,
            locator: css("#directory"),
            fixtures: vec![directory],
        },
    )
    .await;
    wait_text(session, page, "#result", "Acme/nested/Acme.txt").await;
    chooser(session, page, fixture).await;
    successful(session, action(page, css("#download"), Action::Click {})).await;
    let download = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let status = session.status();
            if let Some(download) = status["transfers"]["downloads"]
                .as_array()
                .and_then(|items| {
                    items.iter().find(|download| {
                        download["state"] == "complete" || download["state"] == "partial_or_failed"
                    })
                })
            {
                break download.clone();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(download["state"], "complete", "{download}");
    let path = download["artifact"]["path"].as_str().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"Acme download");
    successful(
        session,
        Request::DownloadDelete {
            download: serde_json::from_value(download["reference"].clone()).unwrap(),
        },
    )
    .await;
    assert!(!Path::new(path).exists());
}
async fn chooser(session: &Session, page: Reference, fixture: Reference) {
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
    successful(session, action(page, css("#file"), Action::Click {})).await;
    let chooser = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(chooser) = session.status()["transfers"]["choosers"]
                .as_array()
                .and_then(|items| items.first())
            {
                break serde_json::from_value::<Reference>(chooser["reference"].clone()).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    successful(
        session,
        Request::ChooserResolve {
            chooser,
            fixtures: vec![fixture],
        },
    )
    .await;
    wait_text(session, page, "#result", "Acme.txt:Acme upload").await;
    let stale = session
        .submit(
            Uuid::new_v4(),
            Request::ChooserResolve {
                chooser,
                fixtures: vec![fixture],
            },
            30000,
        )
        .unwrap();
    assert_eq!(
        settled(session, stale).await.code.as_deref(),
        Some("STALE_REFERENCE")
    );
}
