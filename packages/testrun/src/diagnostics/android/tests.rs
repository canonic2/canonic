use super::*;
fn options() -> Options {
    Options {
        target: "android:ACME".into(),
        app: "com.example.acme".into(),
        user: 0,
        follow: false,
        reports: false,
        system: true,
        since: None,
        state: None,
    }
}
fn log(time: &str, message: &str) -> parse::Log {
    parse::log(&format!("{time} 1000 42 42 E AndroidRuntime: {message}")).unwrap()
}
#[test]
fn android_diagnostic_package_and_user_scope_do_not_accept_substrings_or_commands() {
    assert_eq!(parse::uid_inventory("package:com.example.acme.extra uid:10043\npackage:com.example.acme uid:10042\npackage:com.example.shared uid:10042", "com.example.acme").unwrap(),(10042,2));
    assert!(parse::package_token(
        "ANR in com.example.acme:remote",
        "com.example.acme"
    ));
    assert!(!parse::package_token(
        "ANR in com.example.acme.extra",
        "com.example.acme"
    ));
    for app in [
        "com.example.acme;reboot",
        "../data",
        "com..acme",
        "foo",
        "-s device",
    ] {
        let mut input = options();
        input.app = app.into();
        assert!(Capture::new(input).is_err());
    }
    let mut capture = Capture::new(options()).unwrap();
    capture.uid = 10042;
    assert!(capture.related(&log(
        "1.0",
        "Killing 42:com.example.acme/u0a42 (adj 0): low memory"
    )));
    assert!(!capture.related(&log(
        "1.0",
        "Killing 42:com.example.acme/u10a42 (adj 0): low memory"
    )));
    let mut input = options();
    input.user = 1;
    let mut capture = Capture::new(input).unwrap();
    capture.uid = 110042;
    assert!(!capture.related(&log("1.0", "Force stopping com.example.acme user=10")));
    assert!(capture.related(&log("1.0", "Force stopping com.example.acme user=1")));
    assert!(!capture.related(&log("1.0", "[10,42,com.example.acme]")));
    assert!(capture.related(&log("1.0", " [1,42,com.example.acme]")));
}
#[test]
fn android_diagnostic_process_generation_uses_boot_start_ticks_and_actual_uid() {
    let bytes = format!(
        "42 (name with ) bracket) S {} 100 0 0\nName:\tacme\nUid:\t10042 10042 10042 10042\ncom.example.acme:remote\0",
        vec!["0"; 18].join(" ")
    );
    let first = parse::process(bytes.as_bytes(), 42, 10042, "boot-one").unwrap();
    assert_eq!(first.start_ticks, "100");
    assert_eq!(first.name, "com.example.acme:remote");
    let second = parse::process(
        bytes.replace(" 100 0 0", " 200 0 0").as_bytes(),
        42,
        10042,
        "boot-one",
    )
    .unwrap();
    assert_ne!(first.generation, second.generation);
    assert_ne!(
        first.generation,
        parse::process(bytes.as_bytes(), 42, 10042, "boot-two")
            .unwrap()
            .generation
    );
    assert!(parse::process(bytes.as_bytes(), 42, 10043, "boot-one").is_none());
    let list = "PID UID NAME\n42 10042 com.example.acme\n43 10042 com.example.acme:remote\n44 10042 com.example.neighbor\n45 1010042 com.example.acme";
    assert_eq!(
        parse::processes(list, 10042, "com.example.acme", true)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        parse::processes(list, 10042, "com.example.acme", false)
            .unwrap()
            .len(),
        3
    );
}
#[test]
fn android_diagnostic_epoch_cursor_preserves_identical_boundary_multiplicity() {
    let line = log("1700000000.123456", "FATAL EXCEPTION: main");
    let mut cursor = parse::Cursor::default();
    assert_eq!(cursor.take(vec![line.clone(), line.clone()]).0.len(), 2);
    assert_eq!(
        cursor
            .take(vec![line.clone(), line.clone(), line.clone()])
            .0
            .len(),
        1
    );
    assert_eq!(
        cursor.take(vec![line.clone(), line.clone(), line]).0.len(),
        0
    );
    assert!(cursor.take(vec![log("1699999999.1", "earlier")]).1);
    assert!(parse::time_key("1700.1234567890").is_none());
    assert!(parse::time_key("1700.x").is_none());
}
#[test]
fn android_diagnostic_exit_reasons_keep_uid_unknowns_and_trace_access_separate() {
    let text = "ACTIVITY MANAGER PROCESS EXIT INFO\nApplicationExitInfo #0:\n timestamp=2026-11-07 12:00:00.123 pid=42 realUid=10042 packageUid=10042 user=0\n process=com.example.acme reason=6 (ANR) status=0\n description=Input timeout trace=/data/anr/anr_acme\nApplicationExitInfo #1:\n timestamp=2026-11-07 12:00:00.124 pid=42 realUid=1010042 packageUid=1010042 user=10\n process=com.example.acme reason=3 (LOW_MEMORY) status=0\n trace=null\nApplicationExitInfo #2:\n timestamp=2026-11-07 12:00:00.125 pid=43 realUid=10042 packageUid=10042 user=0\n process=com.example.acme:remote reason=99 (FUTURE) status=1\n trace=null";
    let entries = parse::exit_entries(text, 10042);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["classification"], "anr");
    assert_eq!(entries[1]["classification"], "unknown_exit");
    assert!(entries[0]["processGeneration"].is_null());
    assert_eq!(entries[0]["causeEstablished"], false);
    assert!(parse::safe_report("/data/anr/anr_acme"));
    assert!(parse::safe_report("/data/tombstones/tombstone_00"));
    for path in [
        "/data/anr/../../private",
        "/sdcard/private",
        "/data/anr/anr_x;reboot",
    ] {
        assert!(!parse::safe_report(path));
    }
}
#[test]
fn android_diagnostic_release_metadata_is_installed_evidence_and_unknowns_stay_unknown() {
    let capture = Capture::new(options()).unwrap();
    assert!(capture.packet("readiness", json!({}))["uid"].is_null());
    assert!(capture.summary()["uid"].is_null());
    let observed = parse::installed(
        "versionCode=42 minSdk=26\nversionName=1.0\nflags=[ HAS_CODE ]\nprivateFlags=[ PROFILEABLE_BY_SHELL ]\nlastUpdateTime=2026-11-07 12:00:00\nnativeLibraryDir=/data/app/acme/lib/arm64",
        "package:/data/app/acme/base.apk\npackage:/data/app/acme/split_config.arm64.apk\n",
    );
    assert_eq!(observed["versionCode"], "42");
    assert_eq!(observed["debuggable"], false);
    assert_eq!(observed["profileableByShell"], true);
    assert_eq!(observed["apkPaths"].as_array().unwrap().len(), 2);
    assert!(parse::installed("OEM unknown response", "")["debuggable"].is_null());
}
#[tokio::test]
async fn android_diagnostic_stopped_or_expired_work_does_not_spawn_a_platform_command() {
    let mut capture = Capture::new(options()).unwrap();
    capture.adb.adb = "must-not-exist-acme-adb".into();
    let (sender, receiver) = tokio::sync::watch::channel(Some("stopped".into()));
    capture.control(receiver, Duration::from_secs(1));
    assert_eq!(capture.read().await.err().unwrap().code, "CANCELLED");
    drop(sender);
    let (_sender, receiver) = tokio::sync::watch::channel(None);
    capture.control(receiver, Duration::ZERO);
    assert_eq!(capture.read().await.err().unwrap().code, "TIMEOUT");
}

#[test]
fn android_diagnostic_tombstone_blocks_require_privileged_exact_package_and_uid() {
    let make = |message: &str| parse::log(&format!("1.0 0 300 300 F DEBUG: {message}")).unwrap();
    let lines = vec![
        make("*** *** ***"),
        make("pid: 42 >>> com.example.acme <<<"),
        make("uid: 10042"),
        make("#00 pc 123 libacme.so"),
        make("*** *** ***"),
        make("pid: 44 >>> com.example.acme.extra <<<"),
        make("uid: 10042"),
        make("#00 private"),
    ];
    let retained = parse::tombstone_context(&lines, "com.example.acme", 10042);
    assert_eq!(retained.len(), 4);
    assert!(retained.contains(&3));
    assert!(!retained.contains(&7));
    let unmarked: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 4)
        .map(|(_, l)| l.clone())
        .collect();
    assert!(parse::tombstone_context(&unmarked, "com.example.acme", 10042).is_empty());
    assert!(parse::tombstone_context(&lines, "com.example.acme", 1010042).is_empty());
    let untrusted: Vec<_> = lines
        .into_iter()
        .map(|mut l| {
            l.uid = Some(10043);
            l
        })
        .collect();
    assert!(parse::tombstone_context(&untrusted, "com.example.acme", 10042).is_empty());
    let mut capture = Capture::new(options()).unwrap();
    capture.uid = 10042;
    let mut spoof = log("1.0", "ANR in com.example.acme user=0");
    spoof.uid = Some(10043);
    assert!(!capture.related(&spoof));
}
