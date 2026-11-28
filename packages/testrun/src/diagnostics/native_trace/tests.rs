use super::*;
fn options() -> Options {
    serde_json::from_value(json!({"backend":"perfetto","mode":"import","targetId":"android:ACME","appId":"com.example.acme","path":"/outside/trace","toolPath":"/outside/trace_processor"})).unwrap()
}
#[test]
fn diagnostic_memory_legacy_native_options_preserve_replay_fingerprint_when_opt_ins_are_absent() {
    let legacy = r#"{"backend":"perfetto","mode":"import","targetId":"android:ACME","appId":"com.example.acme","processId":null,"path":"/outside/trace","toolPath":"/outside/trace_processor","template":null,"durationMs":3000,"frequencyHz":99,"allowSystemTrace":false,"allowAttach":false}"#;
    let options: Options = serde_json::from_str(legacy).unwrap();
    assert_eq!(serde_json::to_string(&options).unwrap(), legacy);
}
pub(crate) fn envelope(backend: &str, sections: Value) -> Value {
    json!({"schemaVersion":1,"backend":backend,"targetId":"android:ACME","declaredAppId":"com.example.acme","actionIds":["action:one"],"artifact":{"encoding":"identity","sha256":crate::diagnostics::digest(b"native-owned"),"base64":base64::engine::general_purpose::STANDARD.encode(b"native-owned")},"report":{"identity":{},"gaps":[]},"sections":sections})
}
#[test]
fn diagnostic_native_trace_admission_requires_selected_effects_and_refuses_arbitrary_commands() {
    let mut o = options();
    assert!(o.validate().is_ok());
    o.mode = "capture".into();
    o.path = None;
    assert!(o.validate().is_err());
    o.allow_system_trace = true;
    assert!(o.validate().is_ok());
    o.backend = "simpleperf".into();
    assert!(o.validate().is_err());
    o.process_id = Some(42);
    assert!(o.validate().is_ok());
    o.backend = "xctrace".into();
    o.template = Some("Time Profiler".into());
    o.target_id = "device:ACME".into();
    assert!(o.validate().is_err());
    o.allow_attach = true;
    assert!(o.validate().is_ok());
    for key in ["sql", "command", "args", "allProcesses", "launch"] {
        let mut v = serde_json::to_value(&o).unwrap();
        v[key] = json!("must-not-run");
        assert!(serde_json::from_value::<Options>(v).is_err());
    }
    let mut source = serde_json::to_value(&o).unwrap();
    source["kind"] = json!("native_trace");
    source["sql"] = json!("SELECT 1");
    assert!(serde_json::from_value::<crate::diagnostics::model::Source>(source).is_err());
}
#[test]
fn diagnostic_native_trace_perfetto_scheduling_lifetimes_loss_and_unresolved_samples_stay_distinct()
{
    let v = envelope(
        "perfetto",
        json!([
            {"name":"processes","format":"csv","text":"upid,pid,name,start_ts,end_ts\n1,42,\"Acme, main\",0,1000\n2,42,Reused,1001,2000\n"},
            {"name":"threads","format":"csv","text":"utid,tid,upid,name\n3,42,1,main\n"},
            {"name":"scheduling","format":"csv","text":"id,ts,dur,utid,state,io_wait,blocked_function,waker_utid\n1,100,20,3,Running,0,[NULL],[NULL]\n2,120,30,3,R,0,[NULL],4\n3,150,40,3,D,1,io_schedule,4\n4,190,-1,3,S,0,[NULL],4\n"},
            {"name":"loss","format":"csv","text":"name,idx,severity,value\nftrace_cpu_overrun_end,0,data_loss,5\n"},
            {"name":"cpu_samples","format":"csv","text":"ts,utid,callsite_id\n105,3,2\n"},
            {"name":"callsites","format":"csv","text":"id,parent_id,frame_id,depth\n1,[NULL],1,0\n2,1,2,1\n"},
            {"name":"frames","format":"csv","text":"id,name,rel_pc,mapping,build_id,mapping_name\n1,main,100,1,abcd,libacme.so\n2,work,200,1,abcd,libacme.so\n"}
        ]),
    );
    let index = super::super::analysis::decode::decode("native-trace-v1", v.to_string().as_bytes())
        .unwrap();
    assert_eq!(index.rows.iter().filter(|r| r.kind == "process").count(), 2);
    assert_eq!(index.rows.iter().filter(|r| r.kind == "span").count(), 3);
    assert!(
        index
            .unsupported
            .iter()
            .any(|s| s.contains("data_loss") || s.contains("loss/error"))
    );
    let stack = index.rows.iter().find(|r| r.kind == "stack").unwrap();
    assert_eq!(stack.value, Some(1.0));
    assert_eq!(
        stack.data["observation"]["framesRootFirst"][0]["name"],
        "main"
    );
    assert!(
        index
            .rows
            .iter()
            .all(|r| r.data["appId"].is_null() && r.correlations == vec!["action:one"])
    );
    assert!(index.rows.iter().all(|r| v.pointer(&r.source).is_some()));
    let mut corrupt = v.clone();
    corrupt["artifact"]["sha256"] = json!("0".repeat(64));
    assert!(
        super::super::analysis::decode::decode("native-trace-v1", corrupt.to_string().as_bytes())
            .is_err()
    );
}
#[test]
fn diagnostic_native_trace_simpleperf_preserves_periods_and_never_derives_waits() {
    let v = envelope(
        "simpleperf",
        json!([{"name":"samples","format":"simpleperf-text","text":"meta_info:\n  trace_offcpu: false\nsample:\n  event_type: cpu-clock:u\n  time: 100\n  event_count: 999\n  thread_id: 42\n  thread_name: Acme\n  vaddr_in_file: 20\n  file: /libacme.so\n  symbol: leaf\n  callchain:\n    vaddr_in_file: 10\n    file: /libacme.so\n    symbol: main\nlost_situation:\n  sample_count: 1\n  lost_count: 3\n"}]),
    );
    let index = super::super::analysis::decode::decode("native-trace-v1", v.to_string().as_bytes())
        .unwrap();
    let row = &index.rows[0];
    assert_eq!(row.kind, "stack");
    assert_eq!(row.value, Some(1.0));
    assert_eq!(row.data["observation"]["eventPeriod"], 999.0);
    assert_eq!(
        row.data["observation"]["framesRootFirst"][0]["name"],
        "main"
    );
    assert!(index.rows.iter().all(|r| r.kind != "span"));
}
#[test]
fn diagnostic_native_trace_xctrace_resolves_local_references_and_rejects_cycles_entities() {
    let xml = r#"<trace-query-result><node><row><sample-time id="t">100</sample-time><weight id="w">1000</weight><backtrace id="b"><frame name="leaf" addr="20"/><frame name="main" addr="10"/></backtrace></row><row><sample-time>200</sample-time><weight ref="w"/><backtrace ref="b"/></row></node></trace-query-result>"#;
    let v = envelope(
        "xctrace",
        json!([{"name":"time-profile","format":"xml","text":xml}]),
    );
    let index = super::super::analysis::decode::decode("native-trace-v1", v.to_string().as_bytes())
        .unwrap();
    assert_eq!(index.rows.len(), 2);
    assert_eq!(index.rows[1].value, Some(1000.0));
    assert_eq!(index.rows[1].unit.as_deref(), Some("nanoseconds"));
    assert_eq!(
        index.rows[1].data["observation"]["framesRootFirst"][0]["name"],
        "main"
    );
    for xml in [
        r#"<!DOCTYPE x [<!ENTITY secret SYSTEM "file:///outside/private">]><x>&secret;</x>"#,
        r#"<x><row><backtrace id="b"><backtrace ref="b"/></backtrace></row></x>"#,
    ] {
        let mut v = v.clone();
        v["sections"][0]["text"] = json!(xml);
        assert!(
            super::super::analysis::decode::decode("native-trace-v1", v.to_string().as_bytes())
                .is_err()
        );
    }
}

fn ios_cpu_source() -> Value {
    json!({"backend":"xctrace","mode":"capture","targetId":"device:ACME",
        "appId":"com.example.acme","processId":42,"allowSystemTrace":true,
        "durationMs":3000,"template":"Activity Monitor","toolPath":"/outside/xcrun"})
}

#[test]
fn diagnostic_native_trace_ios_cpu_requires_physical_target_pid_and_system_consent() {
    let source = ios_cpu_source();
    let validate = |value| serde_json::from_value::<Options>(value).unwrap().validate();
    assert!(validate(source.clone()).is_ok());
    for (field, value) in [
        ("allowSystemTrace", json!(false)),
        ("processId", Value::Null),
        ("targetId", json!("simulator:ACME")),
        ("targetId", json!("host:macos")),
        ("targetId", json!("device:-wrong")),
        ("allowAttach", json!(true)),
    ] {
        let mut invalid = source.clone();
        invalid[field] = value;
        assert!(validate(invalid).is_err(), "{field}");
    }
    let mut import = source;
    import["mode"] = json!("import");
    import["path"] = json!("/outside/sample.trace");
    import["allowSystemTrace"] = json!(false);
    assert!(validate(import).is_ok());
}

#[cfg(unix)]
#[tokio::test]
async fn diagnostic_native_trace_ios_cpu_records_and_indexes_both_processes_on_one_clock() {
    let tmp = tempfile::tempdir().unwrap();
    let tool = executable(
        tmp.path(),
        r#"#!/bin/sh
[ "$1" = xctrace ] || exit 2
shift
case "$1" in
record)
  [ "$#" -eq 11 ] && [ "$2" = --template ] && [ "$3" = 'Activity Monitor' ] &&
    [ "$4" = --time-limit ] && [ "$5" = 3000ms ] && [ "$6" = --output ] &&
    [ "$8" = --no-prompt ] && [ "$9" = --all-processes ] &&
    [ "${10}" = --device ] && [ "${11}" = ACME ] || exit 3
  mkdir "$7" || exit 4
  printf 'original system trace' > "$7/native.data"
  ;;
export)
  [ "$2" = --input ] && [ -f "$3/native.data" ] || exit 5
  case "$4" in
    --toc) printf '<trace-toc><run number="1"><data><table schema="sysmon-process"/></data></run></trace-toc>' ;;
    --xpath)
      [ "$5" = '/trace-toc/run[@number="1"]/data/table[@schema="sysmon-process"]' ] || exit 6
      cat <<'XML'
<trace-query-result><node><schema name="sysmon-process"/>
<row><start-time id="1">1000000000</start-time><process id="2" fmt="Acme (42)"><pid id="3">42</pid></process><system-cpu-percent id="4">24.25</system-cpu-percent></row>
<row><start-time ref="1"/><process id="5" fmt="backboardd (70)"><pid id="6">70</pid></process><system-cpu-percent id="7">10.5</system-cpu-percent></row>
<row><start-time ref="1"/><process fmt="backboardd-helper (71)"><pid>71</pid></process><system-cpu-percent>87</system-cpu-percent></row>
<row><start-time id="8">2000000000</start-time><process ref="2"/><system-cpu-percent ref="7"/></row>
<row><start-time ref="8"/><process ref="5"/><sentinel/></row>
</node></trace-query-result>
XML
      ;;
    *) exit 7 ;;
  esac
  ;;
*) exit 8 ;;
esac
"#,
    );
    let mut source = ios_cpu_source();
    source["toolPath"] = json!(tool);
    let mut capture = Capture::new(serde_json::from_value(source).unwrap()).unwrap();
    capture.actions = vec!["action:one".into()];
    let (_tx, rx) = tokio::sync::watch::channel(None);
    capture.control(rx, Duration::from_secs(50));
    let (bytes, done, report) = capture.read(0).await.unwrap();
    assert!(done);
    assert_eq!(report["systemTraceRequested"], true);
    assert_eq!(report["attachEffects"], false);
    let native = super::super::analysis::decode::native_trace::artifact_bytes(&bytes).unwrap();
    assert_eq!(native.1, "tar");
    let mut archive = tar::Archive::new(native.0.as_slice());
    let mut artifact = archive.entries().unwrap().next().unwrap().unwrap();
    let mut contents = String::new();
    std::io::Read::read_to_string(&mut artifact, &mut contents).unwrap();
    assert_eq!(contents, "original system trace");
    let index = super::super::analysis::decode::decode("native-trace-v1", &bytes).unwrap();
    let metrics: Vec<_> = index
        .rows
        .iter()
        .filter(|row| row.kind == "metric")
        .collect();
    assert_eq!(metrics.len(), 3);
    assert!(
        metrics
            .iter()
            .all(|row| row.clock_domain.as_deref() == Some("native:xctrace")
                && row.unit.as_deref() == Some("percent")
                && row.correlations == ["action:one"])
    );
    assert_eq!(metrics[0].data["observation"]["processRole"], "app");
    assert_eq!(
        metrics[1].data["observation"]["processRole"],
        "render_server"
    );
    assert_eq!(metrics[0].timestamp, metrics[1].timestamp);
    assert_eq!(metrics[2].timestamp, Some(2_000_000_000.0));
    assert_eq!(metrics[0].value, Some(24.25));
    assert_eq!(metrics[1].value, Some(10.5));
    assert_eq!(metrics[2].value, Some(10.5));
    assert!(
        index
            .unsupported
            .iter()
            .any(|gap| gap.contains("1 selected rows lacked"))
    );
    let report = capture.settle().await;
    assert_eq!(report["cleanup"], "owned_host_files_removed");
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_native_trace_ios_cpu_retains_large_interrupted_traces_and_reports_device_loss()
{
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("input.trace");
    std::fs::create_dir(&input).unwrap();
    let witness = vec![b'A'; 6 * 1024 * 1024];
    std::fs::write(input.join("native.data"), &witness).unwrap();
    let tool = executable(
        tmp.path(),
        r#"#!/bin/sh
[ "$1" = xctrace ] || exit 2
shift
case "$1" in
  record)
    [ "$6" = --output ] || exit 3
    mkdir "$7" && cp "$(dirname "$0")/input.trace/native.data" "$7/native.data" || exit 4
    printf 'Device got disconnected, ending recording...\nRecording completed. Saving output file...\n' >&2
    ;;
  export)
    [ "$2" = --input ] && [ -f "$3/native.data" ] || exit 5
    case "$4" in
      --toc)
        printf '<trace-toc><run number="1"><info><summary><duration>0.822219</duration><end-reason>Device disconnected</end-reason></summary></info></run><run number="2"><info><summary><duration>15</duration><end-reason>Time limit reached</end-reason></summary></info></run></trace-toc>' ;;
      --xpath)
        if [ -f "$(dirname "$0")/slow-export" ]; then sleep 30; fi
        printf '<trace-query-result><node><schema name="sysmon-process"/></node></trace-query-result>' ;;
      *) exit 6 ;;
    esac
    ;;
  *) exit 7 ;;
esac
"#,
    );
    for mode in ["capture", "import"] {
        use super::super::source::Reader;
        let mut source = ios_cpu_source();
        source["toolPath"] = json!(tool);
        source["mode"] = json!(mode);
        if mode == "import" {
            std::fs::write(tmp.path().join("slow-export"), b"").unwrap();
            source["path"] = json!(input);
            source["allowSystemTrace"] = json!(false);
        }
        let capture = Capture::new(serde_json::from_value(source).unwrap()).unwrap();
        let mut reader = Reader::NativeTrace(Box::new(capture));
        let builds = crate::builds::Owners::default();
        let (_tx, rx) = tokio::sync::watch::channel(None);
        let mut bytes = vec![];
        loop {
            let chunk = reader
                .read(
                    &builds,
                    tmp.path(),
                    "acme",
                    bytes.len() as u64,
                    Duration::from_secs(50),
                    rx.clone(),
                )
                .await
                .unwrap();
            assert_eq!(chunk.reason.as_deref(), Some("device_disconnected"));
            let report = chunk.summary.unwrap();
            assert_eq!(
                report["xctraceRecording"],
                json!({
                    "endReason":"Device disconnected","durationSeconds":0.822219,"deviceDisconnected":true
                })
            );
            if mode == "import" {
                assert!(
                    report["gaps"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|gap| gap["section"] == "sysmon-process"
                            && gap["unavailable"] == "TIMEOUT")
                );
            }
            bytes.extend(chunk.bytes);
            if chunk.complete {
                break;
            }
        }
        assert_retained_activity_archive(&bytes, &witness);
        assert_eq!(
            reader.settle().await.unwrap()["cleanup"],
            "owned_host_files_removed"
        );
    }
    assert!(input.join("native.data").exists());
}

#[cfg(unix)]
fn assert_retained_activity_archive(bytes: &[u8], witness: &[u8]) {
    use super::super::analysis::decode;
    let index = decode::decode("native-trace-v1", bytes).unwrap();
    assert!(
        index
            .unsupported
            .iter()
            .any(|gap| gap.contains("device_disconnected"))
    );
    assert!(index.rows.iter().all(|row| row.kind != "metric"));
    let (artifact, encoding) = decode::native_trace::artifact_bytes(bytes).unwrap();
    assert_eq!(encoding, "tar.gz");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(artifact.as_slice()));
    let mut entry = archive.entries().unwrap().next().unwrap().unwrap();
    assert_eq!(
        entry.path().unwrap(),
        Path::new("capture.trace/native.data")
    );
    let mut retained = vec![];
    std::io::Read::read_to_end(&mut entry, &mut retained).unwrap();
    assert_eq!(retained.as_slice(), witness);
}

#[cfg(unix)]
fn executable(dir: &Path, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("native-peer");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_native_trace_xctrace_capture_keeps_template_target_attach_and_private_archive()
{
    let tmp = tempfile::tempdir().unwrap();
    let tool = executable(
        tmp.path(),
        r#"#!/bin/sh
[ "$1" = xctrace ] || exit 2
shift
case "$1" in
record)
  shift
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --template) [ "$2" = 'Time Profiler' ] || exit 3; shift 2 ;;
      --time-limit) [ "$2" = 100ms ] || exit 3; shift 2 ;;
      --attach) [ "$2" = 42 ] || exit 3; shift 2 ;;
      --device) [ "$2" = ACME ] || exit 3; shift 2 ;;
      --output) output="$2"; shift 2 ;;
      --no-prompt) shift ;;
      *) exit 5 ;;
    esac
  done
  mkdir "$output" || exit 6
  printf 'owned native bundle' > "$output/native.data"
  ;;
export)
  [ "$2" = --input ] || exit 7
  [ -f "$3/native.data" ] || exit 8
  case "$4" in
    --toc) printf '<trace-toc><run number="1"/></trace-toc>' ;;
    --xpath) printf '<trace-query-result><node/></trace-query-result>' ;;
    *) exit 9 ;;
  esac
  ;;
*) exit 10 ;;
esac
"#,
    );
    let o:Options=serde_json::from_value(json!({"backend":"xctrace","mode":"capture","targetId":"device:ACME","appId":"com.example.acme","processId":42,"allowAttach":true,"durationMs":100,"template":"Time Profiler","toolPath":tool})).unwrap();
    let mut capture = Capture::new(o).unwrap();
    let (_tx, rx) = tokio::sync::watch::channel(None);
    capture.control(rx, Duration::from_secs(15));
    let (bytes, done, _) = capture.read(0).await.unwrap();
    assert!(done);
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["artifact"]["encoding"], "tar");
    assert_eq!(v["sections"].as_array().unwrap().len(), 5);
    let decoded = super::super::analysis::decode::decode("native-trace-v1", &bytes).unwrap();
    assert_eq!(decoded.rows.len(), 1);
    assert!(
        decoded
            .unsupported
            .iter()
            .any(|s| s.contains("no exported rows"))
    );
    capture.settle().await;
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_native_trace_offline_tool_runs_once_retains_artifact_and_cleans_owned_files() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("input.pftrace");
    std::fs::write(&input, b"native-owned").unwrap();
    let tool = executable(
        tmp.path(),
        "#!/bin/sh\n[ \"$1\" = query ] || exit 2\n[ \"$2\" = --format ] || exit 2\n[ \"$3\" = csv ] || exit 2\ncase \"$5\" in\n  *'FROM trace_bounds'*) printf 'start_ts,end_ts\\n0,1000\\n' ;;\n  *) exit 4 ;;\nesac\n",
    );
    let mut o = options();
    o.path = Some(input.clone());
    o.tool_path = Some(tool);
    let mut c = Capture::new(o).unwrap();
    let owned = c.directory.clone();
    let (_tx, rx) = tokio::sync::watch::channel(None);
    c.control(rx, Duration::from_secs(10));
    let (bytes, done, _) = c.read(0).await.unwrap();
    assert!(done);
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        v["artifact"]["sha256"],
        crate::diagnostics::digest(b"native-owned")
    );
    std::fs::remove_file(&input).unwrap();
    assert_eq!(c.read(0).await.unwrap().0, bytes);
    let report = c.settle().await;
    assert_eq!(report["cleanup"], "owned_host_files_removed");
    assert!(!owned.exists());
    assert_eq!(v["sections"].as_array().unwrap().len(), 1);
    let gaps = v["report"]["gaps"].as_array().unwrap();
    assert_eq!(gaps.len(), 11);
    for name in ["actual_frames", "expected_frames"] {
        assert!(
            gaps.iter()
                .any(|gap| gap["section"] == name && gap["unavailable"].is_string())
        );
    }
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_native_trace_missing_converter_keeps_the_artifact_and_reports_every_section_gap()
 {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("input.pftrace");
    std::fs::write(&input, b"native-owned").unwrap();
    let mut o = options();
    o.path = Some(input);
    o.tool_path = Some(tmp.path().join("trace_processor_not_installed"));
    let mut c = Capture::new(o).unwrap();
    let (_tx, rx) = tokio::sync::watch::channel(None);
    c.control(rx, Duration::from_secs(10));
    let (bytes, done, _) = c.read(0).await.unwrap();
    assert!(done);
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        v["artifact"]["sha256"],
        crate::diagnostics::digest(b"native-owned")
    );
    assert!(v["sections"].as_array().unwrap().is_empty(), "{v}");
    // After the coverage note, every exported section names the missing tool.
    let sections: Vec<_> = v["report"]["gaps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|gap| gap.is_object())
        .collect();
    assert_eq!(sections.len(), 11, "{v}");
    assert!(
        sections
            .iter()
            .all(|gap| gap["unavailable"] == "PREREQUISITE_MISSING"),
        "{v}"
    );
    // The retained artifact still decodes; its missing sections stay explicit.
    let decoded = super::super::analysis::decode::decode("native-trace-v1", &bytes).unwrap();
    assert!(!decoded.unsupported.is_empty());
    assert_eq!(c.settle().await["cleanup"], "owned_host_files_removed");
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_memory_apple_import_exports_only_advertised_tables_without_recording() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("owned.trace");
    std::fs::create_dir(&input).unwrap();
    std::fs::write(input.join("native.data"), b"owned allocation trace").unwrap();
    let tool = executable(
        tmp.path(),
        r#"#!/bin/sh
[ "$1" = xctrace ] && [ "$2" = export ] && [ "$3" = --input ] || exit 2
[ -f "$4/native.data" ] || exit 3
case "$5" in
  --toc) printf '<trace-toc><run number="1"><data><table schema="heap-allocations"/><table schema="private-gui-memory-graph"/></data></run></trace-toc>' ;;
  --xpath) case "$6" in
    *'table[@schema="heap-allocations"]') printf '<trace-query-result><row><size-in-bytes>32</size-in-bytes></row></trace-query-result>' ;;
    *) exit 4 ;;
  esac ;;
  *) exit 5 ;;
esac
"#,
    );
    let o:Options=serde_json::from_value(json!({"backend":"xctrace","mode":"import","targetId":"device:ACME","appId":"com.example.acme","path":input,"toolPath":tool,"template":"Allocations"})).unwrap();
    let mut c = Capture::new(o).unwrap();
    let (_tx, rx) = tokio::sync::watch::channel(None);
    c.control(rx, Duration::from_secs(10));
    let (bytes, done, report) = c.read(0).await.unwrap();
    assert!(done);
    assert_eq!(
        report["memoryEffects"]["allocationRecordingRequested"],
        false
    );
    assert_eq!(report["memoryEffects"]["forcedGcRequested"], false);
    let packet: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(packet["sections"].as_array().unwrap().len(), 2);
    let index = super::super::analysis::decode::decode("native-trace-v1", &bytes).unwrap();
    assert_eq!(
        index
            .rows
            .iter()
            .find(|r| r.kind == "allocation")
            .unwrap()
            .value,
        Some(32.0)
    );
    c.settle().await;
    assert!(input.join("native.data").exists());
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_native_trace_host_identity_uses_kernel_birth_without_profiling() {
    let o:Options=serde_json::from_value(json!({"backend":"stack_sample","mode":"capture","targetId":"host:macos","appId":"com.example.acme","processId":std::process::id(),"allowAttach":true})).unwrap();
    let mut c = Capture::new(o).unwrap();
    let (_tx, rx) = tokio::sync::watch::channel(None);
    c.control(rx, Duration::from_secs(10));
    let identity = c.host_identity().await.unwrap();
    assert_eq!(
        identity["kernelBirth"],
        json!(process::start_time(std::process::id()).unwrap())
    );
    assert_eq!(identity["appAssociation"], "caller_declared");
    assert!(c.bytes.is_none());
    c.settle().await;
}
#[cfg(target_os = "macos")]
#[tokio::test]
async fn diagnostic_native_sample_defers_symbols_and_owns_the_report_file() {
    let tmp = tempfile::tempdir().unwrap();
    let tool = executable(
        tmp.path(),
        r#"#!/bin/sh
[ "$#" = 5 ] || exit 2
[ "$2" = 1.000 ] || exit 3
[ "$3" = 10 ] || exit 4
[ "$4" = -file ] || exit 5
case "$5" in */testrun-native-*/capture.native) ;; *) exit 6 ;; esac
printf 'Acme owned sample report\n' > "$5"
printf 'tool progress is not the report\n'
"#,
    );
    let o:Options=serde_json::from_value(json!({"backend":"stack_sample","mode":"capture","targetId":"host:macos","appId":"com.example.acme","processId":std::process::id(),"durationMs":1000,"allowAttach":true,"toolPath":tool})).unwrap();
    let mut capture = Capture::new(o).unwrap();
    let directory = capture.directory.clone();
    let (_tx, rx) = tokio::sync::watch::channel(None);
    capture.control(rx, Duration::from_secs(10));
    let (bytes, done, report) = capture.read(0).await.unwrap();
    assert!(done);
    let packet: Value = serde_json::from_slice(&bytes).unwrap();
    let artifact = base64::engine::general_purpose::STANDARD
        .decode(packet["artifact"]["base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(artifact, b"Acme owned sample report\n");
    assert_eq!(packet["sections"][0]["text"], "Acme owned sample report\n");
    assert_eq!(report["artifactSha256"], super::super::digest(&artifact));
    assert_eq!(
        report["identity"]["kernelBirth"],
        report["identity"]["after"]["kernelBirth"]
    );
    assert_eq!(
        capture.settle().await["cleanup"],
        "owned_host_files_removed"
    );
    assert!(!directory.exists());
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn diagnostic_native_sample_allows_bounded_postprocessing() {
    let tmp = tempfile::tempdir().unwrap();
    let tool = executable(
        tmp.path(),
        r#"#!/bin/sh
[ "$2" = 1.000 ] || exit 2
[ "$4" = -file ] || exit 3
sleep 6
printf 'Acme delayed symbol report\n' > "$5"
"#,
    );
    let options: Options = serde_json::from_value(json!({"backend":"stack_sample","mode":"capture","targetId":"host:macos","appId":"com.example.acme","processId":std::process::id(),"durationMs":1000,"allowAttach":true,"toolPath":tool})).unwrap();
    let mut capture = Capture::new(options).unwrap();
    let (_tx, rx) = tokio::sync::watch::channel(None);
    capture.control(rx, Duration::from_secs(10));
    let (bytes, done, _) = capture.read(0).await.unwrap();
    assert!(done);
    let packet: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        packet["sections"][0]["text"],
        "Acme delayed symbol report\n"
    );
    assert_eq!(
        capture.settle().await["cleanup"],
        "owned_host_files_removed"
    );
    let (_tx, rx) = tokio::sync::watch::channel(None);
    capture.control(rx, Duration::ZERO);
    assert_eq!(
        capture.budget(Duration::from_secs(14)).unwrap_err().code,
        "TIMEOUT"
    );
    let (tx, rx) = tokio::sync::watch::channel(None);
    capture.control(rx, Duration::from_secs(1));
    assert!(capture.budget(Duration::from_secs(14)).unwrap() <= Duration::from_secs(1));
    tx.send_replace(Some("stop".into()));
    assert_eq!(
        capture.budget(Duration::from_secs(14)).unwrap_err().code,
        "CANCELLED"
    );
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_native_trace_android_capture_pins_transport_birth_and_finite_owned_recording() {
    let tmp = tempfile::tempdir().unwrap();
    let adb_dir = tmp.path().join("adb");
    let tool_dir = tmp.path().join("tools");
    std::fs::create_dir(&adb_dir).unwrap();
    std::fs::create_dir(&tool_dir).unwrap();
    let adb = executable(
        &adb_dir,
        include_str!("../../../tests/fixtures/diagnostic-native-adb.sh"),
    );
    let tool = executable(
        &tool_dir,
        "#!/bin/sh\ncase \"$1\" in query) case \"$5\" in *'FROM trace_bounds'*) printf 'start_ts,end_ts\\n0,100\\n' ;; *) exit 4 ;; esac ;; report-sample) printf 'lost_situation:\\n  lost_count: 0\\n' ;; *) exit 5 ;; esac\n",
    );
    for (backend, gone) in [
        ("perfetto", false),
        ("simpleperf", false),
        ("perfetto", true),
    ] {
        if gone {
            let _ = std::fs::remove_file(adb.with_file_name("native-peer.recorded"));
            std::fs::write(adb.with_file_name("native-peer.exit-after"), b"yes").unwrap();
        }
        let o:Options=serde_json::from_value(json!({"backend":backend,"mode":"capture","targetId":"android:ACME","appId":"com.example.acme","processId":42,"durationMs":100,"allowSystemTrace":true,"toolPath":tool})).unwrap();
        let mut c = Capture::new(o).unwrap();
        c.adb.adb = adb.clone();
        let (_tx, rx) = tokio::sync::watch::channel(None);
        c.control(rx, Duration::from_secs(30));
        let (bytes, done, report) = c.read(0).await.unwrap();
        assert!(done);
        assert_eq!(report["identity"]["startTicks"], "100");
        if gone {
            assert!(report["identity"]["after"]["unavailable"].is_string());
        } else {
            assert_eq!(report["identity"]["after"]["startTicks"], "100");
        }
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["backend"], backend);
        let index = super::super::analysis::decode::decode("native-trace-v1", &bytes).unwrap();
        assert!(!index.rows.is_empty() || backend == "simpleperf");
        let result = c.settle().await;
        if backend == "simpleperf" {
            assert!(
                result["remoteFileCleanup"]
                    .as_str()
                    .unwrap()
                    .starts_with("owned_path_unlinked")
            );
        }
    }
    let config = std::fs::read_to_string(adb.with_file_name("native-peer.config")).unwrap();
    assert!(config.contains("duration_ms: 100"));
    assert!(config.contains("fill_policy: DISCARD"));
    assert!(config.contains("android.surfaceflinger.frametimeline"));
    assert!(!config.contains("perf_event"));
    let calls = std::fs::read_to_string(adb.with_file_name("native-peer.calls")).unwrap();
    assert!(!calls.contains(" root"));
    assert!(!calls.contains("kill"));
}

#[cfg(unix)]
#[tokio::test]
async fn diagnostic_native_trace_perfetto_waits_for_device_start_and_keeps_stdout_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let adb_dir = tmp.path().join("adb");
    let tool_dir = tmp.path().join("tools");
    std::fs::create_dir(&adb_dir).unwrap();
    std::fs::create_dir(&tool_dir).unwrap();
    let adb = executable(
        &adb_dir,
        include_str!("../../../tests/fixtures/diagnostic-native-adb.sh"),
    );
    let tool = executable(&tool_dir, "#!/bin/sh\nexit 4\n");
    let source = json!({
        "backend":"perfetto","mode":"capture","targetId":"android:ACME","appId":"com.example.acme",
        "durationMs":3000,"allowSystemTrace":true,"toolPath":tool,
        "perfettoBufferKib":3840,"perfettoFillPolicy":"ring_buffer","waitForPerfettoStart":true
    });
    for refuse in [false, true] {
        if refuse {
            std::fs::write(adb.with_file_name("native-peer.refuse-start"), b"").unwrap();
        }
        let mut capture = Capture::new(serde_json::from_value(source.clone()).unwrap()).unwrap();
        capture.adb.adb = adb.clone();
        let (_tx, rx) = tokio::sync::watch::channel(None);
        capture.control(rx, Duration::from_secs(12));
        let started = capture.perfetto_start().unwrap();
        let request = capture.read(0);
        let check_start = async {
            let signal = started.await;
            if refuse {
                assert!(signal.is_err());
            } else {
                assert!(signal.unwrap() > Instant::now());
                assert!(!adb.with_file_name("native-peer.recorded").exists());
                let config =
                    std::fs::read_to_string(adb.with_file_name("native-peer.config")).unwrap();
                assert!(config.contains("size_kb: 3840 fill_policy: RING_BUFFER"));
                std::fs::write(adb.with_file_name("native-peer.release"), b"").unwrap();
            }
        };
        let (result, ()) = tokio::join!(request, check_start);
        if refuse {
            assert_eq!(result.unwrap_err().code, "PLATFORM_FAILED");
        } else {
            let (bytes, _, report) = result.unwrap();
            let (raw, _) =
                super::super::analysis::decode::native_trace::artifact_bytes(&bytes).unwrap();
            assert_eq!(raw, b"owned perfetto bytes");
            assert_eq!(
                report["perfettoBuffer"],
                json!({"sizeKib":3840,"fillPolicy":"ring_buffer"})
            );
        }
        capture.settle().await;
    }
    let calls = std::fs::read_to_string(adb.with_file_name("native-peer.calls")).unwrap();
    assert_eq!(calls.matches(" --notify-fd 3").count(), 2);
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_native_trace_cancelled_tool_is_not_replayed_and_input_links_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("input");
    std::fs::write(&input, b"trace").unwrap();
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink(&input, &link).unwrap();
    assert!(files::read(&link, MAX_ARTIFACT).is_err());
    let tool = executable(tmp.path(), "#!/bin/sh\nexec /bin/sleep 10\n");
    let mut o = options();
    o.path = Some(input);
    o.tool_path = Some(tool);
    let mut c = Capture::new(o).unwrap();
    let (tx, rx) = tokio::sync::watch::channel(None);
    c.control(rx, Duration::from_secs(15));
    {
        let request = c.read(0);
        tokio::pin!(request);
        tokio::select! {r=&mut request=>panic!("unexpected {r:?}"),_=tokio::time::sleep(Duration::from_millis(30))=>{tx.send_replace(Some("stop".into()));}}
        assert_eq!(request.await.unwrap_err().code, "CANCELLED");
    }
    c.settle().await;
}
