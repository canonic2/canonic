//! The published schema, proposal owner and candidate evaluator share GraphQL rules.
use super::*;

#[test]
fn graphql_proposal_schema_admission_preview_and_inspection_preserve_activation() {
    let (root, context, connection) = fixture();
    let selection = json!({"root":"query","states":{
        "query":{"types":["Query"],"fields":{"repository":{"select":"repository","arguments":{
            "owner":{"required":true,"match":{"text":"acme"}},"name":{"required":true,"match":{"text":"web"}}
        }}}},
        "repository":{"types":["Repository"],"fields":{"nameWithOwner":{"leaf":true}}}
    }});
    let row = json!({"id":"github-read","decision":"allow","conditions":{
        "domain":"argv","executable":"gh","arguments":{"kind":"prefix","values":["api"]}},
        "requirements":{"graphql":{"carrier":"gh-api","operation":"query","selection":selection}},
        "on_unmet":{"decision":"deny"}});
    let proposal_input = json!({"change":{"kind":"rule_add","rule":row}});
    let tools = tools();
    let schema = &tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "policy_propose")
        .unwrap()["inputSchema"];
    canonic_mcp_kit::schema::validate(schema, &proposal_input).unwrap();
    let before = Store::open(root.path())
        .unwrap()
        .read_checkpoint()
        .unwrap()
        .policy_revision;
    let proposed = invoke(&context, &connection, "policy_propose", proposal_input);
    assert!(proposed["error"].is_null(), "{proposed}");
    let after = proposed["data"]["after_candidate_id"].clone();
    for (owner, expected) in [("acme", "allow"), ("other", "deny")] {
        let argv = json!([
            "api",
            "graphql",
            "-f",
            format!("query={{repository(owner:\"{owner}\",name:\"web\"){{nameWithOwner}}}}")
        ]);
        let input = json!({"candidate_id":after,"input":{"kind":"argv","executable":"gh","arguments":argv,"original_input":{}},"context":{"cwd":context.project_root,"environment":{}}});
        let result = invoke(&context, &connection, "policy_evaluate", input);
        assert!(result["error"].is_null(), "{result}");
        assert_eq!(result["data"]["decision"], expected, "{result}");
    }
    let inspected = invoke(
        &context,
        &connection,
        "rule_list",
        json!({"candidate_id":after,"rule_ref":{"scope":"custom","rule_id":"github-read"}}),
    );
    assert!(inspected["error"].is_null(), "{inspected}");
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .policy_revision,
        before
    );
}
