//! Browser feature ownership and interruption against a synthetic CDP peer.
use super::{investigation::owned_page, *};
use crate::browser::{directory, extensions, pwa};

async fn register_extension_source(worker: &mut Worker) -> (Reference, std::path::PathBuf) {
    let reference = worker.transfer_reference();
    let directory = directory::create(
        worker.journal.directory().into(),
        reference,
        "acme-extension".into(),
        vec![(
            directory::Entry {
                path: "manifest.json".into(),
                fixture: worker.transfer_reference(),
            },
            json!({}),
            br#"{"manifest_version":3,"name":"Acme","version":"1.0","action":{}}"#.to_vec(),
        )],
    )
    .await
    .unwrap();
    let path = std::fs::canonicalize(&directory.path).unwrap();
    worker.transfers.directories.insert(reference, directory);
    (reference, path)
}

async fn extension_inventory(peer: &mut Peer, entries: Value) {
    let list = command(peer, "Extensions.getExtensions").await;
    reply(peer, &list, json!({"extensions":entries})).await;
}

#[tokio::test]
async fn native_extension_install_and_removal_are_bound_to_one_fixture() {
    let (mut worker, mut peer, _root, context, _) = owned_page().await;
    worker.managed_apps = true;
    assert_eq!(
        worker
            .extension_operation(
                Uuid::new_v4(),
                extensions::Action::List {
                    cursor: None,
                    limit: None
                },
                deadline()
            )
            .await
            .unwrap_err()
            .code,
        "EXTENSION_REGULAR_CONTEXT_REQUIRED"
    );
    worker.persistent_context = Some(context);
    worker.extension_profile = true;
    let (reference, path) = register_extension_source(&mut worker).await;
    let installed = tokio::spawn(async move {
        let rejected = worker
            .extension_operation(
                Uuid::new_v4(),
                extensions::Action::Install {
                    directory: reference,
                },
                deadline(),
            )
            .await
            .unwrap_err();
        assert_eq!(rejected.code, "BROWSER_METHOD_FAILED");
        assert!(worker.extensions.possible_registrations().is_empty());
        let result = worker
            .extension_operation(
                Uuid::new_v4(),
                extensions::Action::Install {
                    directory: reference,
                },
                deadline(),
            )
            .await
            .unwrap();
        (worker, result)
    });
    let foreign = json!({"id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","name":"Acme neighbor","version":"1","enabled":true,"path":"/private/tmp/acme-neighbor"});
    extension_inventory(&mut peer, json!([foreign])).await;
    let load = command(&mut peer, "Extensions.loadUnpacked").await;
    send(
        &mut peer,
        json!({"id":load["id"],"error":{"code":-32000,"message":"Acme load refused"}}),
    )
    .await;
    extension_inventory(&mut peer, json!([foreign])).await;
    let load = command(&mut peer, "Extensions.loadUnpacked").await;
    assert_eq!(load["params"], json!({"path":path}));
    let native = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    reply(&mut peer, &load, json!({"id":native})).await;
    let own = json!({"id":native,"name":"Acme","version":"1.0","enabled":true,"path":path});
    extension_inventory(&mut peer, json!([foreign, own])).await;
    let (mut worker, result) = installed.await.unwrap();
    assert_eq!(result["extension"]["source"], json!(reference));
    assert!(result["extension"].get("path").is_none());
    let extension_id = serde_json::from_value(result["extension"]["extensionId"].clone()).unwrap();

    let uninstall = tokio::spawn(async move {
        assert_eq!(
            worker
                .extension_operation(
                    Uuid::new_v4(),
                    extensions::Action::Reload { extension_id },
                    deadline(),
                )
                .await
                .unwrap()["reloaded"],
            true
        );
        worker
            .extension_operation(
                Uuid::new_v4(),
                extensions::Action::Uninstall { extension_id },
                deadline(),
            )
            .await
            .unwrap()
    });
    extension_inventory(&mut peer, json!([foreign, own])).await;
    let reload = command(&mut peer, "Extensions.loadUnpacked").await;
    assert_eq!(reload["params"]["path"], json!(path));
    reply(&mut peer, &reload, json!({"id":native})).await;
    extension_inventory(&mut peer, json!([foreign, own])).await;
    let remove = command(&mut peer, "Extensions.uninstall").await;
    assert_eq!(remove["params"]["id"], native);
    reply(&mut peer, &remove, json!({})).await;
    let targets = command(&mut peer, "Target.getTargets").await;
    reply(&mut peer, &targets, json!({"targetInfos":[{"targetId":"acme-worker","type":"service_worker","title":"Acme","url":format!("chrome-extension://{native}/worker.js"),"attached":false,"canAccessOpener":false}]})).await;
    let targets = command(&mut peer, "Target.getTargets").await;
    reply(&mut peer, &targets, json!({"targetInfos":[]})).await;
    assert_eq!(uninstall.await.unwrap()["removed"], true);
}

async fn pwa_absent(peer: &mut Peer, command: &Value, manifest: &str) {
    send(peer, json!({"id":command["id"],"error":{"code":-32602,"message":format!("Unknown web-app manifest id {manifest}")}})).await;
}

#[tokio::test]
async fn native_extension_refuses_regular_context_discovered_by_a_pwa() {
    let (mut worker, mut peer, _root, isolated, page) = owned_page().await;
    worker.managed_apps = true;
    let launching = tokio::spawn(async move {
        let id = Uuid::new_v4();
        let installed = worker
            .pwa_operation(id, pwa::Action::Install { page }, deadline())
            .await
            .unwrap();
        let app_id = serde_json::from_value(installed["appId"].clone()).unwrap();
        let launched = worker
            .pwa_operation(id, pwa::Action::Launch { app_id, url: None }, deadline())
            .await
            .unwrap();
        assert_ne!(launched["page"], json!(page));
        worker
    });
    let install = pending_pwa_install(&mut peer).await;
    reply(&mut peer, &install, json!({})).await;
    let targets = command(&mut peer, "Target.getTargets").await;
    reply(&mut peer, &targets, json!({"targetInfos":[]})).await;
    let launch = command(&mut peer, "PWA.launch").await;
    reply(&mut peer, &launch, json!({"targetId":"pwa-tab"})).await;
    let probe = command(&mut peer, "Target.attachToTarget").await;
    assert_eq!(probe["params"]["targetId"], "pwa-tab");
    reply(&mut peer, &probe, json!({"sessionId":"pwa-probe"})).await;
    let auto = command(&mut peer, "Target.setAutoAttach").await;
    let info = json!({"targetId":"pwa-page","type":"page","title":"Acme","url":"http://localhost:9999/acme/","attached":true,"canAccessOpener":false,"browserContextId":"regular-acme"});
    send(&mut peer, json!({"sessionId":"pwa-probe","method":"Target.attachedToTarget","params":{"sessionId":"pwa-child","targetInfo":info,"waitingForDebugger":false}})).await;
    reply(&mut peer, &auto, json!({})).await;
    let detach = command(&mut peer, "Target.detachFromTarget").await;
    assert_eq!(detach["params"]["sessionId"], "pwa-probe");
    reply(&mut peer, &detach, json!({})).await;
    serve_page_setup(
        &mut peer,
        &info,
        "pwa-session",
        "pwa-frame",
        "pwa-world",
        true,
    )
    .await;
    let mut worker = launching.await.unwrap();
    let regular = worker.persistent_context.unwrap();
    assert_ne!(regular, isolated);
    assert_eq!(worker.index.retained_page_context(page), Some(isolated));
    let directory = worker.transfer_reference();
    let result = tokio::select! {
        command = peer.next() => panic!("Unexpected CDP command: {command:?}"),
        result = worker.extension_operation(
            Uuid::new_v4(),
            extensions::Action::Install { directory },
            deadline(),
        ) => result,
    };
    assert_eq!(
        result.unwrap_err().code,
        "EXTENSION_REGULAR_CONTEXT_REQUIRED"
    );
}

async fn pending_pwa_install(peer: &mut Peer) -> Value {
    let info = command(peer, "Target.getTargetInfo").await;
    assert_eq!(info["params"]["targetId"], "page-acme");
    let manifest = "http://localhost:9999/acme/";
    reply(peer, &info, json!({"targetInfo":{"targetId":"page-acme","type":"page","title":"Acme","url":manifest,"attached":true,"canAccessOpener":false}})).await;
    let get = command(peer, "Page.getAppManifest").await;
    assert_eq!(get["sessionId"], "session-acme");
    reply(peer, &get, json!({"url":format!("{manifest}manifest.webmanifest"),"errors":[],"manifest":{"id":manifest,"startUrl":manifest,"scope":manifest}})).await;
    let probe = command(peer, "PWA.getOsAppState").await;
    pwa_absent(peer, &probe, manifest).await;
    let install = command(peer, "PWA.install").await;
    assert_eq!(install["params"]["manifestId"], manifest);
    install
}

#[tokio::test]
async fn native_pwa_interrupted_install_is_settled_before_owned_removal() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    worker.managed_apps = true;
    let id = Uuid::new_v4();
    let owner = worker.shared.clone();
    owner
        .state
        .lock()
        .unwrap()
        .begin(id, "install".into())
        .unwrap();
    let installing = tokio::spawn(async move {
        let error = worker
            .pwa_operation(id, pwa::Action::Install { page }, deadline())
            .await
            .unwrap_err();
        (worker, error)
    });
    let install = pending_pwa_install(&mut peer).await;
    let manifest = "http://localhost:9999/acme/";
    owner
        .state
        .lock()
        .unwrap()
        .operations
        .get_mut(&id)
        .unwrap()
        .cancel_requested = true;
    owner.changed.notify_waiters();
    let (mut worker, error) = installing.await.unwrap();
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    assert!(worker.pwas.owns_pending(id));
    assert_eq!(error.partial.unwrap()["replayed"], false);

    let cleanup = tokio::spawn(async move { worker.cleanup_pwas(deadline()).await });
    assert!(
        tokio::time::timeout(Duration::from_millis(50), peer.next())
            .await
            .is_err()
    );
    reply(&mut peer, &install, json!({})).await;
    let probe = command(&mut peer, "PWA.getOsAppState").await;
    reply(&mut peer, &probe, json!({"badgeCount":0,"fileHandlers":[]})).await;
    let remove = command(&mut peer, "PWA.uninstall").await;
    assert_eq!(remove["params"]["manifestId"], manifest);
    reply(&mut peer, &remove, json!({})).await;
    let targets = command(&mut peer, "Target.getTargets").await;
    reply(&mut peer, &targets, json!({"targetInfos":[]})).await;
    let probe = command(&mut peer, "PWA.getOsAppState").await;
    pwa_absent(&mut peer, &probe, manifest).await;
    cleanup.await.unwrap().unwrap();
}

#[tokio::test]
async fn native_pwa_rejected_install_frees_its_slot_and_manifest() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    worker.managed_apps = true;
    let operation = tokio::spawn(async move {
        let id = Uuid::new_v4();
        let error = worker
            .pwa_operation(id, pwa::Action::Install { page }, deadline())
            .await
            .unwrap_err();
        assert_eq!(error.code, "BROWSER_METHOD_FAILED");
        let listed = worker
            .pwa_operation(
                id,
                pwa::Action::List {
                    cursor: None,
                    limit: None,
                },
                deadline(),
            )
            .await
            .unwrap();
        assert_eq!(listed["apps"], json!([]));
        worker
            .pwa_operation(id, pwa::Action::Install { page }, deadline())
            .await
    });
    let install = pending_pwa_install(&mut peer).await;
    send(
        &mut peer,
        json!({"id":install["id"],"error":{"code":-32000,"message":"Acme install refused"}}),
    )
    .await;
    let retry = pending_pwa_install(&mut peer).await;
    reply(&mut peer, &retry, json!({})).await;
    assert_eq!(operation.await.unwrap().unwrap()["installed"], true);
}

#[tokio::test]
async fn native_pwa_launch_refuses_a_preexisting_foreign_tab() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    worker.managed_apps = true;
    let launch = tokio::spawn(async move {
        let id = Uuid::new_v4();
        let value = worker
            .pwa_operation(id, pwa::Action::Install { page }, deadline())
            .await
            .unwrap();
        let app_id = serde_json::from_value(value["appId"].clone()).unwrap();
        let error = worker
            .pwa_operation(id, pwa::Action::Launch { app_id, url: None }, deadline())
            .await
            .unwrap_err();
        (worker, error)
    });
    let install = pending_pwa_install(&mut peer).await;
    reply(&mut peer, &install, json!({})).await;
    let targets = command(&mut peer, "Target.getTargets").await;
    reply(&mut peer, &targets, json!({"targetInfos":[{"targetId":"foreign-tab","type":"tab","title":"Acme","url":"about:blank","attached":false,"canAccessOpener":false}]})).await;
    let launching = command(&mut peer, "PWA.launch").await;
    reply(&mut peer, &launching, json!({"targetId":"foreign-tab"})).await;
    let (_worker, error) = tokio::select! {
        command = peer.next() => panic!("Unexpected command for an unowned tab: {command:?}"),
        result = launch => result.unwrap(),
    };
    assert_eq!(error.code, "PWA_TARGET_NOT_OWNED");
}

#[tokio::test]
async fn native_feature_list_cursors_preserve_scope_and_order() {
    let (worker, _, _root) = fixture().await;
    let items = vec![
        (Uuid::from_u128(1), json!("first")),
        (Uuid::from_u128(2), json!("second")),
    ];
    let first = worker
        .page_profile_apps("apps", None, Some(1), items.clone())
        .unwrap();
    assert_eq!(first["apps"], json!(["first"]));
    let cursor = first["nextCursor"].as_str().unwrap();
    let second = worker
        .page_profile_apps("apps", Some(cursor), Some(1), items.clone())
        .unwrap();
    assert_eq!(second, json!({"apps":["second"],"nextCursor":null}));
    assert!(
        worker
            .page_profile_apps("extensions", Some(cursor), Some(1), items)
            .is_err()
    );
}

#[tokio::test]
async fn native_pwa_unresolved_mutation_allows_close_with_transport_still_open() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    worker.managed_apps = true;
    let operation = tokio::spawn(async move {
        let installed = worker
            .pwa_operation(Uuid::new_v4(), pwa::Action::Install { page }, deadline())
            .await
            .unwrap();
        let app_id = serde_json::from_value(installed["appId"].clone()).unwrap();
        worker.pwas.mark_outcome_unknown(app_id);
        assert!(worker.connection.as_ref().unwrap().status().open);
        let receipt = disconnect_receipt(&mut worker).await;
        assert_eq!(receipt["cleanup"]["pwaCleanupUnverified"], json!([app_id]));
    });
    let install = pending_pwa_install(&mut peer).await;
    reply(&mut peer, &install, json!({})).await;
    operation.await.unwrap();
}

#[tokio::test]
async fn native_pwa_transport_loss_releases_profile_and_journals_unverified_cleanup() {
    let (mut worker, mut peer, _root, _, page) = owned_page().await;
    worker.managed_apps = true;
    let profile = Process::profile_path(worker.journal.directory());
    crate::artifacts::directory(&profile).unwrap();
    let operation = tokio::spawn(async move {
        let error = worker
            .pwa_operation(Uuid::new_v4(), pwa::Action::Install { page }, deadline())
            .await
            .unwrap_err();
        (worker, error)
    });
    pending_pwa_install(&mut peer).await;
    drop(peer);
    let (mut worker, error) = operation.await.unwrap();
    assert_eq!(error.code, "PWA_OUTCOME_UNRESOLVED");
    let app_id = error.partial.unwrap()["appId"].clone();
    let receipt = disconnect_receipt(&mut worker).await;
    assert!(!profile.exists());
    assert_eq!(receipt["cleanup"]["pwaCleanupUnverified"], json!([app_id]));
    assert!(
        std::fs::read_dir(worker.journal.directory())
            .unwrap()
            .any(|file| {
                let Ok(bytes) = std::fs::read(file.unwrap().path()) else {
                    return false;
                };
                serde_json::from_slice::<Value>(&bytes).is_ok_and(|row| {
                    row["stage"] == "pwa_cleanup_unverified" && row["apps"][0]["appId"] == app_id
                })
            })
    );
}

async fn disconnect_receipt(worker: &mut Worker) -> Value {
    let id = Uuid::new_v4();
    {
        let mut state = worker.shared.state.lock().unwrap();
        state.begin(id, "disconnect".into()).unwrap();
        state.phase = Phase::Closing;
    }
    worker
        .perform(Command {
            id,
            request: None,
            deadline: deadline(),
            queue_slot: None,
        })
        .await;
    let state = worker.shared.state.lock().unwrap();
    assert_eq!(state.phase, Phase::Closed);
    let receipt = &state.operations[&id];
    assert_eq!(receipt.outcome, Outcome::Succeeded, "{receipt:?}");
    receipt.result.clone().unwrap()
}
