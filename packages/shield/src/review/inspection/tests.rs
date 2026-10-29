//! Real enrolled MCP inspection of large admitted packs and compact rule references.
use super::*;
use crate::{
    configuration::Bundle,
    review::{self, mcp, storage::Store},
};
use std::collections::BTreeMap;

const SELECTOR: &str = "pack:00000000-0000-0000-0000-000000000001";

#[test]
fn cli_and_mcp_inspect_effective_local_custom_rules() {
    let root = assert_fs::TempDir::new().unwrap();
    let document = |rules: Value| {
        json!({
            "schema_version":1,"kind":"custom",
            "capabilities":["structured-rules-v1","argument-rules-v1"],"rules":rules
        })
        .to_string()
    };
    let rule = |id: &str, decision: &str| {
        json!({
            "id":id,"decision":decision,
            "conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":[id]}}
        })
    };
    let shared = document(json!([rule("shared", "allow"), rule("replaced", "deny")]));
    let local = document(json!([rule("replaced", "allow"), rule("personal", "deny")]));
    let bundle = Bundle::capture_layers(
        r#"{"schema_version":1,"packs":[],"rules":"rules.yml"}"#,
        Some(r#"{"schema_version":1,"packs":[],"rules":"rules.local.yml"}"#),
        &[],
        Some(&shared),
        Some(&local),
        &[],
    )
    .unwrap();
    let context = mcp::initialize_fixture(root.path(), &bundle);
    let listed = rules(&context, None).unwrap();
    assert_eq!(listed["rules"].as_array().unwrap().len(), 3);
    let connection = mcp::Connection::new("local-inspection".into());
    for (id, decision) in [
        ("shared", "allow"),
        ("replaced", "allow"),
        ("personal", "deny"),
    ] {
        let selector = json!({"scope":"custom","rule_id":id});
        let cli = rules(&context, Some(&selector)).unwrap();
        assert_eq!(cli["decision"], decision);
        let inspected = mcp::call(
            &context,
            &connection,
            "rule_list",
            &json!({"rule_ref":selector}),
            &|| Ok(()),
        );
        assert!(inspected["error"].is_null(), "{inspected}");
        assert_eq!(inspected["data"], cli);
        let evaluated = mcp::call(
            &context,
            &connection,
            "policy_evaluate",
            &json!({
                "input":{"kind":"argv","executable":"acme","arguments":[id],"original_input":{}},
                "context":{"cwd":context.project_root,"environment":null}
            }),
            &|| Ok(()),
        );
        assert_eq!(evaluated["data"]["decision"], decision, "{evaluated}");
    }
}

fn enroll(source: &str) -> (assert_fs::TempDir, Context) {
    let root = assert_fs::TempDir::new().unwrap();
    let config = json!({"schema_version":1,"packs":[{"id":"acme/inspection","version":"0.1.0","profile":"recommended","path":"pack.json"}]}).to_string();
    let bundle = Bundle::capture(&config, &[source.into()], None).unwrap();
    let candidates = BTreeMap::from([("project".into(), json!(bundle))]);
    let revision = crate::configuration::Evaluator::new(review::snapshots(&candidates).unwrap())
        .unwrap()
        .revision()
        .to_owned();
    Store::initialize_active(root.path(),candidates,&revision,json!({"sources":{},"default":null,"installed":[{"selector":SELECTOR,"id":"acme/inspection","version":"0.1.0","profile":"recommended","digest":crate::policy::fingerprint(source.as_bytes())}],"projection":null,"compiled_cache_root":root.path().join("cache")})).unwrap();
    let mut context = review::open(root.path()).unwrap();
    context.surface = "mcp".into();
    context.connection_id = Some("inspection".into());
    (root, context)
}

fn pack(rules: Value) -> Value {
    let mut pack = json!({"schema_version":1,"kind":"pack","id":"acme/inspection","version":"0.1.0","description":"Acme inspection fixture","license":"MIT","maintainers":["Acme"],"engines":{"shield":"*"},"capabilities":["structured-rules-v1","argument-rules-v1"]});
    pack["rules"] = rules;
    pack
}

#[test]
fn admitted_ten_thousand_rule_pack_round_trips_its_last_rule_through_mcp() {
    let source = pack(json!((0..10_000).map(|n| json!({"id":format!("r{n}"),"conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"sequence","prefix":[format!("op{n}"),"inspect"],"rest":{"kind":"none"}}},"decision":{"loose":"allow","recommended":"allow","strict":"allow"},"reason":"Inspect Acme"})).collect::<Vec<_>>())).to_string();
    let (_root, context) = enroll(&source);
    let connection = mcp::Connection::new("inspection".into());
    let result = mcp::call(
        &context,
        &connection,
        "rule_list",
        &json!({"rule_ref":{"scope":"pack","pack_selector":SELECTOR,"rule_id":"r9999"}}),
        &|| Ok(()),
    );
    assert_eq!(result["data"]["id"], "r9999", "{result}");
    assert_eq!(
        result["data"]["conditions"]["arguments"]["prefix"],
        json!(["op9999", "inspect"])
    );
    assert_eq!(result["data"]["effective_decision"], "allow");
    assert!(result["error"].is_null());
}

#[test]
fn compact_mcp_preview_references_materialize_and_lists_keep_shared_definitions() {
    let mut source = pack(
        json!([{"id":"inspection","invocation":"globals","command_set":"safe","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}},"decision":"allow","reason":"Inspect Acme"}]),
    );
    source["capabilities"] = json!([
        "structured-rules-v1",
        "argument-rules-v1",
        "invocation-groups-v1",
        "command-sets-v1"
    ]);
    source["invocations"] = json!({"globals":{"id_prefix":"w","segments":[[[],["--plain"]]]}});
    source["command_sets"] = json!({"safe":{"status":"base-status","diff":"base-diff"}});
    let (_root, context) = enroll(&source.to_string());
    let connection = mcp::Connection::new("inspection".into());
    let preview = mcp::call(
        &context,
        &connection,
        "policy_evaluate",
        &json!({"input":{"kind":"argv","executable":"acme","arguments":["--plain","status"],"original_input":{}},"context":{"cwd":context.project_root,"environment":null}}),
        &|| Ok(()),
    );
    assert_eq!(preview["data"]["decision"], "allow", "{preview}");
    let reference = preview["data"]["explanation"]["matches"][0]["rule_ref"].clone();
    let row = mcp::call(
        &context,
        &connection,
        "rule_list",
        &json!({"rule_ref":reference}),
        &|| Ok(()),
    );
    assert_eq!(row["data"]["id"], "w1-base-status", "{row}");
    assert_eq!(
        row["data"]["conditions"]["arguments"],
        json!({"kind":"prefix","values":["--plain","status"]})
    );
    let listed = mcp::call(&context, &connection, "rule_list", &json!({}), &|| Ok(()));
    assert_eq!(listed["data"]["rules"].as_array().unwrap().len(), 1);
    assert_eq!(
        listed["data"]["shared"][0]["command_sets"]["safe"]["status"],
        "base-status"
    );
}
