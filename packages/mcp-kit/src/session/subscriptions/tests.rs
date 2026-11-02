use super::*;
use crate::{
    Catalog, ModernOptions, Resource, ResourceCatalog, ServerInfo, SessionOptions,
    SubscriptionOptions, Tool,
};

fn options() -> SessionOptions {
    SessionOptions {
        modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
        subscriptions: Some(SubscriptionOptions {
            resource_uris: vec!["acme://report".into()],
            ..SubscriptionOptions::default()
        }),
        resources: Some(Arc::new(
            ResourceCatalog::new(vec![Resource::new("acme://report", "report")], vec![]).unwrap(),
        )),
        ..SessionOptions::default()
    }
}

fn session() -> Session {
    Session::try_new(Arc::new(Catalog::new(vec![]).unwrap()), options()).unwrap()
}

fn request(id: Value, method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({crate::modern::VERSION_KEY:crate::modern::PROTOCOL_VERSION,crate::modern::CAPABILITIES_KEY:{}});
    rpc::request(id, method, params)
}

fn listen(session: &mut Session, id: Value, filter: Value) -> Value {
    let mut params = json!({});
    params["notifications"] = filter;
    let Inbound::Subscription { acknowledged, .. } =
        session.receive_value(request(id, "subscriptions/listen", params))
    else {
        panic!("subscription acknowledgement");
    };
    acknowledged
}

#[test]
fn subscriptions_acknowledge_only_supported_explicit_filters() {
    let mut session = session();
    let ack = listen(
        &mut session,
        json!("stream"),
        json!({"toolsListChanged":true,"promptsListChanged":true,"resourcesListChanged":false,"resourceSubscriptions":["acme://unknown","acme://report","acme://report"]}),
    );
    assert_eq!(
        ack,
        rpc::notification(
            "notifications/subscriptions/acknowledged",
            json!({"_meta":{SUBSCRIPTION_KEY:"stream"},"notifications":{"toolsListChanged":true,"resourceSubscriptions":["acme://report"]}})
        )
    );
    assert_eq!(session.outstanding(), 1);
    assert!(session.request_context(&json!("stream")).is_some());
    let discovery = session.receive_value(request(json!(2), "server/discover", json!({})));
    let Inbound::Reply(discovery) = discovery else {
        panic!("discovery")
    };
    assert_eq!(
        discovery["result"]["capabilities"]["tools"]["listChanged"],
        true
    );
    assert_eq!(
        discovery["result"]["capabilities"]["resources"]["subscribe"],
        true
    );
    assert_eq!(
        session
            .publish(Change::ResourceUpdated("acme://unknown".into()))
            .unwrap(),
        Vec::<Value>::new()
    );
    let changed = session
        .publish(Change::ResourceUpdated("acme://report".into()))
        .unwrap();
    assert_eq!(
        changed,
        vec![rpc::notification(
            "notifications/resources/updated",
            json!({"uri":"acme://report","_meta":{SUBSCRIPTION_KEY:"stream"}})
        )]
    );
    listen(&mut session, json!(9), json!({}));
    assert_eq!(
        session
            .publish(Change::Tools(Arc::new(Catalog::new(vec![]).unwrap())))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn subscriptions_refuse_malformed_filters_without_retaining_ids() {
    let mut session = session();
    let mut inputs = vec![
        json!(null),
        json!([]),
        json!({"unknown":true}),
        json!({"toolsListChanged":null}),
        json!({"promptsListChanged":1}),
        json!({"resourcesListChanged":"true"}),
        json!({"resourceSubscriptions":null}),
        json!({"resourceSubscriptions":[false]}),
        json!({"resourceSubscriptions":["relative"]}),
        json!({"resourceSubscriptions":vec!["acme://report";65]}),
        json!({"resourceSubscriptions":[format!("acme://{}", "x".repeat(crate::subscription::MAX_FILTER_BYTES))]}),
    ];
    for filter in inputs.drain(..) {
        let Inbound::Reply(reply) = session.receive_value(request(
            json!(1),
            "subscriptions/listen",
            json!({"notifications":filter}),
        )) else {
            panic!("refusal")
        };
        assert_eq!(reply["error"]["code"], rpc::INVALID_PARAMS);
        assert_eq!(session.outstanding(), 0);
        assert!(session.request_context(&json!(1)).is_none());
    }
    for params in [
        json!({}),
        json!({"notifications":{},"requestState":"retry"}),
        json!({"notifications":{},"inputResponses":{}}),
    ] {
        assert!(matches!(
            session.receive_value(request(json!(1), "subscriptions/listen", params)),
            Inbound::Reply(_)
        ));
        assert_eq!(session.outstanding(), 0);
    }
    listen(
        &mut session,
        json!(1),
        json!({"resourceSubscriptions":vec!["acme://report";64]}),
    );
}

#[test]
fn subscriptions_capacity_duplicate_ids_cancel_and_reuse() {
    let mut options = options();
    options.subscriptions.as_mut().unwrap().max_streams = 1;
    let mut session = Session::try_new(Arc::new(Catalog::new(vec![]).unwrap()), options).unwrap();
    listen(&mut session, json!(7), json!({"toolsListChanged":true}));
    assert!(matches!(
        session.receive_value(request(json!(7), "tools/list", json!({}))),
        Inbound::Fatal { .. }
    ));
    let Inbound::Reply(full) = session.receive_value(request(
        json!("7"),
        "subscriptions/listen",
        json!({"notifications":{}}),
    )) else {
        panic!("capacity")
    };
    assert_eq!(full["error"]["code"], rpc::SERVER_ERROR);
    assert_eq!(
        session.receive_value(rpc::notification(
            "notifications/cancelled",
            json!({"requestId":7})
        )),
        Inbound::Cancel { id: json!(7) }
    );
    assert_eq!(session.outstanding(), 0);
    listen(&mut session, json!("7"), json!({}));
    let closed = session.end_subscription(&json!("7")).unwrap();
    assert_eq!(closed["id"], "7");
    assert_eq!(closed["result"]["_meta"][SUBSCRIPTION_KEY], "7");
    assert_eq!(
        closed["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "acme"
    );
    assert!(session.request_context(&json!("7")).is_none());
}

#[test]
fn subscriptions_expiry_and_abrupt_close_release_all_state() {
    let mut session = session();
    listen(&mut session, json!(1), json!({"toolsListChanged":true}));
    listen(&mut session, json!("other"), json!({}));
    assert!(session.subscription_wait().is_some());
    let expired = session.expire_subscriptions(Instant::now() + Duration::from_secs(301));
    assert_eq!(expired.len(), 2);
    assert!(
        expired
            .iter()
            .all(|reply| reply["result"]["resultType"] == "complete"
                && reply["result"]["_meta"][SUBSCRIPTION_KEY] == reply["id"])
    );
    assert_eq!(session.outstanding(), 0);
    assert_eq!(session.subscription_wait(), None);
    listen(&mut session, json!(1), json!({}));
    session.close();
    assert_eq!(session.outstanding(), 0);
    assert!(session.request_context(&json!(1)).is_none());
    let Inbound::Reply(closed) = session.receive_value(request(
        json!(1),
        "subscriptions/listen",
        json!({"notifications":{}}),
    )) else {
        panic!("closed refusal")
    };
    assert_eq!(closed["error"]["code"], rpc::SERVER_ERROR);
    assert_eq!(session.outstanding(), 0);
    assert!(
        session
            .publish(Change::Tools(Arc::new(Catalog::new(vec![]).unwrap())))
            .is_err()
    );
}

#[test]
fn subscriptions_prompt_fanout_keeps_numeric_and_string_ids_distinct() {
    let mut options = options();
    options.prompts = Some(Arc::new(crate::PromptCatalog::new(vec![]).unwrap()));
    let mut session = Session::try_new(Arc::new(Catalog::new(vec![]).unwrap()), options).unwrap();
    for id in [json!(1), json!("1")] {
        let ack = listen(&mut session, id, json!({"promptsListChanged":true}));
        assert_eq!(ack["params"]["notifications"]["promptsListChanged"], true);
    }
    let frames = session
        .publish(Change::Prompts(Arc::new(
            crate::PromptCatalog::new(vec![crate::Prompt::new("acme_report")]).unwrap(),
        )))
        .unwrap();
    assert_eq!(frames.len(), 2);
    assert!(
        frames
            .iter()
            .all(|frame| frame["method"] == "notifications/prompts/list_changed")
    );
    assert!(
        frames
            .iter()
            .any(|frame| frame["params"]["_meta"][SUBSCRIPTION_KEY] == 1)
    );
    assert!(
        frames
            .iter()
            .any(|frame| frame["params"]["_meta"][SUBSCRIPTION_KEY] == "1")
    );
    let Inbound::Reply(listed) =
        session.receive_value(request(json!(2), "prompts/list", json!({})))
    else {
        panic!("prompt listing")
    };
    assert_eq!(listed["result"]["prompts"][0]["name"], "acme_report");
    assert_eq!(session.end_subscriptions().len(), 2);
    assert_eq!(session.outstanding(), 0);
}

#[test]
fn subscriptions_resource_replacement_preserves_skills_and_refuses_collisions() {
    let mut options = options();
    options.skills = Some(Arc::new(
        crate::SkillCatalog::new(vec![crate::SkillDefinition::new(
            "skill://acme/status",
            vec![crate::SkillFile::text(
                "SKILL.md",
                "---\nname: status\ndescription: Read Acme status.\n---\nInspect status.\n",
            )],
        )])
        .unwrap(),
    ));
    let mut session = Session::try_new(Arc::new(Catalog::new(vec![]).unwrap()), options).unwrap();
    listen(&mut session, json!(1), json!({"resourcesListChanged":true}));
    let replacement =
        Arc::new(ResourceCatalog::new(vec![Resource::new("acme://new", "new")], vec![]).unwrap());
    assert_eq!(
        session
            .publish(Change::Resources(replacement))
            .unwrap()
            .len(),
        1
    );
    let before = session.options.resources.clone();
    let conflicting = Arc::new(
        ResourceCatalog::new(
            vec![Resource::new("skill://acme/status/SKILL.md", "collision")],
            vec![],
        )
        .unwrap(),
    );
    assert!(session.publish(Change::Resources(conflicting)).is_err());
    assert_eq!(session.options.resources, before);
    let Inbound::Reply(listed) =
        session.receive_value(request(json!(2), "resources/list", json!({})))
    else {
        panic!("resource listing")
    };
    let entries = listed["result"]["resources"].as_array().unwrap();
    assert!(entries.iter().any(|entry| entry["uri"] == "acme://new"));
    assert!(
        entries
            .iter()
            .any(|entry| entry["uri"] == "skill://acme/status/SKILL.md")
    );
}

#[test]
fn subscriptions_replace_catalogs_before_notification_and_keep_call_contracts() {
    let mut session = session();
    listen(
        &mut session,
        json!(1),
        json!({"toolsListChanged":true,"resourcesListChanged":true}),
    );
    let tools = Arc::new(
        Catalog::new(vec![
            Tool::new("acme_new", "Read state.", json!({"type":"object"}))
                .with_output_schema(json!({"type":"object"})),
        ])
        .unwrap(),
    );
    let changed = session.publish(Change::Tools(tools)).unwrap();
    assert_eq!(changed[0]["method"], "notifications/tools/list_changed");
    assert!(session.catalog().get("acme_new").is_some());
    assert!(matches!(
        session.receive_value(request(json!(2), "tools/call", json!({"name":"acme_new"}))),
        Inbound::Tool { .. }
    ));
    let context = session.request_context(&json!(2)).unwrap();
    session
        .publish(Change::Tools(Arc::new(Catalog::new(vec![]).unwrap())))
        .unwrap();
    assert!(context.result_schema().is_some());
    assert!(
        context
            .result_schema()
            .unwrap()
            .validate(&crate::ToolResult::structured(json!({}), false))
            .is_ok()
    );
    assert!(
        context
            .result_schema()
            .unwrap()
            .validate(&crate::ToolResult::structured(json!([]), false))
            .is_err()
    );
    let resources =
        Arc::new(ResourceCatalog::new(vec![Resource::new("acme://new", "new")], vec![]).unwrap());
    let changed = session.publish(Change::Resources(resources)).unwrap();
    assert_eq!(changed[0]["method"], "notifications/resources/list_changed");
    let Inbound::Reply(listed) =
        session.receive_value(request(json!(3), "resources/list", json!({})))
    else {
        panic!("listing")
    };
    assert_eq!(listed["result"]["resources"][0]["uri"], "acme://new");
    assert!(
        session
            .publish(Change::ResourceUpdated("relative".into()))
            .is_err()
    );
}

#[test]
fn subscriptions_validate_opt_in_and_finite_policy() {
    let catalog = Arc::new(Catalog::new(vec![]).unwrap());
    for policy in [
        SubscriptionOptions {
            max_streams: 0,
            ..SubscriptionOptions::default()
        },
        SubscriptionOptions {
            max_streams: 65,
            ..SubscriptionOptions::default()
        },
        SubscriptionOptions {
            timeout: Duration::ZERO,
            ..SubscriptionOptions::default()
        },
        SubscriptionOptions {
            timeout: Duration::MAX,
            ..SubscriptionOptions::default()
        },
        SubscriptionOptions {
            resource_uris: vec!["relative".into()],
            ..SubscriptionOptions::default()
        },
    ] {
        let mut options = options();
        options.subscriptions = Some(policy);
        assert!(Session::try_new(catalog.clone(), options).is_err());
    }
    let mut options = options();
    options.modern = None;
    assert!(Session::try_new(catalog.clone(), options).is_err());
    let mut disabled = Session::new(
        catalog,
        SessionOptions {
            modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
            extension_methods: vec!["subscriptions/listen".into()],
            ..SessionOptions::default()
        },
    );
    let Inbound::Reply(reply) = disabled.receive_value(request(
        json!(1),
        "subscriptions/listen",
        json!({"notifications":{}}),
    )) else {
        panic!("disabled")
    };
    assert_eq!(reply["error"]["code"], rpc::METHOD_NOT_FOUND);
}
