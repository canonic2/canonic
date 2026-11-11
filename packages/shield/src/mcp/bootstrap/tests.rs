use super::*;
use crate::{
    configuration::Bundle,
    mcp::tests::{Transport, initialize},
};
use std::{fs, path::Path};

fn fixture(agent: Option<&str>) -> (assert_fs::TempDir, PathBuf, Value, Transport) {
    fixture_with_recovery(agent, false)
}

fn fixture_with_recovery(
    agent: Option<&str>,
    fail: bool,
) -> (assert_fs::TempDir, PathBuf, Value, Transport) {
    let root = assert_fs::TempDir::new().expect("fixture root");
    let original = root.path().join("original");
    let copy = root.path().join("copy");
    let config = "schema_version: 1\npacks: []\n";
    let bundle = Bundle::capture(config, &[], None).expect("fixture policy");
    // `original` is an enrolled project; `copy` holds only committed policy
    // this user never reviewed, so it awaits its own first enrollment.
    fs::create_dir(&original).expect("fixture project");
    crate::review::mcp::initialize_fixture(&original, &bundle);
    fs::write(original.join(".shield/config.yml"), config).expect("fixture config");
    fs::create_dir_all(copy.join(".shield")).expect("fixture project");
    // Unique per fixture: consent from an earlier fixture in this process
    // would otherwise cover identical content.
    let marker = root.path().file_name().unwrap().to_string_lossy();
    fs::write(
        copy.join(".shield/config.yml"),
        format!("# Acme unreviewed {marker}\n{config}"),
    )
    .expect("fixture config");
    let bytes = fs::read(original.join(".shield/review/state.json")).expect("original state");
    let old = serde_json::from_slice(&bytes).expect("checkpoint fixture");
    let attribution = ReviewContext {
        project: copy.clone(),
        agent: agent.map(str::to_owned),
        session_id: Some("bootstrap-fixture".into()),
        tool_use_id: None,
    };
    let transport = if fail {
        Transport::bootstrap_after_publication_failure(attribution)
    } else {
        Transport::bootstrap(attribution)
    };
    (root, copy, old, transport)
}

#[test]
fn bootstrap_postpublication_failure_recovers_without_another_confirmation_or_identity() {
    let (_root, project, old, transport) = fixture_with_recovery(None, true);
    begin(&transport, &json!({"elicitation":{"form":{}}}), "acme");
    let form = transport.call(json!(2), "project_enroll", &json!({}));
    assert_eq!(form["method"], "elicitation/create", "{form}");
    let interrupted = transport.request(&answer(&form, "accept"));
    assert_eq!(
        data(&interrupted)["error"]["code"],
        "enrollment_failed",
        "{interrupted}"
    );
    let committed = checkpoint(&project);
    assert_ne!(committed["project_id"], old["project_id"]);
    // The next request resumes the already accepted, durable enrollment. It
    // opens this connection without issuing another form or project identity.
    let recovered = transport.call(json!(3), "status", &json!({}));
    assert!(data(&recovered)["error"].is_null(), "{recovered}");
    assert_eq!(
        data(&recovered)["project_id"],
        committed["project_id"],
        "{recovered}"
    );
    assert_eq!(checkpoint(&project), committed);
    let status = transport.call(json!(4), "status", &json!({}));
    assert!(data(&status)["error"].is_null(), "{status}");
    assert_eq!(data(&status)["project_id"], committed["project_id"]);
    transport.close().1.unwrap();
}

#[test]
fn review_resources_require_enrollment_consent_and_work_without_reconnecting() {
    let (_root, project, old, transport) = fixture(None);
    begin(&transport, &json!({"elicitation":{"form":{}}}), "acme");
    let uri = format!(
        "shield-review://{}/proposal-acme/{}/comparison",
        old["project_id"].as_str().unwrap(),
        "a".repeat(64)
    );
    let request =
        |id, uri| json!({"jsonrpc":"2.0","id":id,"method":"resources/read","params":{"uri":uri}});
    let refused = transport.request(&request(2, uri.as_str()));
    assert_eq!(refused["error"]["code"], rpc::RESOURCE_NOT_FOUND);
    assert!(unenrolled(&project));
    let form = transport.call(json!(3), "project_enroll", &json!({}));
    let enrolled = transport.request(&answer(&form, "accept"));
    assert!(data(&enrolled)["error"].is_null(), "{enrolled}");
    let proposed = transport.call(json!(4), "policy_propose", &json!({"change":{"kind":"rule_add",
        "rule":{"id":"acme-read","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}}}}));
    let uri = data(&proposed)["data"]["report_resources"][0]["uri"]
        .as_str()
        .unwrap();
    let before = checkpoint(&project);
    let read = transport.request(&request(5, uri));
    let report: Value =
        serde_json::from_str(read["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(report["project_id"], before["project_id"]);
    assert_ne!(report["project_id"], old["project_id"]);
    assert_eq!(checkpoint(&project), before);
}

fn begin(transport: &Transport, capabilities: &Value, agent: &str) {
    for message in initialize(capabilities, agent) {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
}

/// Nothing enrolled the folder: it still has no checkpoint.
fn unenrolled(path: &Path) -> bool {
    !path.join(".shield/review/state.json").exists()
}

fn checkpoint(path: &Path) -> Value {
    serde_json::from_slice(
        &fs::read(path.join(".shield/review/state.json")).expect("checkpoint bytes"),
    )
    .expect("checkpoint document")
}

fn data(response: &Value) -> &Value {
    &response["result"]["structuredContent"]
}

fn answer(form: &Value, action: &str) -> Value {
    json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":action,"content":{}}})
}

fn modern(id: u64, token: Option<&str>, action: Option<&str>) -> Value {
    let mut params = json!({"name":"project_enroll","arguments":{},"_meta":{
        "io.modelcontextprotocol/protocolVersion":"2026-07-28",
        "io.modelcontextprotocol/clientCapabilities":{"elicitation":{"form":{}}}
    }});
    if let Some(token) = token {
        params["requestState"] = json!(token);
    }
    if let Some(action) = action {
        params["inputResponses"] = json!({"confirmation":{"action":action}});
    }
    json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":params})
}

#[test]
fn bootstrap_legacy_confirmation_keeps_consent_and_normal_review_on_one_connection() {
    for agent in [Some("claude"), Some("codex"), None] {
        let (_root, project, old, transport) = fixture(agent);
        begin(&transport, &json!({"elicitation":{"form":{}}}), "acme");
        let status = transport.call(json!(10), "status", &json!({}));
        assert!(data(&status)["error"].is_null(), "{status}");
        assert!(data(&status)["project_id"].is_null(), "{status}");
        assert_eq!(
            data(&status)["data"]["bootstrap_required"],
            true,
            "{status}"
        );
        let denied = transport.call(json!(2), "policy_propose", &json!({"idempotency_key":"unreviewed","change":{"kind":"rule_add","rule":{"id":"unreviewed","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}}}}));
        assert_eq!(data(&denied)["error"]["code"], "enrollment_required");
        let review = transport.call(json!(3), "project_enroll", &json!({}));
        assert_eq!(review["method"], "elicitation/create", "{review}");
        if let Some(agent) = agent {
            assert!(
                review["params"]["message"]
                    .as_str()
                    .unwrap()
                    .contains(agent)
            );
            assert!(
                review["params"]["message"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("Also connect this MCP session as {agent}"))
            );
        }
        assert!(unenrolled(&project));
        let enrolled = transport.request(&answer(&review, "accept"));
        assert!(data(&enrolled)["error"].is_null(), "{enrolled}");
        assert_ne!(checkpoint(&project)["project_id"], old["project_id"]);
        let reconnected = crate::review::cli::context(&ReviewContext {
            project: project.clone(),
            agent: agent.map(str::to_owned),
            session_id: Some("new-session".into()),
            tool_use_id: None,
        })
        .unwrap();
        assert!(
            reconnected.agent.is_none(),
            "session consent cannot persist adapter trust"
        );
        let status = transport.call(json!(4), "status", &json!({}));
        assert!(data(&status)["error"].is_null(), "{status}");
        let proposed = transport.call(json!(5), "policy_propose", &json!({"idempotency_key":"after-enrollment","change":{"kind":"rule_add","rule":{"id":"reviewed","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}}}}));
        assert!(data(&proposed)["error"].is_null(), "{proposed}");
        let apply = transport.call(
            json!(6),
            "policy_apply",
            &data(&proposed)["data"]["apply_arguments"],
        );
        assert_eq!(apply["method"], "elicitation/create", "{apply}");
        let applied = transport.request(&answer(&apply, "accept"));
        assert!(data(&applied)["error"].is_null(), "{applied}");
        transport.close().1.unwrap();
    }
}

#[test]
fn bootstrap_decline_missing_forms_and_model_confirmation_preserve_original_bytes() {
    let (_root, project, _old, transport) = fixture(Some("codex"));
    begin(&transport, &json!({"elicitation":{"form":{}}}), "codex");
    let forged = transport.call(json!(2), "project_enroll", &json!({"confirmed":true}));
    assert!(
        forged["error"].is_object() || forged["result"]["isError"] == true,
        "{forged}"
    );
    let review = transport.call(json!(3), "project_enroll", &json!({}));
    assert_eq!(review["method"], "elicitation/create", "{review}");
    let declined = transport.request(&answer(&review, "decline"));
    assert_eq!(data(&declined)["error"]["code"], "approval_declined");
    assert!(unenrolled(&project));
    let retry = transport.call(json!(4), "project_enroll", &json!({}));
    assert_eq!(retry["method"], "elicitation/create", "{retry}");
    let _ = transport.request(&answer(&retry, "cancel"));
    transport.close().1.unwrap();
    let (_root, project, _old, transport) = fixture(Some("claude"));
    begin(&transport, &json!({}), "claude");
    let unavailable = transport.call(json!(2), "project_enroll", &json!({}));
    assert!(!data(&unavailable)["error"].is_null(), "{unavailable}");
    assert!(unenrolled(&project));
    transport.close().1.unwrap();
}

#[test]
fn bootstrap_modern_reviews_retain_exact_plans_and_reject_changed_sources() {
    let (_root, project, old, transport) = fixture(Some("codex"));
    let first = transport.request(&modern(1, None, None));
    let token = first["result"]["requestState"].as_str().unwrap();
    fs::write(
        project.join(".shield/config.yml"),
        "# changed during confirmation\nschema_version: 1\npacks: []\n",
    )
    .unwrap();
    let retry = transport.request(&modern(2, None, None));
    let stale = transport.request(&modern(3, Some(token), Some("accept")));
    assert!(!data(&stale)["error"].is_null(), "{stale}");
    assert!(unenrolled(&project));
    let token = retry["result"]["requestState"].as_str().unwrap();
    let declined = transport.request(&modern(4, Some(token), Some("decline")));
    assert!(!data(&declined)["error"].is_null(), "{declined}");
    assert!(unenrolled(&project));
    let fresh = transport.request(&modern(5, None, None));
    let token = fresh["result"]["requestState"].as_str().unwrap();
    let accepted = transport.request(&modern(6, Some(token), Some("accept")));
    assert!(data(&accepted)["error"].is_null(), "{accepted}");
    let status = transport.request(&json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"status","arguments":{},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}));
    assert!(data(&status)["error"].is_null(), "{status}");
    assert_ne!(checkpoint(&project)["project_id"], old["project_id"]);
    transport.close().1.unwrap();
}

#[test]
fn bootstrap_reviewed_enrollment_keeps_forms_after_cli_enrollment_and_mcp_reconnection() {
    for source in ["cli", "mcp"] {
        let (_root, project, _old, transport) = fixture(None);
        if source == "cli" {
            transport.close().1.unwrap();
            crate::review::bootstrap::prepare(&project)
                .unwrap()
                .commit_cli()
                .unwrap();
        } else {
            begin(&transport, &json!({"elicitation":{"form":{}}}), "acme");
            let form = transport.call(json!(2), "project_enroll", &json!({}));
            assert_eq!(form["method"], "elicitation/create", "{form}");
            let accepted = transport.request(&answer(&form, "accept"));
            assert!(data(&accepted)["error"].is_null(), "{accepted}");
            transport.close().1.unwrap();
        }
        let review = crate::mcp_io::review_context(&ReviewContext {
            project: project.clone(),
            agent: Some("codex".into()),
            session_id: Some("reconnected".into()),
            tool_use_id: None,
        })
        .unwrap();
        assert!(review.confirmed_mcp_forms);
        assert!(
            review.agent.is_none(),
            "copied registration supplies no adapter trust"
        );
        let transport = Transport::start(Server::for_review(review).unwrap());
        for message in initialize(&json!({"elicitation":{"form":{}}}), "codex") {
            transport.send(&message);
        }
        let initialized = transport.receive();
        assert!(
            initialized["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("wait for its MCP confirmation form"),
            "{initialized}"
        );
        let proposed = transport.call(json!(3), "policy_propose", &json!({"idempotency_key":"after-reconnect","change":{"kind":"rule_add","rule":{"id":"read-after-reconnect","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}}}}));
        assert!(data(&proposed)["error"].is_null(), "{proposed}");
        let form = transport.call(
            json!(4),
            "policy_apply",
            &data(&proposed)["data"]["apply_arguments"],
        );
        assert_eq!(form["method"], "elicitation/create", "{form}");
        let applied = transport.request(&answer(&form, "accept"));
        assert!(data(&applied)["error"].is_null(), "{applied}");
        transport.close().1.unwrap();
    }
}

#[test]
fn bootstrap_failed_cache_choice_can_reach_a_fresh_in_session_cache_review() {
    let (root, project, old, transport) = fixture(None);
    begin(&transport, &json!({"elicitation":{"form":{}}}), "acme");
    let blocked = root.path().join("blocked-cache");
    fs::write(&blocked, "This is a file, not a cache directory.").unwrap();
    let first = transport.call(json!(2), "project_enroll", &json!({"cache_dir":blocked}));
    assert_eq!(first["method"], "elicitation/create", "{first}");
    let failed = transport.request(&answer(&first, "accept"));
    assert_eq!(
        data(&failed)["error"]["code"],
        "enrollment_failed",
        "{failed}"
    );
    assert!(unenrolled(&project));
    let cache = root.path().join("working-cache");
    let retry = transport.call(json!(3), "project_enroll", &json!({"cache_dir":cache}));
    assert_eq!(retry["method"], "elicitation/create", "{retry}");
    assert!(unenrolled(&project));
    let accepted = transport.request(&answer(&retry, "accept"));
    assert!(data(&accepted)["error"].is_null(), "{accepted}");
    let committed = checkpoint(&project);
    assert_ne!(committed["project_id"], old["project_id"]);
    assert!(cache.join("compiled").is_dir());
    let status = transport.call(json!(4), "status", &json!({}));
    assert!(data(&status)["error"].is_null(), "{status}");
    assert_eq!(data(&status)["project_id"], committed["project_id"]);
    transport.close().1.unwrap();
}

#[test]
fn bootstrap_cancelled_accepted_enrollment_cannot_publish_on_a_later_status() {
    let (_root, project, old, initial) = fixture(None);
    initial.close().1.unwrap();
    let (transport, pending_cancellation) =
        Transport::bootstrap_before_publication_cancellation(ReviewContext {
            project: project.clone(),
            agent: None,
            session_id: Some("cancelled-bootstrap".into()),
            tool_use_id: None,
        });
    begin(&transport, &json!({"elicitation":{"form":{}}}), "acme");
    let form = transport.call(json!(2), "project_enroll", &json!({}));
    assert_eq!(form["method"], "elicitation/create", "{form}");
    transport.send(&answer(&form, "accept"));
    let deadline = Instant::now() + Duration::from_secs(5);
    while pending_cancellation.load(std::sync::atomic::Ordering::Acquire) {
        assert!(
            Instant::now() < deadline,
            "enrollment did not reach its cancellation checkpoint"
        );
        std::thread::yield_now();
    }
    transport.send(&json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"status","arguments":{}}}));
    let status = loop {
        let response = transport.receive();
        if response["id"] == 3 {
            break response;
        }
        assert_eq!(response["id"], 2, "{response}");
    };
    assert!(data(&status)["error"].is_null(), "{status}");
    assert_eq!(
        data(&status)["data"]["bootstrap_required"],
        true,
        "{status}"
    );
    assert!(unenrolled(&project));
    let fresh = transport.call(json!(4), "project_enroll", &json!({}));
    assert_eq!(fresh["method"], "elicitation/create", "{fresh}");
    let accepted = transport.request(&answer(&fresh, "accept"));
    assert!(data(&accepted)["error"].is_null(), "{accepted}");
    assert_ne!(checkpoint(&project)["project_id"], old["project_id"]);
    transport.close().1.unwrap();
}

#[test]
fn bootstrap_invalid_local_policy_keeps_metadata_and_consent_reachable_without_preparation() {
    let (_root, project, _old, transport) = fixture(None);
    fs::write(project.join(".shield/config.yml"), "not valid: [\n").unwrap();
    begin(&transport, &json!({"elicitation":{"form":{}}}), "acme");
    let status = transport.call(json!(2), "status", &json!({}));
    assert!(data(&status)["error"].is_null(), "{status}");
    assert_eq!(
        data(&status)["data"]["bootstrap_required"],
        true,
        "{status}"
    );
    assert_eq!(
        data(&status)["data"]["project_root"],
        project.canonicalize().unwrap().to_str().unwrap(),
        "{status}"
    );
    assert!(unenrolled(&project), "metadata inspection writes nothing");
    let refused = transport.call(json!(3), "project_enroll", &json!({}));
    assert_eq!(
        data(&refused)["error"]["code"],
        "enrollment_unavailable",
        "{refused}"
    );
    assert!(
        refused["method"].is_null(),
        "invalid source must not obtain a confirmation"
    );
    assert!(unenrolled(&project));
    transport.close().1.unwrap();
}

#[test]
fn bootstrap_another_confirmed_writer_opens_this_connection_without_reenrollment() {
    for writer in ["cli", "mcp"] {
        let (_root, project, old, transport) = fixture(Some("codex"));
        begin(&transport, &json!({"elicitation":{"form":{}}}), "codex");
        let initial = transport.call(json!(2), "status", &json!({}));
        assert_eq!(
            data(&initial)["data"]["bootstrap_required"],
            true,
            "{initial}"
        );
        let other = if writer == "cli" {
            crate::review::bootstrap::prepare(&project)
                .unwrap()
                .commit_cli()
                .unwrap();
            crate::review::operator::integrations::configure_fixture(
                &project,
                crate::review::integrations::IntegrationChoice::agent("codex"),
                crate::review::integrations::AskFallback::Block,
            );
            None
        } else {
            let other = Transport::bootstrap(ReviewContext {
                project: project.clone(),
                agent: None,
                session_id: Some("other-bootstrap-writer".into()),
                tool_use_id: None,
            });
            begin(&other, &json!({"elicitation":{"form":{}}}), "acme");
            let form = other.call(json!(2), "project_enroll", &json!({}));
            assert_eq!(form["method"], "elicitation/create", "{form}");
            let accepted = other.request(&answer(&form, "accept"));
            assert!(data(&accepted)["error"].is_null(), "{accepted}");
            Some(other)
        };
        let enrolled = checkpoint(&project);
        assert_ne!(enrolled["project_id"], old["project_id"]);
        let status = transport.call(json!(3), "status", &json!({}));
        assert!(data(&status)["error"].is_null(), "{status}");
        assert_eq!(
            data(&status)["project_id"],
            enrolled["project_id"],
            "{status}"
        );
        assert_eq!(checkpoint(&project), enrolled);
        let proposed = transport.call(json!(4), "policy_propose", &json!({"idempotency_key":"after-concurrent-enrollment","change":{"kind":"rule_add","rule":{"id":"read-after-concurrent-enrollment","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}}}}));
        assert!(data(&proposed)["error"].is_null(), "{proposed}");
        let form = transport.call(
            json!(5),
            "policy_apply",
            &data(&proposed)["data"]["apply_arguments"],
        );
        assert_eq!(form["method"], "elicitation/create", "{form}");
        let applied = transport.request(&answer(&form, "accept"));
        assert!(data(&applied)["error"].is_null(), "{applied}");
        assert_eq!(checkpoint(&project)["project_id"], enrolled["project_id"]);
        transport.close().1.unwrap();
        if let Some(other) = other {
            other.close().1.unwrap();
        }
    }
}
