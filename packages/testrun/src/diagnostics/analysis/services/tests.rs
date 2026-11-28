use super::*;
fn js() -> Value {
    json!({"nodes":[{"id":1,"callFrame":{"functionName":"root","scriptId":"1","url":"acme.js","lineNumber":0,"columnNumber":0},"children":[2]},{"id":2,"callFrame":{"functionName":"Acme.work","scriptId":"1","url":"acme.js","lineNumber":12,"columnNumber":0}}],"startTime":0,"endTime":100,"samples":[2,2],"timeDeltas":[40,60]})
}
fn scope(runtime: &str, format: &str, payload: Value) -> Value {
    json!({"schemaVersion":1,"selection":{"targetId":"service:Acme:api","serviceId":"api","hostId":"Acme","os":"linux","transport":"local_export","pid":42,"processGeneration":"boot:42:birth","runtime":runtime,"runtimeVersion":"fixture-1","buildId":"build-a","architecture":"x86_64","scope":{"kind":"host"},"owner":{"kind":"borrowed"}},"format":format,"encoding":"json","payload":payload})
}
fn parse(format: &str, v: &Value) -> Result<Index> {
    decode::decode(format, &serde_json::to_vec(v).unwrap())
}
fn speedscope(evented: bool) -> Value {
    let profile = if evented {
        json!({"type":"evented","name":"managed worker","unit":"milliseconds","startValue":0,"endValue":2,"events":[{"type":"O","frame":0,"at":0},{"type":"C","frame":0,"at":2}]})
    } else {
        json!({"type":"sampled","name":"Python thread 1","unit":"seconds","startValue":0,"endValue":0.01,"samples":[[0]],"weights":[0.01]})
    };
    json!({"$schema":"https://www.speedscope.app/file-format-schema.json","shared":{"frames":[{"name":"Acme.work","file":"acme.py","line":12}]},"profiles":[profile]})
}
#[test]
fn diagnostic_services_js_runtime_routes_keep_sample_witnesses_and_refuse_cycles() {
    for runtime in ["node", "bun", "deno"] {
        let i = parse(
            "service-evidence-v1",
            &scope(runtime, "js-cpu-profile", js()),
        )
        .unwrap();
        assert_eq!(i.rows[1].unit.as_deref(), Some("samples"));
        assert_eq!(i.rows[1].timestamp, Some(40.0));
        assert_eq!(i.rows[1].data["frames"][1]["functionName"], "Acme.work");
        assert_eq!(
            i.rows[1].data["serviceEvidence"]["selection"]["runtime"],
            runtime
        );
    }
    let mut bad = js();
    bad["nodes"][1]["children"] = json!([1]);
    assert!(parse("js-cpu-profile", &bad).is_err());
    let mut bad = js();
    bad["samples"] = json!([999]);
    assert!(parse("js-cpu-profile", &bad).is_err());
    let mut bad = js();
    bad["timeDeltas"] = json!([1]);
    assert!(parse("js-cpu-profile", &bad).is_err());
}
#[test]
fn diagnostic_services_dotnet_python_profiles_preserve_runtime_thread_and_units() {
    for (runtime, evented, unit, value) in [
        ("dotnet", true, "milliseconds", 2.0),
        ("python", false, "seconds", 0.01),
    ] {
        let i = parse(
            "service-evidence-v1",
            &scope(runtime, "speedscope", speedscope(evented)),
        )
        .unwrap();
        assert_eq!(i.rows[1].unit.as_deref(), Some(unit));
        assert_eq!(i.rows[1].value, Some(value));
        assert!(!i.rows[1].data["profileName"].is_null());
    }
    assert!(
        parse(
            "service-evidence-v1",
            &scope("python", "js-cpu-profile", js())
        )
        .is_err()
    );
}
#[test]
fn diagnostic_services_jfr_retains_gc_locks_io_and_sampled_thread_without_invented_durations() {
    let events=["jdk.ExecutionSample","jdk.GarbageCollection","jdk.JavaMonitorEnter","jdk.FileRead","jdk.SocketWrite","jdk.ThreadPark","jdk.ObjectAllocationInNewTLAB"].map(|name|json!({"type":name,"values":{"startTime":"2026-01-01T00:00:00Z","duration":"PT0.001S","sampledThread":{"javaThreadId":7},"stackTrace":{"truncated":true,"frames":[{"method":{"name":"work","type":{"name":"Acme"},"descriptor":"()V"},"lineNumber":12}]}}}));
    let i = parse(
        "service-evidence-v1",
        &scope("jvm", "jfr-json", json!({"recording":{"events":events}})),
    )
    .unwrap();
    assert_eq!(i.rows[1].kind, "stack");
    assert_eq!(i.rows[1].value, Some(1.0));
    assert_eq!(i.rows[1].data["stackTruncated"], true);
    assert_eq!(i.rows[2].kind, "event");
    assert!(i.rows[2].value.is_none());
    assert_eq!(i.rows[2].data["values"]["duration"], "PT0.001S");
    assert!(
        i.rows[3]
            .correlations
            .contains(&"event-type:jdk.JavaMonitorEnter".into())
    );
}
#[test]
fn diagnostic_services_ruby_self_costs_and_php_call_costs_remain_distinct() {
    let ruby = json!({"version":1.0,"mode":"object","interval":1,"samples":10,"missed_samples":2,"frames":{"1":{"name":"Acme#work","samples":3,"total_samples":10,"edges":{"2":7}},"2":{"name":"Acme#load","samples":7,"total_samples":7,"edges":{}}}});
    let i = parse(
        "service-evidence-v1",
        &scope("ruby", "stackprof-json", ruby),
    )
    .unwrap();
    assert_eq!(i.rows[1].unit.as_deref(), Some("sampled_objects"));
    assert_eq!(i.rows[1].value, Some(3.0));
    let php = "version: 1\ncreator: xdebug\nevents: Time_(10ns) Memory_(bytes)\nfl=(1) acme.php\nfn=(1) main\n10 200 32\ncfn=(2) load\ncalls=1 12\n10 100 16\nfn=(2)\n12 50 8\n";
    let mut p = scope("php", "xdebug-cachegrind", json!(php));
    p["encoding"] = json!("utf8");
    let i = parse("service-evidence-v1", &p).unwrap();
    assert_eq!(i.rows[1].unit.as_deref(), Some("10_nanoseconds"));
    assert_eq!(i.rows[2].unit.as_deref(), Some("bytes"));
    assert_eq!(i.rows[3].data["costKind"], "inclusive_call_cost");
    assert_eq!(i.rows[5].data["costKind"], "exclusive_self_cost");
    assert!(decode::decode("xdebug-cachegrind", b"events: Time\nfn=(99)\n10 12\n").is_err());
    assert!(decode::decode("stackprof-json", b"\x04\x08Marshal").is_err());
}
fn vi(mut n: u64) -> Vec<u8> {
    let mut b = vec![];
    while n >= 128 {
        b.push((n as u8 & 127) | 128);
        n >>= 7;
    }
    b.push(n as u8);
    b
}
fn num(id: u32, n: u64) -> Vec<u8> {
    let mut b = vi((id as u64) << 3);
    b.extend(vi(n));
    b
}
fn data(id: u32, value: &[u8]) -> Vec<u8> {
    let mut b = vi(((id as u64) << 3) | 2);
    b.extend(vi(value.len() as u64));
    b.extend(value);
    b
}
fn message(parts: Vec<Vec<u8>>) -> Vec<u8> {
    parts.into_iter().flatten().collect()
}
fn profile() -> Vec<u8> {
    let mut b = message(vec![
        data(1, &message(vec![num(1, 1), num(2, 2)])),
        data(1, &message(vec![num(1, 3), num(2, 4)])),
        data(5, &message(vec![num(1, 10), num(2, 5), num(4, 6)])),
        data(
            4,
            &message(vec![
                num(1, 20),
                num(2, 1),
                data(4, &message(vec![num(1, 10), num(2, 12)])),
            ]),
        ),
        data(
            2,
            &message(vec![
                data(1, &vi(20)),
                data(2, &message(vec![vi(3), vi(3000)])),
                data(3, &message(vec![num(1, 9), num(3, 42)])),
            ]),
        ),
    ]);
    b.extend(data(3, &message(vec![num(1, 1), num(5, 7), num(6, 8)])));
    for s in [
        "",
        "samples",
        "count",
        "cpu",
        "nanoseconds",
        "Acme.work",
        "acme.go",
        "Acme",
        "build-a",
        "goroutine",
    ] {
        b.extend(data(6, s.as_bytes()));
    }
    b
}
#[test]
fn diagnostic_services_go_pprof_decodes_real_wire_samples_and_rejects_bad_ids_and_varints() {
    let raw = profile();
    let mut p = scope(
        "go",
        "pprof",
        json!(base64::engine::general_purpose::STANDARD.encode(&raw)),
    );
    p["encoding"] = json!("base64");
    let i = parse("service-evidence-v1", &p).unwrap();
    assert_eq!(i.rows[1].unit.as_deref(), Some("count"));
    assert_eq!(i.rows[1].value, Some(3.0));
    assert_eq!(i.rows[2].unit.as_deref(), Some("nanoseconds"));
    assert_eq!(i.rows[2].value, Some(3000.0));
    assert_eq!(i.rows[1].data["frames"][0]["mapping"]["buildId"], "build-a");
    assert_eq!(i.rows[1].data["labels"][0]["number"], 42);
    assert_eq!(i.rows[1].data["frames"][0]["name"], "Acme.work");
    let mut bad = raw.clone();
    bad.extend(data(
        2,
        &message(vec![
            data(1, &vi(999)),
            data(2, &message(vec![vi(1), vi(2)])),
        ]),
    ));
    assert!(decode::decode("pprof", &bad).is_err());
    assert!(decode::decode("pprof", &[0x80; 11]).is_err());
    let mut wrong_wire = raw.clone();
    wrong_wire.push(0x31);
    wrong_wire.extend(b"unused!!");
    assert!(decode::decode("pprof", &wrong_wire).is_err());
    use std::io::Write;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&raw).unwrap();
    let i = decode::decode("pprof", &gz.finish().unwrap()).unwrap();
    assert_eq!(i.rows.len(), 2);
}
fn dump() -> Vec<u8> {
    let mut b = vec![0u8; 280];
    b[..4].copy_from_slice(b"MDMP");
    for (at, value) in [
        (4, 0xa793u32),
        (8, 2),
        (12, 32),
        (32, 7),
        (36, 56),
        (40, 56),
        (44, 6),
        (48, 168),
        (52, 112),
        (112, 42),
        (120, 0xc0000005),
    ] {
        b[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    b[56..58].copy_from_slice(&9u16.to_le_bytes());
    b[136..144].copy_from_slice(&4096u64.to_le_bytes());
    b
}
#[test]
fn diagnostic_services_native_minidump_reports_exception_without_unwinding_or_symbol_claims() {
    let raw = dump();
    let i = decode::decode("windows-minidump", &raw).unwrap();
    assert_eq!(i.rows[0].data["architecture"], "x86_64");
    assert_eq!(i.rows[1].kind, "failure");
    assert_eq!(i.rows[1].data["code"], "0xc0000005");
    assert_eq!(i.rows[1].data["causeEstablished"], false);
    let mut bad = raw.clone();
    bad[52..56].copy_from_slice(&99999u32.to_le_bytes());
    assert!(decode::decode("windows-minidump", &bad).is_err());
    assert!(decode::decode("windows-minidump", &raw[..20]).is_err());
    let mut no_code = raw;
    no_code[120..124].fill(0);
    assert_eq!(
        decode::decode("windows-minidump", &no_code).unwrap().rows[1].kind,
        "event"
    );
}
#[test]
fn diagnostic_services_minidump_module_names_and_codeview_stay_raw_identities() {
    let mut raw = dump();
    let old = raw[32..56].to_vec();
    raw.resize(536, 0);
    raw[280..304].copy_from_slice(&old);
    for (at, value) in [
        (8, 4u32),
        (12, 280),
        (304, 3),
        (308, 52),
        (312, 328),
        (316, 4),
        (320, 112),
        (324, 380),
        (328, 1),
        (332, 42),
        (380, 1),
        (392, 4096),
        (404, 492),
        (460, 24),
        (464, 512),
        (492, 16),
        (532, 1),
    ] {
        raw[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    raw[384..392].copy_from_slice(&4096u64.to_le_bytes());
    for (n, unit) in "Acme.exe".encode_utf16().enumerate() {
        raw[496 + n * 2..498 + n * 2].copy_from_slice(&unit.to_le_bytes());
    }
    raw[512..516].copy_from_slice(b"RSDS");
    raw[516..532].fill(1);
    let i = decode::decode("windows-minidump", &raw).unwrap();
    assert_eq!(i.rows[2].data["threadId"], 42);
    assert_eq!(i.rows[3].data["name"], "Acme.exe");
    assert_eq!(i.rows[3].data["codeView"]["age"], 1);
    assert_eq!(i.rows[3].data["symbolication"], "unresolved");
    raw[404..408].copy_from_slice(&9999u32.to_le_bytes());
    assert!(decode::decode("windows-minidump", &raw).is_err());
}
#[test]
fn diagnostic_services_workload_generations_and_box_ownership_do_not_authorize_exec_or_relabel_hosts()
 {
    let mut p = scope("node", "js-cpu-profile", js());
    p["selection"]["transport"] = json!("kubernetes_export");
    p["selection"]["scope"] = json!({"kind":"kubernetes","cluster":"acme","namespace":"qa","podUid":"pod-1","containerId":"container-1","restartCount":3,"imageId":"sha256:image-a"});
    let i = parse("service-evidence-v1", &p).unwrap();
    assert_eq!(
        i.rows[1].data["serviceEvidence"]["selection"]["scope"]["restartCount"],
        3
    );
    assert_eq!(
        i.rows[1].data["serviceEvidence"]["executionPerformed"],
        false
    );
    p["selection"]["scope"]["exec"] = json!(["never"]);
    assert!(parse("service-evidence-v1", &p).is_err());
    let mut p = scope(
        "node",
        "box-log-v1",
        json!({"workspace":"acme#feature","operation":"node-1","stream":"stderr","tail_only":true,"text":"Acme failure"}),
    );
    p["selection"]["transport"] = json!("box_export");
    p["selection"]["owner"] = json!({"kind":"box","projectId":"project-1","workspace":"acme#feature","operationId":"node-1","generationId":"generation-1"});
    let i = parse("service-evidence-v1", &p).unwrap();
    assert_eq!(i.rows[1].data["tail_only"], true);
    p["payload"]["operation"] = json!("another-operation");
    assert!(parse("service-evidence-v1", &p).is_err());
    p["format"] = json!("service-evidence-v1");
    assert!(parse("service-evidence-v1", &p).is_err());
}
