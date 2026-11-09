//! Proposal report links, retained authority boundaries and wire revision parity.
use super::confirmation::rule;
use super::*;

fn start(server: Server, version: &str) -> Transport {
    let transport = Transport::start(server);
    let mut messages = initialize(&json!({}), "acme-reviewer");
    messages[0]["params"]["protocolVersion"] = json!(version);
    for message in messages {
        transport.send(&message);
    }
    assert!(transport.receive()["result"]["capabilities"]["resources"].is_object());
    transport
}

fn read(transport: &Transport, id: i64, uri: &str, modern: bool) -> Value {
    let params = json!({"uri":uri});
    let request = if modern {
        super::skills::modern(id, "resources/read", params, json!({}))
    } else {
        json!({"jsonrpc":"2.0","id":id,"method":"resources/read","params":params})
    };
    transport.request(&request)
}

fn report(response: &Value) -> Value {
    serde_json::from_str(
        response["result"]["contents"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{response}")),
    )
    .unwrap()
}

#[test]
fn review_resources_link_exact_comparisons_and_evidence_without_authority_or_preparation() {
    for version in ["2025-06-18", "2025-11-25"] {
        let (server, project) = fixture(Some("claude"));
        let transport = start(server, version);
        let response = transport.call(json!(2), "policy_propose", &rule("acme-read", "acme"));
        let data = &response["result"]["structuredContent"]["data"];
        assert_eq!(response["result"]["isError"], false, "{response}");
        let links = data["report_resources"].as_array().unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(response["result"]["content"][2]["type"], "resource_link");
        let text: Value =
            serde_json::from_str(response["result"]["content"][1]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(text["data"]["report_resources"], data["report_resources"]);
        let checkpoint = project.root.path().join(".shield/review/state.json");
        let before = std::fs::read(&checkpoint).unwrap();
        for modern in [false, true] {
            let comparison = report(&read(
                &transport,
                3,
                links[0]["uri"].as_str().unwrap(),
                modern,
            ));
            assert_eq!(comparison["data"], data["comparison"]);
            assert_eq!(comparison["candidate_digest"], data["candidate_digest"]);
            let evidence = report(&read(
                &transport,
                4,
                links[1]["uri"].as_str().unwrap(),
                modern,
            ));
            assert_eq!(evidence["data"]["provenance"], data["provenance"]);
            assert_eq!(evidence["data"]["reasons"], data["reasons"]);
            assert!(evidence["data"].get("apply_arguments").is_none());
            assert_eq!(std::fs::read(&checkpoint).unwrap(), before);
        }
        let templates = transport.request(
            &json!({"jsonrpc":"2.0","id":5,"method":"resources/templates/list","params":{}}),
        );
        assert_eq!(
            templates["result"]["resourceTemplates"][0]["uriTemplate"],
            crate::review::reports::TEMPLATE
        );
        let (other, _other_project) = fixture(None);
        let foreign = start(other, version);
        for modern in [false, true] {
            let response = read(&foreign, 6, links[0]["uri"].as_str().unwrap(), modern);
            assert_eq!(
                response["error"]["code"],
                if modern {
                    rpc::INVALID_PARAMS
                } else {
                    rpc::RESOURCE_NOT_FOUND
                }
            );
        }
        // A new connection can inspect retained evidence without taking over the live review.
        let peer = start(
            Server::for_review((*project.review).clone()).unwrap(),
            version,
        );
        assert_eq!(
            report(&read(&peer, 7, links[0]["uri"].as_str().unwrap(), false))["data"],
            data["comparison"]
        );
        assert_eq!(std::fs::read(&checkpoint).unwrap(), before);
        let shown = transport.call(
            json!(8),
            "policy_show",
            &json!({"proposal_id":data["proposal_id"]}),
        );
        assert_eq!(
            shown["result"]["structuredContent"]["data"]["apply_arguments"],
            data["apply_arguments"]
        );
        let revision = project.revision();
        let refused = transport.call(json!(9), "policy_apply", &data["apply_arguments"]);
        assert_eq!(
            refused["result"]["structuredContent"]["error"]["code"],
            "approval_unavailable"
        );
        assert_eq!(project.revision(), revision);
        let closed = report(&read(
            &transport,
            10,
            links[1]["uri"].as_str().unwrap(),
            false,
        ));
        assert_eq!(closed["data"]["status"], "withdrawn");
    }
}

#[test]
fn review_resources_refuse_changed_selectors_and_respect_frame_bounds() {
    let (server, project) = fixture(Some("claude"));
    let transport = start(server, VERSION);
    let proposed = transport.call(json!(2), "policy_propose", &rule("acme-read", "acme"));
    let uri = proposed["result"]["structuredContent"]["data"]["report_resources"][0]["uri"]
        .as_str()
        .unwrap();
    let before = project.revision();
    for (index, invalid) in [
        format!("{uri}/extra"),
        format!("{uri}?reveal=true"),
        uri.replace("/comparison", "/%63omparison"),
        uri.replace("/comparison", "/../evidence"),
        uri.replace("proposal-", "missing-"),
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            read(
                &transport,
                10 + i64::try_from(index).unwrap(),
                invalid,
                false
            )["error"]["code"],
            rpc::RESOURCE_NOT_FOUND
        );
    }
    let mut options = test_options();
    options.max_output_bytes = 1024;
    let limited = Transport::with_options(
        Server::for_review((*project.review).clone()).unwrap(),
        options,
    );
    let result = read(&limited, 20, uri, true);
    assert_eq!(result["error"]["code"], rpc::INTERNAL_ERROR, "{result}");
    assert!(result["result"].is_null());
    assert_eq!(project.revision(), before);
}
