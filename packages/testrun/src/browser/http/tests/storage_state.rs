//! Independent state, wire, privacy and failed-publication witnesses.
use super::*;

fn cookie(name: &str, domain: &str, path: &str, value: &str) -> Value {
    json!({"name":name,"value":value,"domain":domain,"path":path,"expires":-1,"httpOnly":true,"secure":false,"sameSite":"Lax"})
}
fn initial(cookies: Vec<Value>) -> Value {
    json!({"cookies":cookies,"origins":[{"origin":"https://example.com","localStorage":[{"name":"Acme","value":"Acme-private-local"}],"indexedDB":[{"name":"Acme-private-db","version":1,"stores":[]}],"opfs":[{"path":"Acme-private.txt","type":"file","base64":"QWNtZQ=="}]}]})
}
fn options(state: Value) -> Options {
    Options {
        storage_state: Some(StorageStateInput::Inline(state)),
        ..Default::default()
    }
}
fn create_state(contexts: &mut Contexts, state: Value) -> Reference {
    serde_json::from_value(
        contexts.create(Uuid::new_v4(), options(state)).unwrap()["httpContext"].clone(),
    )
    .unwrap()
}
fn read_state(contexts: &Contexts, receipt: &Value) -> Value {
    let evidence = serde_json::from_value(receipt["reports"][0]["evidenceId"].clone()).unwrap();
    serde_json::from_slice(&contexts.evidence_get(evidence).unwrap().read().unwrap()).unwrap()
}
fn write_private(path: &Path, value: &Value) {
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    serde_json::to_writer(options.open(path).unwrap(), value).unwrap();
}

#[test]
fn http_storage_state_cookie_scope_identity_order_and_secure_loopback_match_the_pinned_store() {
    let mut secure = cookie("secure", "localhost", "/", "Acme-secure");
    secure["secure"] = json!(true);
    let mut cookies = Cookies::initialize(&[
        cookie("first", "example.com", "/", "old"),
        cookie("second", "example.com", "/", "two"),
        cookie("first", ".example.com", "/", "domain"),
        cookie("first", "example.com", "/", "replaced"),
        cookie("deep", "example.com", "/acme/", "deep"),
        secure,
    ])
    .unwrap();
    assert_eq!(
        cookies.header(&absolute_url("https://example.com/acme/").unwrap()),
        "first=domain; first=replaced; second=two; deep=deep"
    );
    assert_eq!(
        cookies.header(&absolute_url("https://child.example.com/acme/").unwrap()),
        "first=domain"
    );
    assert_eq!(
        cookies.header(&absolute_url("https://notexample.com/acme/").unwrap()),
        ""
    );
    assert_eq!(
        cookies.header(&absolute_url("https://example.com/acme-other").unwrap()),
        "first=domain; first=replaced; second=two"
    );
    assert_eq!(
        cookies.header(&absolute_url("http://localhost/").unwrap()),
        "secure=Acme-secure"
    );
    let mut duplicate_slashes =
        Cookies::initialize(&[cookie("slash", "example.com", "/acme//", "private")]).unwrap();
    assert_eq!(
        duplicate_slashes.header(&absolute_url("https://example.com/acme/").unwrap()),
        ""
    );
    assert_eq!(
        duplicate_slashes.header(&absolute_url("https://example.com/acme//child").unwrap()),
        "slash=private"
    );
}

#[test]
fn http_storage_state_exports_current_cookies_and_selected_opaque_origin_records_privately() {
    let mut contexts = Contexts::default();
    let state = initial(vec![cookie(
        "Acme",
        "example.com",
        "/",
        "Acme-private-cookie",
    )]);
    let context = create_state(&mut contexts, state.clone());
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("private");
    crate::artifacts::directory(&directory).unwrap();
    for (indexed_db, opfs) in [(false, false), (true, false), (false, true), (true, true)] {
        let receipt = contexts
            .storage_state(
                context,
                StorageStateOptions {
                    indexed_db,
                    opfs,
                    ..Default::default()
                },
                &directory,
            )
            .unwrap();
        assert!(!receipt.to_string().contains("Acme-private"));
        let captured = read_state(&contexts, &receipt);
        let evidence = contexts
            .evidence_get(
                serde_json::from_value(receipt["reports"][0]["evidenceId"].clone()).unwrap(),
            )
            .unwrap();
        assert_eq!(
            evidence_body(&evidence).unwrap_err().code,
            "BODY_EVIDENCE_FORMAT",
            "Credential state must not become route fulfillment body evidence"
        );
        assert_eq!(captured["cookies"], state["cookies"]);
        assert_eq!(
            captured["origins"][0]["localStorage"],
            state["origins"][0]["localStorage"]
        );
        assert_eq!(
            captured["origins"][0]["indexedDB"],
            if indexed_db {
                state["origins"][0]["indexedDB"].clone()
            } else {
                json!([])
            }
        );
        if opfs {
            assert_eq!(captured["origins"][0]["opfs"], state["origins"][0]["opfs"]);
        } else {
            assert!(captured["origins"][0].get("opfs").is_none());
        }
        super::super::super::schema::validate(
            "APIRequestContextStorageStateResult",
            Some(&captured),
            &super::super::super::objects::ObjectTable::default(),
        )
        .unwrap();
    }
    assert!(!contexts.list().to_string().contains("Acme-private"));
    assert!(
        !contexts
            .evidence_list()
            .to_string()
            .contains("Acme-private")
    );
    contexts.clear_cookies(context).unwrap();
    let empty = contexts
        .storage_state(context, StorageStateOptions::default(), &directory)
        .unwrap();
    let empty = read_state(&contexts, &empty);
    assert_eq!(empty["cookies"], json!([]));
    assert_eq!(
        empty["origins"][0]["localStorage"],
        state["origins"][0]["localStorage"]
    );
}

#[test]
fn http_storage_state_recovery_keeps_original_bytes_after_cookie_changes_and_context_removal() {
    let mut contexts = Contexts::default();
    let state = initial(vec![cookie(
        "Acme",
        "example.com",
        "/",
        "Acme-private-original",
    )]);
    let context = create_state(&mut contexts, state.clone());
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("blocked");
    std::fs::write(&directory, b"blocked").unwrap();
    let error = contexts
        .storage_state(
            context,
            StorageStateOptions {
                indexed_db: true,
                opfs: true,
                ..Default::default()
            },
            &directory,
        )
        .unwrap_err();
    let partial = error.partial.unwrap();
    assert_eq!(partial["stateRetained"], true);
    assert_eq!(partial["replayed"], false);
    assert!(!partial.to_string().contains("Acme-private-original"));
    let capture = serde_json::from_value(partial["captureId"].clone()).unwrap();
    let pending = contexts.evidence_list();
    assert_eq!(pending["pendingArtifacts"].as_array().unwrap().len(), 1);
    contexts.clear_cookies(context).unwrap();
    contexts.remove(context).unwrap();
    assert_eq!(contexts.close().unwrap_err().code, "CLEANUP_PENDING");
    std::fs::remove_file(&directory).unwrap();
    crate::artifacts::directory(&directory).unwrap();
    let receipt = contexts.finalize_evidence(&directory, capture).unwrap();
    assert_eq!(read_state(&contexts, &receipt), state);
    assert_eq!(contexts.evidence_list()["pendingArtifacts"], json!([]));
    contexts.close().unwrap();
    assert_eq!(
        contexts
            .finalize_evidence(&directory, Uuid::new_v4())
            .unwrap_err()
            .code,
        "UNKNOWN_CAPTURE"
    );
}

#[tokio::test]
async fn http_storage_state_private_file_is_snapshotted_before_allocation_and_omitted_from_debug() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("private");
    crate::artifacts::directory(&directory).unwrap();
    let path = directory.join("Acme-private-state.json");
    let state = initial(vec![cookie(
        "Acme",
        "example.com",
        "/",
        "Acme-private-original",
    )]);
    write_private(&path, &state);
    let options = Options {
        storage_state: Some(StorageStateInput::File(path.to_str().unwrap().into())),
        ..Default::default()
    };
    assert!(!format!("{options:?}").contains("Acme-private"));
    let prepared = prepare_options(options, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    std::fs::write(&path, b"changed after preparation").unwrap();
    let mut contexts = Contexts::default();
    let context = serde_json::from_value(
        contexts.create(Uuid::new_v4(), prepared).unwrap()["httpContext"].clone(),
    )
    .unwrap();
    let captured = contexts
        .storage_state(
            context,
            StorageStateOptions {
                indexed_db: true,
                opfs: true,
                ..Default::default()
            },
            &directory,
        )
        .unwrap();
    assert_eq!(read_state(&contexts, &captured), state);
}

#[tokio::test]
async fn http_storage_state_caller_files_follow_links_and_ignore_permissions_like_read_file() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("Acme shared state.json");
    let state = initial(vec![cookie("Acme", "example.com", "/", "Acme-shared")]);
    std::fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let link = root.path().join("Acme link.json");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&path, &link).unwrap();
    #[cfg(not(unix))]
    std::fs::copy(&path, &link).unwrap();
    for selected in [&path, &link] {
        let prepared = prepare_options(
            Options {
                storage_state: Some(StorageStateInput::File(selected.to_str().unwrap().into())),
                ..Default::default()
            },
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
        let mut contexts = Contexts::default();
        let context = serde_json::from_value(
            contexts.create(Uuid::new_v4(), prepared).unwrap()["httpContext"].clone(),
        )
        .unwrap();
        let directory = root.path().join(format!("evidence-{}", Uuid::new_v4()));
        crate::artifacts::directory(&directory).unwrap();
        let receipt = contexts
            .storage_state(
                context,
                StorageStateOptions {
                    indexed_db: true,
                    opfs: true,
                    ..Default::default()
                },
                &directory,
            )
            .unwrap();
        assert_eq!(read_state(&contexts, &receipt), state);
    }
    let refused = |path: &str| Options {
        storage_state: Some(StorageStateInput::File(path.into())),
        ..Default::default()
    };
    assert_eq!(
        refused("relative.json").validate().unwrap_err().code,
        "HTTP_STORAGE_STATE_PATH_INVALID"
    );
    let missing = root.path().join("Acme missing.json");
    assert_eq!(
        prepare_options(
            refused(missing.to_str().unwrap()),
            Instant::now() + Duration::from_secs(5)
        )
        .await
        .unwrap_err()
        .code,
        "HTTP_STORAGE_STATE_FILE_IO"
    );
    let large = root.path().join("Acme large.json");
    std::fs::write(
        &large,
        vec![b' '; super::super::super::storage::MAX_BYTES + 1],
    )
    .unwrap();
    assert_eq!(
        prepare_options(
            refused(large.to_str().unwrap()),
            Instant::now() + Duration::from_secs(5)
        )
        .await
        .unwrap_err()
        .code,
        "STORAGE_BOUNDS"
    );
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let fifo = root.path().join("Acme state.fifo");
        let native = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
        let refusal = tokio::time::timeout(
            Duration::from_secs(1),
            prepare_options(
                refused(fifo.to_str().unwrap()),
                Instant::now() + Duration::from_secs(5),
            ),
        )
        .await
        .expect("Special files must refuse without waiting for a writer")
        .unwrap_err();
        assert_eq!(refusal.code, "HTTP_STORAGE_STATE_FILE_NOT_REGULAR");
    }
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one export scenario checked end to end"
)]
async fn http_storage_state_caller_export_creates_parents_and_replaces_an_owner_only_file() {
    let mut contexts = Contexts::default();
    let state = initial(vec![cookie(
        "Acme",
        "example.com",
        "/",
        "Acme-private-export",
    )]);
    let context = create_state(&mut contexts, state.clone());
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("evidence");
    crate::artifacts::directory(&directory).unwrap();
    let target = root.path().join("nested/state/Acme.json");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, b"Acme stale state").unwrap();
    let options = StorageStateOptions {
        indexed_db: true,
        opfs: true,
        path: Some(target.to_str().unwrap().into()),
    };
    options.validate().unwrap();
    let path = options.path.clone();
    let receipt = contexts
        .storage_state(context, options, &directory)
        .unwrap();
    let artifact = contexts.published_storage_state(&receipt).unwrap();
    let copied = storage_state_copy(
        artifact,
        receipt,
        path,
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert_eq!(copied["file"]["path"], target.to_str().unwrap());
    assert!(!copied.to_string().contains("Acme-private"));
    let written: Value = serde_json::from_slice(&std::fs::read(&target).unwrap()).unwrap();
    assert_eq!(written, state);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let created = root.path().join("missing/parents/Acme.json");
    let receipt = contexts
        .storage_state(context, StorageStateOptions::default(), &directory)
        .unwrap();
    let artifact = contexts.published_storage_state(&receipt).unwrap();
    storage_state_copy(
        artifact,
        receipt,
        Some(created.to_str().unwrap().into()),
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(created.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    let blocked = root.path().join("blocked");
    std::fs::write(&blocked, b"Acme file").unwrap();
    let receipt = contexts
        .storage_state(context, StorageStateOptions::default(), &directory)
        .unwrap();
    let evidence = receipt["reports"][0]["evidenceId"].clone();
    let artifact = contexts.published_storage_state(&receipt).unwrap();
    let error = storage_state_copy(
        artifact,
        receipt,
        Some(blocked.join("Acme.json").to_str().unwrap().into()),
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "HTTP_STORAGE_STATE_EXPORT_IO");
    let partial = error.partial.unwrap();
    assert_eq!(partial["evidenceId"], evidence);
    assert_eq!(partial["evidencePublished"], true);
    assert!(
        contexts
            .evidence_get(serde_json::from_value(evidence).unwrap())
            .is_ok()
    );
    assert_eq!(
        StorageStateOptions {
            path: Some("relative.json".into()),
            ..Default::default()
        }
        .validate()
        .unwrap_err()
        .code,
        "HTTP_STORAGE_STATE_PATH_INVALID"
    );
}

#[tokio::test]
async fn http_storage_state_reader_capacity_and_deadline_refuse_without_context_allocation() {
    let _readers = hold_readers().await;
    let options = Options {
        storage_state: Some(StorageStateInput::File(
            "/private/tmp/Acme-unread-state.json".into(),
        )),
        ..Default::default()
    };
    assert_eq!(
        prepare_options(options, Instant::now() + Duration::from_millis(10))
            .await
            .unwrap_err()
            .code,
        "HTTP_DEADLINE"
    );
}

#[test]
fn http_storage_state_invalid_shapes_injection_and_capacity_refuse_before_allocation() {
    let mut contexts = Contexts::default();
    let mut injection = cookie("Acme", "example.com", "/", "private");
    injection["value"] = json!("private; other=unsafe");
    let mut invalid_db = initial(vec![]);
    invalid_db["origins"][0]["indexedDB"][0]["version"] = json!("not-an-integer");
    for state in [
        json!(true),
        json!({"cookies":[{}]}),
        json!({"cookies":[injection]}),
        invalid_db,
        json!({"cookies":(0..257).map(|index|cookie(&format!("Acme-{index}"),"example.com","/","private")).collect::<Vec<_>>()}),
    ] {
        assert!(contexts.create(Uuid::new_v4(), options(state)).is_err());
        assert_eq!(contexts.list()["contexts"], json!([]));
    }
    let inline = options(initial(vec![cookie(
        "Acme",
        "example.com",
        "/",
        "Acme-private-cookie",
    )]));
    assert!(!format!("{inline:?}").contains("Acme-private"));
    let context = Reference {
        connection_id: Uuid::new_v4(),
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    };
    let associated = Options {
        browser_context: Some(context),
        ..inline
    };
    assert_eq!(
        associated.validate().unwrap_err().code,
        "HTTP_BROWSER_STORAGE_STATE_UNAVAILABLE"
    );
}

#[test]
fn http_storage_state_private_input_rejects_links_shared_files_and_nonregular_sources_without_waiting()
 {
    #[cfg(unix)]
    {
        use std::os::unix::{
            ffi::OsStrExt,
            fs::{PermissionsExt, symlink},
        };
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("private");
        crate::artifacts::directory(&directory).unwrap();
        let path = directory.join("state.json");
        write_private(&path, &json!({}));
        let link = directory.join("link.json");
        symlink(&path, &link).unwrap();
        assert_eq!(
            super::super::super::storage::read_private(&link)
                .unwrap_err()
                .code,
            "PRIVATE_STATE_REQUIRED"
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            super::super::super::storage::read_private(&path)
                .unwrap_err()
                .code,
            "PRIVATE_STATE_REQUIRED"
        );
        let fifo = directory.join("state.fifo");
        let native = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            let _ = send.send(super::super::super::storage::read_private(&fifo));
        });
        assert_eq!(
            receive
                .recv_timeout(Duration::from_secs(1))
                .expect("Nonregular sources must refuse without waiting for a writer")
                .unwrap_err()
                .code,
            "STORAGE_BOUNDS"
        );
        reader.join().unwrap();
    }
}

#[tokio::test]
async fn http_storage_state_initial_cookie_reaches_only_its_owned_isolated_wire_jar() {
    let site = Site::start(None).await;
    let mut contexts = Contexts::default();
    let connection = Uuid::new_v4();
    let mut configured = options(initial(vec![cookie(
        "Acme",
        "127.0.0.1",
        "/",
        "Acme-private-cookie",
    )]));
    configured.base_url = Some(site.url.clone());
    let first = serde_json::from_value(
        contexts.create(connection, configured).unwrap()["httpContext"].clone(),
    )
    .unwrap();
    let second = create(&mut contexts, connection, &site.url);
    let echoed: Value =
        serde_json::from_slice(&body(&mut contexts, first, fetch("/echo")).await).unwrap();
    assert_eq!(echoed["cookie"], "Acme=Acme-private-cookie");
    let isolated: Value =
        serde_json::from_slice(&body(&mut contexts, second, fetch("/echo")).await).unwrap();
    assert_eq!(isolated["cookie"], "");
    contexts.close().unwrap();
    site.close().await;
}
