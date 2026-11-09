//! A fresh same-scope review stays reachable after a recorded human decline.
use super::*;

fn confirm(transport: &Transport, args: &Value, modern: bool, action: &str) -> Value {
    let call = |id, state: Option<&Value>, answer: Option<&str>| {
        let mut params = json!({"name":"session_grant_approve","arguments":args});
        if let Some(state) = state {
            params["requestState"] = state.clone();
        }
        if let Some(answer) = answer {
            params["inputResponses"] = json!({"confirmation":{"action":answer}});
        }
        super::super::skills::modern(id, "tools/call", params, json!({"elicitation":{"form":{}}}))
    };
    let form = if modern {
        transport.request(&call(20, None, None))
    } else {
        transport.call(json!(20), "session_grant_approve", args)
    };
    let text = if modern {
        assert_eq!(form["result"]["resultType"], "input_required", "{form}");
        form.to_string()
    } else {
        assert_eq!(form["method"], "elicitation/create", "{form}");
        form["params"]["message"].as_str().unwrap().into()
    };
    assert!(
        text.contains("Fresh review of declined proposal") && text.contains("acme"),
        "{text}"
    );
    if modern {
        transport.request(&call(
            21,
            Some(&form["result"]["requestState"]),
            Some(action),
        ))
    } else {
        transport.request(
            &json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":action,"content":{}}}),
        )
    }
}

#[test]
fn session_reconsideration_shipping_forms_approve_or_decline_without_changing_the_offer() {
    for agent in ["claude", "codex", "gemini", "agy"] {
        for modern in [false, true] {
            for action in ["accept", "decline", "cancel"] {
                let (transport, _root, context, reference) = prepared(agent, true);
                let original = crate::review::session_grants::propose(&context,
                    &json!({"session_ref":reference,"idempotency_key":"original","scope":scope(),"duration_seconds":600})).unwrap();
                let first = transport.call(
                    json!(2),
                    "session_grant_approve",
                    &original["apply_arguments"],
                );
                assert_eq!(first["method"], "elicitation/create");
                let declined = transport.request(
                    &json!({"jsonrpc":"2.0","id":first["id"],"result":{"action":"decline"}}),
                );
                assert_eq!(
                    declined["result"]["structuredContent"]["error"]["code"],
                    "approval_declined"
                );
                let automatic = transport.call(json!(3), "session_grant_propose",
                    &json!({"session_ref":reference,"idempotency_key":"new-automatic-key","scope":scope(),"duration_seconds":600}));
                assert_eq!(
                    automatic["result"]["structuredContent"]["data"]["proposal_status"],
                    "declined"
                );
                let args = json!({"proposal_id":original["proposal_id"],"candidate_digest":original["candidate_digest"],"idempotency_key":"explicit-request"});
                let fresh = transport.call(json!(4), "session_grant_reconsider", &args);
                let candidate = &fresh["result"]["structuredContent"]["data"];
                assert!(
                    fresh["result"]["structuredContent"]["error"].is_null(),
                    "{fresh}"
                );
                assert_ne!(candidate["proposal_id"], original["proposal_id"]);
                for field in ["scope", "duration_seconds", "override_denies", "target"] {
                    assert_eq!(candidate[field], original[field]);
                }
                assert!(candidate["grant_id"].is_null());
                let result = confirm(&transport, &candidate["apply_arguments"], modern, action);
                let shown = transport.call(
                    json!(30),
                    "session_grant_show",
                    &json!({"proposal_id":candidate["proposal_id"]}),
                );
                let row = &shown["result"]["structuredContent"]["data"];
                if action == "accept" {
                    assert!(
                        result["result"]["structuredContent"]["error"].is_null(),
                        "{result}"
                    );
                    assert_eq!(row["grant_status"], "active");
                    let replay = transport.call(
                        json!(31),
                        "session_grant_approve",
                        &candidate["apply_arguments"],
                    );
                    assert_eq!(
                        replay["result"]["structuredContent"]["data"]["grant_id"],
                        row["grant_id"]
                    );
                } else {
                    assert_eq!(
                        result["result"]["structuredContent"]["error"]["code"],
                        "approval_declined"
                    );
                    assert_eq!(row["proposal_status"], "declined");
                    assert!(row["grant_id"].is_null());
                }
                let previous = crate::review::session_grants::show(
                    &context,
                    &json!({"proposal_id":original["proposal_id"]}),
                )
                .unwrap();
                assert_eq!(previous["proposal_status"], "declined");
                assert!(previous["grant_id"].is_null());
                let replay = transport.call(json!(32), "session_grant_reconsider", &args);
                assert_eq!(
                    replay["result"]["structuredContent"]["data"]["proposal_id"],
                    candidate["proposal_id"]
                );
                assert_eq!(
                    replay["result"]["structuredContent"]["data"]["expires_at_ms"],
                    row["expires_at_ms"]
                );
            }
        }
    }
}

#[test]
fn session_reconsideration_missing_forms_or_disconnect_never_issue_permission() {
    for forms in [false, true] {
        let (transport, _root, context, reference) = prepared("codex", forms);
        let original = crate::review::session_grants::propose(
            &context,
            &json!({"session_ref":reference,"idempotency_key":"original","scope":scope()}),
        )
        .unwrap();
        crate::review::session_grants::decline(&context, &original["apply_arguments"]).unwrap();
        let fresh = transport.call(json!(2), "session_grant_reconsider",
            &json!({"proposal_id":original["proposal_id"],"candidate_digest":original["candidate_digest"],"idempotency_key":"explicit"}));
        let candidate = &fresh["result"]["structuredContent"]["data"];
        let result = transport.call(
            json!(3),
            "session_grant_approve",
            &candidate["apply_arguments"],
        );
        if forms {
            assert_eq!(result["method"], "elicitation/create", "{result}");
            assert!(transport.close().1.is_ok());
        } else {
            assert_eq!(
                result["result"]["structuredContent"]["error"]["code"],
                "approval_unavailable"
            );
        }
        let row = crate::review::session_grants::show(
            &context,
            &json!({"proposal_id":candidate["proposal_id"]}),
        )
        .unwrap();
        assert_eq!(row["proposal_status"], "pending");
        assert!(row["grant_id"].is_null());
    }
}
