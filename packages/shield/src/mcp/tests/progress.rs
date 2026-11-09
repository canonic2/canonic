//! Real preparation stages over legacy and modern requests, without authority changes.
use super::confirmation::rule;
use super::*;

pub(crate) fn collect(transport: &Transport, request: &Value) -> (Value, Vec<Value>) {
    transport.send(request);
    receive(transport, &request["id"])
}

pub(crate) fn receive(transport: &Transport, id: &Value) -> (Value, Vec<Value>) {
    let mut progress = Vec::new();
    for _ in 0..65 {
        let response = transport.receive();
        if response["id"] == *id || response["method"] == "elicitation/create" {
            return (response, progress);
        }
        assert_eq!(response["method"], "notifications/progress", "{response}");
        progress.push(response["params"].clone());
    }
    panic!("progress emission exceeded its bound");
}

pub(crate) fn assert_progress(progress: &[Value], token: &Value) {
    assert!(!progress.is_empty());
    let mut previous = 0.0;
    for step in progress {
        assert_eq!(&step["progressToken"], token);
        let current = step["progress"].as_f64().unwrap();
        assert!(current > previous && current <= 64.0);
        assert!(step.get("total").is_none());
        assert!(step["message"].as_str().unwrap().len() < 80);
        previous = current;
    }
}

#[test]
fn operation_progress_covers_preparation_and_comparison_without_changing_approval() {
    for version in ["2025-06-18", "2025-11-25"] {
        for modern in [false, true] {
            let (server, project) = fixture(Some("claude"));
            let transport = Transport::start(server);
            let mut messages = initialize(&json!({}), "acme-progress");
            messages[0]["params"]["protocolVersion"] = json!(version);
            for message in messages {
                transport.send(&message);
            }
            assert!(transport.receive()["result"].is_object());
            let revision = project.revision();
            let token = if modern {
                json!(73)
            } else {
                json!("acme-progress")
            };
            let params = json!({"name":"policy_propose","arguments":rule("acme-read", "acme-private-command"),"_meta":{"progressToken":token}});
            let mut request = if modern {
                super::skills::modern(2, "tools/call", params, json!({}))
            } else {
                json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":params})
            };
            request["params"]["_meta"]["progressToken"] = token.clone();
            let (response, progress) = collect(&transport, &request);
            assert_progress(&progress, &token);
            assert!(
                !serde_json::to_string(&progress)
                    .unwrap()
                    .contains("acme-private-command")
            );
            assert_eq!(response["result"]["isError"], false, "{response}");
            assert_eq!(project.revision(), revision);
            let data = &response["result"]["structuredContent"]["data"];
            let args = json!({"before_candidate_id":data["before_candidate_id"],"after_candidate_id":data["after_candidate_id"],"domain":"literal_argv"});
            let mut compare = tool(json!(3), "policy_compare", &args);
            compare["params"]["_meta"] = json!({"progressToken":token});
            if modern {
                compare =
                    super::skills::modern(3, "tools/call", compare["params"].clone(), json!({}));
            }
            compare["params"]["_meta"]["progressToken"] = token.clone();
            let (comparison, steps) = collect(&transport, &compare);
            assert_progress(&steps, &token);
            assert_eq!(comparison["result"]["isError"], false, "{comparison}");
            assert!(
                steps
                    .iter()
                    .any(|step| step["message"] == "Comparing policy permissions and coverage")
            );
            let silent = transport.call(json!(30), "policy_compare", &args);
            assert_eq!(silent["id"], 30);
            assert_eq!(
                silent["result"]["structuredContent"],
                comparison["result"]["structuredContent"]
            );
            // A following ordinary call has no progress, and no earlier notification leaks past its reply.
            assert_eq!(transport.call(json!(4), "status", &json!({}))["id"], 4);
            assert_eq!(project.revision(), revision);
            let refused = transport.call(json!(5), "policy_apply", &data["apply_arguments"]);
            assert_eq!(
                refused["result"]["structuredContent"]["error"]["code"],
                "approval_unavailable"
            );
            assert_eq!(project.revision(), revision);
        }
    }
}
