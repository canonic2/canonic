use super::*;
fn entry(url: &str, method: &str, status: u16, body: &str) -> Value {
    json!({"request":{"url":url,"method":method,"headers":[]},"response":{"status":status,"headers":[],"content":{"text":body}}})
}
fn archive(entries: Vec<Value>) -> Archive {
    Archive::parse(&json!({"log":{"version":"1.2","entries":entries}}), |_| {
        Err("HAR_ATTACHMENT_UNAVAILABLE".into())
    })
    .unwrap()
}
fn request(url: &str, method: &str) -> Request {
    Request {
        url: url.into(),
        method: method.into(),
        headers: Vec::new(),
        body: None,
    }
}

fn create_export_directory(path: &std::path::Path) {
    #[cfg(windows)]
    canonic_process_kit::endpoint::prepare_directory(path).unwrap();
    #[cfg(not(windows))]
    crate::artifacts::directory(path).unwrap();
}

#[test]
fn caller_exports_validate_private_new_paths_and_reimport() {
    let root = tempfile::tempdir().unwrap();
    let private = root.path().join("private");
    create_export_directory(&private);
    let path = |name: &str| private.join(name).to_string_lossy().into_owned();
    #[cfg(unix)]
    {
        let public = root.path().join("open");
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir(&public).unwrap();
        std::fs::set_permissions(&public, std::fs::Permissions::from_mode(0o755)).unwrap();
        let file = public.join("open.har");
        assert!(Export::new(&file.to_string_lossy(), None, CaptureContent::Embed).is_err());
    }
    for (candidate, resources, content) in [
        ("relative.har".to_owned(), None, CaptureContent::Embed),
        (path("capture.txt"), None, CaptureContent::Embed),
        (
            path("bundle.zip"),
            Some(private.to_string_lossy().into_owned()),
            CaptureContent::Attach,
        ),
        (
            path("embedded.har"),
            Some(private.to_string_lossy().into_owned()),
            CaptureContent::Embed,
        ),
    ] {
        assert!(
            Export::new(&candidate, resources.as_deref(), content).is_err(),
            "{candidate}"
        );
    }
    let json_type = json!([{"name":"Content-Type","value":"application/json"}]);
    let source = archive(vec![
        json!({"request":{"url":"https://example.com/one","method":"GET","headers":[]},"response":{"status":200,"headers":json_type,"content":{"text":"{\"acme\":1}"}}}),
    ]);
    let zip = Export::new(&path("bundle.zip"), None, CaptureContent::Attach).unwrap();
    assert_eq!(
        zip.write(&source, CaptureContent::Attach).unwrap()["format"],
        "zip"
    );
    let attached = Export::new(&path("attached.har"), None, CaptureContent::Attach).unwrap();
    assert_eq!(
        attached.write(&source, CaptureContent::Attach).unwrap()["attachments"],
        1
    );
    let embedded = Export::new(&path("embedded.har"), None, CaptureContent::Embed).unwrap();
    embedded.write(&source, CaptureContent::Embed).unwrap();
    for name in ["bundle.zip", "attached.har", "embedded.har"] {
        let reopened = Archive::open(&private.join(name)).unwrap();
        assert_eq!(
            reopened
                .find(&request("https://example.com/one", "GET"))
                .unwrap()
                .unwrap()
                .response
                .body
                .as_deref(),
            Some(&b"{\"acme\":1}"[..]),
            "{name}"
        );
    }
    assert!(Export::new(&path("embedded.har"), None, CaptureContent::Embed).is_err());
    let resources = private.join("bodies");
    create_export_directory(&resources);
    let separate = Export::new(
        &path("separate.har"),
        Some(&resources.to_string_lossy()),
        CaptureContent::Attach,
    )
    .unwrap();
    separate.write(&source, CaptureContent::Attach).unwrap();
    assert_eq!(std::fs::read_dir(&resources).unwrap().count(), 1);
    #[cfg(windows)]
    for directory in [&private, &resources] {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                let file = std::fs::File::open(entry.path()).unwrap();
                canonic_process_kit::endpoint::check_private_child_file(&file).unwrap();
            }
        }
    }
}

#[cfg(windows)]
#[test]
fn caller_exports_refuse_explicitly_shared_windows_destinations() {
    fn share(path: &std::path::Path) {
        let output = std::process::Command::new("icacls")
            .arg(path)
            .args(["/grant", "*S-1-1-0:(OI)(CI)RX"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    let root = tempfile::tempdir().unwrap();
    let private = root.path().join("private");
    let shared = root.path().join("shared");
    create_export_directory(&private);
    create_export_directory(&shared);
    share(&shared);
    let export = private.join("capture.har");
    for result in [
        Export::new(
            &shared.join("capture.zip").to_string_lossy(),
            None,
            CaptureContent::Attach,
        ),
        Export::new(
            &export.to_string_lossy(),
            Some(&shared.to_string_lossy()),
            CaptureContent::Attach,
        ),
    ] {
        assert_eq!(result.unwrap_err().code, "HAR_PRIVATE_FILE_REQUIRED");
    }
    let exporter = Export::new(&export.to_string_lossy(), None, CaptureContent::Embed).unwrap();
    share(&private);
    let source = archive(vec![entry("https://example.com/acme", "GET", 200, "body")]);
    let error = exporter.write(&source, CaptureContent::Embed).unwrap_err();
    assert_eq!(error.code, "HAR_PRIVATE_FILE_REQUIRED");
    assert!(std::fs::read_dir(&private).unwrap().next().is_none());
}
#[test]
fn websocket_entries_round_trip_in_place_and_are_never_replayed() {
    let socket = json!({"_resourceType":"websocket","request":{"url":"wss://example.com/acme","method":"GET","headers":[]},"response":{"status":101,"headers":[],"content":{"size":-1}},"_webSocketMessages":[{"type":"send","time":1.0,"opcode":1,"data":"Acme"}]});
    let source = archive(vec![
        entry("https://example.com/one", "GET", 200, "one"),
        socket.clone(),
        entry("https://example.com/two", "GET", 200, "two"),
    ]);
    assert_eq!(source.entries.len(), 2);
    assert!(
        source
            .find(&request("wss://example.com/acme", "GET"))
            .unwrap()
            .is_none()
    );
    let encoded: Value = serde_json::from_slice(&source.encode().unwrap()).unwrap();
    let entries = encoded["log"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries[1]["_webSocketMessages"],
        socket["_webSocketMessages"]
    );
    assert_eq!(entries[2]["request"]["url"], "https://example.com/two");
    // Pinned attach: messages move to a `.jsonl` member named by `content._file`.
    let attached = source.encode_attached().unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(attached.clone())).unwrap();
    let mut har = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("har.har").unwrap(), &mut har).unwrap();
    let value: Value = serde_json::from_str(&har).unwrap();
    let entry = &value["log"]["entries"][1];
    assert!(entry.get("_webSocketMessages").is_none(), "{entry}");
    let name = entry["response"]["content"]["_file"].as_str().unwrap();
    assert!(name.ends_with(".jsonl"), "{name}");
    let mut lines = String::new();
    std::io::Read::read_to_string(&mut zip.by_name(name).unwrap(), &mut lines).unwrap();
    assert_eq!(
        lines,
        "{\"data\":\"Acme\",\"opcode\":1,\"time\":1.0,\"type\":\"send\"}\n"
    );
    // Import restores the messages, so a registered archive exports either way.
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("private");
    crate::artifacts::directory(&directory).unwrap();
    let path = directory.join("sockets.zip");
    crate::artifacts::write(&path, &attached).unwrap();
    let imported = Archive::open(&path).unwrap();
    let encoded: Value = serde_json::from_slice(&imported.encode().unwrap()).unwrap();
    let entry = &encoded["log"]["entries"][1];
    assert_eq!(entry["_webSocketMessages"], socket["_webSocketMessages"]);
    assert!(entry["response"]["content"].get("_file").is_none());
    // A socket without messages gets no member.
    let quiet = json!({"_resourceType":"websocket","request":{"url":"wss://example.com/quiet","method":"GET","headers":[]},"response":{"status":101,"headers":[],"content":{"size":-1}},"_webSocketMessages":[]});
    let bytes = archive(vec![quiet]).encode_attached().unwrap();
    assert_eq!(
        zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn archive_miss_policies_keep_their_public_names() {
    for (name, policy) in [
        ("abort", NotFound::Abort),
        ("continue", NotFound::Continue),
        ("fallback", NotFound::Fallback),
    ] {
        assert_eq!(
            serde_json::from_value::<NotFound>(json!(name)).unwrap(),
            policy
        );
        assert_eq!(serde_json::to_value(policy).unwrap(), name);
    }
    assert_eq!(NotFound::default(), NotFound::Abort);
}
#[test]
fn attached_encoding_round_trips_through_import_with_deduplicated_members() {
    let json_type = json!([{"name":"Content-Type","value":"application/json; charset=utf-8"}]);
    let source = archive(vec![
        json!({"request":{"url":"https://example.com/one","method":"GET","headers":[]},"response":{"status":200,"headers":json_type,"content":{"text":"{\"acme\":1}"}}}),
        json!({"request":{"url":"https://example.com/two","method":"GET","headers":[]},"response":{"status":200,"headers":json_type,"content":{"text":"{\"acme\":1}"}}}),
        json!({"request":{"url":"https://example.com/form","method":"POST","headers":[],"postData":{"text":"AP8="}},"response":{"status":204,"headers":[],"content":{"text":""}}}),
        json!({"request":{"url":"https://example.com/missing","method":"GET","headers":[]},"response":{"status":200,"headers":[],"content":{"size":-1,"_testrunBodyUnavailable":true}}}),
    ]);
    let bytes = source.encode_attached().unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
    let names = (0..zip.len())
        .map(|index| zip.by_index(index).unwrap().name().unwrap().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 3, "{names:?}");
    assert_eq!(names[0], "har.har");
    assert!(names.iter().any(|name| name.ends_with(".json")));
    assert!(names.iter().any(|name| name.ends_with(".dat")));
    let mut har = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("har.har").unwrap(), &mut har).unwrap();
    let value: Value = serde_json::from_str(&har).unwrap();
    let entries = value["log"]["entries"].as_array().unwrap();
    assert_eq!(
        entries[0]["response"]["content"]["_file"],
        entries[1]["response"]["content"]["_file"]
    );
    assert!(entries[0]["response"]["content"].get("text").is_none());
    assert_eq!(entries[2]["response"]["content"]["text"], "");
    assert_eq!(
        entries[3]["response"]["content"]["_testrunBodyUnavailable"],
        true
    );
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("private");
    crate::artifacts::directory(&directory).unwrap();
    let path = directory.join("capture.zip");
    crate::artifacts::write(&path, &bytes).unwrap();
    assert!(entries[0]["request"].get("postData").is_none());
    let imported = Archive::open(&path).unwrap();
    for url in ["https://example.com/one", "https://example.com/two"] {
        assert_eq!(
            imported
                .find(&request(url, "GET"))
                .unwrap()
                .unwrap()
                .response
                .body
                .as_deref(),
            Some(&b"{\"acme\":1}"[..])
        );
    }
    assert_eq!(
        imported.entries[2].request.body.as_deref(),
        Some(&b"AP8="[..])
    );
    assert_eq!(imported.encode().unwrap(), source.encode().unwrap());
}
#[test]
fn invalid_capture_filter_does_not_reserve_a_capture_or_artifact() {
    let context = crate::browser::contract::Reference {
        connection_id: uuid::Uuid::new_v4(),
        id: uuid::Uuid::new_v4(),
        generation: uuid::Uuid::new_v4(),
    };
    let mut store = Store::default();
    assert!(
        store
            .capture_start(
                context,
                None,
                (CaptureContent::Embed, CaptureMode::Minimal, None),
                None,
                Some(crate::browser::routing::Matcher::Regex {
                    pattern: "(?=secret".into(),
                    flags: String::new(),
                })
            )
            .is_err()
    );
    assert!(!store.capturing(context));
    assert!(store.list()["captures"].as_array().unwrap().is_empty());
    store
        .capture_start(
            context,
            None,
            (CaptureContent::Embed, CaptureMode::Minimal, None),
            None,
            None,
        )
        .unwrap();
}
#[test]
fn capture_publication_failure_retains_collected_bytes_without_recollection() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("output");
    std::fs::write(&output, b"blocked").unwrap();
    let context = crate::browser::contract::Reference {
        connection_id: uuid::Uuid::new_v4(),
        id: uuid::Uuid::new_v4(),
        generation: uuid::Uuid::new_v4(),
    };
    let mut store = Store::default();
    let seed = root.path().join("seed");
    crate::artifacts::directory(&seed).unwrap();
    let source = seed.join("source.har");
    crate::artifacts::write(&source,&serde_json::to_vec(&json!({"log":{"version":"1.2","entries":[entry("https://example.com/acme","GET",200,"original")]}})).unwrap()).unwrap();
    let original = store.register(&source, &seed).unwrap();
    let original_id = serde_json::from_value(original["archiveId"].clone()).unwrap();
    let original_path = original["reports"][0]["attachment"]["path"]
        .as_str()
        .unwrap();
    let original_bytes = std::fs::read(original_path).unwrap();
    let capture = store
        .capture_start(
            context,
            None,
            (CaptureContent::Embed, CaptureMode::Minimal, None),
            Some(original_id),
            None,
        )
        .unwrap();
    assert_eq!(
        store.remove(original_id).unwrap_err().code,
        "HAR_ARCHIVE_IN_USE"
    );
    let id = uuid::Uuid::new_v4();
    store.capture_request(
        context,
        (id, RequestOwner::Unattributed),
        json!({"url":"https://example.com/acme","method":"GET","headers":[]}),
        false,
        None,
    );
    store.capture_response(context, id, json!({"status":200,"headers":[]}));
    store.capture_finish(context, id, false);
    assert_eq!(store.capture_stop(context).unwrap(), vec![id]);
    store.capture_body(context, id, Some(&[0, 255, 128]), "");
    let failed = store.capture_publish(&output, context).unwrap_err();
    let archive_id = serde_json::from_value(failed.partial.unwrap()["archiveId"].clone()).unwrap();
    assert_eq!(capture["captureId"], json!(archive_id));
    assert!(store.list()["captures"].as_array().unwrap().is_empty());
    assert!(
        store.list()["archives"]
            .as_array()
            .unwrap()
            .iter()
            .any(|archive| archive["archiveId"] == json!(archive_id)
                && archive["publicationPending"] == true)
    );
    assert_eq!(
        store.capture_stop(context).unwrap_err().code,
        "HAR_CAPTURE_INACTIVE"
    );
    std::fs::remove_file(&output).unwrap();
    crate::artifacts::directory(&output).unwrap();
    let published = store.finalize(&output, archive_id).unwrap();
    assert_eq!(published["replacesArchiveId"], json!(original_id));
    assert_eq!(std::fs::read(original_path).unwrap(), original_bytes);
    assert_eq!(
        store
            .get(original_id)
            .unwrap()
            .find(&request("https://example.com/acme", "GET"))
            .unwrap()
            .unwrap()
            .response
            .body
            .as_deref(),
        Some(b"original".as_slice())
    );
    assert_eq!(store.finalize(&output, archive_id).unwrap(), published);
    assert_eq!(
        store
            .get(archive_id)
            .unwrap()
            .find(&request("https://example.com/acme", "GET"))
            .unwrap()
            .unwrap()
            .response
            .body
            .as_deref(),
        Some(&[0, 255, 128][..])
    );
    store.remove(archive_id).unwrap();
    store.remove(original_id).unwrap();
    assert!(source.exists());
}

#[test]
fn matching_keeps_exact_url_post_bytes_and_stable_header_ties() {
    let url = "https://example.com/acme?account=private";
    let mut first = entry(url, "POST", 200, "first");
    first["request"]["postData"] = json!({"text":"one"});
    let mut second = first.clone();
    second["response"]["content"]["text"] = json!("second");
    second["request"]["headers"] = json!([{"name":"X-Acme","value":"selected"}]);
    let source = archive(vec![first, second]);
    let mut query = request(url, "POST");
    query.body = Some(b"one".to_vec());
    assert_eq!(
        source
            .find(&query)
            .unwrap()
            .unwrap()
            .response
            .body
            .as_deref(),
        Some(b"first".as_slice())
    );
    query.headers.push(Header {
        name: "x-acme".into(),
        value: "selected".into(),
    });
    assert_eq!(
        source
            .find(&query)
            .unwrap()
            .unwrap()
            .response
            .body
            .as_deref(),
        Some(b"second".as_slice())
    );
    query.body = Some(b"two".to_vec());
    assert!(source.find(&query).unwrap().is_none());
    query.url = "https://example.com/acme".into();
    assert!(source.find(&query).unwrap().is_none());
}
#[test]
fn redirects_change_post_to_get_preserve_307_and_detect_cycles() {
    for (status, next_method) in [
        (301, "GET"),
        (302, "GET"),
        (303, "GET"),
        (307, "POST"),
        (308, "POST"),
    ] {
        let mut initial = entry("https://example.com/start", "POST", status, "");
        initial["response"]["headers"] = json!([{"name":"Location","value":"/end"}]);
        let source = archive(vec![
            initial,
            entry("https://example.com/end", next_method, 200, "complete"),
        ]);
        let found = source
            .find(&request("https://example.com/start", "POST"))
            .unwrap()
            .unwrap();
        assert_eq!(found.url, "https://example.com/end");
        assert_eq!(found.redirects, 1);
    }
    let mut cycle = entry("https://example.com/cycle", "GET", 302, "");
    cycle["response"]["headers"] = json!([{"name":"location","value":"/cycle"}]);
    assert_eq!(
        archive(vec![cycle])
            .find(&request("https://example.com/cycle", "GET"))
            .err()
            .unwrap()
            .code,
        "HAR_REDIRECT_CYCLE"
    );
}
#[test]
fn multipart_boundaries_are_normalized_without_ignoring_body_differences() {
    let mut source = entry("https://example.com/upload", "POST", 200, "matched");
    source["request"]["headers"] =
        json!([{"name":"content-type","value":"multipart/form-data; boundary=old"}]);
    source["request"]["postData"] = json!({"text":"--old\r\nAcme\r\n--old--"});
    let source = archive(vec![source]);
    let mut query = request("https://example.com/upload", "POST");
    query.headers.push(Header {
        name: "Content-Type".into(),
        value: "multipart/form-data; boundary=new".into(),
    });
    query.body = Some(b"--new\r\nAcme\r\n--new--".to_vec());
    assert!(source.find(&query).unwrap().is_some());
    query.body = Some(b"--new\r\nChanged\r\n--new--".to_vec());
    assert!(source.find(&query).unwrap().is_none());
}
#[test]
fn missing_payload_is_unavailable_and_binary_content_is_bounded() {
    let mut missing = entry("https://example.com/missing", "GET", 200, "");
    missing["response"]["content"] = json!({"size":12});
    assert_eq!(
        archive(vec![missing])
            .find(&request("https://example.com/missing", "GET"))
            .err()
            .unwrap()
            .code,
        "HAR_BODY_UNAVAILABLE"
    );
    let mut binary = entry("https://example.com/binary", "GET", 200, "");
    binary["response"]["content"] = json!({"text":"AP+A","encoding":"base64"});
    assert_eq!(
        archive(vec![binary.clone()])
            .find(&request("https://example.com/binary", "GET"))
            .unwrap()
            .unwrap()
            .response
            .body
            .as_deref(),
        Some([0, 255, 128].as_slice())
    );
    binary["response"]["content"] = json!({"text":"x".repeat(MAX_BODY+1)});
    assert_eq!(
        Archive::parse(
            &json!({"log":{"version":"1.2","entries":[binary]}}),
            |_| unreachable!()
        )
        .err()
        .unwrap()
        .code,
        "HAR_BODY_BOUNDS"
    );
}

#[test]
fn private_attached_files_are_copied_and_traversal_and_links_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let private = root.path().join("private");
    crate::artifacts::directory(&private).unwrap();
    let body = private.join("body.bin");
    crate::artifacts::write(&body, &[0, 255, 128]).unwrap();
    let mut source = entry("https://example.com/acme", "GET", 200, "");
    source["response"]["content"] = json!({"_file":"body.bin"});
    let path = private.join("source.har");
    let save = |source: &Value| {
        if path.exists() {
            std::fs::remove_file(&path).unwrap();
        }
        crate::artifacts::write(
            &path,
            &serde_json::to_vec(&json!({"log":{"version":"1.2","entries":[source]}})).unwrap(),
        )
        .unwrap()
    };
    save(&source);
    let archive = Archive::open(&path).unwrap();
    std::fs::remove_file(&body).unwrap();
    assert_eq!(
        archive
            .find(&request("https://example.com/acme", "GET"))
            .unwrap()
            .unwrap()
            .response
            .body
            .as_deref(),
        Some([0, 255, 128].as_slice())
    );
    source["response"]["content"]["_file"] = json!("../body.bin");
    save(&source);
    assert_eq!(
        Archive::open(&path).err().unwrap().code,
        "HAR_INVALID_ATTACHMENT"
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&path, &body).unwrap();
        source["response"]["content"]["_file"] = json!("body.bin");
        save(&source);
        assert_eq!(
            Archive::open(&path).err().unwrap().code,
            "HAR_INVALID_ATTACHMENT"
        );
    }
}

#[test]
fn zip_attachments_are_read_without_extraction() {
    use std::io::{Cursor, Write};
    let root = tempfile::tempdir().unwrap();
    let private = root.path().join("private");
    crate::artifacts::directory(&private).unwrap();
    let path = private.join("source.zip");
    let mut source = entry("https://example.com/acme", "GET", 200, "");
    source["response"]["content"] = json!({"_file":"resources/body.bin"});
    let value = serde_json::to_vec(&json!({"log":{"version":"1.2","entries":[source]}})).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("har.har", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&value).unwrap();
    zip.start_file(
        "resources/body.bin",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(&[0, 255, 128]).unwrap();
    crate::artifacts::write(&path, &zip.finish().unwrap().into_inner()).unwrap();
    assert_eq!(
        Archive::open(&path)
            .unwrap()
            .find(&request("https://example.com/acme", "GET"))
            .unwrap()
            .unwrap()
            .response
            .body
            .as_deref(),
        Some([0, 255, 128].as_slice())
    );
    assert!(!private.join("resources").exists());
}

#[test]
fn normalized_archives_preserve_metadata_and_do_not_turn_missing_post_bytes_into_a_match() {
    let mut source = entry("https://example.com/acme", "POST", 200, "result");
    source["request"]["headers"] =
        json!([{"name":"x-acme","value":"value","comment":"HAR metadata"}]);
    source["request"]["postData"] = json!({"size":12});
    source["startedDateTime"] = json!("2026-10-30T00:00:00Z");
    let value = json!({"log":{"version":"1.2","creator":{"name":"Acme","version":"1"},"pages":[{"id":"Acme"}],"entries":[source]}});
    let source = Archive::parse(&value, |_| unreachable!()).unwrap();
    let mut query = request("https://example.com/acme", "POST");
    query.body = Some(b"actual".to_vec());
    assert_eq!(
        source.find(&query).err().unwrap().code,
        "HAR_BODY_UNAVAILABLE"
    );
    let normalized: Value = serde_json::from_slice(&source.encode().unwrap()).unwrap();
    assert_eq!(normalized["log"]["creator"], value["log"]["creator"]);
    assert_eq!(normalized["log"]["pages"], value["log"]["pages"]);
    assert_eq!(
        normalized["log"]["entries"][0]["startedDateTime"],
        "2026-10-30T00:00:00Z"
    );
    let copy = Archive::parse(&normalized, |_| unreachable!()).unwrap();
    assert_eq!(
        copy.find(&query).err().unwrap().code,
        "HAR_BODY_UNAVAILABLE"
    );
}

#[test]
fn archive_publication_retries_retained_bytes_and_removal_refuses_changed_evidence() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input");
    crate::artifacts::directory(&input).unwrap();
    let path = input.join("source.har");
    let value = json!({"log":{"version":"1.2","entries":[entry("https://example.com/acme","GET",200,"original")]}});
    crate::artifacts::write(&path, &serde_json::to_vec(&value).unwrap()).unwrap();
    let output = root.path().join("output");
    std::fs::write(&output, b"blocked").unwrap();
    let mut store = Store::default();
    let failed = store.register(&path, &output).unwrap_err();
    let id = serde_json::from_value(failed.partial.unwrap()["archiveId"].clone()).unwrap();
    assert!(
        store.list()["archives"][0]["publicationPending"]
            .as_bool()
            .unwrap()
    );
    std::fs::write(&path, b"changed source").unwrap();
    std::fs::remove_file(&output).unwrap();
    crate::artifacts::directory(&output).unwrap();
    let result = store.finalize(&output, id).unwrap();
    assert_eq!(store.finalize(&output, id).unwrap(), result);
    assert_eq!(
        store
            .get(id)
            .unwrap()
            .find(&request("https://example.com/acme", "GET"))
            .unwrap()
            .unwrap()
            .response
            .body
            .as_deref(),
        Some(b"original".as_slice())
    );
    let evidence =
        std::path::Path::new(result["reports"][0]["attachment"]["path"].as_str().unwrap());
    let bytes = std::fs::read(evidence).unwrap();
    std::fs::write(evidence, b"modified evidence").unwrap();
    assert_eq!(store.remove(id).unwrap_err().code, "EVIDENCE_CHANGED");
    std::fs::write(evidence, &bytes).unwrap();
    store.remove(id).unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "changed source");
}

#[test]
fn representation_metadata_does_not_require_a_body_for_head_or_no_body_statuses() {
    for (method, status) in [("HEAD", 200), ("GET", 204), ("GET", 205), ("GET", 304)] {
        let mut source = entry("https://example.com/metadata", method, status, "");
        source["response"]["content"] = json!({"size":123});
        let archive = archive(vec![source]);
        let found = archive
            .find(&request("https://example.com/metadata", method))
            .unwrap()
            .unwrap();
        assert_eq!(found.response.body.as_deref(), Some([].as_slice()));
    }
}

#[test]
fn decoded_replay_reframes_transfer_headers_and_preserves_head_metadata() {
    let source = |method: &str| {
        let mut value = entry("https://example.com/compressed", method, 200, "decoded");
        value["response"]["headers"] = json!([{"name":"Content-Encoding","value":"gzip"},{"name":"Content-Length","value":"123"},{"name":"X-Acme","value":"kept"}]);
        value
    };
    let mut store = Store::default();
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input");
    crate::artifacts::directory(&input).unwrap();
    let path = input.join("source.har");
    crate::artifacts::write(
        &path,
        &serde_json::to_vec(
            &json!({"log":{"version":"1.2","entries":[source("GET"),source("HEAD")]}}),
        )
        .unwrap(),
    )
    .unwrap();
    let output = root.path().join("output");
    crate::artifacts::directory(&output).unwrap();
    let registered = store.register(&path, &output).unwrap();
    let id = serde_json::from_value(registered["archiveId"].clone()).unwrap();
    let reply = store
        .reply(id, &request("https://example.com/compressed", "GET"))
        .unwrap()
        .unwrap();
    assert_eq!(reply.headers.len(), 1);
    assert_eq!(reply.headers[0].name, "X-Acme");
    assert_eq!(
        store
            .reply(id, &request("https://example.com/compressed", "HEAD"))
            .unwrap()
            .unwrap()
            .headers
            .len(),
        3
    );
}
