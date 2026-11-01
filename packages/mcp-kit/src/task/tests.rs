//! Pure task, capability, lifecycle and bounded-input behavior through the public core.

use super::*;
use crate::{Catalog, Content, Inbound, ModernOptions, ServerInfo, Session, SessionOptions, Tool};
use std::sync::Arc;

fn time(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}
fn now() -> Timestamp {
    time("2026-10-30T12:00:00Z")
}
fn options() -> TaskOptions {
    TaskOptions {
        max_records: 4,
        max_bytes: 4 * MAX_SNAPSHOT_BYTES,
        max_running: 2,
        max_input_rounds: 2,
        execution_timeout: Duration::from_secs(60),
        input_timeout: Duration::from_secs(30),
    }
}
fn metadata() -> Metadata {
    Metadata::new(TaskId::new("acme-job").unwrap(), now(), 60_000).unwrap()
}
fn session(enabled: bool) -> Session {
    Session::try_new(
        Arc::new(
            Catalog::new(vec![Tool::new(
                "acme_work",
                "Start Acme work.",
                json!({"type":"object","additionalProperties":false}),
            )])
            .unwrap(),
        ),
        SessionOptions {
            modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
            tasks: enabled.then(options),
            ..SessionOptions::default()
        },
    )
    .unwrap()
}
fn request(id: u64, method: &str, mut params: Value, capabilities: Value) -> Value {
    params["_meta"] = json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION});
    params["_meta"][modern::CAPABILITIES_KEY] = capabilities;
    rpc::request(json!(id), method, params)
}
fn capabilities() -> Value {
    json!({"extensions":{EXTENSION:{}},"elicitation":{"form":{}}})
}
fn context(method: &str, task_id: Option<&str>) -> Arc<RequestContext> {
    let mut params = json!({});
    if let Some(id) = task_id {
        params["taskId"] = json!(id);
    }
    let value = request(1, method, params, capabilities());
    RequestContext::parse(
        &value["params"],
        &ModernOptions::new(ServerInfo::new("acme", "1")),
        method,
        Some(&options()),
        None,
    )
    .unwrap()
}
fn reply(event: Inbound) -> Value {
    if let Inbound::Reply(value) = event {
        value
    } else {
        panic!("expected reply, got {event:?}")
    }
}

#[test]
fn tasks_default_off_and_configuration_is_explicit() {
    assert!(SessionOptions::default().tasks.is_none());
    let empty = Arc::new(Catalog::new(vec![]).unwrap());
    assert!(
        Session::try_new(
            empty.clone(),
            SessionOptions {
                tasks: Some(options()),
                ..SessionOptions::default()
            }
        )
        .is_err()
    );
    for reserved in ["tasks/get", "tasks/list", EXTENSION, "notifications/tasks"] {
        assert!(
            Session::try_new(
                empty.clone(),
                SessionOptions {
                    extension_methods: vec![reserved.into()],
                    ..SessionOptions::default()
                }
            )
            .is_err()
        );
    }
    let mut invalid = options();
    invalid.max_records = 0;
    assert!(invalid.validate().is_err());
    invalid = options();
    invalid.execution_timeout = Duration::ZERO;
    assert!(invalid.validate().is_err());
    invalid.execution_timeout = Duration::from_nanos(1);
    assert!(invalid.validate().is_err());
    invalid = options();
    invalid.max_input_rounds = MAX_MILLIS + 1;
    assert!(invalid.validate().is_err());
    assert_eq!(options().admit(4, 4 * MAX_SNAPSHOT_BYTES, 2), Ok(()));
    for reservation in [(5, 1, 1), (1, 4 * MAX_SNAPSHOT_BYTES + 1, 1), (1, 1, 3)] {
        assert_eq!(
            options().admit(reservation.0, reservation.1, reservation.2),
            Err(TaskError::Capacity)
        );
    }
}

#[test]
fn tasks_discovery_and_capabilities_are_per_request() {
    let mut composed = Session::new(
        Arc::new(Catalog::new(vec![]).unwrap()),
        SessionOptions {
            modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
            tasks: Some(options()),
            skills: Some(Arc::new(crate::SkillCatalog::new(vec![]).unwrap())),
            ..SessionOptions::default()
        },
    );
    let discovery =
        reply(composed.receive_value(request(1, "server/discover", json!({}), json!({}))));
    assert!(
        discovery["result"]["capabilities"]["extensions"]
            .get(EXTENSION)
            .is_some()
    );
    assert!(
        discovery["result"]["capabilities"]["extensions"]
            .get(crate::skill::EXTENSION)
            .is_some()
    );
    for enabled in [false, true] {
        let mut s = session(enabled);
        let discovery = reply(s.receive_value(request(1, "server/discover", json!({}), json!({}))));
        assert_eq!(
            discovery["result"]["capabilities"]["extensions"]
                .get(EXTENSION)
                .is_some(),
            enabled
        );
        let event = s.receive_value(request(
            2,
            "tasks/get",
            json!({"taskId":"acme-job"}),
            capabilities(),
        ));
        if enabled {
            assert!(matches!(
                event,
                Inbound::Task {
                    request: TaskRequest::Get { .. },
                    ..
                }
            ));
            s.finish(&json!(2));
        } else {
            assert_eq!(reply(event)["error"]["code"], rpc::METHOD_NOT_FOUND);
        }
        let missing = reply(s.receive_value(request(
            3,
            "tasks/get",
            json!({"taskId":"acme-job"}),
            json!({}),
        )));
        assert_eq!(
            missing["error"]["code"],
            if enabled {
                -32021
            } else {
                rpc::METHOD_NOT_FOUND
            }
        );
        assert_eq!(s.outstanding(), 0);
    }
    let mut s = session(true);
    for declaration in [json!(true), json!(null), json!({"unexpected":true})] {
        assert_eq!(
            reply(s.receive_value(request(
                1,
                "tasks/get",
                json!({"taskId":"acme-job"}),
                json!({"extensions":{EXTENSION:declaration}})
            )))["error"]["code"],
            rpc::INVALID_PARAMS
        );
    }
    assert_eq!(
        reply(s.receive_value(request(1, "tasks/list", json!({}), capabilities())))["error"]["code"],
        rpc::METHOD_NOT_FOUND
    );
}

#[test]
fn tasks_parse_strict_management_and_isolate_update_from_mrtr() {
    let mut s = session(true);
    for (method, params) in [
        ("tasks/get", json!({})),
        ("tasks/get", json!({"taskId":12})),
        ("tasks/get", json!({"taskId":"acme-job","extra":true})),
        (
            "tasks/get",
            json!({"taskId":"acme-job","requestState":"state"}),
        ),
        ("tasks/update", json!({"taskId":"acme-job"})),
        (
            "tasks/update",
            json!({"taskId":"acme-job","inputResponses":{"x":null}}),
        ),
        ("tasks/cancel", json!({"taskId":"x".repeat(MAX_ID_BYTES+1)})),
    ] {
        assert_eq!(
            reply(s.receive_value(request(1, method, params, capabilities())))["error"]["code"],
            rpc::INVALID_PARAMS
        );
        assert_eq!(s.outstanding(), 0);
    }
    let event = s.receive_value(request(
        1,
        "tasks/update",
        json!({"taskId":"acme-job","inputResponses":{"different-key":{"action":"accept"}}}),
        capabilities(),
    ));
    assert!(matches!(
        event,
        Inbound::Task {
            request: TaskRequest::Update { .. },
            ..
        }
    ));
    assert!(
        s.request_context(&json!(1))
            .unwrap()
            .input_responses
            .is_none()
    );
    let duplicate = s.receive_value(request(
        1,
        "tasks/get",
        json!({"taskId":"acme-job"}),
        capabilities(),
    ));
    assert!(matches!(duplicate, Inbound::Fatal { .. }));
}

#[test]
fn tasks_keep_both_legacy_revisions_ordinary() {
    for version in crate::SUPPORTED_PROTOCOL_VERSIONS {
        let mut s = session(true);
        let Inbound::Initialize{id,..} = s.receive_value(rpc::request(json!(1),"initialize",json!({"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"acme","version":"1"}}))) else {panic!("initialize")};
        let init = s.accept_initialize(id, &ServerInfo::new("acme", "1"));
        assert_eq!(init["result"]["protocolVersion"], *version);
        assert!(init["result"]["capabilities"].get("extensions").is_none());
        s.receive_value(rpc::notification("notifications/initialized", json!({})));
        assert!(matches!(
            s.receive_value(rpc::request(
                json!(2),
                "tools/call",
                json!({"name":"acme_work"})
            )),
            Inbound::Tool { .. }
        ));
        assert!(s.request_context(&json!(2)).is_none());
        assert_eq!(
            reply(s.receive_value(rpc::request(
                json!(3),
                "tasks/get",
                json!({"taskId":"acme-job"})
            )))["error"]["code"],
            rpc::METHOD_NOT_FOUND
        );
    }
}

#[test]
fn tasks_validate_calendar_and_exact_fractional_expiry() {
    assert_eq!(
        Timestamp::from_unix_millis(0).unwrap(),
        time("1970-01-01T00:00:00Z")
    );
    assert_eq!(
        Timestamp::from_unix_millis(253_402_300_799_999).unwrap(),
        time("9999-12-31T23:59:59.999Z")
    );
    assert!(Timestamp::from_unix_millis(253_402_300_800_000).is_err());
    for text in [
        "2025-02-29T00:00:00Z",
        "2026-04-31T00:00:00Z",
        "0000-01-01T00:00:00Z",
        "2026-01-01T24:00:00Z",
        "2026-01-01T00:00:60Z",
        "2026-01-01T00:00:00+00:00",
        "2026-01-01T00:00:00.Z",
    ] {
        assert!(Timestamp::parse(text).is_err(), "{text}");
    }
    assert!(Timestamp::parse("2024-02-29T00:00:00Z").is_ok());
    assert_eq!(
        time("2026-01-01T00:00:00.100Z"),
        time("2026-01-01T00:00:00.1Z")
    );
    let full = format!("2026-01-01T00:00:00.{}Z", "1".repeat(43));
    assert_eq!(full.len(), 64);
    assert!(Timestamp::parse(&full).is_ok());
    assert!(Timestamp::parse(&full.replace('Z', "1Z")).is_err());
    let m = Metadata::new(
        TaskId::new("acme").unwrap(),
        time("2026-01-01T00:00:00.00090000000001Z"),
        1,
    )
    .unwrap();
    assert!(m.ensure_live(&time("2026-01-01T00:00:00.0019Z")).is_ok());
    assert_eq!(
        m.ensure_live(&time("2026-01-01T00:00:00.00190000000001Z")),
        Err(TaskError::Expired)
    );
}

#[test]
fn tasks_validate_metadata_and_identifier_boundaries() {
    assert!(TaskId::new("é".repeat(MAX_ID_BYTES / 2)).is_ok());
    for id in [
        String::new(),
        "é".repeat(MAX_ID_BYTES / 2) + "a",
        "a\nb".into(),
    ] {
        assert!(TaskId::new(id).is_err());
    }
    assert!(
        metadata()
            .with_message("x".repeat(MAX_STATUS_BYTES))
            .is_ok()
    );
    assert!(
        metadata()
            .with_message("x".repeat(MAX_STATUS_BYTES + 1))
            .is_err()
    );
    for duration in [1, MAX_MILLIS] {
        assert!(Metadata::new(TaskId::new("acme").unwrap(), now(), duration).is_ok());
        assert!(metadata().with_poll_interval(duration).is_ok());
    }
    for duration in [0, MAX_MILLIS + 1] {
        assert!(Metadata::new(TaskId::new("acme").unwrap(), now(), duration).is_err());
        assert!(metadata().with_poll_interval(duration).is_err());
    }
    assert_eq!(
        metadata().ensure_live(&time("2026-10-30T12:01:00Z")),
        Err(TaskError::Expired)
    );
    assert!(
        metadata()
            .updated(time("2026-10-30T12:00:01Z"), 120_000)
            .is_ok()
    );
    assert!(
        metadata()
            .updated(time("2026-10-30T12:01:00Z"), 120_000)
            .is_err()
    );
}

#[test]
fn tasks_snapshots_restore_and_refuse_contradictions() {
    let ctx = context("tools/call", None);
    let states = vec![
        State::Working,
        State::InputRequired(Question::new(&ctx, "approval-1", "Continue?").unwrap()),
        State::Completed(ToolResult::error(vec![Content::text("Build refused.")])),
        State::Failed(RpcError::new(-32603, "Worker lost.")),
        State::Cancelled,
    ];
    for state in states {
        let snapshot = Snapshot::new(metadata(), state).unwrap();
        let value = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(Snapshot::restore(&value).unwrap(), snapshot);
        let mut conflicting = value.clone();
        conflicting["unknown"] = json!(true);
        assert!(Snapshot::restore(&conflicting).is_err());
        let mut conflicting = value;
        conflicting["lastUpdatedAt"] = json!("2026-10-30T11:59:59Z");
        assert!(Snapshot::restore(&conflicting).is_err());
    }
    for extra in ["result", "error", "inputRequests"] {
        let mut value =
            serde_json::to_value(Snapshot::new(metadata(), State::Working).unwrap()).unwrap();
        value[extra] = json!({});
        assert!(Snapshot::restore(&value).is_err());
    }
}

#[test]
fn tasks_rich_questions_restore_and_validate_updates_atomically() {
    use crate::elicitation::{Action, Form, Request, Requests};
    let form = Request::form("Choose report", Form::new(json!({"type":"object","properties":{"format":{"type":"string","enum":["md","json"]}},"required":["format"]})).unwrap()).unwrap();
    let questions = Requests::new(vec![
        ("format".into(), form.clone()),
        ("review".into(), form),
    ])
    .unwrap();
    let initial = Snapshot::new(metadata(), State::RichInputRequired(questions)).unwrap();
    let saved = serde_json::to_value(&initial).unwrap();
    let restored = Snapshot::restore(&saved).unwrap();
    assert_eq!(restored, initial);
    assert_eq!(
        restored.pending_keys().collect::<Vec<_>>(),
        vec!["format", "review"]
    );
    let mut session = session(true);
    let update = |id, responses| {
        request(
            id,
            "tasks/update",
            json!({"taskId":"acme-job","inputResponses":responses}),
            capabilities(),
        )
    };
    let mut dispatch = |id, responses| {
        let Inbound::Task {
            id: rpc_id,
            request,
        } = session.receive_value(update(id, responses))
        else {
            panic!("admitted update")
        };
        let context = session.request_context(&rpc_id).unwrap();
        let result = restored.input_answers(&request, &context, &now());
        assert!(session.finish(&rpc_id));
        result
    };
    assert!(dispatch(1,json!({"format":{"action":"accept","content":{"format":"md"}},"review":{"action":"accept","content":{"format":"wrong"}}})).is_err());
    let partial = dispatch(
        2,
        json!({"format":{"action":"accept","content":{"format":"json"}},"unknown":{}}),
    )
    .unwrap();
    assert_eq!(partial.len(), 1);
    assert_eq!(partial["format"].action, Action::Accept);
    assert_eq!(partial["format"].content, Some(json!({"format":"json"})));
    assert_eq!(serde_json::to_value(&restored).unwrap(), saved);
    let mut changed = saved.clone();
    changed["inputRequests"]["review"]["params"]["message"] = json!("Different meaning");
    let changed = Snapshot::restore(&changed).unwrap();
    assert!(initial.validate_successor(&changed, &now()).is_err());
    let mut corrupt = saved;
    corrupt["inputRequests"]["format"]["params"]["requestedSchema"]["properties"]["format"]["pattern"] =
        json!(".*");
    assert!(Snapshot::restore(&corrupt).is_err());
}

#[test]
fn tasks_rich_empty_form_keeps_answer_semantics_after_restore() {
    use crate::elicitation::{Form, Request, Requests};
    let question = Request::form(
        "Continue?",
        Form::new(json!({"type":"object","properties":{}})).unwrap(),
    )
    .unwrap();
    let rich = Snapshot::new(
        metadata(),
        State::RichInputRequired(Requests::new(vec![("review".into(), question)]).unwrap()),
    )
    .unwrap();
    let restored = Snapshot::restore(&serde_json::to_value(&rich).unwrap()).unwrap();
    let update_context = context("tasks/update", Some("acme-job"));
    let update = |answer| TaskRequest::Update {
        task_id: TaskId::new("acme-job").unwrap(),
        responses: InputResponses::new(json!({"review":answer})).unwrap(),
    };
    let decline_with_content = update(json!({"action":"decline","content":{"ignored":true}}));
    let before = rich
        .input_answers(&decline_with_content, &update_context, &now())
        .unwrap();
    assert_eq!(
        restored
            .input_answers(&decline_with_content, &update_context, &now())
            .unwrap(),
        before
    );
    let decline = update(json!({"action":"decline"}));
    for snapshot in [&rich, &restored] {
        assert!(
            snapshot
                .input_decision(&decline, &update_context, &now())
                .is_err()
        );
    }
    let basic = Snapshot::new(
        metadata(),
        State::InputRequired(Question::new(&update_context, "review", "Continue?").unwrap()),
    )
    .unwrap();
    assert_ne!(rich, basic);
    assert_eq!(
        Snapshot::restore(&serde_json::to_value(&basic).unwrap()).unwrap(),
        basic
    );
    assert_eq!(serde_json::to_value(&rich).unwrap()["inputKind"], "rich");
    let get = context("tasks/get", Some("acme-job"));
    let reply: Value = serde_json::from_slice(
        &get.encode_task(&json!(1), &TaskResult::Snapshot(rich), 4096, &|| Ok(()))
            .unwrap(),
    )
    .unwrap();
    assert!(reply["result"].get("inputKind").is_none());
}

#[test]
fn tasks_terminal_identity_and_clock_rules_reject_stale_updates() {
    let initial = Snapshot::new(metadata(), State::Working).unwrap();
    let later = time("2026-10-30T12:00:01Z");
    let done = Snapshot::new(
        metadata().updated(later.clone(), 60_000).unwrap(),
        State::Completed(ToolResult::success(vec![])),
    )
    .unwrap();
    assert!(initial.validate_successor(&done, &later).is_ok());
    assert!(done.validate_successor(&done, &later).is_ok());
    assert!(done.validate_successor(&initial, &later).is_err());
    assert!(initial.validate_successor(&done, &now()).is_err());
    let other = Snapshot::new(
        Metadata::new(TaskId::new("other").unwrap(), now(), 60_000).unwrap(),
        State::Working,
    )
    .unwrap();
    assert!(initial.validate_successor(&other, &now()).is_err());
    assert_eq!(
        initial.validate_successor(&done, &time("2026-10-30T12:01:00Z")),
        Err(TaskError::Expired)
    );
}

#[test]
fn schema_task_results_validate_before_persistence_and_after_restore() {
    let catalog = Catalog::new(vec![
        Tool::new(
            "acme_output",
            "Return checked output.",
            json!({"type":"object"}),
        )
        .with_output_schema(json!({"type":"null"})),
    ])
    .unwrap();
    let output = catalog.result_schema("acme_output").unwrap();
    let valid = Snapshot::new(
        metadata(),
        State::Completed(ToolResult::structured(Value::Null, false)),
    )
    .unwrap();
    output.validate_snapshot(&valid).unwrap();
    let restored = Snapshot::restore(&json!(&valid)).unwrap();
    output.validate_snapshot(&restored).unwrap();
    assert_eq!(json!(&restored), json!(&valid));
    let invalid = Snapshot::new(
        metadata(),
        State::Completed(ToolResult::structured(json!("wrong"), false)),
    )
    .unwrap();
    assert!(output.validate_snapshot(&invalid).is_err());
    let creation = RequestContext::parse(
        &request(1, "tools/call", json!({}), capabilities())["params"],
        &ModernOptions::new(ServerInfo::new("acme", "1")),
        "tools/call",
        Some(&options()),
        Some(output.clone()),
    )
    .unwrap();
    assert!(
        creation
            .encode_task(&json!(1), &TaskResult::Created(valid), 4096, &|| Ok(()))
            .is_ok()
    );
    assert!(
        creation
            .encode_task(&json!(1), &TaskResult::Created(invalid), 4096, &|| Ok(()))
            .is_err()
    );
    let no_result =
        Snapshot::new(metadata(), State::Completed(ToolResult::success(vec![]))).unwrap();
    assert!(output.validate_snapshot(&no_result).is_err());
}

#[test]
fn tasks_audio_content_and_annotations_preserve_payloads_after_restore() {
    let hints = crate::ContentAnnotations::new(
        Some(vec![crate::Role::User]),
        Some(0.5),
        Some("2026-10-31T12:00:00.000Z".into()),
    )
    .unwrap();
    let snapshot = Snapshot::new(
        metadata(),
        State::Completed(ToolResult::success(vec![
            Content::audio(&[0, 255, 42], "audio/wav")
                .unwrap()
                .with_annotations(hints.clone()),
            Content::text("Acme").with_annotations(hints.clone()),
            Content::image(&[0, 255], "image/png").with_annotations(hints.clone()),
            Content::resource_link(crate::Resource::new("acme://report", "Report"))
                .unwrap()
                .with_annotations(hints.clone()),
            Content::resource(
                crate::ResourceContents::text("acme://summary", None, "Passed").unwrap(),
            )
            .with_annotations(hints.clone()),
            Content::resource(
                crate::ResourceContents::blob("acme://capture", None, &[0, 255]).unwrap(),
            )
            .with_annotations(hints),
        ])),
    )
    .unwrap();
    let saved = json!(&snapshot);
    assert_eq!(Snapshot::restore(&saved).unwrap(), snapshot);
    assert_eq!(json!(Snapshot::restore(&saved).unwrap()), saved);
    let mut integer_priority = saved.clone();
    integer_priority["result"]["content"][0]["annotations"]["priority"] = json!(1);
    assert_eq!(
        json!(Snapshot::restore(&integer_priority).unwrap()),
        integer_priority
    );
    for value in [
        json!({"priority":2}),
        json!({"audience":["system"]}),
        json!({"priority":0.5,"future":true}),
    ] {
        let mut bad = saved.clone();
        bad["result"]["content"][0]["annotations"] = value;
        assert!(Snapshot::restore(&bad).is_err());
    }
    let full = Content::audio(&vec![0; crate::content::MAX_AUDIO_BYTES], "audio/wav").unwrap();
    assert!(
        Snapshot::new(
            metadata(),
            State::Completed(ToolResult::success(vec![full]))
        )
        .is_err()
    );
}

#[test]
fn tasks_resource_content_survives_restore_without_loss() {
    use crate::{Resource, ResourceContents};
    let snapshot = Snapshot::new(
        metadata(),
        State::Completed(ToolResult::success(vec![
            Content::resource_link(Resource::new("acme://report", "Report")).unwrap(),
            Content::resource(ResourceContents::text("acme://summary", None, "Passed").unwrap()),
            Content::resource(
                ResourceContents::blob("acme://capture", Some("image/png".into()), &[0, 255, 42])
                    .unwrap(),
            ),
        ])),
    )
    .unwrap();
    let saved = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(Snapshot::restore(&saved).unwrap(), snapshot);
    for index in [0, 1, 2] {
        let mut extra = saved.clone();
        if index == 0 {
            extra["result"]["content"][index]["future"] = json!(true);
        } else {
            extra["result"]["content"][index]["resource"]["future"] = json!(true);
        }
        assert!(Snapshot::restore(&extra).is_err());
    }
    let mut malformed = saved;
    malformed["result"]["content"][2]["resource"]["blob"] = json!("AB==");
    assert!(Snapshot::restore(&malformed).is_err());
    // A valid individual block can still exceed the enclosing snapshot budget.
    let full = Content::resource(
        ResourceContents::blob("acme://capture", None, &vec![0; crate::resource::MAX_BYTES])
            .unwrap(),
    );
    assert!(
        Snapshot::new(
            metadata(),
            State::Completed(ToolResult::success(vec![full]))
        )
        .is_err()
    );
}

#[test]
fn tasks_restore_refuses_extra_fields_inside_tool_results() {
    let snapshot = Snapshot::new(
        metadata(),
        State::Completed(ToolResult::success(vec![Content::text("Acme")])),
    )
    .unwrap();
    let value = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(Snapshot::restore(&value).unwrap(), snapshot);
    let mut extra_result = value.clone();
    extra_result["result"]["future"] = json!({"data":"retained"});
    assert!(Snapshot::restore(&extra_result).is_err());
    let mut extra_content = value;
    extra_content["result"]["content"][0]["future"] = json!(true);
    assert!(Snapshot::restore(&extra_content).is_err());
}

#[test]
fn tasks_input_is_pending_only_and_history_refuses_reuse() {
    let ctx = context("tools/call", None);
    let pending = Snapshot::new(
        metadata(),
        State::InputRequired(Question::new(&ctx, "approval-1", "Continue?").unwrap()),
    )
    .unwrap();
    let update = |responses| TaskRequest::Update {
        task_id: metadata().task_id,
        responses: InputResponses::new(responses).unwrap(),
    };
    let ctx = context("tasks/update", Some("acme-job"));
    for (action, decision) in [
        ("accept", Decision::Accepted),
        ("decline", Decision::Declined),
        ("cancel", Decision::Declined),
    ] {
        assert_eq!(
            pending.input_decision(
                &update(json!({"approval-1":{"action":action}})),
                &ctx,
                &now()
            ),
            Ok(Some(decision))
        );
    }
    assert_eq!(
        pending.input_decision(&update(json!({"consumed":{"result":{}}})), &ctx, &now()),
        Ok(None)
    );
    assert!(
        pending
            .input_decision(
                &update(json!({"approval-1":{"action":"wrong"}})),
                &ctx,
                &now()
            )
            .is_err()
    );
    let done = Snapshot::new(metadata(), State::Cancelled).unwrap();
    assert_eq!(
        done.input_decision(
            &update(json!({"approval-1":{"action":"wrong"}})),
            &ctx,
            &now()
        ),
        Ok(None)
    );
    let mut history = InputHistory::default();
    history.issue("approval-1", &options()).unwrap();
    assert_eq!(
        history.issue("approval-1", &options()),
        Err(TaskError::Transition)
    );
    history.issue("approval-2", &options()).unwrap();
    assert_eq!(
        history.issue("approval-3", &options()),
        Err(TaskError::Capacity)
    );
    assert_eq!(
        InputHistory::restore(vec!["x".into(), "x".into()], &options()),
        Err(TaskError::Transition)
    );
}

#[test]
fn tasks_encoders_check_operation_identity_caps_and_output_budget() {
    let snapshot = Snapshot::new(metadata(), State::Working).unwrap();
    let created = TaskResult::Created(snapshot.clone());
    let ctx = context("tools/call", None);
    assert!(ctx.task_eligible());
    let ordinary: Value = serde_json::from_slice(
        &ctx.encode_success(&json!(0), &ToolResult::success(vec![]), 4096, &|| Ok(()))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(ordinary["result"]["resultType"], "complete");
    let bytes = ctx
        .encode_task(&json!(1), &created, 4096, &|| Ok(()))
        .unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["result"]["resultType"], "task");
    assert!(value["result"].get("task").is_none());
    assert!(value["result"].get("cacheScope").is_none());
    assert!(
        ctx.encode_task(&json!(1), &created, bytes.len() - 1, &|| Ok(()))
            .is_err()
    );
    assert_eq!(
        ctx.encode_task(&json!(1), &created, 4096, &|| Err(crate::Interrupted)),
        Err(crate::json::EncodeError::Interrupted)
    );
    for method in [
        "tools/list",
        "resources/read",
        "tasks/get",
        "tasks/update",
        "tasks/cancel",
    ] {
        assert!(
            context(method, Some("acme-job"))
                .encode_task(&json!(1), &created, 4096, &|| Ok(()))
                .is_err()
        );
    }
    let get = context("tasks/get", Some("acme-job"));
    assert!(
        get.encode_task(
            &json!(2),
            &TaskResult::Snapshot(snapshot.clone()),
            4096,
            &|| Ok(())
        )
        .is_ok()
    );
    assert!(
        get.encode_success(&json!(2), &json!({}), 4096, &|| Ok(()))
            .is_err()
    );
    assert!(
        context("tasks/get", Some("other"))
            .encode_task(&json!(2), &TaskResult::Snapshot(snapshot), 4096, &|| Ok(()))
            .is_err()
    );
    let missing = RequestContext::parse(
        &json!({"_meta":{modern::CAPABILITIES_KEY:{}}}),
        &ModernOptions::new(ServerInfo::new("acme", "1")),
        "tools/call",
        Some(&options()),
        None,
    )
    .unwrap();
    assert!(!missing.task_eligible());
    assert_eq!(missing.require_tasks().unwrap_err().code, -32021);
}

#[test]
fn tasks_aggregate_and_input_budgets_refuse_without_truncation() {
    let result = |length| ToolResult::success(vec![Content::text("x".repeat(length))]);
    let base = Snapshot::new(metadata(), State::Completed(result(0))).unwrap();
    let overhead = serde_json::to_vec(&base).unwrap().len();
    let full = Snapshot::new(
        metadata(),
        State::Completed(result(MAX_SNAPSHOT_BYTES - overhead)),
    )
    .unwrap();
    assert_eq!(serde_json::to_vec(&full).unwrap().len(), MAX_SNAPSHOT_BYTES);
    assert_eq!(
        Snapshot::new(
            metadata(),
            State::Completed(result(MAX_SNAPSHOT_BYTES - overhead + 1))
        ),
        Err(TaskError::Limit)
    );
    let ctx = context("tools/call", None);
    let overhead = serde_json::to_vec(&Question::new(&ctx, "q", "").unwrap().value())
        .unwrap()
        .len();
    assert!(Question::new(&ctx, "q", &"x".repeat(MAX_INPUT_BYTES - overhead)).is_ok());
    assert!(Question::new(&ctx, "q", &"x".repeat(MAX_INPUT_BYTES - overhead + 1)).is_err());
    let overhead = serde_json::to_vec(&json!({"q":{"text":""}})).unwrap().len();
    assert!(
        InputResponses::new(json!({"q":{"text":"x".repeat(MAX_RESPONSE_BYTES-overhead)}})).is_ok()
    );
    assert!(
        InputResponses::new(json!({"q":{"text":"x".repeat(MAX_RESPONSE_BYTES-overhead+1)}}))
            .is_err()
    );
    let mut nested = json!(true);
    for _ in 0..33 {
        nested = json!({"x":nested});
    }
    assert!(InputResponses::new(json!({"q":nested})).is_err());
    let mut at_limit = json!(true);
    for _ in 0..32 {
        at_limit = json!({"x":at_limit});
    }
    assert!(InputResponses::new(at_limit.clone()).is_ok());
    assert!(InputResponses::new(json!({"x":at_limit})).is_err());
    for (depth, accepted) in [(62, true), (63, false)] {
        let mut structured = json!(true);
        for _ in 0..depth {
            structured = json!({"x":structured});
        }
        assert_eq!(
            Snapshot::new(
                metadata(),
                State::Completed(ToolResult::structured(structured, false))
            )
            .is_ok(),
            accepted
        );
    }
}
