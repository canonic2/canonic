//! Explicit collector-volume campaign. Native launch and model costs are separate.
use super::*;
use crate::review::{self, observations};
use clap::Parser as _;
use std::time::Instant;

fn distribution(samples: &[f64]) -> Value {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let rank = |percent: usize| sorted[(sorted.len() * percent).div_ceil(100) - 1];
    json!({"samples":sorted.len(),"p50_ms":rank(50),"p95_ms":rank(95),
        "p99_ms":rank(99),"maximum_ms":sorted.last()})
}

#[test]
#[ignore = "explicit history-volume campaign; use make history-volume"]
#[expect(
    clippy::too_many_lines,
    reason = "one explicit campaign keeps its measured boundary and post-run evidence together"
)]
fn history_volume_campaign() {
    let count: usize = std::env::var("SHIELD_HISTORY_VOLUME_COUNT")
        .unwrap()
        .parse()
        .unwrap();
    assert!((500..=500_000).contains(&count) && count.is_multiple_of(10));
    let output = PathBuf::from(std::env::var_os("SHIELD_HISTORY_VOLUME_REPORT_DIR").unwrap());
    fs::create_dir_all(&output).unwrap();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let stem = format!("{stamp}-{}-{count}", std::process::id());
    let report_path = output.join(format!("{stem}.json"));
    let binary_path = output.join(format!("{stem}.binary"));
    let binary = std::env::current_exe().unwrap();
    fs::copy(&binary, &binary_path).unwrap();
    let binary_digest = crate::policy::fingerprint(&fs::read(&binary_path).unwrap());

    let project = assert_fs::TempDir::new().unwrap();
    let rules = json!({"schema_version":1,"kind":"custom",
        "capabilities":["structured-rules-v1","argument-rules-v1"],
        "rules":[{"id":"volume-fixture","decision":"allow","conditions":{
            "domain":"argv","executable":"acme","arguments":{"kind":"any"}}}]})
    .to_string();
    let bundle = crate::configuration::Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    let mut context = review::mcp::initialize_fixture(project.path(), &bundle);
    context.surface = "hook".into();
    context.source = "host".into();
    context.agent = Some("acme-volume-fixture".into());
    context.session_id = Some("acme-volume-session".into());
    context.cwd = Some(context.project_root.clone());
    review::history_settings(&context, None, Some(10_000), false, &|| Ok(())).unwrap();
    let store = Store::open(project.path()).unwrap();
    let authority_before = store.checkpoint_bytes().unwrap();

    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
    let trace = log.request("lab", "history-volume", None, None).unwrap();
    let mut latest_request = None;
    let mut latest_operation = Value::Null;
    let mut latest_completion = Value::Null;
    let mut samples = Vec::with_capacity(count);
    let began = Instant::now();
    {
        let _scope = trace.enter();
        for index in 0..count {
            let started = Instant::now();
            match index % 10 {
                0 | 2 | 4 | 6 => {
                    context.host_event_name = Some("PreToolUse".into());
                    context.tool_use_id = Some(format!("call-{index}"));
                    latest_operation =
                        json!({"kind":"argv","executable":"acme","arguments":[index.to_string()]});
                    let receipt = observations::decision(&context, json!({"operation":latest_operation,
                        "host_decision":"allow","evaluation":{"decision":"allow"},"disposition":"allowed"})).unwrap();
                    latest_request = receipt["request_id"].as_str().map(str::to_owned);
                }
                1 | 3 | 5 | 7 => {
                    context.host_event_name = Some("PostToolUse".into());
                    latest_completion = observations::completion(
                        &context,
                        &review::EventInput {
                            delivery_id: format!("delivery-{index}"),
                            request_id: latest_request.clone(),
                            status: "completed".into(),
                        },
                        &latest_operation,
                    )
                    .unwrap();
                    assert!(latest_completion["request_id"].is_null());
                }
                _ => {
                    context.host_event_name = Some("PostToolUse".into());
                    context.tool_use_id = Some(format!("unlinked-call-{index}"));
                    observations::completion(&context, &review::EventInput {
                        delivery_id: format!("unlinked-{index}"), request_id: None, status: "completed".into(),
                    }, &json!({"kind":"argv","executable":"acme","arguments":[format!("unlinked-{index}")]})).unwrap();
                }
            }
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let elapsed = began.elapsed().as_secs_f64();
    let counters: serde_json::Map<String, Value> = [
        "policy.preparations",
        "source.hydrated",
        "source.read_bytes",
        "state.read_bytes",
        "state.checkpoint_reads",
        "storage.sync_calls",
        "history.captures",
        "history.directory_entries",
        "history.read_bytes",
        "history.decoded_records",
        "history.tail_read_bytes",
        "history.rotation_read_bytes",
        "history.appended_bytes",
        "history.appended_records",
        "history.rotations",
        "history.control_rewrites",
        "history.correlations",
    ]
    .into_iter()
    .map(|name| (name.into(), json!(trace.recorded_count(name))))
    .collect();
    trace.finish("measured");
    for name in [
        "policy.preparations",
        "source.hydrated",
        "source.read_bytes",
        "history.captures",
        "history.read_bytes",
        "history.decoded_records",
        "history.control_rewrites",
        "history.correlations",
    ] {
        assert_eq!(counters[name], 0, "callback work: {name}");
    }
    assert_eq!(counters["history.appended_records"], count);
    assert_eq!(store.checkpoint_bytes().unwrap(), authority_before);

    // Inspection starts only after every timed callback and its scope ended.
    let retained = store.read_history().unwrap();
    assert!(retained.history_available);
    let decisions = retained
        .events
        .iter()
        .filter(|row| row["kind"] == "decision")
        .count();
    assert_eq!(decisions, (count / 10 * 4).min(10_000));
    assert!(
        retained
            .events
            .iter()
            .any(|row| row["event_id"] == latest_completion["event_id"]
                && row["links"]["request_id"] == json!(latest_request))
    );
    let unlinked: Vec<_> = retained
        .events
        .iter()
        .filter(|row| row["kind"] == "completion" && row["links"]["request_id"].is_null())
        .collect();
    assert_eq!(unlinked.len(), (count / 10 * 2).min(128));
    assert!(crate::json::fits(&unlinked, 1024 * 1024));
    let mut bytes = 0_u64;
    let mut files = 0;
    for entry in fs::read_dir(&store.directory).unwrap() {
        let entry = entry.unwrap();
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("history-") || name.starts_with(".history-"))
        {
            if entry.file_type().unwrap().is_dir() {
                assert_eq!(entry.file_name(), "history-readers");
                bytes += resources::reader_bytes(&store).unwrap();
                files += fs::read_dir(entry.path()).unwrap().count();
                continue;
            }
            bytes += entry.metadata().unwrap().len();
            files += 1;
        }
    }
    assert!(bytes <= STORE_LIMIT as u64);
    let width = samples.len().min(1000);
    let report = json!({"schema_version":1,"kind":"history-collector-volume",
        "scope":"In-process shipping observation collector; synthetic host decisions, no native transport, model or policy evaluation.",
        "profile":if cfg!(debug_assertions) {"debug-assertions"} else {"optimized"},
        "engine":env!("CARGO_PKG_VERSION"),"binary_sha256":binary_digest,"frozen_binary":binary_path.file_name().unwrap().to_str(),
        "host":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"logical_cpus":std::thread::available_parallelism().map(usize::from).ok()},
        "invocations":count,"decisions":count / 10 * 4,"correlated_completion_inputs":count / 10 * 4,
        "uncorrelated_completion_inputs":count / 10 * 2,"joined_history_reads_during_callbacks":0,
        "elapsed_seconds":elapsed,"latency":distribution(&samples),
        "first_window":distribution(&samples[..width]),"last_window":distribution(&samples[samples.len()-width..]),
        "callback_counters":counters,"journal":{"logical_bytes":bytes,"files":files,"cap_bytes":STORE_LIMIT,
            "retained_decisions":decisions,"retained_unlinked_completions":unlinked.len()},
        "invariants":"passed","release_acceptance":"not established by collector-only measurement"});
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&report_path)
        .unwrap();
    serde_json::to_writer_pretty(&mut file, &report).unwrap();
    println!("History volume report: {}", report_path.display());
}
