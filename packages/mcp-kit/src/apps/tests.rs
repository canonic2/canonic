//! Apps catalog, negotiation, visibility and session composition contracts.
use super::*;
use crate::{Inbound, ModernOptions, ServerInfo, Session, SessionOptions, Tool, ToolUi};
use serde_json::json;
use std::sync::Arc;

fn catalog() -> AppsCatalog {
    AppsCatalog::new(vec![UiResource::new(
        "ui://acme/status",
        "Status",
        b"<!doctype html><p>running</p>\r\n",
    )])
    .unwrap()
}

fn tools() -> Arc<Catalog> {
    Arc::new(
        Catalog::new(vec![
            Tool::new("acme_show", "Show status.", json!({"type":"object"}))
                .with_ui(ToolUi::new("ui://acme/status")),
            Tool::new("acme_refresh", "Refresh status.", json!({"type":"object"})).with_ui(
                ToolUi {
                    resource_uri: None,
                    visibility: Some(vec![UiVisibility::App]),
                },
            ),
        ])
        .unwrap(),
    )
}

fn session() -> Session {
    Session::try_new(
        tools(),
        SessionOptions {
            apps: Some(Arc::new(catalog())),
            modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
            ..SessionOptions::default()
        },
    )
    .unwrap()
}

fn capabilities() -> Value {
    json!({"extensions":{EXTENSION:{"mimeTypes":[MIME_TYPE]}}})
}

fn request(id: u64, method: &str, mut params: Value, apps: bool) -> Value {
    params["_meta"] = json!({crate::modern::VERSION_KEY:crate::modern::PROTOCOL_VERSION,crate::modern::CAPABILITIES_KEY:if apps { capabilities() } else { json!({}) }});
    crate::rpc::request(json!(id), method, params)
}

fn reply(inbound: Inbound) -> Value {
    let Inbound::Reply(value) = inbound else {
        panic!("reply expected")
    };
    value
}

#[test]
fn apps_modern_catalog_reads_and_visibility_are_request_local() {
    let mut session = session();
    for apps in [true, false, true] {
        let list = reply(session.receive_value(request(1, "tools/list", json!({}), apps)));
        assert_eq!(
            list["result"]["tools"].as_array().unwrap().len(),
            if apps { 2 } else { 1 }
        );
        assert_eq!(
            list["result"]["tools"][0]["_meta"]["ui"]["resourceUri"],
            "ui://acme/status"
        );
        let call = session.receive_value(request(
            2,
            "tools/call",
            json!({"name":"acme_refresh"}),
            apps,
        ));
        if apps {
            assert!(matches!(call, Inbound::Tool { .. }));
            assert!(session.supports_apps(Some(&json!(2))));
            session.finish(&json!(2));
        } else {
            assert_eq!(reply(call)["error"]["code"], crate::rpc::INVALID_PARAMS);
        }
    }
    let discovered = reply(session.receive_value(request(1, "server/discover", json!({}), false)));
    assert_eq!(
        discovered["result"]["capabilities"]["extensions"][EXTENSION],
        json!({})
    );
    let Inbound::BundledResource { id, read, .. } = session.receive_value(request(
        3,
        "resources/read",
        json!({"uri":"ui://acme/status"}),
        false,
    )) else {
        panic!("prepared UI read")
    };
    let encoded = session
        .request_context(&id)
        .unwrap()
        .encode_success(&id, read.result(), 4096, &|| Ok(()))
        .unwrap();
    let read: Value = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(
        read["result"]["contents"][0]["text"],
        "<!doctype html><p>running</p>\r\n"
    );
    assert_eq!(read["result"]["contents"][0]["mimeType"], MIME_TYPE);
    session.finish(&id);
    let missing = reply(session.receive_value(request(
        4,
        "resources/read",
        json!({"uri":"ui://acme/missing"}),
        false,
    )));
    assert_eq!(missing["error"]["code"], crate::rpc::INVALID_PARAMS);
    assert_eq!(missing["error"]["data"]["uri"], "ui://acme/missing");
}

#[test]
fn apps_construction_refuses_invalid_documents_and_bounds() {
    for uri in [
        "acme://status/view",
        "ui://acme",
        "ui:///status",
        "ui://acme/status?q=1",
        "ui://acme/status#x",
    ] {
        let error = AppsCatalog::new(vec![UiResource::new(uri, "Status", "HTML")]).unwrap_err();
        assert_eq!(error.uri, uri);
    }
    for bytes in [
        vec![],
        vec![0xff],
        vec![b'x'; crate::resource::MAX_BYTES + 1],
    ] {
        assert!(
            AppsCatalog::new(vec![UiResource::new("ui://acme/status", "Status", bytes)]).is_err()
        );
    }
    let document = UiResource::new("ui://acme/status", "Status", "HTML");
    assert!(AppsCatalog::new(vec![document.clone(), document.clone()]).is_err());
    assert!(AppsCatalog::new(vec![document; MAX_RESOURCES + 1]).is_err());
    let docs = |count, size| {
        (0..count)
            .map(|i| UiResource::new(format!("ui://acme/{i}"), "Status", vec![b'x'; size]))
            .collect()
    };
    assert!(AppsCatalog::new(docs(MAX_RESOURCES, 1)).is_ok());
    assert!(AppsCatalog::new(docs(16, crate::resource::MAX_BYTES)).is_ok());
    assert!(AppsCatalog::new(docs(17, crate::resource::MAX_BYTES)).is_err());
    let mut wrong_mime = UiResource::new("ui://acme/status", "Status", "HTML");
    wrong_mime.resource.mime_type = Some("text/plain".into());
    assert!(AppsCatalog::new(vec![wrong_mime]).is_err());
}

#[test]
fn apps_metadata_is_identical_on_listed_and_read_contents() {
    let ui = json!({"csp":{"connectDomains":["https://api.example.com","wss://events.example.com","ws://[::1]:80"],"resourceDomains":["https://*.example.com"],"frameDomains":["http://localhost:8080"],"baseUriDomains":["https://example.com"]},"permissions":{"camera":{},"microphone":{},"geolocation":{},"clipboardWrite":{}},"domain":"opaque.example.com","prefersBorder":true});
    let mut document = UiResource::new("ui://acme/status", "Status", "<p>café</p>\r\n");
    document.ui = serde_json::from_value(ui.clone()).unwrap();
    document.resource.meta =
        Some(crate::Metadata::new(json!({"com.acme/source":"fixture"})).unwrap());
    let apps = AppsCatalog::new(vec![document]).unwrap();
    let list = apps
        .compose(None, None)
        .unwrap()
        .listing(&json!({}), false)
        .unwrap();
    let read = serde_json::to_value(apps.read("ui://acme/status").unwrap().result()).unwrap();
    assert_eq!(list["resources"][0]["_meta"], read["contents"][0]["_meta"]);
    assert_eq!(read["contents"][0]["_meta"]["ui"], ui);
    assert_eq!(read["contents"][0]["text"], "<p>café</p>\r\n");
    let mut hidden = UiResource::new("ui://acme/private", "Unlisted", "HTML");
    hidden.listed = false;
    let apps = AppsCatalog::new(vec![hidden]).unwrap();
    assert_eq!(
        apps.compose(None, None)
            .unwrap()
            .listing(&json!({}), false)
            .unwrap()["resources"],
        json!([])
    );
    assert!(apps.read("ui://acme/private").is_some());
}

#[test]
fn apps_metadata_refuses_invalid_origins_permissions_and_custom_keys() {
    for (key, origin) in [
        ("connectDomains", "*"),
        ("connectDomains", "http://example.com"),
        ("connectDomains", "https://example.com/path"),
        ("connectDomains", "https://user@example.com"),
        ("connectDomains", "https://example.com?query"),
        ("connectDomains", "https://example.com#fragment"),
        ("connectDomains", "https://[::1]junk"),
        ("connectDomains", "https://example.com:65536"),
        ("connectDomains", "'self'"),
        ("resourceDomains", "wss://example.com"),
        ("frameDomains", "https://*"),
        ("baseUriDomains", "ws://localhost"),
    ] {
        let mut document = UiResource::new("ui://acme/status", "Status", "HTML");
        document.ui = serde_json::from_value(json!({"csp":{key:[origin]}})).unwrap();
        assert!(AppsCatalog::new(vec![document]).is_err(), "{key}: {origin}");
    }
    for ui in [
        json!({"csp":{"connectDomains":["https://example.com","https://example.com"]}}),
        json!({"domain":""}),
        json!({"domain":"bad domain"}),
        json!({"domain":"x".repeat(1025)}),
        json!({"csp":{"resourceDomains":vec!["https://example.com";65]}}),
    ] {
        let mut document = UiResource::new("ui://acme/status", "Status", "HTML");
        document.ui = serde_json::from_value(ui).unwrap();
        assert!(AppsCatalog::new(vec![document]).is_err());
    }
    for ui in [
        json!({"permissions":{"unknown":{}}}),
        json!({"permissions":{"camera":{"allow":true}}}),
        json!({"prefersBorder":"yes"}),
    ] {
        assert!(serde_json::from_value::<UiMetadata>(ui).is_err());
    }
    for key in ["ui", "ui/resourceUri"] {
        let mut document = UiResource::new("ui://acme/status", "Status", "HTML");
        document.resource.meta = Some(crate::Metadata::new(json!({key:{}})).unwrap());
        assert!(AppsCatalog::new(vec![document]).is_err());
        let mut tool = Tool::new("acme_show", "Show.", json!({"type":"object"}));
        tool.meta = Some(crate::Metadata::new(json!({key:{}})).unwrap());
        assert!(Catalog::new(vec![tool]).is_err());
    }
}

#[test]
fn apps_legacy_negotiation_and_lookup_errors_preserve_each_revision() {
    for version in crate::SUPPORTED_PROTOCOL_VERSIONS {
        for supports in [false, true] {
            let mut session = session();
            let caps = if supports { capabilities() } else { json!({}) };
            let Inbound::Initialize { id, .. } = session.receive_value(crate::rpc::request(json!(1), "initialize", json!({"protocolVersion":version,"capabilities":caps,"clientInfo":{"name":"acme-host","version":"1"}}))) else { panic!("initialize") };
            let init = session.accept_initialize(id, &ServerInfo::new("acme", "1"));
            assert_eq!(
                init["result"]["capabilities"]["extensions"]
                    .get(EXTENSION)
                    .is_some(),
                *version == "2025-11-25"
            );
            session.receive_value(crate::rpc::notification(
                "notifications/initialized",
                json!({}),
            ));
            let listed = reply(session.receive_value(crate::rpc::request(
                json!(2),
                "tools/list",
                json!({}),
            )));
            assert_eq!(
                listed["result"]["tools"].as_array().unwrap().len(),
                if supports && *version == "2025-11-25" {
                    2
                } else {
                    1
                }
            );
            let missing = reply(session.receive_value(crate::rpc::request(
                json!(3),
                "resources/read",
                json!({"uri":"ui://acme/missing"}),
            )));
            assert_eq!(missing["error"]["code"], crate::rpc::RESOURCE_NOT_FOUND);
            assert_eq!(missing["error"]["data"]["uri"], "ui://acme/missing");
            assert!(matches!(
                session.receive_value(crate::rpc::request(
                    json!(4),
                    "resources/read",
                    json!({"uri":"ui://acme/status"})
                )),
                Inbound::BundledResource { .. }
            ));
        }
    }
}

#[test]
fn apps_malformed_capabilities_refuse_before_admission() {
    for declaration in [
        json!(false),
        json!({}),
        json!({"mimeTypes":false}),
        json!({"mimeTypes":[1]}),
        json!({"mimeTypes":[""]}),
        json!({"mimeTypes":vec![MIME_TYPE;33]}),
        json!({"mimeTypes":["x".repeat(257)]}),
    ] {
        let caps = json!({"extensions":{EXTENSION:declaration}});
        assert!(client_support(&caps).is_err());
        let mut session = session();
        let refused = reply(session.receive_value(crate::rpc::request(json!(1), "initialize", json!({"protocolVersion":"2025-11-25","capabilities":caps,"clientInfo":{"name":"acme-host","version":"1"}}))));
        assert_eq!(refused["error"]["code"], crate::rpc::INVALID_PARAMS);
        let refused = reply(session.receive_value(crate::rpc::request(json!(1), "tools/list", json!({"_meta":{crate::modern::VERSION_KEY:crate::modern::PROTOCOL_VERSION,crate::modern::CAPABILITIES_KEY:caps}}))));
        assert_eq!(refused["error"]["code"], crate::rpc::INVALID_PARAMS);
        assert_eq!(session.outstanding(), 0);
    }
    for types in [json!([]), json!(["text/plain"])] {
        assert!(!client_support(&json!({"extensions":{EXTENSION:{"mimeTypes":types}}})).unwrap());
    }
}

#[test]
fn apps_disabled_sessions_leave_malformed_app_settings_opaque() {
    for declaration in [
        json!({}),
        json!({"mimeTypes":false}),
        json!({"mimeTypes":[1]}),
    ] {
        let caps = json!({"extensions":{EXTENSION:declaration}});
        for version in crate::SUPPORTED_PROTOCOL_VERSIONS {
            let tools = Arc::new(
                Catalog::new(vec![Tool::new(
                    "acme_status",
                    "Status.",
                    json!({"type":"object"}),
                )])
                .unwrap(),
            );
            let mut session = Session::try_new(
                tools,
                SessionOptions {
                    modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
                    ..SessionOptions::default()
                },
            )
            .unwrap();
            let Inbound::Initialize {id, ..} = session.receive_value(crate::rpc::request(json!(1), "initialize", json!({"protocolVersion":version,"capabilities":caps,"clientInfo":{"name":"acme-host","version":"1"}}))) else {panic!("opaque Apps settings must not refuse initialization")};
            assert!(
                session
                    .accept_initialize(id, &ServerInfo::new("acme", "1"))
                    .get("result")
                    .is_some()
            );
            session.receive_value(crate::rpc::notification(
                "notifications/initialized",
                json!({}),
            ));
            let modern = reply(session.receive_value(crate::rpc::request(json!(2), "tools/list", json!({"_meta":{crate::modern::VERSION_KEY:crate::modern::PROTOCOL_VERSION,crate::modern::CAPABILITIES_KEY:caps}}))));
            assert_eq!(modern["result"]["tools"][0]["name"], "acme_status");
            assert!(modern.get("error").is_none());
        }
    }
}

#[test]
fn apps_oldest_revision_does_not_negotiate_or_validate_apps() {
    for declaration in [json!(false), json!({}), json!({"mimeTypes":[1]})] {
        let mut session = session();
        let Inbound::Initialize {id, ..} = session.receive_value(crate::rpc::request(json!(1), "initialize", json!({"protocolVersion":"2025-06-18","capabilities":{"extensions":{EXTENSION:declaration}},"clientInfo":{"name":"acme-host","version":"1"}}))) else {panic!("oldest revision has no Apps negotiation")};
        let initialized = session.accept_initialize(id, &ServerInfo::new("acme", "1"));
        assert!(
            initialized["result"]["capabilities"]["extensions"]
                .get(EXTENSION)
                .is_none()
        );
        session.receive_value(crate::rpc::notification(
            "notifications/initialized",
            json!({}),
        ));
        assert!(!session.supports_apps(None));
        let list =
            reply(session.receive_value(crate::rpc::request(json!(2), "tools/list", json!({}))));
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn apps_visibility_is_filtered_before_paging_and_cursors_bind_to_the_view() {
    let tools = Catalog::new(
        (0..140)
            .map(|i| {
                let mut tool = Tool::new(format!("acme_{i}"), "Report.", json!({"type":"object"}));
                if i % 2 == 1 {
                    tool.ui = Some(ToolUi {
                        visibility: Some(vec![UiVisibility::App]),
                        ..ToolUi::default()
                    });
                }
                tool
            })
            .collect(),
    )
    .unwrap();
    for version in ["2025-11-25", crate::modern::PROTOCOL_VERSION] {
        for apps in [false, true] {
            let mut params = json!({});
            let mut names = Vec::new();
            loop {
                crate::test_work::reset();
                let page = tools.list_page_for_client(&params, version, apps).unwrap();
                assert_eq!(crate::test_work::counts().preparations, 0);
                assert_eq!(
                    crate::test_work::counts().page_definitions,
                    page["tools"].as_array().unwrap().len()
                );
                for tool in page["tools"].as_array().unwrap() {
                    names.push(tool["name"].as_str().unwrap().to_owned());
                }
                let Some(cursor) = page.get("nextCursor") else {
                    break;
                };
                params = json!({"cursor":cursor});
                assert!(tools.list_page_for_client(&params, version, !apps).is_err());
            }
            let expected: Vec<_> = (0..140)
                .filter(|i| apps || i % 2 == 0)
                .map(|i| format!("acme_{i}"))
                .collect();
            assert_eq!(names, expected);
        }
    }
}

#[test]
fn apps_configuration_and_replacement_are_atomic() {
    assert!(Session::try_new(tools(), SessionOptions::default()).is_err());
    let missing = Arc::new(
        Catalog::new(vec![
            Tool::new("acme_missing", "Missing.", json!({"type":"object"}))
                .with_ui(ToolUi::new("ui://acme/missing")),
        ])
        .unwrap(),
    );
    let options = || SessionOptions {
        apps: Some(Arc::new(catalog())),
        modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
        subscriptions: Some(crate::SubscriptionOptions::default()),
        ..SessionOptions::default()
    };
    assert!(Session::try_new(missing.clone(), options()).is_err());
    let mut session = Session::try_new(tools(), options()).unwrap();
    let old = session.catalog().clone();
    assert!(session.publish(crate::Change::Tools(missing)).is_err());
    assert!(Arc::ptr_eq(session.catalog(), &old));
    let ordinary = Arc::new(
        ResourceCatalog::new(vec![Resource::new("ui://acme/foreign", "Foreign")], vec![]).unwrap(),
    );
    assert!(
        session
            .publish(crate::Change::Resources(ordinary.clone()))
            .is_err()
    );
    assert!(
        Session::try_new(
            tools(),
            SessionOptions {
                resources: Some(ordinary),
                ..options()
            }
        )
        .is_err()
    );
    let templates = Arc::new(
        ResourceCatalog::new(
            vec![],
            vec![crate::ResourceTemplate::new("ui://acme/{id}", "UI")],
        )
        .unwrap(),
    );
    assert!(
        Session::try_new(
            tools(),
            SessionOptions {
                resources: Some(templates),
                ..options()
            }
        )
        .is_err()
    );
    let mut subscribed = options();
    subscribed
        .subscriptions
        .as_mut()
        .unwrap()
        .resource_uris
        .push("ui://acme/status".into());
    assert!(Session::try_new(tools(), subscribed).is_err());
    let mut declared = options();
    declared.extensions.insert(EXTENSION.into(), json!({}));
    assert!(Session::try_new(tools(), declared).is_err());
    for notification in [false, true] {
        let mut reserved = options();
        if notification {
            reserved
                .extension_notifications
                .push("ui/initialize".into());
        } else {
            reserved.extension_methods.push("ui/initialize".into());
        }
        assert!(Session::try_new(tools(), reserved).is_err());
    }
    let changed = Arc::new(
        ResourceCatalog::new(vec![Resource::new("acme://report", "Report")], vec![]).unwrap(),
    );
    session.publish(crate::Change::Resources(changed)).unwrap();
    let listed = reply(session.receive_value(request(1, "resources/list", json!({}), false)));
    assert_eq!(listed["result"]["resources"][0]["uri"], "acme://report");
    assert_eq!(listed["result"]["resources"][1]["uri"], "ui://acme/status");
}

#[test]
fn apps_linked_tool_results_keep_fallback_content_even_in_durable_snapshots() {
    let contract = tools().result_schema("acme_show").unwrap();
    let empty = crate::ToolResult::success(vec![]);
    assert!(contract.validate(&empty).is_err());
    assert!(
        contract
            .encode_legacy(&json!(1), &empty, 4096, &|| Ok(()))
            .is_err()
    );
    let context = crate::RequestContext::parse(
        &json!({"_meta":{crate::modern::CAPABILITIES_KEY:{}}}),
        &ModernOptions::new(ServerInfo::new("acme", "1")),
        "tools/call",
        None,
        Some(contract.clone()),
    )
    .unwrap();
    assert!(
        context
            .encode_success(&json!(1), &empty, 4096, &|| Ok(()))
            .is_err()
    );
    let metadata = crate::task::Metadata::new(
        crate::task::TaskId::new("acme-task").unwrap(),
        crate::task::Timestamp::parse("2026-11-05T00:00:00Z").unwrap(),
        1000,
    )
    .unwrap();
    let snapshot =
        crate::task::Snapshot::new(metadata, crate::task::State::Completed(empty)).unwrap();
    assert!(contract.validate_snapshot(&snapshot).is_err());
    let restored =
        crate::task::Snapshot::restore(&serde_json::to_value(snapshot).unwrap()).unwrap();
    assert!(contract.validate_snapshot(&restored).is_err());
    let result = crate::ToolResult::success(vec![crate::Content::text("Acme is running.")]);
    assert!(contract.validate(&result).is_ok());
}

#[test]
fn apps_reads_reuse_prepared_content_and_refuse_retry_fields() {
    let catalog = catalog();
    let first = catalog.read("ui://acme/status").unwrap();
    let second = catalog.clone().read("ui://acme/status").unwrap();
    assert!(std::ptr::eq(first.result(), second.result()));
    let mut session = Session::try_new(
        tools(),
        SessionOptions {
            apps: Some(Arc::new(catalog)),
            apps_cache: crate::CachePolicy::new(1000, crate::CacheScope::Public).unwrap(),
            modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    let bad = reply(session.receive_value(request(
        1,
        "resources/read",
        json!({"uri":"ui://acme/status","requestState":"opaque"}),
        false,
    )));
    assert_eq!(bad["error"]["code"], crate::rpc::INVALID_PARAMS);
    let Inbound::BundledResource { id, read, .. } = session.receive_value(request(
        1,
        "resources/read",
        json!({"uri":"ui://acme/status"}),
        false,
    )) else {
        panic!("UI read")
    };
    let bytes = session
        .request_context(&id)
        .unwrap()
        .encode_success(&id, read.result(), 4096, &|| Ok(()))
        .unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["result"]["ttlMs"], 1000);
    assert_eq!(value["result"]["cacheScope"], "public");
}

#[test]
fn apps_tool_metadata_bounds_and_legacy_view_are_enforced() {
    for visibility in [
        vec![],
        vec![UiVisibility::App, UiVisibility::App],
        vec![UiVisibility::Model, UiVisibility::App, UiVisibility::Model],
    ] {
        let mut tool = Tool::new("acme_show", "Show.", json!({"type":"object"}));
        tool.ui = Some(ToolUi {
            visibility: Some(visibility),
            ..ToolUi::default()
        });
        assert!(Catalog::new(vec![tool]).is_err());
    }
    let mut tool = Tool::new("acme_show", "Show.", json!({"type":"object"}))
        .with_ui(ToolUi::new("ui://acme/status"));
    tool.meta = Some(
        crate::Metadata::new(
            json!({"com.acme/data":"x".repeat(crate::metadata::MAX_METADATA_BYTES - 30)}),
        )
        .unwrap(),
    );
    assert!(Catalog::new(vec![tool]).is_err());
    let mut resource = UiResource::new("ui://acme/status", "Status", "HTML");
    resource.resource.meta = Some(
        crate::Metadata::new(
            json!({"com.acme/data":"x".repeat(crate::metadata::MAX_METADATA_BYTES - 30)}),
        )
        .unwrap(),
    );
    resource.ui.domain = Some("x".repeat(1024));
    assert!(AppsCatalog::new(vec![resource]).is_err());
    assert_eq!(
        tools()
            .list_page_for_client(&json!({}), "2025-06-18", true)
            .unwrap()["tools"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn apps_skill_composition_and_combined_catalog_bounds() {
    let skills = |scheme: &str| {
        Arc::new(
            crate::SkillCatalog::new(vec![crate::SkillDefinition::new(
                format!("{scheme}://acme/status"),
                vec![crate::SkillFile::text(
                    "SKILL.md",
                    "---\nname: status\ndescription: Inspect status.\n---\nInspect Acme.",
                )],
            )])
            .unwrap(),
        )
    };
    let mut options = SessionOptions {
        apps: Some(Arc::new(catalog())),
        skills: Some(skills("ui")),
        modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
        ..SessionOptions::default()
    };
    assert!(Session::try_new(tools(), options.clone()).is_err());
    options.skills = Some(skills("skill"));
    options.resources = Some(Arc::new(
        ResourceCatalog::new(vec![Resource::new("acme://report", "Report")], vec![]).unwrap(),
    ));
    let mut session = Session::try_new(tools(), options.clone()).unwrap();
    let list = reply(session.receive_value(request(1, "resources/list", json!({}), false)));
    let uris: Vec<_> = list["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|resource| resource["uri"].as_str().unwrap())
        .collect();
    assert_eq!(
        uris,
        vec![
            "acme://report",
            "ui://acme/status",
            "skill://acme/status/SKILL.md"
        ]
    );
    options.resources = Some(Arc::new(
        ResourceCatalog::new(
            (0..1023)
                .map(|i| Resource::new(format!("acme://report/{i}"), "Report"))
                .collect(),
            vec![],
        )
        .unwrap(),
    ));
    assert!(Session::try_new(tools(), options).is_err());
    let options = SessionOptions {
        resources: Some(Arc::new(
            ResourceCatalog::new(
                vec![Resource::new("ui://acme/ordinary", "Ordinary")],
                vec![],
            )
            .unwrap(),
        )),
        modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
        ..SessionOptions::default()
    };
    let mut ordinary = Session::try_new(Arc::new(Catalog::new(vec![]).unwrap()), options).unwrap();
    assert!(matches!(
        ordinary.receive_value(request(
            1,
            "resources/read",
            json!({"uri":"ui://acme/ordinary"}),
            false
        )),
        Inbound::Method { method, .. } if method == "resources/read"
    ));
}

#[test]
fn apps_prepared_read_work_and_ownership_do_not_grow_with_catalog_or_history() {
    let mut baseline = None;
    for size in [1, MAX_RESOURCES] {
        let mut definitions = vec![UiResource::new(
            "ui://acme/status",
            "Status",
            "<p>Acme</p>".repeat(256),
        )];
        definitions.extend(
            (1..size).map(|i| UiResource::new(format!("ui://acme/{i}"), "Other", "x".repeat(4096))),
        );
        let apps = Arc::new(AppsCatalog::new(definitions).unwrap());
        let prepared = apps.read("ui://acme/status").unwrap();
        let mut session = Session::try_new(
            tools(),
            SessionOptions {
                apps: Some(apps),
                modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
                ..SessionOptions::default()
            },
        )
        .unwrap();
        for _ in 0..128 {
            let message = request(1, "resources/read", json!({"uri":"ui://acme/status"}), true);
            crate::test_work::reset();
            let Inbound::BundledResource { id, read, .. } = session.receive_value(message) else {
                panic!("prepared read")
            };
            assert!(std::ptr::eq(read.result(), prepared.result()));
            let context = session.request_context(&id).unwrap();
            let weak = Arc::downgrade(&context);
            let bytes = context
                .encode_success(&id, read.result(), 16384, &|| Ok(()))
                .unwrap();
            assert!(session.finish(&id));
            drop(context);
            assert!(weak.upgrade().is_none());
            assert_eq!(session.outstanding(), 0);
            let counts = crate::test_work::counts();
            assert_eq!(counts.preparations, 0);
            assert_eq!(counts.page_definitions, 0);
            assert_eq!(counts.encodings, 1);
            if let Some(baseline) = &baseline {
                assert_eq!(&(bytes, counts), baseline);
            } else {
                baseline = Some((bytes, counts));
            }
        }
    }
}

#[test]
fn apps_metadata_and_negotiation_accept_exact_limits_and_refuse_one_more() {
    let mut document = UiResource::new("ui://acme/status", "Status", "HTML");
    let origin = format!(
        "https://{}.{}",
        vec!["a".repeat(63); 15].join("."),
        "a".repeat(56)
    );
    assert_eq!(origin.len(), 1024);
    document.ui.csp = Some(UiCsp {
        resource_domains: vec![origin.clone()],
        connect_domains: (0..64)
            .map(|i| format!("https://api{i}.example.com"))
            .collect(),
        ..UiCsp::default()
    });
    document.ui.domain = Some("a".repeat(1024));
    assert!(AppsCatalog::new(vec![document.clone()]).is_ok());
    document.ui.csp.as_mut().unwrap().resource_domains = vec![format!("{origin}a")];
    assert!(AppsCatalog::new(vec![document]).is_err());
    let mut types = vec!["a".repeat(256); 31];
    types.push(MIME_TYPE.into());
    assert!(client_support(&json!({"extensions":{EXTENSION:{"mimeTypes":types}}})).unwrap());
    let ui = ToolUi::new("ui://acme/status");
    let custom = crate::Metadata::new(json!({"com.acme/data":""})).unwrap();
    let overhead = serde_json::to_vec(&WithUi {
        custom: Some(&custom),
        ui: Some(&ui),
    })
    .unwrap()
    .len();
    let mut tool = Tool::new("acme_show", "Show.", json!({"type":"object"})).with_ui(ui);
    tool.meta = Some(
        crate::Metadata::new(
            json!({"com.acme/data":"x".repeat(crate::metadata::MAX_METADATA_BYTES - overhead)}),
        )
        .unwrap(),
    );
    assert!(Catalog::new(vec![tool.clone()]).is_ok());
    tool.meta = Some(
        crate::Metadata::new(
            json!({"com.acme/data":"x".repeat(crate::metadata::MAX_METADATA_BYTES - overhead + 1)}),
        )
        .unwrap(),
    );
    assert!(Catalog::new(vec![tool]).is_err());
}
