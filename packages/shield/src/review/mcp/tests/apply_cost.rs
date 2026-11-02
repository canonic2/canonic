//! Real stdio apply with optional read-only public pack sources, never the caller's state.
use super::*;
use crate::{diagnostics, mcp};
use clap::Parser as _;
use std::{fs, sync::Arc, time::Instant};

fn pack_fixture() -> Bundle {
    let mut selections = Vec::new();
    let mut packs = Vec::new();
    if let Some(project) = std::env::var_os("SHIELD_APPLY_PROFILE_PROJECT") {
        // Read immutable artifacts only. No settings, rules, history, integration,
        // credentials or project identity are copied into the temporary project.
        let store = Store::open(Path::new(&project)).unwrap();
        let state = store.read_checkpoint().unwrap();
        let candidate = store
            .captured_candidate(&state.candidates["project"])
            .unwrap();
        for bytes in candidate["packs"].as_array().unwrap() {
            let bytes = bytes.as_str().unwrap();
            let pack: Value = crate::policy::parse(bytes).unwrap();
            selections.push(json!({"id":pack["id"],"version":pack["version"],"profile":"recommended","path":format!("cache/fixture-{}.yml", packs.len())}));
            packs.push(bytes.to_owned());
        }
    }
    Bundle::capture(
        &json!({"schema_version":1,"packs":selections}).to_string(),
        &packs,
        None,
    )
    .unwrap()
}

#[test]
fn apply_request_trace_explains_work_and_preserves_exact_local_activation() {
    let (root, mut context, _) = fixture_with(&pack_fixture());
    context.agent = Some("codex".into());
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = Arc::new(diagnostics::Log::open(&cli));
    let path = log.path().unwrap().to_owned();
    let server = mcp::Server::for_review(context.clone())
        .unwrap()
        .with_log(log.clone());
    let transport = mcp::tests::Transport::start(server);
    for message in mcp::tests::initialize(&json!({}), "codex") {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    let mut timings = Vec::new();
    for index in 0..3 {
        let changes: Vec<_> = [("gate", vec!["doctor", "--json"]), ("box", vec!["recipe", "check", "--json"])]
            .into_iter().map(|(executable, arguments)| json!({"kind":"rule_add","layer":"local","rule":{
                "id":format!("acme-{executable}-{index}"),"decision":"allow","conditions":{"domain":"argv","executable":executable,
                "arguments":{"kind":"exact","values":arguments},"cwd":{"exact":{"root":"project"}}}}})).collect();
        let proposal = transport.call(
            json!(10 + index * 2),
            "policy_propose",
            &json!({"changes":changes}),
        );
        let value = &proposal["result"]["structuredContent"];
        assert!(value["error"].is_null(), "{proposal}");
        let started = Instant::now();
        let applied = transport.call(
            json!(11 + index * 2),
            "policy_apply",
            &value["data"]["apply_arguments"],
        );
        timings.push(started.elapsed().as_micros());
        assert_eq!(
            applied["result"]["structuredContent"]["data"]["status"], "applied",
            "{applied}"
        );
    }
    // Timings must become visible while a retained connection remains alive.
    let deadline = Instant::now() + std::time::Duration::from_millis(500);
    let records: Vec<Value> = loop {
        let live = fs::read_to_string(&path).unwrap();
        // The asynchronous writer may be partway through its final record.
        // Completed records must still be valid JSON; do not silently skip corruption.
        let records: Vec<Value> = live
            .split_inclusive('\n')
            .filter(|line| line.ends_with('\n'))
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        if records
            .iter()
            .filter(|record| record["event"] == "request_end")
            .count()
            >= 6
        {
            break records;
        }
        assert!(
            Instant::now() < deadline,
            "completed request traces were not published"
        );
        std::thread::yield_now();
    };
    let (_, ended) = transport.close();
    assert!(ended.is_ok());
    drop(log);
    let local: Value = crate::policy::parse(
        &fs::read_to_string(root.path().join(".shield/rules.local.yml")).unwrap(),
    )
    .unwrap();
    assert_eq!(local["rules"].as_array().unwrap().len(), 6);
    assert_apply_traces(&records, &timings);
}

fn assert_apply_traces(records: &[Value], timings: &[u128]) {
    let starts: Vec<_> = records
        .iter()
        .filter(|v| v["event"] == "request_start" && v["fields"]["operation"] == "policy_apply")
        .collect();
    assert_eq!(starts.len(), 3);
    let summaries: Vec<_> = starts
        .iter()
        .map(|start| {
            records
                .iter()
                .find(|v| {
                    v["event"] == "request_end"
                        && v["fields"]["trace_id"] == start["fields"]["trace_id"]
                })
                .unwrap()
        })
        .collect();
    assert!(
        summaries
            .iter()
            .all(|v| v["fields"]["outcome"] == "success")
    );
    // A connection already prepared both immutable plans during proposal review.
    // Apply verifies only current and selected sources, independently of old proposals.
    for summary in &summaries {
        assert_eq!(
            summary["fields"]["counts"]["policy.preparations"], 0,
            "{summary}"
        );
        assert_eq!(
            summary["fields"]["counts"]["source.hydrated"], 2,
            "{summary}"
        );
    }
    assert!(!records.iter().any(|v| {
        v["event"] == "request_span"
            && starts
                .iter()
                .any(|start| start["fields"]["trace_id"] == v["fields"]["trace_id"])
            && ["history.read", "state.hydrate_all"]
                .contains(&v["fields"]["stage"].as_str().unwrap_or(""))
    }));
    eprintln!("apply round_trip_us={timings:?}");
    for summary in summaries {
        eprintln!("apply work={}", summary["fields"]);
    }
    let mut stages = BTreeMap::<String, (u64, u64)>::new();
    for record in records.iter().filter(|v| {
        v["event"] == "request_span"
            && starts
                .iter()
                .any(|start| start["fields"]["trace_id"] == v["fields"]["trace_id"])
    }) {
        let fields = &record["fields"];
        let entry = stages
            .entry(fields["stage"].as_str().unwrap().to_owned())
            .or_default();
        entry.0 += 1;
        entry.1 += fields["inclusive_us"].as_u64().unwrap();
    }
    eprintln!("apply stages (calls,inclusive_us)={stages:?}");
}
