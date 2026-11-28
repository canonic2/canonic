use super::*;
#[test]
fn diagnostic_scenario_context_android_media_device_plan_has_only_fixed_read_commands() {
    let capture = Capture::new(super::super::Options {
        target: "android:Acme".into(),
        app: "com.example.acme".into(),
        user: 10,
        follow: false,
        reports: false,
        system: false,
        since: None,
        state: Some(Options {
            groups: vec!["media".into(), "device_context".into()],
            interval_ms: 1000,
            max_processes: 1,
            label: None,
        }),
    })
    .unwrap();
    let plan = capture.state_plan();
    let commands: Vec<_> = plan
        .into_iter()
        .map(|r| match r {
            Read::Text(_, args) => args,
            _ => panic!("Unexpected non-read context operation"),
        })
        .collect();
    assert_eq!(
        commands,
        vec![
            vec!["dumpsys", "audio"],
            vec!["dumpsys", "media_session"],
            vec!["getprop", "persist.sys.locale"],
            vec!["getprop", "persist.sys.timezone"],
            vec!["settings", "--user", "10", "get", "system", "font_scale"],
            vec!["settings", "--user", "10", "get", "system", "user_rotation"],
            vec![
                "settings",
                "--user",
                "10",
                "get",
                "system",
                "accelerometer_rotation"
            ]
        ]
    );
}
#[test]
fn diagnostic_scenario_context_android_media_filters_uid_and_context_nulls_stay_unknown() {
    let text = "client com.example.acme uid=10042 state=playing\nclient com.example.acme uid=1010042 state=paused\nclient com.example.acme.extra uid=10042 state=playing\nposition=12345 route=private-neighbor";
    for group in ["audio", "media_sessions"] {
        let result = parse::context(group, text, "com.example.acme", 0, 10042);
        assert_eq!(result["fields"]["records"].as_array().unwrap().len(), 1);
        assert!(!result.to_string().contains("private-neighbor"));
        assert!(!result.to_string().contains("paused"));
    }
    assert_eq!(
        parse::context("font_scale", "1.25", "com.example.acme", 0, 10042)["fields"]["fontScale"],
        1.25
    );
    assert_eq!(
        parse::context("user_rotation", "3", "com.example.acme", 0, 10042)["fields"]["user_rotation"],
        3
    );
    for (group, text) in [
        ("locale", "null"),
        ("timezone", ""),
        ("font_scale", "NaN"),
        ("accelerometer_rotation", "2"),
    ] {
        assert_eq!(
            parse::context(group, text, "com.example.acme", 0, 10042)["status"],
            "unrecognized_format"
        );
    }
}
fn stat(user: u64, system: u64, start: u64) -> String {
    let mut fields = vec!["0".to_string(); 22];
    fields[0] = "S".into();
    fields[11] = user.to_string();
    fields[12] = system.to_string();
    fields[19] = start.to_string();
    format!(
        "42 (acme worker) {}\nState:\tS (sleeping)\nUid:\t10042 10042 10042 10042\nVmRSS:\t1024 kB\nRssAnon:\t512 kB\nThreads:\t3\ncom.example.acme\0",
        fields.join(" ")
    )
}
#[test]
fn android_state_cpu_intervals_require_same_clock_and_monotonic_counters() {
    let first = parse::Counters::read(&stat(100, 50, 1), 10.0, Some(100)).unwrap();
    let next = parse::Counters::read(&stat(150, 100, 1), 12.0, Some(100)).unwrap();
    let metrics = next.metrics(Some(&first));
    assert_eq!(
        metrics
            .iter()
            .find(|v| v["name"] == "cpu_time_delta")
            .unwrap()["value"],
        1.0
    );
    assert_eq!(
        metrics
            .iter()
            .find(|v| v["name"] == "cpu_utilization")
            .unwrap()["value"],
        50.0
    );
    for sample in [
        parse::Counters::read(&stat(150, 100, 1), 9.0, Some(100)),
        parse::Counters::read(&stat(1, 1, 1), 12.0, Some(100)),
        parse::Counters::read(&stat(150, 100, 1), 12.0, None),
        parse::Counters::read(&stat(150, 100, 1), 12.0, Some(1000)),
    ] {
        assert!(
            !sample
                .unwrap()
                .metrics(Some(&first))
                .iter()
                .any(|v| v["name"] == "cpu_utilization")
        );
    }
}
#[test]
fn android_state_memory_units_keep_rss_pss_and_live_heap_separate() {
    let metrics = parse::status_metrics(&stat(1, 1, 1));
    assert_eq!(
        metrics.iter().find(|v| v["name"] == "rss").unwrap()["value"],
        1048576.0
    );
    assert!(parse::status_metrics("VmRSS: 100 MB").is_empty());
    let parsed = parse::memory(
        "** MEMINFO in pid 42 [com.example.acme] **\nApp Summary\n Java Heap: 100\n Native Heap: 200\n Graphics: 30\n TOTAL PSS: 500 TOTAL RSS: 900 TOTAL SWAP PSS: 2",
        42,
    );
    assert_eq!(
        parsed["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "pss")
            .unwrap()["value"],
        512000.0
    );
    assert_eq!(
        parse::memory(
            "** MEMINFO in pid 43 [other] **\nApp Summary\nJava Heap: 1",
            42
        )["status"],
        "unrecognized_format"
    );
    let fields = parse::process_fields(&stat(1, 1, 1));
    assert!(fields["heapLiveBytes"].is_null());
    assert!(fields["gcCount"].is_null());
}
#[test]
fn android_state_frames_require_exact_process_header_valid_flags_and_completed_time() {
    let frames = parse::frames(
        "** Graphics info for pid 42 [com.example.acme] **\n---PROFILEDATA---\nFlags,IntendedVsync,FrameCompleted,\n0,1000000000,1016000000,\n1,2000000000,2010000000,\n0,3000000000,9223372036854775807,\n---PROFILEDATA---",
        42,
    );
    assert_eq!(frames["frames"].as_array().unwrap().len(), 1);
    assert_eq!(frames["frames"][0]["durationMs"], 16.0);
    assert_eq!(frames["frames"][0]["intendedVsyncNs"], "1000000000");
    assert_eq!(
        parse::frames("** Graphics info for pid 43 [other] **", 42)["status"],
        "unrecognized_format"
    );
}
#[test]
fn android_state_system_context_filters_other_users_and_keeps_policy_unknowns() {
    let text = "JOB #u0a42/1: com.example.acme/.Job\nJOB #u10a42/2: com.example.acme/.Secret\nJOB #u0a43/3: com.example.acme.extra/.Job";
    let parsed = parse::context("jobs", text, "com.example.acme", 0, 10042);
    assert_eq!(parsed["fields"]["records"].as_array().unwrap().len(), 1);
    assert!(!parsed.to_string().contains("Secret"));
    assert_eq!(
        parse::context("standby", "40\n", "com.example.acme", 0, 10042)["fields"]["bucketCode"],
        40
    );
    assert!(parse::context("background","No operations.","com.example.acme",0,10042)["fields"]["mode"].is_null());
    assert!(parse::context("manufacturer","Acme","com.example.acme",0,10042)["fields"]["oemRestrictionPolicy"].is_null());
    assert_eq!(
        parse::context(
            "battery",
            "USB powered: true\nlevel: 90\nscale: 100\ntemperature: 327",
            "com.example.acme",
            0,
            10042
        )["metrics"][0]["value"],
        32.7
    );
}
#[test]
fn android_state_fd_io_thread_reads_do_not_guess_unknown_output_or_neighbor_scope() {
    assert_eq!(parse::fds("0\n1\n3\n")["metrics"][0]["value"], 3.0);
    assert_eq!(
        parse::fds("Permission denied")["status"],
        "unrecognized_format"
    );
    let io = parse::io("rchar: 20\nread_bytes: 10\nsyscr: 2");
    assert_eq!(io["metrics"].as_array().unwrap().len(), 3);
    assert_eq!(parse::io("OEM unknown")["status"], "unrecognized_format");
    let threads = parse::threads(
        "PID TID UID STAT NAME\n42 42 10042 S acme\n43 43 10043 R neighbor\n42 44 1010042 R other-user",
        42,
        10042,
    );
    assert_eq!(threads["threads"].as_array().unwrap().len(), 1);
}
#[test]
fn android_state_selection_rejects_mutators_arbitrary_commands_duplicates_and_bad_bounds() {
    let mut options = Options {
        groups: groups(),
        interval_ms: interval(),
        max_processes: process_limit(),
        label: None,
    };
    assert!(options.validate().is_ok());
    for groups in [
        vec!["reset".into()],
        vec!["process".into(), "process".into()],
        vec![],
    ] {
        options.groups = groups;
        assert!(options.validate().is_err());
    }
    options.groups = groups();
    options.interval_ms = 1;
    assert!(options.validate().is_err());
}

#[test]
fn android_state_foreground_services_and_boolean_power_fields_preserve_selected_user() {
    let parsed = parse::context(
        "services",
        "ServiceRecord{abc u0 com.example.acme/.Service}\nisForeground=true foregroundId=42 startRequested=true\nServiceRecord{def u10 com.example.acme/.Other}\nisForeground=false foregroundId=99",
        "com.example.acme",
        0,
        10042,
    );
    assert_eq!(parsed["fields"]["records"].as_array().unwrap().len(), 1);
    assert_eq!(parsed["fields"]["records"][0]["isForeground"], true);
    assert_eq!(parsed["fields"]["records"][0]["foregroundId"], 42);
    let power = parse::context(
        "power",
        "mIsPowered=true\nmDeviceIdleMode=false\nPARTIAL_WAKE_LOCK com.example.acme (uid=10042 pid=42)\nPARTIAL_WAKE_LOCK com.example.acme (uid=1010042 pid=43)",
        "com.example.acme",
        0,
        10042,
    );
    assert_eq!(power["fields"]["mIsPowered"], true);
    assert_eq!(power["fields"]["wakeLocks"].as_array().unwrap().len(), 1);
    let modern = parse::memory(
        "** MEMINFO in pid 42 [com.example.acme] **\nApp Summary\nPss(KB) Rss(KB)\nJava Heap: 100 200",
        42,
    );
    assert_eq!(modern["metrics"][0]["value"], 102400.0);
}
