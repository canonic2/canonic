//! Fresh/aged request costs through the retained MCP transport and CLI owners.
use super::*;
use crate::{diagnostics, mcp};
use clap::Parser as _;
use std::{
    fs,
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn reconnected_apply_reuses_active_plan_and_prepares_only_the_selected_candidate() {
    let (_root, mut context, connection) = fixture();
    context.agent = Some("codex".into());
    let proposed = proposal(&context, &connection, "acme-cold-apply");
    context.connection_id = Some("reconnected".into());
    let reconnected = Connection::enrolled(&context).unwrap();
    let logs = assert_fs::TempDir::new().unwrap();
    let cli = crate::cli::Cli::parse_from([
        "shield",
        "--debug-log-dir",
        logs.path().to_str().unwrap(),
        "status",
    ]);
    let log = Arc::new(diagnostics::Log::open(&cli));
    let path = log.path().unwrap().to_owned();
    let request = log
        .request("mcp_owner", "policy_apply", None, Some("reconnected"))
        .unwrap();
    let scope = request.enter();
    let applied =
        super::application::apply(&context, &reconnected, &proposed["data"]["apply_arguments"]);
    assert_eq!(applied["data"]["status"], "applied");
    drop(scope);
    request.finish("success");
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let text = fs::read_to_string(&path).unwrap();
        let end = text
            .split_inclusive('\n')
            .filter(|line| line.ends_with('\n'))
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|r| r["event"] == "request_end");
        if let Some(end) = end {
            assert_eq!(end["fields"]["counts"]["policy.preparations"], 1, "{end}");
            assert_eq!(end["fields"]["counts"]["source.hydrated"], 2, "{end}");
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "One transport and trace lifetime owns all fresh, aged and registered measurements and cleanup"
)]
fn ordinary_request_sweep_tracks_fresh_and_aged_work() {
    let (root, mut context, connection) = fixture_with(&super::apply_cost::pack_fixture());
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
    let transport = mcp::tests::Transport::start(
        mcp::Server::for_review(context.clone())
            .unwrap()
            .with_log(log.clone()),
    );
    for message in mcp::tests::initialize(&json!({}), "codex") {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    let mut id = 10;
    let mut summaries = Vec::new();
    for (age, retained) in [("fresh", 0), ("aged", 8), ("registered", 0)] {
        if age == "registered" {
            // A stale registration exercises prefix and connection diagnostics
            // without launching a host or writing its protected settings.
            let binary = root.path().join("acme-shield");
            fs::write(&binary, b"Acme diagnostic binary").unwrap();
            Store::open(root.path())
                .unwrap()
                .update_checkpoint(&context, &|| Ok(()), |state| {
                    state.operator["integration"] = json!({"binary":binary,
                    "binary_digest":crate::policy::fingerprint(b"Acme diagnostic binary"),
                    "version":env!("CARGO_PKG_VERSION"),"entry":{},
                    "agents":{"codex":{"groups":{},"mcp":null}},"ask_fallback":{}});
                    Ok(Value::Null)
                })
                .unwrap();
        }
        for index in 0..retained {
            proposal(&context, &connection, &format!("acme-retained-{index}"));
        }
        let mut selected = Value::Null;
        for tool in [
            "policy_propose",
            "policy_show",
            "pack_list",
            "registry_list",
            "request_list",
            "rule_list",
            "status",
            "doctor",
            "selected",
        ] {
            let (name, args) = match tool {
                "policy_propose" => (
                    tool,
                    json!({"idempotency_key":format!("acme-{age}"),"change":{"kind":"rule_add","rule":rule(&format!("acme-{age}"),"allow")}}),
                ),
                "selected" => ("policy_show", json!({"proposal_id":selected})),
                _ => (tool, json!({})),
            };
            let started = Instant::now();
            let reply = transport.call(json!(id), name, &args);
            let elapsed = started.elapsed().as_micros();
            assert!(
                reply["result"]["structuredContent"]["error"].is_null(),
                "{name}: {reply}"
            );
            if tool == "policy_propose" {
                selected = reply["result"]["structuredContent"]["data"]["proposal_id"].clone();
            }
            if age == "registered" && tool == "status" {
                let coverage = &reply["result"]["structuredContent"]["data"]["coverage"];
                assert_eq!(coverage["installed_integrations"][0]["agent"], "codex");
                assert_eq!(coverage["repair_needed"], true);
                assert!(
                    coverage["diagnostics"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|row| { row == "owned native hook configuration is missing" }),
                    "{coverage}"
                );
            }
            summaries.push((age, tool, id, elapsed));
            id += 1;
        }
        for (name, run) in [
            (
                "cli_policy",
                review::project::policy as fn(&Context) -> Result<Value, Error>,
            ),
            ("cli_pack_list", review::packs::list),
            ("native_tool_lookup", |_: &Context| {
                assert!(review::mcp::is_tool("policy_show"));
                Ok(Value::Null)
            }),
            ("tool_summaries", |_: &Context| {
                assert_ne!(review::mcp::tool_summaries(), []);
                Ok(Value::Null)
            }),
            ("cli_rules", |context: &Context| {
                review::inspection::rules(context, None)
            }),
        ] {
            let trace = log
                .request("cli_owner", name, Some(&json!(id)), Some("sweep"))
                .unwrap();
            let scope = trace.enter();
            let started = Instant::now();
            run(&context).unwrap();
            summaries.push((age, name, id, started.elapsed().as_micros()));
            drop(scope);
            trace.finish("success");
            id += 1;
        }
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    let records: Vec<Value> = loop {
        let text = fs::read_to_string(&path).unwrap();
        let records: Vec<Value> = text
            .split_inclusive('\n')
            .filter(|line| line.ends_with('\n'))
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        if records
            .iter()
            .filter(|r| r["event"] == "request_end")
            .count()
            == summaries.len()
        {
            break records;
        }
        assert!(Instant::now() < deadline, "request trace missing");
        std::thread::yield_now();
    };
    assert!(transport.close().1.is_ok());
    for (age, tool, id, elapsed) in summaries {
        let start = records
            .iter()
            .find(|r| r["event"] == "request_start" && r["fields"]["request_id"] == id)
            .unwrap();
        let end = records
            .iter()
            .find(|r| {
                r["event"] == "request_end"
                    && r["fields"]["trace_id"] == start["fields"]["trace_id"]
            })
            .unwrap();
        let history = records
            .iter()
            .filter(|r| {
                r["event"] == "request_span"
                    && r["fields"]["trace_id"] == start["fields"]["trace_id"]
                    && r["fields"]["stage"] == "history.read"
            })
            .count();
        let (preparations, sources, history_reads) = match tool {
            "policy_propose" => (1, 1, 0),
            "pack_list" | "registry_list" | "request_list" | "native_tool_lookup"
            | "tool_summaries" | "cli_pack_list" => (0, 0, 0),
            "status" => (0, 0, 1),
            "doctor" => (1, 1, 1),
            "selected" => (0, 2, 0),
            // Registration changes metadata, while the active policy's retained
            // handle can reuse its already captured immutable source.
            "rule_list" if age == "registered" => (0, 0, 0),
            _ => (0, 1, 0),
        };
        assert_eq!(
            end["fields"]["counts"]["policy.preparations"], preparations,
            "{age} {tool}: {end}"
        );
        assert_eq!(
            end["fields"]["counts"]["source.hydrated"], sources,
            "{age} {tool}: {end}"
        );
        assert_eq!(history, history_reads, "{age} {tool}");
        if matches!(tool, "request_list" | "status" | "doctor") {
            let reads = records
                .iter()
                .filter(|record| {
                    record["event"] == "request_span"
                        && record["fields"]["trace_id"] == start["fields"]["trace_id"]
                        && record["fields"]["stage"] == "state.checkpoint"
                })
                .count();
            let expected = match tool {
                "status" => 3,
                "doctor" => 4,
                _ => 1,
            };
            assert_eq!(reads, expected, "{age} {tool}: authority checkpoint budget");
        }
        if matches!(tool, "status" | "doctor") {
            let loads = records
                .iter()
                .filter(|record| {
                    record["event"] == "request_span"
                        && record["fields"]["trace_id"] == start["fields"]["trace_id"]
                        && record["fields"]["stage"] == "cache.load"
                })
                .count();
            assert_eq!(
                loads,
                if tool == "doctor" { 2 } else { 1 },
                "{age} {tool}: active plan load budget"
            );
        }
        if matches!(
            tool,
            "native_tool_lookup" | "tool_summaries" | "status" | "doctor"
        ) {
            assert_eq!(
                end["fields"]["counts"]["mcp.schema_builds"]
                    .as_u64()
                    .unwrap_or(0),
                0,
                "{age}: metadata queries must not construct full schemas"
            );
        }
        let admissions = if tool == "policy_propose" {
            // The changed registration also invalidates the active handle.
            if age == "registered" { 2 } else { 1 }
        } else {
            0
        };
        assert_eq!(
            end["fields"]["counts"]["handle.admission_encodings"]
                .as_u64()
                .unwrap_or(0),
            admissions,
            "cached handles must skip admission encoding: {age} {tool}"
        );
        assert_eq!(
            end["fields"]["counts"]["policy.posting_validations"]
                .as_u64()
                .unwrap_or(0),
            0,
            "ordinary requests must not revalidate just-encoded postings: {age} {tool}"
        );
        if tool == "policy_propose" {
            assert!(
                end["fields"]["counts"]["policy.layout_checks"]
                    .as_u64()
                    .unwrap_or(0)
                    > 0,
                "fresh plans must still check their encoded layout"
            );
            assert_eq!(
                end["fields"]["counts"]["pattern.compiled"]
                    .as_u64()
                    .unwrap_or(0),
                end["fields"]["counts"]["pattern.reused"]
                    .as_u64()
                    .unwrap_or(0),
                "an unchanged pack's patterns are compiled once and reused for the edit: {age}",
            );
        }
        let captures = if tool == "policy_propose" {
            2
        } else {
            u64::from(tool == "doctor")
        };
        assert_eq!(
            end["fields"]["counts"]["policy.source_captures"]
                .as_u64()
                .unwrap_or(0),
            captures,
            "{age} {tool}: {end}"
        );
        eprintln!(
            "sweep {age} {tool} round_trip_us={elapsed} history_loads={history} counts={}",
            end["fields"]["counts"]
        );
        for span in records.iter().filter(|r| {
            r["event"] == "request_span" && r["fields"]["trace_id"] == start["fields"]["trace_id"]
        }) {
            eprintln!("sweep_span {age} {tool} {}", span["fields"]);
        }
    }
}

#[test]
fn unrelated_unavailable_source_does_not_block_selected_reads_or_new_proposals() {
    let (root, mut context, connection) = fixture();
    context.agent = Some("codex".into());
    let old = proposal(&context, &connection, "acme-unrelated");
    let selected = proposal(&context, &connection, "acme-selected");
    let store = Store::open(root.path()).unwrap();
    let state = store.read_checkpoint().unwrap();
    let unrelated = state
        .proposals
        .iter()
        .find(|p| p.id == old["data"]["proposal_id"])
        .unwrap();
    fs::remove_file(root.path().join(".shield/review/sources").join(format!(
        "{}.json",
        unrelated.candidate["digest"].as_str().unwrap()
    )))
    .unwrap();
    for tool in ["pack_list", "registry_list", "rule_list", "policy_show"] {
        let response = invoke(&context, &connection, tool, json!({}));
        assert!(response["error"].is_null(), "{tool}: {response}");
    }
    let inspection = invoke(
        &context,
        &connection,
        "policy_show",
        json!({"proposal_id":selected["data"]["proposal_id"]}),
    );
    assert!(inspection["error"].is_null(), "{inspection}");
    let listing = invoke(&context, &connection, "policy_show", json!({}));
    let summary = listing["data"]["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["proposal_id"] == selected["data"]["proposal_id"])
        .unwrap();
    assert_eq!(
        summary["candidate_digest"],
        inspection["data"]["candidate_digest"]
    );
    let repeated = proposal(&context, &connection, "acme-selected");
    assert_eq!(
        selected["data"]["apply_arguments"],
        repeated["data"]["apply_arguments"]
    );
    let created = proposal(&context, &connection, "acme-new");
    assert!(created["error"].is_null(), "{created}");
    let broken = invoke(
        &context,
        &connection,
        "policy_show",
        json!({"proposal_id":old["data"]["proposal_id"]}),
    );
    assert!(
        !broken["error"].is_null(),
        "selected missing bytes must still refuse: {broken}"
    );
    assert_eq!(
        store.read_checkpoint().unwrap().policy_revision,
        state.policy_revision
    );
}
