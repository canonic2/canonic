//! Builder mapping, explicit environment isolation and retained operation tests.
use super::*;
fn request(root: &Path, profile: Value) -> Request {
    Request {
        root: None,
        task: None,
        check: None,
        prepare: false,
        submission_id: None,
        client_capture: false,
        project: root.to_owned(),
        config: None,
        profile: None,
        inline: Some(profile),
        env: Env::new(),
        bindings: Env::new(),
        baseline: Env::new(),
        overrides: Overrides::default(),
        install: false,
        launch: false,
        replace: false,
        boot: false,
        target: None,
    }
}
fn custom() -> Value {
    json!({"builder":"custom","platform":"android","target_kind":"emulator","custom":{"command":["make","apk"]},"artifact":{"path":"dist/acme.apk"}})
}

#[tokio::test]
async fn cancellation_before_native_dispatch_does_not_start_the_command() {
    let project = tempfile::tempdir().unwrap();
    let (_stop, cancelled) = tokio::sync::watch::channel(true);
    let error = crate::process::build(
        &["/bin/sh".into(), "-c".into(), "touch dispatched".into()],
        project.path(),
        &Env::new(),
        tokio::time::Instant::now() + std::time::Duration::from_secs(10),
        cancelled,
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.code, "CANCELLED");
    assert!(!project.path().join("dispatched").exists());
}

#[tokio::test]
async fn timeout_retains_partial_logs_and_reaps_the_owned_child() {
    let state = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let mut p = custom();
    p["timeout_ms"] = json!(1000);
    p["custom"]["command"] = json!([
        "/bin/sh",
        "-c",
        "echo $$ > timed.pid; printf started; exec sleep 30"
    ]);
    let r = request(project.path(), p);
    let owners = Owners::default();
    let (_, ticket) = owners
        .begin(state.path(), "session", "timeout", &r)
        .unwrap();
    let ticket = ticket.unwrap();
    let result = owners.build(&ticket, &prepare(&r).unwrap()).await;
    assert_eq!(result.err().unwrap().code, "TIMEOUT");
    assert_eq!(
        std::fs::read_to_string(ticket.directory.join("build.log")).unwrap(),
        "started"
    );
    let pid: u32 = std::fs::read_to_string(project.path().join("timed.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(crate::process::start_time(pid).is_none());
    owners.finish(&ticket, Err(Error::new("TIMEOUT", "Timed out")));
}

#[tokio::test]
async fn oversized_logs_stop_the_build_and_keep_only_the_private_cap() {
    let state = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let mut p = custom();
    p["custom"]["command"] = json!([
        "/usr/bin/python3",
        "-c",
        "import sys; sys.stdout.buffer.write(b'x' * (8*1024*1024+1))"
    ]);
    let r = request(project.path(), p);
    let owners = Owners::default();
    let (_, ticket) = owners
        .begin(state.path(), "session", "log-cap", &r)
        .unwrap();
    let ticket = ticket.unwrap();
    let error = owners
        .build(&ticket, &prepare(&r).unwrap())
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "OUTPUT_LIMIT");
    assert_eq!(
        std::fs::metadata(ticket.directory.join("build.log"))
            .unwrap()
            .len(),
        8 * 1024 * 1024
    );
    owners.finish(&ticket, Err(error));
    assert!(
        !owners
            .status(state.path(), "session", &ticket.id, false)
            .unwrap()
            .to_string()
            .contains("privateOutput")
    );
}

#[test]
fn recovery_keys_history_and_offline_replay_do_not_read_yaml() {
    let state = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let owners = Owners::default();
    let mut r = request(project.path(), custom());
    r.submission_id = Some("acme-request".into());
    let (record, ticket) = owners
        .begin(state.path(), "session", "transport-1", &r)
        .unwrap();
    let ticket = ticket.unwrap();
    owners.finish(&ticket, Ok(json!({"built":true})));
    std::fs::write(project.path().join("testrun.yml"), "invalid: [").unwrap();
    assert_eq!(
        owners
            .replay(state.path(), "session", "different-transport", &r)
            .unwrap()
            .unwrap()["state"],
        "completed"
    );
    assert_eq!(
        Owners::default()
            .replay(state.path(), "session", "new-owner", &r)
            .unwrap()
            .unwrap()["operationId"],
        record["operationId"]
    );
    assert_eq!(
        owners
            .query(
                state.path(),
                "session",
                &json!({"submissionId":"acme-request"}),
                false
            )
            .unwrap()["state"],
        "completed"
    );
    assert_eq!(
        owners
            .query(state.path(), "session", &json!({}), false)
            .unwrap()["operations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    r.env.insert("CHANGED".into(), "value".into());
    assert_eq!(
        owners
            .replay(state.path(), "session", "same-key", &r)
            .err()
            .unwrap()
            .code,
        "REQUEST_CONFLICT"
    );
    assert!(
        owners
            .query(state.path(), "session", &json!({}), true)
            .is_err()
    );
}

#[test]
fn build_history_decodes_only_its_page_and_skips_nonrecord_filename_windows() {
    let state = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let owners = Owners::default();
    let mut records = Vec::new();
    for i in 0..3 {
        let request = request(project.path(), custom());
        let (record, ticket) = owners
            .begin(state.path(), "session", &format!("request-{i}"), &request)
            .unwrap();
        let ticket = ticket.unwrap();
        owners.finish(&ticket, Ok(json!({"built":true})));
        records.push((
            record["operationId"].as_str().unwrap().to_owned(),
            ticket.directory,
        ));
    }
    records.sort_by(|a, b| a.0.cmp(&b.0));
    let directory = records[0].1.parent().unwrap();
    for i in 0..102 {
        std::fs::write(directory.join(format!("a-{i:03}")), b"ignored").unwrap();
    }
    let last = records.last().unwrap().1.join("operation.json");
    let bytes = std::fs::read(&last).unwrap();
    std::fs::write(&last, b"invalid JSON").unwrap();
    let offline = Owners::default();
    let first = offline
        .query(state.path(), "session", &json!({"limit":1}), false)
        .unwrap();
    assert_eq!(first["operations"][0]["operationId"], records[0].0);
    std::fs::write(&last, bytes).unwrap();
    let rest = offline
        .query(
            state.path(),
            "session",
            &json!({"cursor":first["nextCursor"]}),
            false,
        )
        .unwrap();
    assert_eq!(rest["operations"].as_array().unwrap().len(), 2);
    assert!(rest["nextCursor"].is_null());
    assert_eq!(
        offline
            .query(
                state.path(),
                "other-session",
                &json!({"cursor":first["nextCursor"]}),
                false
            )
            .unwrap_err()
            .code,
        "invalid_cursor"
    );
}

#[test]
fn artifact_copy_observes_cancellation_between_chunks() {
    struct Input {
        stop: tokio::sync::watch::Sender<bool>,
        read: bool,
    }
    impl std::io::Read for Input {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            assert!(!self.read, "Cancelled copy read another chunk");
            self.read = true;
            buffer[..4].copy_from_slice(b"acme");
            self.stop.send_replace(true);
            Ok(4)
        }
    }
    let (stop, receiver) = tokio::sync::watch::channel(false);
    let mut input = Input { stop, read: false };
    let mut output = Vec::new();
    let error = artifacts::copy_checked(
        &mut input,
        &mut output,
        &crate::process::Cancellation::new(receiver),
    )
    .unwrap_err();
    assert_eq!(error.code, "CANCELLED");
    assert_eq!(output, b"acme");
}

#[test]
fn provenance_includes_defaults_overrides_and_separate_environment_sources() {
    let project = tempfile::tempdir().unwrap();
    let mut r = request(project.path(), custom());
    r.overrides.timeout_ms = Some(5000);
    r.env.insert("API_URL".into(), "private-value".into());
    let prepared = prepare(&r).unwrap();
    assert_eq!(prepared.source["effectiveProvenance"]["/cwd"], "default");
    assert_eq!(prepared.source["effectiveProvenance"]["/builder"], "inline");
    assert_eq!(
        prepared.source["effectiveProvenance"]["/timeout_ms"],
        "invocation.overrides"
    );
    assert_eq!(
        prepared.source["environmentProvenance"]["build"]["API_URL"],
        "invocation.env"
    );
    assert!(!prepared.source.to_string().contains("private-value"));
}
#[test]
fn environment_precedence_missing_bindings_and_launch_separation() {
    let root = tempfile::tempdir().unwrap();
    let mut p = custom();
    p["inherit_env"] = json!(["TOKEN"]);
    p["env"] = json!({"VALUE":"configured"});
    let mut r = request(root.path(), p);
    assert_eq!(prepare(&r).err().unwrap().code, "BUILD_ENV_MISSING");
    r.bindings.insert("TOKEN".into(), "private".into());
    r.bindings.insert("UNREQUESTED".into(), "absent".into());
    r.env.insert("VALUE".into(), "override".into());
    let p = prepare(&r).unwrap();
    assert_eq!(p.env["VALUE"], "override");
    assert_eq!(p.env["TOKEN"], "private");
    assert!(!p.env.contains_key("UNREQUESTED"));
    assert!(p.launch_env.is_empty());
}
#[test]
fn platform_and_unsupported_launch_refuse_before_build() {
    let root = tempfile::tempdir().unwrap();
    let mut p = custom();
    p["launch"] = json!({"env":{"SECRET":"value"}});
    let mut r = request(root.path(), p);
    r.install = true;
    r.launch = true;
    r.target = Some("android:emulator-5554".into());
    assert_eq!(prepare(&r).err().unwrap().code, "CAPABILITY_UNAVAILABLE");
    r.target = Some("simulator:Acme".into());
    assert_eq!(prepare(&r).err().unwrap().code, "PACKAGE_PLATFORM_MISMATCH");
}
#[test]
fn xcode_preserves_configuration_and_dependency_product_layout() {
    let project = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let profile = json!({"builder":"xcode","platform":"ios","target_kind":"simulator","xcode":{"project":"Acme.xcodeproj","scheme":"Acme","configuration":"Release"}});
    let prepared = prepare(&request(project.path(), profile)).unwrap();
    let plan = adapters::plan(&prepared, output.path()).unwrap();
    assert!(
        plan.commands[0]
            .iter()
            .any(|arg| arg.starts_with("SYMROOT="))
    );
    assert!(
        !plan.commands[0]
            .iter()
            .any(|arg| arg.starts_with("CONFIGURATION_BUILD_DIR="))
    );
    let products = output.path().join("products/Release-iphonesimulator");
    std::fs::create_dir_all(products.join("React-cxxreact/React-cxxreact_privacy.bundle")).unwrap();
    std::fs::create_dir_all(products.join("Acme.app")).unwrap();
    assert_eq!(
        artifacts::select(&plan.cwd, &plan.selector, &prepared.root, output.path()).unwrap(),
        products.join("Acme.app").canonicalize().unwrap()
    );
}

#[test]
fn builder_commands_keep_arguments_and_build_without_install() {
    let root = tempfile::tempdir().unwrap();
    for p in [
        json!({"builder":"xcode","platform":"ios","target_kind":"simulator","xcode":{"workspace":"Acme.xcworkspace","scheme":"Acme","configuration":"Debug"}}),
        json!({"builder":"gradle","platform":"android","target_kind":"emulator","gradle":{"tasks":["assembleStagingRelease"],"properties":{"endpoint":"value with spaces"}}}),
        json!({"builder":"react-native","platform":"android","target_kind":"emulator","react-native":{"variant":"stagingRelease"}}),
        json!({"builder":"flutter","platform":"android","target_kind":"device","flutter":{"mode":"release","flavor":"staging","dart_defines":{"API":"https://staging.example.com"}}}),
        custom(),
    ] {
        let prepared = prepare(&request(root.path(), p)).unwrap();
        let plan = adapters::plan(&prepared, &root.path().join("out")).unwrap();
        assert!(!plan.commands.is_empty());
        for command in &plan.commands {
            assert!(!command.contains(&"run-ios".into()));
            assert!(!command.contains(&"install".into()));
            assert!(!command.contains(&"run-android".into()));
        }
    }
}
#[test]
fn selection_refuses_ambiguity_and_escape() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("dist")).unwrap();
    for name in ["a.apk", "b.apk"] {
        std::fs::write(root.path().join("dist").join(name), b"apk").unwrap();
    }
    let selector = crate::configuration::Artifact {
        path: None,
        glob: Some("dist/*.apk".into()),
    };
    assert_eq!(
        artifacts::select(root.path(), &selector, root.path(), root.path())
            .err()
            .unwrap()
            .code,
        "BUILD_ARTIFACT_SELECTION"
    );
    let other = tempfile::tempdir().unwrap();
    let outside_glob = crate::configuration::Artifact {
        path: None,
        glob: Some(other.path().join("*.apk").to_string_lossy().into()),
    };
    assert_eq!(
        artifacts::select(root.path(), &outside_glob, root.path(), root.path())
            .unwrap_err()
            .code,
        "BUILD_ARTIFACT_PATH"
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(other.path(), root.path().join("linked")).unwrap();
        let linked_glob = crate::configuration::Artifact {
            path: None,
            glob: Some("linked/*.apk".into()),
        };
        assert_eq!(
            artifacts::select(root.path(), &linked_glob, root.path(), root.path())
                .unwrap_err()
                .code,
            "BUILD_ARTIFACT_PATH"
        );
    }
    std::fs::write(other.path().join("a.apk"), b"apk").unwrap();
    let selector = crate::configuration::Artifact {
        path: Some(other.path().join("a.apk").to_string_lossy().into()),
        glob: None,
    };
    assert_eq!(
        artifacts::select(root.path(), &selector, root.path(), root.path())
            .err()
            .unwrap()
            .code,
        "BUILD_ARTIFACT_PATH"
    );
}
#[test]
fn retained_submission_deduplicates_and_cancel_never_replays() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let owners = Owners::default();
    let r = request(project.path(), custom());
    let (first, ticket) = owners.begin(root.path(), "session", "request", &r).unwrap();
    let ticket = ticket.unwrap();
    let (second, repeated) = owners.begin(root.path(), "session", "request", &r).unwrap();
    assert_eq!(first, second);
    assert!(repeated.is_none());
    owners
        .status(root.path(), "session", &ticket.id, true)
        .unwrap();
    assert!(*ticket.cancelled.borrow());
    owners.finish(&ticket, Err(Error::new("CANCELLED", "Cancelled")));
    let record = owners
        .status(root.path(), "session", &ticket.id, false)
        .unwrap();
    assert_eq!(record["settled"], true);
    assert_eq!(record["state"], "cancelled");
    assert!(!record.to_string().contains("signature"));
    assert!(
        owners
            .status(root.path(), "other-session", &ticket.id, false)
            .is_err()
    );
}

#[tokio::test]
async fn custom_build_snapshots_ios_output_and_keeps_private_logs() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let bundle = project.path().join("template.app");
    std::fs::create_dir(&bundle).unwrap();
    let plist = json!({"CFBundleIdentifier":"com.example.acme","CFBundleVersion":"1","CFBundleShortVersionString":"1.0","CFBundleExecutable":"Acme","CFBundleSupportedPlatforms":["iPhoneSimulator"]});
    plist::to_file_xml(bundle.join("Info.plist"), &plist).unwrap();
    std::fs::write(bundle.join("Acme"), b"fixture executable").unwrap();
    let p = json!({"builder":"custom","platform":"ios","target_kind":"simulator","custom":{"command":["/bin/cp","-R","template.app","result.app"]},"artifact":{"path":"result.app"},"diagnostics":{"symbols":"external","profileable":true,"sanitizers":["address"],"coverage":false,"probes":["Acme-marker"],"timing":"instrumented"}});
    let r = request(project.path(), p);
    let owners = Owners::default();
    let (_, ticket) = owners
        .begin(root.path(), "session", "ios-build", &r)
        .unwrap();
    let ticket = ticket.unwrap();
    let artifact = owners.build(&ticket, &prepare(&r).unwrap()).await.unwrap();
    assert_eq!(artifact["identity"]["appId"], "com.example.acme");
    assert_eq!(
        artifact["diagnostics"]["declared"]["sanitizers"],
        json!(["address"])
    );
    assert_eq!(
        artifact["diagnostics"]["verification"],
        "profile_claims_not_binary_verified"
    );
    assert_eq!(artifact["diagnostics"]["automaticInstrumentation"], false);
    assert!(
        Path::new(artifact["path"].as_str().unwrap())
            .join("Acme")
            .exists()
    );
    std::fs::write(project.path().join("result.app/Acme"), b"changed source").unwrap();
    assert_eq!(
        std::fs::read(Path::new(artifact["path"].as_str().unwrap()).join("Acme")).unwrap(),
        b"fixture executable"
    );
    owners.finish(&ticket, Ok(json!({"artifact":artifact})));
    let record = owners
        .status(root.path(), "session", &ticket.id, false)
        .unwrap();
    assert_eq!(
        record["stages"]["preparation"]["effective"]["diagnostics"]["timing"],
        "instrumented"
    );
    assert!(!owners.active());
}

#[tokio::test]
async fn build_cancellation_reaps_owned_process_before_settling() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let p = json!({"builder":"custom","platform":"ios","target_kind":"simulator","custom":{"command":["/bin/sh","-c","echo $$ > parent.pid; sleep 30 & echo $! > child.pid; echo started > started; wait"]},"artifact":{"path":"never.app"}});
    let r = request(project.path(), p);
    let owners = Owners::default();
    let (_, ticket) = owners
        .begin(root.path(), "session", "cancel-build", &r)
        .unwrap();
    let ticket = ticket.unwrap();
    let id = ticket.id.clone();
    let prepared = prepare(&r).unwrap();
    let build = owners.build(&ticket, &prepared);
    tokio::pin!(build);
    let cancel = async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if project.path().join("started").exists() {
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        owners.status(root.path(), "session", &id, true).unwrap();
    };
    let (result, ()) = tokio::join!(build, cancel);
    assert_eq!(result.err().unwrap().code, "CANCELLED");
    for file in ["parent.pid", "child.pid"] {
        let pid: u32 = std::fs::read_to_string(project.path().join(file))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(
            crate::process::start_time(pid).is_none(),
            "owned build process {pid} remains alive"
        );
    }
    owners.finish(&ticket, Err(Error::new("CANCELLED", "Cancelled")));
    assert!(!owners.active());
}

#[test]
fn expo_uses_local_eas_output_and_validates_native_profile() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("eas.json"),r#"{"build":{"testing":{"android":{"buildType":"apk"}},"phone":{"distribution":"internal"}}}"#).unwrap();
    let p = json!({"builder":"expo","platform":"android","target_kind":"device","expo":{"profile":"testing"}});
    let prepared = prepare(&request(root.path(), p)).unwrap();
    let plan = adapters::plan(&prepared, &root.path().join("out")).unwrap();
    assert!(plan.commands[0].contains(&"--local".into()));
    assert!(plan.commands[0].contains(&"--non-interactive".into()));
    assert!(!plan.archive);
    let p = json!({"builder":"expo","platform":"ios","target_kind":"device","expo":{"profile":"phone"}});
    let prepared = prepare(&request(root.path(), p)).unwrap();
    let plan = adapters::plan(&prepared, &root.path().join("out")).unwrap();
    assert!(plan.archive);
    assert!(plan.selector.path.unwrap().ends_with(".ipa"));
}

#[test]
fn ipa_unpack_refuses_parent_traversal_without_writing_outside_output() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("bad.ipa");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    writer
        .start_file("../escape", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"escape").unwrap();
    writer.finish().unwrap();
    assert!(artifacts::unpack(&archive, root.path()).is_err());
    assert!(!root.path().join("escape").exists());
}

#[test]
fn typed_overrides_preserve_builder_and_validate_mode() {
    let root = tempfile::tempdir().unwrap();
    let p = json!({"builder":"flutter","platform":"android","target_kind":"device","flutter":{"mode":"debug"}});
    let mut r = request(root.path(), p);
    r.overrides.mode = Some("release".into());
    r.overrides.flavor = Some("staging".into());
    let prepared = prepare(&r).unwrap();
    assert_eq!(prepared.profile.flutter.unwrap().mode, "release");
    r.overrides.mode = Some("production".into());
    assert!(prepare(&r).is_err());
}

#[tokio::test]
async fn failed_build_keeps_private_diagnostics_without_artifact_or_device_work() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let mut p = custom();
    p["custom"]["command"] = json!(["/bin/sh", "-c", "printf private-build-token >&2; exit 7"]);
    let r = request(project.path(), p);
    let owners = Owners::default();
    let (_, ticket) = owners
        .begin(root.path(), "session", "failed-build", &r)
        .unwrap();
    let ticket = ticket.unwrap();
    let error = owners
        .build(&ticket, &prepare(&r).unwrap())
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "BUILD_FAILED");
    owners.finish(&ticket, Err(error));
    let status = owners
        .status(root.path(), "session", &ticket.id, false)
        .unwrap();
    assert!(!status.to_string().contains("private-build-token"));
    assert_eq!(
        std::fs::read_to_string(ticket.directory.join("build.log"))
            .unwrap()
            .trim(),
        "private-build-token"
    );
    assert_eq!(status["stages"]["artifact"]["state"], "not_started");
    assert_eq!(status["stages"]["build"]["state"], "failed");
}

fn located(root: &Path, file: &str, version: u32) {
    let build = json!({"builder":"custom","platform":"android","target_kind":"emulator","cwd":"android","custom":{"command":["make","apk"]},"artifact":{"path":"dist/acme.apk"}});
    let document = if version == 1 {
        json!({"version":1,"builds":{"acme":build}})
    } else {
        let mut build = build;
        build.as_object_mut().unwrap().remove("platform");
        build.as_object_mut().unwrap().remove("target_kind");
        build["cwd"] = json!(".");
        json!({"version":2,"projects":{"mobile":{"root":"android","kind":"custom","profiles":{"acme":{"platform":"android","target_kind":"emulator","build":build}}}}})
    };
    let path = root.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, document.to_string()).unwrap();
    std::fs::create_dir_all(root.join("android")).unwrap();
}
fn selected(root: &Path, version: u32, config: Option<PathBuf>) -> Request {
    let mut r = request(root, Value::Null);
    r.inline = None;
    r.profile = Some("acme".into());
    r.config = config;
    if version == 2 {
        r.root = Some(root.to_owned());
        r.project = "mobile".into();
    }
    r
}
#[test]
fn builds_use_the_selected_file_with_unchanged_root_paths_for_both_versions() {
    for version in [1, 2] {
        let root = tempfile::tempdir().unwrap();
        let canonical = root.path().canonicalize().unwrap();
        located(root.path(), ".config/testrun.yml", version);
        let prepared = prepare(&selected(root.path(), version, None)).unwrap();
        assert_eq!(prepared.cwd, canonical.join("android"));
        assert_eq!(
            prepared.source["location"]["file"],
            json!(canonical.join(".config/testrun.yml"))
        );
        let other = tempfile::tempdir().unwrap();
        located(other.path(), "qa/acme.yml", version);
        let explicit = other.path().join("qa/acme.yml");
        let prepared = prepare(&selected(root.path(), version, Some(explicit.clone()))).unwrap();
        // Paths stay relative to the configuration root, not the file's directory.
        assert_eq!(prepared.cwd, canonical.join("android"));
        assert_eq!(
            prepared.source["location"]["shadowed"],
            json!([canonical.join(".config/testrun.yml")])
        );
        let missing = selected(root.path(), version, Some(other.path().join("missing.yml")));
        assert_eq!(prepare(&missing).err().unwrap().code, "CONFIG_IO");
        let relative = selected(root.path(), version, Some("qa/acme.yml".into()));
        assert_eq!(prepare(&relative).err().unwrap().code, "INVALID_ARGUMENT");
    }
    let root = tempfile::tempdir().unwrap();
    let mut inline = request(root.path(), custom());
    inline.config = Some(root.path().join("testrun.yml"));
    assert!(prepare(&inline).is_err());
}
#[test]
fn mcp_configuration_requests_select_the_file_and_report_its_actual_path() {
    for version in [1, 2] {
        let root = tempfile::tempdir().unwrap();
        let canonical = root.path().canonicalize().unwrap();
        located(root.path(), ".config/testrun.yml", version);
        let shown = config("config_show", &json!({"root":root.path()})).unwrap();
        assert_eq!(
            shown["location"]["file"],
            json!(canonical.join(".config/testrun.yml"))
        );
        let other = tempfile::tempdir().unwrap();
        located(other.path(), "acme.yml", version);
        let explicit = other.path().join("acme.yml").canonicalize().unwrap();
        let args = json!({"root":root.path(),"config":explicit});
        assert_eq!(
            config("config_check", &args).unwrap()["location"]["file"],
            json!(explicit)
        );
        if version == 2 {
            let listed = config("project_list", &args).unwrap();
            assert_eq!(
                listed["projects"][0]["root"],
                json!(canonical.join("android"))
            );
            let shown = config(
                "project_show",
                &json!({"root":root.path(),"config":explicit,"project":"mobile"}),
            )
            .unwrap();
            assert_eq!(shown["applicationRoot"], json!(canonical.join("android")));
        }
        assert!(
            config(
                "config_show",
                &json!({"root":root.path(),"config":"acme.yml"})
            )
            .is_err()
        );
    }
    let root = tempfile::tempdir().unwrap();
    let grouped = root.path().join(".config/testrun.yml");
    let created = config("config_init", &json!({"root":root.path(),"config":grouped})).unwrap();
    assert_eq!(created["ignore"]["entry"], "/.config/testrun.local.yml");
    assert_eq!(
        config("config_init", &json!({"root":root.path()}))
            .err()
            .unwrap()
            .code,
        "CONFIG_EXISTS"
    );
}
#[test]
fn recovery_is_bound_to_the_retained_config_file_not_current_yaml() {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    located(root.path(), "qa/acme.yml", 1);
    let explicit = root.path().join("qa/acme.yml");
    let owners = Owners::default();
    let mut r = selected(root.path(), 1, Some(explicit.clone()));
    r.submission_id = Some("acme-located".into());
    let (record, ticket) = owners
        .begin(state.path(), "session", "transport", &r)
        .unwrap();
    owners.finish(&ticket.unwrap(), Ok(json!({"built":true})));
    std::fs::remove_file(&explicit).unwrap();
    assert_eq!(
        owners
            .replay(state.path(), "session", "other-transport", &r)
            .unwrap()
            .unwrap()["operationId"],
        record["operationId"]
    );
    r.config = Some(root.path().join("qa/other.yml"));
    assert_eq!(
        owners
            .replay(state.path(), "session", "same-key", &r)
            .err()
            .unwrap()
            .code,
        "REQUEST_CONFLICT"
    );
}
