//! Durable preparation handles survive reconnect and cannot replace approval.
use super::confirmation::rule;
use super::*;

fn capabilities() -> Value {
    json!({"extensions":{"io.modelcontextprotocol/tasks":{}}})
}
fn modern(id: i64, method: &str, params: Value) -> Value {
    super::skills::modern(id, method, params, capabilities())
}
fn connect(server: Server) -> Transport {
    let transport = Transport::start(server);
    for message in initialize(&json!({"elicitation":{"form":{}}}), "claude-code") {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    transport
}
fn get(transport: &Transport, id: i64, task: &Value) -> Value {
    transport.request(&modern(id, "tasks/get", json!({"taskId":task})))
}

fn completed_task(
    transport: &Transport,
    jobs: &crate::review::preparation_tasks::Runtime,
    id: i64,
    args: &Value,
) -> Value {
    let created = transport.request(&modern(
        id,
        "tools/call",
        json!({"name":"policy_propose","arguments":args}),
    ));
    assert_eq!(created["result"]["resultType"], "task", "{created}");
    let task = created["result"]["taskId"].clone();
    assert!(task.is_string());
    jobs.join();
    assert_eq!(
        get(transport, id + 1, &task)["result"]["status"],
        "completed"
    );
    task
}

fn accept_task_review(transport: &Transport, task: &Value, id: i64, before_accept: impl FnOnce()) {
    let proposed = transport.call(
        json!(id),
        "policy_propose",
        &json!({"preparation_task_id":task}),
    );
    assert_eq!(proposed["result"]["isError"], false, "{proposed}");
    let args = &proposed["result"]["structuredContent"]["data"]["apply_arguments"];
    assert!(args.is_object());
    let form = transport.call(json!(id + 1), "policy_apply", args);
    assert_eq!(form["method"], "elicitation/create", "{form}");
    before_accept();
    let applied = transport.request(&json!({"jsonrpc":"2.0","id":form["id"],
        "result":{"action":"accept","content":{}}}));
    assert_eq!(applied["result"]["isError"], false, "{applied}");
}

#[test]
fn copied_policy_workflow_preparation_tasks_cannot_be_polled_cancelled_or_reviewed() {
    let (server, original) = fixture(Some("claude"));
    let jobs = server.preparations.clone();
    let source = connect(server);
    let arguments = rule("acme-copy-task", "acme");
    let old = completed_task(&source, &jobs, 2, &arguments);
    let original_path = original.root.path().join(".shield/review/state.json");
    let original_bytes = std::fs::read(&original_path).unwrap();
    let record = std::path::Path::new(".shield/review/preparation-tasks")
        .join(old.as_str().unwrap())
        .join("record.json");
    let original_record = std::fs::read(original.root.path().join(&record)).unwrap();
    let copy = assert_fs::TempDir::new().unwrap();
    crate::test_support::copy_fixture_directory(original.root.path(), copy.path());
    let opened = crate::review::open(copy.path()).unwrap();
    let mut context = (*original.review).clone();
    context.project_id = opened.project_id;
    context.project_root = opened.project_root;
    let server = Server::for_review(context).unwrap();
    let child_jobs = server.preparations.clone();
    let child = Project {
        root: copy,
        review: server.review(),
    };
    let transport = connect(server);
    assert_ne!(child.review.project_id, original.review.project_id);
    let revision = child.revision();
    let copied_record = std::fs::read(child.root.path().join(&record)).unwrap();
    assert_eq!(
        get(&transport, 2, &old)["error"]["code"],
        rpc::INVALID_PARAMS
    );
    assert_eq!(
        std::fs::read(child.root.path().join(&record)).unwrap(),
        copied_record
    );
    let refused = transport.call(
        json!(3),
        "policy_propose",
        &json!({"preparation_task_id":old}),
    );
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert!(refused["result"]["structuredContent"]["data"]["apply_arguments"].is_null());
    let cancelled = transport.request(&modern(4, "tasks/cancel", json!({"taskId":old})));
    assert_eq!(cancelled["error"]["code"], rpc::INVALID_PARAMS);
    assert_eq!(child.revision(), revision);

    let fresh = completed_task(&transport, &child_jobs, 5, &arguments);
    assert_ne!(fresh, old);
    assert_eq!(child.revision(), revision);
    accept_task_review(&transport, &fresh, 10, || {
        assert_eq!(child.revision(), revision);
    });
    assert_ne!(child.revision(), revision);
    assert_eq!(std::fs::read(&original_path).unwrap(), original_bytes);
    assert_eq!(
        std::fs::read(original.root.path().join(&record)).unwrap(),
        original_record
    );
    assert_eq!(get(&source, 4, &old)["result"]["status"], "completed");
    let revision = original.revision();
    accept_task_review(&source, &old, 10, || {
        assert_eq!(original.revision(), revision);
    });
    assert_ne!(original.revision(), revision);
}

#[test]
fn preparation_tasks_lost_creation_reply_recovers_the_same_published_task() {
    struct LostReply(std::sync::mpsc::SyncSender<()>);
    impl std::io::Write for LostReply {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            let _ = self.0.try_send(());
            Err(std::io::Error::other("owned fixture lost delivery"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (server, project) = fixture(Some("claude"));
    let jobs = server.preparations.clone();
    let (sent, attempted) = std::sync::mpsc::sync_channel(1);
    let first = Transport::with_runner(move |input, _| {
        serve(server, input, LostReply(sent), &test_options())
    });
    let args = rule("acme-uncertain-delivery", "acme");
    first.send(&modern(
        1,
        "tools/call",
        json!({"name":"policy_propose","arguments":args}),
    ));
    attempted.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(first.close().1.is_err());
    jobs.join();
    let second = connect(Server::for_review((*project.review).clone()).unwrap());
    let recovered = second.request(&modern(
        2,
        "tools/call",
        json!({"name":"policy_propose","arguments":args}),
    ));
    assert_eq!(recovered["result"]["status"], "completed", "{recovered}");
    let id = recovered["result"]["taskId"].clone();
    let directory = project.review.project_root.clone();
    let path = std::path::Path::new(&directory)
        .join(".shield/review/preparation-tasks")
        .join(id.as_str().unwrap())
        .join("record.json");
    let before = std::fs::read(&path).unwrap();
    let result = get(&second, 3, &id);
    assert_eq!(result["result"]["status"], "completed");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(get(&second, 4, &id)["result"], result["result"]);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn preparation_tasks_pack_capture_survives_source_loss_and_engine_change_refuses_review() {
    let (server, project) = fixture(Some("claude"));
    let source = std::path::Path::new(&project.review.project_root).join("registry");
    std::fs::create_dir(&source).unwrap();
    let bytes = include_str!("../../../fixtures/elicitation/pack.yml");
    std::fs::write(source.join("pack.yml"), bytes).unwrap();
    std::fs::write(source.join("catalog.json"), json!({"schema_version":1,"packs":[{"id":"acme/settings","version":"1.0.0","path":"pack.yml","sha256":crate::policy::fingerprint(bytes.as_bytes())}]}).to_string()).unwrap();
    crate::review::mcp::register_fixture(&project.review, &source);
    let jobs = server.preparations.clone();
    let transport = connect(server);
    let created = transport.request(&modern(2, "tools/call", json!({"name":"policy_propose","arguments":{"change":{"kind":"pack_add","reference":"fixture/acme/settings"},"idempotency_key":"acme-pack-capture"}})));
    let id = created["result"]["taskId"].clone();
    jobs.join();
    assert_eq!(get(&transport, 3, &id)["result"]["status"], "completed");
    std::fs::remove_file(source.join("pack.yml")).unwrap();
    let review = transport.call(
        json!(4),
        "policy_propose",
        &json!({"preparation_task_id":id}),
    );
    assert!(
        review["result"]["structuredContent"]["data"]["apply_arguments"].is_object(),
        "{review}"
    );
    let path = std::path::Path::new(&project.review.project_root)
        .join(".shield/review/preparation-tasks")
        .join(id.as_str().unwrap())
        .join("record.json");
    let mut record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    record["engine"] = json!("incompatible-fixture-engine");
    std::fs::write(&path, record.to_string()).unwrap();
    let refused = transport.call(
        json!(5),
        "policy_propose",
        &json!({"preparation_task_id":id}),
    );
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert_eq!(get(&transport, 6, &id)["result"]["status"], "completed");
}

#[test]
fn preparation_tasks_failed_execution_is_a_product_failure_not_an_internal_error() {
    let (server, _project) = fixture(Some("claude"));
    let jobs = server.preparations.clone();
    let transport = connect(server);
    let created = transport.request(&modern(2,"tools/call",json!({"name":"policy_propose","arguments":{"change":{"kind":"pack_add","reference":"unknown-registry/acme/missing"}}})));
    let id = created["result"]["taskId"].clone();
    assert!(id.is_string(), "{created}");
    jobs.join();
    let failed = get(&transport, 3, &id);
    assert_eq!(failed["result"]["status"], "failed", "{failed}");
    assert_eq!(failed["result"]["error"]["code"], -32000, "{failed}");
    assert_eq!(
        failed["result"]["error"]["data"]["code"],
        "preparation_failed"
    );
}

#[test]
fn preparation_tasks_same_arguments_prepare_again_after_another_change_is_applied() {
    for explicit in [false, true] {
        let (server, project) = fixture(Some("claude"));
        let jobs = server.preparations.clone();
        let transport = connect(server);
        let mut args = rule("acme-change-b", "acme-b");
        if explicit {
            args["idempotency_key"] = json!("acme-explicit");
        }
        let created = transport.request(&modern(
            2,
            "tools/call",
            json!({"name":"policy_propose","arguments":args}),
        ));
        let first = created["result"]["taskId"].clone();
        jobs.join();
        let a = transport.call(json!(3), "policy_propose", &rule("acme-change-a", "acme-a"));
        super::confirmation::commit_elsewhere(
            &project,
            &a["result"]["structuredContent"]["data"]["apply_arguments"],
        );
        let stale = transport.call(
            json!(4),
            "policy_propose",
            &json!({"preparation_task_id":first}),
        );
        assert_eq!(
            stale["result"]["structuredContent"]["error"]["code"],
            "stale_revision"
        );
        assert!(
            stale["result"]["structuredContent"]["error"]["message"]
                .as_str()
                .unwrap()
                .contains("new key")
        );
        if explicit {
            let same = transport.request(&modern(
                5,
                "tools/call",
                json!({"name":"policy_propose","arguments":args}),
            ));
            assert_eq!(same["result"]["taskId"], first);
            args["idempotency_key"] = json!("acme-explicit-fresh");
        }
        let fresh = transport.request(&modern(
            6,
            "tools/call",
            json!({"name":"policy_propose","arguments":args}),
        ));
        let second = fresh["result"]["taskId"].clone();
        assert_ne!(first, second, "{fresh}");
        jobs.join();
        assert_eq!(get(&transport, 7, &second)["result"]["status"], "completed");
        let reviewed = transport.call(
            json!(8),
            "policy_propose",
            &json!({"preparation_task_id":second}),
        );
        assert!(
            reviewed["result"]["structuredContent"]["data"]["apply_arguments"].is_object(),
            "{reviewed}"
        );
    }
}

#[test]
fn preparation_tasks_corrupt_optional_storage_preserves_ordinary_tools() {
    let (server, project) = fixture(Some("claude"));
    drop(server);
    let root =
        std::path::Path::new(&project.review.project_root).join(".shield/review/preparation-tasks");
    std::fs::write(&root, b"invalid retained task storage").unwrap();
    let server = Server::for_review((*project.review).clone()).unwrap();
    let jobs = server.preparations.clone();
    let transport = connect(server);
    let unavailable = transport.request(&modern(
        2,
        "tools/call",
        json!({"name":"policy_propose","arguments":rule("acme-unavailable", "acme")}),
    ));
    assert_eq!(
        unavailable["result"]["resultType"], "complete",
        "{unavailable}"
    );
    assert!(unavailable["result"]["structuredContent"]["data"]["apply_arguments"].is_object());
    assert_eq!(
        transport.call(json!(3), "status", &json!({}))["result"]["isError"],
        false
    );
    let ordinary = transport.call(json!(4), "policy_propose", &rule("acme-sync", "acme"));
    assert!(
        ordinary["result"]["structuredContent"]["data"]["apply_arguments"].is_object(),
        "{ordinary}"
    );
    std::fs::remove_file(&root).unwrap();
    let repaired = transport.request(&modern(
        5,
        "tools/call",
        json!({"name":"policy_propose","arguments":rule("acme-after-recovery","acme")}),
    ));
    assert_eq!(repaired["result"]["resultType"], "task", "{repaired}");
    jobs.join();
}

#[test]
fn preparation_tasks_survive_reconnect_then_require_fresh_owned_review_and_confirmation() {
    let (server, project) = fixture(Some("claude"));
    let jobs = server.preparations.clone();
    let first = connect(server);
    let revision = project.revision();
    let request = modern(
        2,
        "tools/call",
        json!({"name":"policy_propose","arguments":rule("acme-durable", "acme")}),
    );
    let created = first.request(&request);
    assert_eq!(created["result"]["resultType"], "task", "{created}");
    let id = created["result"]["taskId"].clone();
    assert!(id.is_string());
    first.close().1.unwrap();
    jobs.join();
    assert_eq!(project.revision(), revision);
    let second = connect(Server::for_review((*project.review).clone()).unwrap());
    let completed = get(&second, 3, &id);
    assert_eq!(completed["result"]["status"], "completed", "{completed}");
    let payload = completed["result"]["result"].clone();
    assert_eq!(
        payload["structuredContent"]["data"]["preparation_task_id"],
        id
    );
    assert!(payload["structuredContent"]["data"]["apply_arguments"].is_null());
    assert_eq!(get(&second, 4, &id)["result"]["result"], payload);
    for (rpc_id, method, extra) in [
        (
            5,
            "tasks/update",
            json!({"inputResponses":{"unissued":{"action":"accept"}}}),
        ),
        (6, "tasks/cancel", json!({})),
    ] {
        let mut params = extra;
        params["taskId"] = id.clone();
        assert!(second.request(&modern(rpc_id, method, params))["error"].is_null());
    }
    assert_eq!(get(&second, 7, &id)["result"]["result"], payload);
    let proposed = second.call(
        json!(8),
        "policy_propose",
        &json!({"preparation_task_id":id}),
    );
    assert_eq!(proposed["result"]["isError"], false, "{proposed}");
    let args = &proposed["result"]["structuredContent"]["data"]["apply_arguments"];
    assert!(args.is_object());
    assert_eq!(project.revision(), revision);
    let form = second.call(json!(9), "policy_apply", args);
    assert_eq!(form["method"], "elicitation/create", "{form}");
    assert_eq!(project.revision(), revision);
    let accepted = second.request(
        &json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":"accept","content":{}}}),
    );
    assert_eq!(accepted["result"]["isError"], false, "{accepted}");
    assert_ne!(project.revision(), revision);
    assert_eq!(get(&second, 10, &id)["result"]["result"], payload);
    let stale = second.call(
        json!(11),
        "policy_propose",
        &json!({"preparation_task_id":id}),
    );
    assert_eq!(
        stale["result"]["structuredContent"]["error"]["code"],
        "stale_revision"
    );
}

#[test]
fn preparation_tasks_scope_capabilities_and_lost_creation_handles_preserve_results() {
    let (server, project) = fixture(Some("claude"));
    let jobs = server.preparations.clone();
    let transport = connect(server);
    let args = rule("acme-replay", "acme");
    let created = transport.request(&modern(
        2,
        "tools/call",
        json!({"name":"policy_propose","arguments":args}),
    ));
    jobs.join();
    let id = created["result"]["taskId"].clone();
    assert!(id.is_string(), "{created}");
    let replay = transport.request(&modern(
        3,
        "tools/call",
        json!({"name":"policy_propose","arguments":args}),
    ));
    assert_eq!(replay["result"]["taskId"], id);
    assert_eq!(replay["result"]["status"], "completed");
    let (other, _root) = fixture(Some("claude"));
    let foreign = connect(other);
    assert_eq!(get(&foreign, 4, &id)["error"]["code"], rpc::INVALID_PARAMS);
    assert_eq!(
        get(&transport, 5, &json!("../record.json"))["error"]["code"],
        rpc::INVALID_PARAMS
    );
    let missing = transport.request(&super::skills::modern(
        6,
        "tasks/get",
        json!({"taskId":id}),
        json!({}),
    ));
    assert!(missing["error"].is_object());
    let ordinary = transport.request(&super::skills::modern(
        7,
        "tools/call",
        json!({"name":"policy_propose","arguments":rule("acme-ordinary", "other")}),
        json!({}),
    ));
    assert_eq!(ordinary["result"]["resultType"], "complete");
    assert!(ordinary["result"]["structuredContent"]["data"]["apply_arguments"].is_object());
    let revision = project.revision();
    assert_eq!(
        transport.call(json!(8), "status", &json!({}))["result"]["isError"],
        false
    );
    assert_eq!(project.revision(), revision);
}
