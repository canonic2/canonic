use super::*;
use crate::{Catalog, Inbound, Interrupted, ModernOptions, ServerInfo, Session, SessionOptions};
use std::sync::Arc;

fn definition() -> Prompt {
    let mut prompt = Prompt::new("acme_review");
    prompt.arguments = vec![
        PromptArgument::new("code", true),
        PromptArgument::new("language", false),
    ];
    prompt
}

#[test]
fn prompts_catalog_rejects_duplicate_invalid_and_excessive_definitions() {
    assert!(matches!(
        PromptCatalog::new(vec![definition(), definition()]),
        Err(PromptError::Duplicate(_))
    ));
    let mut prompt = definition();
    prompt.arguments.push(prompt.arguments[0].clone());
    assert!(matches!(
        PromptCatalog::new(vec![prompt]),
        Err(PromptError::Duplicate(_))
    ));
    for name in ["", "bad\nname", &"x".repeat(129)] {
        assert!(PromptCatalog::new(vec![Prompt::new(name)]).is_err());
    }
    let mut prompt = definition();
    prompt.description = Some("x".repeat(16385));
    assert!(PromptCatalog::new(vec![prompt]).is_err());
    assert!(
        PromptCatalog::new(
            (0..=MAX_PROMPTS)
                .map(|i| Prompt::new(format!("acme_{i}")))
                .collect()
        )
        .is_err()
    );
    let mut prompt = definition();
    prompt.arguments = (0..=MAX_ARGUMENTS)
        .map(|i| PromptArgument::new(format!("arg{i}"), false))
        .collect();
    assert!(PromptCatalog::new(vec![prompt]).is_err());
    let mut prompt = definition();
    prompt.description = Some("x".repeat(16384));
    assert!(
        PromptCatalog::new(
            (0..100)
                .map(|i| {
                    let mut p = prompt.clone();
                    p.name = format!("acme_{i}");
                    p
                })
                .collect()
        )
        .is_err()
    );
}

#[test]
fn prompts_pages_preserve_order_and_refuse_foreign_cursors() {
    let catalog =
        PromptCatalog::new((0..130).map(|i| Prompt::new(format!("acme_{i}"))).collect()).unwrap();
    let mut page = catalog.listing(&json!({})).unwrap();
    assert_eq!(page["prompts"].as_array().unwrap().len(), PAGE_SIZE);
    assert_eq!(page["prompts"][0]["name"], "acme_0");
    let first = page["nextCursor"].clone();
    page = catalog.listing(&json!({"cursor":first})).unwrap();
    assert_eq!(page["prompts"][0]["name"], "acme_64");
    page = catalog
        .listing(&json!({"cursor":page["nextCursor"]}))
        .unwrap();
    assert_eq!(page["prompts"].as_array().unwrap().len(), 2);
    assert!(page.get("nextCursor").is_none());
    let other = PromptCatalog::new(
        (0..130)
            .map(|i| Prompt::new(format!("other_{i}")))
            .collect(),
    )
    .unwrap();
    for params in [
        json!({"cursor":null}),
        json!({"cursor":"resources:64"}),
        json!({"cursor":format!("prompts:{}:064",catalog.digest)}),
        json!({"cursor":format!("prompts:{}:0",catalog.digest)}),
        json!({"requestState":"state"}),
    ] {
        assert_eq!(
            catalog.listing(&params).unwrap_err().code,
            rpc::INVALID_PARAMS
        );
    }
    assert!(other.listing(&json!({"cursor":first})).is_err());
    assert_eq!(
        PromptCatalog::new(vec![])
            .unwrap()
            .listing(&json!({}))
            .unwrap(),
        json!({"prompts":[]})
    );
}

#[test]
fn prompts_arguments_are_checked_without_interpolation() {
    let catalog = PromptCatalog::new(vec![definition()]).unwrap();
    for params in [
        json!({"name":"missing"}),
        json!({"name":"acme_review"}),
        json!({"name":"acme_review","arguments":null}),
        json!({"name":"acme_review","arguments":{"code":1}}),
        json!({"name":"acme_review","arguments":{"code":"a","unknown":"b"}}),
        json!({"name":"acme_review","arguments":{"code":"a"},"extra":true}),
        json!({"name":"acme_review","arguments":{"code":"a"},"_meta":{"progressToken":null}}),
        json!({"name":"acme_review","arguments":{"code":"x".repeat(MAX_ARGUMENT_BYTES+1)}}),
    ] {
        assert_eq!(
            catalog.call(&params).unwrap_err().code,
            rpc::INVALID_PARAMS,
            "{params}"
        );
    }
    let call = catalog.call(&json!({"name":"acme_review","arguments":{"code":"","language":"{{exact}}\n"},"_meta":{"progressToken":7}})).unwrap();
    assert_eq!(call.arguments["code"], "");
    assert_eq!(call.arguments["language"], "{{exact}}\n");
    assert_eq!(call.progress_token, Some(json!(7)));
    let catalog = PromptCatalog::new(vec![Prompt::new("empty")]).unwrap();
    assert!(
        catalog
            .call(&json!({"name":"empty"}))
            .unwrap()
            .arguments
            .is_empty()
    );
}

#[test]
fn prompts_argument_aggregate_budget_refuses_individually_valid_strings() {
    let mut prompt = Prompt::new("acme_large");
    prompt.arguments = (0..5)
        .map(|i| PromptArgument::new(format!("arg{i}"), false))
        .collect();
    let catalog = PromptCatalog::new(vec![prompt]).unwrap();
    let mut arguments = serde_json::Map::new();
    for i in 0..5 {
        arguments.insert(format!("arg{i}"), json!("x".repeat(MAX_ARGUMENT_BYTES)));
    }
    assert_eq!(
        catalog
            .call(&json!({"name":"acme_large","arguments":arguments}))
            .unwrap_err()
            .code,
        rpc::INVALID_PARAMS
    );
    arguments.retain(|key, _| key == "arg0");
    assert!(
        catalog
            .call(&json!({"name":"acme_large","arguments":arguments}))
            .is_ok()
    );
}

#[test]
fn prompts_results_preserve_content_and_check_legacy_and_encoding_bounds() {
    let hints = crate::ContentAnnotations::new(Some(vec![Role::User]), Some(0.5), None).unwrap();
    let contents = vec![
        Content::text("exact\n").with_annotations(hints),
        Content::image(&[0, 255], "image/png"),
        Content::audio(&[1, 2], "audio/wav").unwrap(),
        Content::resource(crate::ResourceContents::text("acme://report", None, "report").unwrap()),
    ];
    let messages = contents
        .into_iter()
        .map(|content| PromptMessage {
            role: Role::Assistant,
            content,
        })
        .collect();
    let result = GetPromptResult::new(Some("Review".into()), messages).unwrap();
    let bytes = result
        .encode_legacy(&json!(1), MAX_BYTES, &|| Ok(()))
        .unwrap();
    let wire: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(wire["result"]["messages"][0]["content"]["text"], "exact\n");
    assert_eq!(
        wire["result"]["messages"][0]["content"]["annotations"]["priority"],
        0.5
    );
    assert_eq!(wire["result"]["messages"][1]["content"]["data"], "AP8=");
    assert!(wire["result"].get("resultType").is_none());
    assert_eq!(
        result
            .encode_legacy(&json!(1), bytes.len(), &|| Ok(()))
            .unwrap(),
        bytes
    );
    assert!(matches!(
        result.encode_legacy(&json!(1), bytes.len() - 1, &|| Ok(())),
        Err(json::EncodeError::TooLarge { .. })
    ));
    assert_eq!(
        result.encode_legacy(&json!(1), MAX_BYTES, &|| Err(Interrupted)),
        Err(json::EncodeError::Interrupted)
    );
    let link = GetPromptResult::new(
        None,
        vec![PromptMessage {
            role: Role::User,
            content: Content::resource_link(crate::Resource::new("acme://report", "Report"))
                .unwrap(),
        }],
    )
    .unwrap();
    assert_eq!(
        link.encode_legacy(&json!(1), MAX_BYTES, &|| Ok(())),
        Err(json::EncodeError::Invalid)
    );
    for content in [
        Content::Image {
            data: "bad".into(),
            mime_type: "image/png".into(),
            annotations: None,
            meta: None,
        },
        Content::image(&[1], "text/plain"),
        Content::image(&[1], "image/png/extra"),
        Content::text("x".repeat(MAX_BYTES)),
    ] {
        assert!(
            GetPromptResult::new(
                None,
                vec![PromptMessage {
                    role: Role::User,
                    content
                }]
            )
            .is_err()
        );
    }
    assert!(
        GetPromptResult::new(
            None,
            (0..=MAX_MESSAGES)
                .map(|_| PromptMessage {
                    role: Role::User,
                    content: Content::text("")
                })
                .collect()
        )
        .is_err()
    );
}

fn modern(params: Value) -> Value {
    let mut params = params;
    params["_meta"] = json!({crate::modern::VERSION_KEY:crate::modern::PROTOCOL_VERSION,crate::modern::CAPABILITIES_KEY:{"elicitation":{"form":{}}}});
    params
}

#[test]
fn prompts_modern_core_retains_context_and_requires_typed_results() {
    let mut session = Session::try_new(
        Arc::new(Catalog::new(vec![]).unwrap()),
        SessionOptions {
            prompts: Some(Arc::new(PromptCatalog::new(vec![definition()]).unwrap())),
            modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    let request = |id, method, params| rpc::request(json!(id), method, modern(params));
    let Inbound::Reply(list) = session.receive_value(request(1, "prompts/list", json!({}))) else {
        panic!("list")
    };
    assert_eq!(list["result"]["resultType"], "complete");
    assert_eq!(list["result"]["ttlMs"], 0);
    assert_eq!(list["result"]["cacheScope"], "private");
    assert!(!session.ready());
    assert!(matches!(session.receive_value(request(2,"prompts/get",json!({"name":"acme_review","arguments":{"code":"x"},"requestState":"opaque","inputResponses":{"confirmation":{"action":"accept"}}}))), Inbound::Prompt {..}));
    let context = session.request_context(&json!(2)).unwrap();
    assert_eq!(context.request_state.as_deref(), Some("opaque"));
    assert_eq!(
        context.confirmation_decision().unwrap(),
        Some(crate::Decision::Accepted)
    );
    let interim = context.confirmation("Continue?", "state").unwrap();
    assert!(
        context
            .encode_interim(&json!(2), &interim, 4096, &|| Ok(()))
            .is_ok()
    );
    assert_eq!(
        context.encode_success(&json!(2), &json!({"messages":[]}), 4096, &|| Ok(())),
        Err(json::EncodeError::Invalid)
    );
    let result = GetPromptResult::new(
        None,
        vec![PromptMessage {
            role: Role::User,
            content: Content::resource_link(crate::Resource::new("acme://report", "Report"))
                .unwrap(),
        }],
    )
    .unwrap();
    let encoded = context
        .encode_success(&json!(2), &result, 4096, &|| Ok(()))
        .unwrap();
    let wire: Value = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(wire["result"]["resultType"], "complete");
    assert!(wire["result"].get("ttlMs").is_none());
    assert_eq!(
        wire["result"]["messages"][0]["content"]["type"],
        "resource_link"
    );
    assert!(session.finish(&json!(2)));
    assert!(session.request_context(&json!(2)).is_none());
    assert!(
        matches!(session.receive_value(request(3,"prompts/list",json!({"requestState":"not allowed"}))), Inbound::Reply(reply) if reply["error"]["code"]==rpc::INVALID_PARAMS)
    );
}
