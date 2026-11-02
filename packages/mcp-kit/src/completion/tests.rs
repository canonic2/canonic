use super::*;
use crate::{
    Catalog, Inbound, Interrupted, ModernOptions, Prompt, PromptArgument, ResourceTemplate,
    ServerInfo, Session, SessionOptions,
};
use serde_json::json;
use std::sync::Arc;

fn catalogs() -> (PromptCatalog, ResourceCatalog) {
    let mut prompt = Prompt::new("acme_review");
    prompt.arguments = vec![
        PromptArgument::new("language", true),
        PromptArgument::new("framework", true),
    ];
    (
        PromptCatalog::new(vec![prompt]).expect("valid prompts"),
        ResourceCatalog::new(
            vec![],
            vec![ResourceTemplate::new(
                "acme://reports{/path*}{?query:4,other,a%20b}",
                "Reports",
            )],
        )
        .expect("valid resources"),
    )
}

fn params() -> Value {
    json!({"ref":{"type":"ref/prompt","name":"acme_review","title":"Acme"},"argument":{"name":"framework","value":"fl"},"context":{"arguments":{"language":"python"}}})
}

#[test]
fn completion_preserves_context_and_handles_template_modifiers() {
    let (prompts, resources) = catalogs();
    let mut input = params();
    input["argument"]["value"] = json!("fl\n🦀");
    let request = CompleteRequest::parse(&input, Some(&prompts), Some(&resources)).unwrap();
    assert_eq!(request.argument.value, "fl\n🦀");
    assert_eq!(request.context["language"], "python");
    assert!(CompleteRequest::parse(&json!({"ref":{"type":"ref/prompt","name":"acme_review"},"argument":{"name":"language","value":""}}),Some(&prompts),None).is_ok());
    for name in ["path", "query", "other", "a%20b"] {
        let input = json!({"ref":{"type":"ref/resource","uri":"acme://reports{/path*}{?query:4,other,a%20b}"},"argument":{"name":name,"value":""},"context":{"arguments":{"path":"one/two"}}});
        assert!(
            CompleteRequest::parse(&input, None, Some(&resources)).is_ok(),
            "{name}"
        );
    }
}

#[test]
fn completion_refuses_unknown_nested_shapes_and_catalog_targets() {
    let (prompts, resources) = catalogs();
    for input in [
        json!({}),
        json!({"ref":{"type":"ref/tool","name":"acme_review"},"argument":{"name":"language","value":""}}),
        json!({"ref":{"type":"ref/prompt","name":"unknown"},"argument":{"name":"language","value":""}}),
        json!({"ref":{"type":"ref/resource","uri":"acme://reports/one"},"argument":{"name":"path","value":""}}),
        json!({"ref":{"type":"ref/prompt","name":"acme_review","title":null},"argument":{"name":"language","value":""}}),
    ] {
        assert_eq!(
            CompleteRequest::parse(&input, Some(&prompts), Some(&resources))
                .unwrap_err()
                .code,
            rpc::INVALID_PARAMS
        );
    }
    for (path, value) in [
        (
            "ref",
            json!({"type":"ref/prompt","name":"acme_review","extra":true}),
        ),
        ("argument", json!({"name":"unknown","value":""})),
        ("argument", json!({"name":"language","value":null})),
        (
            "argument",
            json!({"name":"language","value":"","extra":true}),
        ),
        ("context", Value::Null),
        ("context", json!({"arguments":null})),
        ("context", json!({"extra":true})),
        ("context", json!({"arguments":{"language":false}})),
        ("context", json!({"arguments":{"unknown":"x"}})),
        ("requestState", json!("not a retry")),
        ("inputResponses", json!({})),
        ("_meta", json!({"progressToken":false})),
    ] {
        let mut input = params();
        input[path] = value;
        assert_eq!(
            CompleteRequest::parse(&input, Some(&prompts), Some(&resources))
                .unwrap_err()
                .code,
            rpc::INVALID_PARAMS,
            "{input}"
        );
    }
}

#[test]
fn completion_input_budgets_refuse_without_truncation() {
    let (prompts, _) = catalogs();
    let mut input = params();
    input["argument"]["value"] = json!("x".repeat(MAX_INPUT_VALUE_BYTES));
    assert!(CompleteRequest::parse(&input, Some(&prompts), None).is_ok());
    input["argument"]["value"] = json!("x".repeat(MAX_INPUT_VALUE_BYTES + 1));
    assert!(CompleteRequest::parse(&input, Some(&prompts), None).is_err());
    input = params();
    input["ref"]["title"] = json!("x".repeat(4097));
    assert!(CompleteRequest::parse(&input, Some(&prompts), None).is_err());
    input = params();
    input["context"]["arguments"]["language"] = json!("x".repeat(MAX_INPUT_VALUE_BYTES + 1));
    assert!(CompleteRequest::parse(&input, Some(&prompts), None).is_err());
    input["context"]["arguments"] = json!(
        (0..65)
            .map(|i| (format!("arg{i}"), json!("")))
            .collect::<serde_json::Map<_, _>>()
    );
    assert!(CompleteRequest::parse(&input, Some(&prompts), None).is_err());
    let mut prompt = Prompt::new("acme_large");
    prompt.arguments = (0..5)
        .map(|i| PromptArgument::new(format!("arg{i}"), false))
        .collect();
    let prompts = PromptCatalog::new(vec![prompt]).unwrap();
    let input = json!({"ref":{"type":"ref/prompt","name":"acme_large"},"argument":{"name":"arg0","value":""},"context":{"arguments":(0..5).map(|i|(format!("arg{i}"),json!("x".repeat(MAX_INPUT_VALUE_BYTES)))).collect::<serde_json::Map<_,_>>()}});
    assert!(CompleteRequest::parse(&input, Some(&prompts), None).is_err());
}

#[test]
fn completion_exact_input_byte_boundary_excludes_metadata() {
    let mut prompt = Prompt::new("acme_large");
    prompt.arguments = (0..4)
        .map(|i| PromptArgument::new(format!("arg{i}"), false))
        .collect();
    let prompts = PromptCatalog::new(vec![prompt]).unwrap();
    let mut input = json!({"ref":{"type":"ref/prompt","name":"acme_large"},"argument":{"name":"arg3","value":""},"context":{"arguments":(0..3).map(|i|(format!("arg{i}"),json!("x".repeat(MAX_INPUT_VALUE_BYTES)))).collect::<serde_json::Map<_,_>>()}});
    let remaining = MAX_BYTES - serde_json::to_vec(&input).unwrap().len();
    assert!(remaining < MAX_INPUT_VALUE_BYTES);
    input["argument"]["value"] = json!("x".repeat(remaining));
    assert_eq!(serde_json::to_vec(&input).unwrap().len(), MAX_BYTES);
    input["_meta"] = json!({"acme/diagnostic":"x".repeat(16384)});
    assert!(CompleteRequest::parse(&input, Some(&prompts), None).is_ok());
    input["argument"]["value"] = json!("x".repeat(remaining + 1));
    assert!(CompleteRequest::parse(&input, Some(&prompts), None).is_err());
}

#[test]
fn completion_results_preserve_order_and_enforce_count_and_byte_budgets() {
    let result = CompleteResult::new(
        vec!["z".into(), "a\n".into(), "z".into()],
        Some(4),
        Some(true),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({"completion":{"values":["z","a\n","z"],"total":4,"hasMore":true}})
    );
    for (values, total, more) in [
        (vec![], Some(0), Some(false)),
        (vec![], None, Some(true)),
        (vec!["x".into()], None, None),
        (vec!["x".into(); 100], Some(100), Some(false)),
    ] {
        assert!(CompleteResult::new(values, total, more).is_ok());
    }
    for (total, more) in [
        (Some(0), None),
        (Some(1), Some(true)),
        (Some(2), Some(false)),
        (Some(MAX_TOTAL + 1), None),
    ] {
        assert_eq!(
            CompleteResult::new(vec!["x".into()], total, more),
            Err(CompletionError::Counts)
        );
    }
    assert!(CompleteResult::new(vec!["x".into(); 101], None, None).is_err());
    assert!(CompleteResult::new(vec!["x".repeat(MAX_VALUE_BYTES + 1)], None, None).is_err());
    assert!(CompleteResult::new(vec!["x".repeat(MAX_VALUE_BYTES); 100], None, None).is_err());
    let bytes = result.encode_legacy(&json!(1), 4096, &|| Ok(())).unwrap();
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
        result.encode_legacy(&json!(1), 4096, &|| Err(Interrupted)),
        Err(json::EncodeError::Interrupted)
    );
}

#[test]
fn completion_modern_encoding_is_typed_and_has_no_input_round_or_cache() {
    let (prompts, _) = catalogs();
    let mut session = Session::try_new(
        Arc::new(Catalog::new(vec![]).unwrap()),
        SessionOptions {
            prompts: Some(Arc::new(prompts)),
            completions: true,
            modern: Some(ModernOptions::new(ServerInfo::new("acme", "1"))),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    let mut input = params();
    input["_meta"] = json!({crate::modern::VERSION_KEY:crate::modern::PROTOCOL_VERSION,crate::modern::CAPABILITIES_KEY:{"elicitation":{"form":{}}}});
    assert!(matches!(
        session.receive_value(rpc::request(json!(1), "completion/complete", input)),
        Inbound::Completion { .. }
    ));
    let context = session.request_context(&json!(1)).unwrap();
    assert!(context.confirmation("Proceed?", "state").is_err());
    assert_eq!(
        context.encode_success(
            &json!(1),
            &json!({"completion":{"values":[]}}),
            4096,
            &|| Ok(())
        ),
        Err(json::EncodeError::Invalid)
    );
    let result = CompleteResult::new(vec!["flask".into()], Some(1), Some(false)).unwrap();
    let bytes = context
        .encode_success(&json!(1), &result, 4096, &|| Ok(()))
        .unwrap();
    let wire: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(wire["result"]["resultType"], "complete");
    assert_eq!(
        wire["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "acme"
    );
    assert!(wire["result"].get("ttlMs").is_none());
    assert_eq!(
        context.encode_success(&json!(1), &result, 4096, &|| Err(Interrupted)),
        Err(json::EncodeError::Interrupted)
    );
    assert!(session.finish(&json!(1)));
    assert!(!session.ready());
}
