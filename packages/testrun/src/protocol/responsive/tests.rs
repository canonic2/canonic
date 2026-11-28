//! Responsive MCP admission and scoped cancellation contracts.
use super::*;

#[tokio::test]
async fn documentation_reads_use_the_cancelable_lane_without_runtime_work() {
    let protocol = crate::protocol::tests::ready_protocol();
    let client = Client::default();
    let mut reads = Reads::default();
    let search = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"docs_search","arguments":{"query":"testrun"}}});
    assert!(reads.admit(&protocol, &client, &search).is_none());
    assert!(reads.cancel(&json!(1)));
    for (id, name, args) in [
        (2, "docs_list", json!({})),
        (3, "docs_read", json!({"path":"mcp.md"})),
    ] {
        let request = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}});
        assert!(reads.admit(&protocol, &client, &request).is_none());
    }
    let mut completed = Vec::new();
    while !reads.is_empty() {
        if let Some(response) = reads.next().await.unwrap() {
            assert_eq!(response["result"]["structuredContent"]["ok"], true);
            assert_ne!(response["id"], 1);
            completed.push(response["id"].as_u64().unwrap());
        }
    }
    completed.sort_unstable();
    assert_eq!(completed, [2, 3]);
}

#[tokio::test]
async fn documentation_answers_while_a_qa_mutation_occupies_the_connection() {
    // The transport polls responsive reads beside the mutation it is running;
    // a mutation that never finishes must not delay documentation, which
    // needs no manager, target or runtime client.
    let protocol = crate::protocol::tests::ready_protocol();
    let mut reads = Reads::default();
    for (id, name, args) in [
        (1, "docs_list", json!({})),
        (2, "docs_search", json!({"query":"testrun"})),
        (3, "docs_read", json!({"path":"mcp.md"})),
    ] {
        let request = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}});
        assert!(
            reads
                .admit(&protocol, &Client::default(), &request)
                .is_none()
        );
    }
    let mutation = std::future::pending::<()>();
    tokio::pin!(mutation);
    let mut completed = Vec::new();
    while completed.len() < 3 {
        tokio::select! {
            () = &mut mutation => unreachable!("the fixture mutation never finishes"),
            response = reads.next() => if let Some(response) = response.unwrap() {
                assert_eq!(response["result"]["structuredContent"]["ok"], true, "{response}");
                completed.push(response["id"].as_u64().unwrap());
            },
        }
    }
    completed.sort_unstable();
    assert_eq!(completed, [1, 2, 3]);
}

#[tokio::test]
async fn a_panicked_read_is_reported_to_the_connection_owner() {
    let mut reads = Reads::default();
    reads.spawn(
        &json!({"id":"Acme-failed"}),
        Box::pin(async { panic!("fixture read panic") }),
        1,
    );
    assert_eq!(reads.next().await.unwrap_err().code, "INTERNAL");
    assert!(
        !reads
            .requests
            .contains_key(&json!("Acme-failed").to_string())
    );
    assert!(reads.is_empty());
}
#[test]
fn a_timed_out_browser_connect_points_to_status_instead_of_another_connect() {
    let connect = timed_out("browser_connect", 5);
    assert_eq!(connect.code, "OUTCOME_UNKNOWN");
    assert_eq!(connect.details.unwrap()["nextAction"], "browser_status");
    assert_eq!(timed_out("browser_status", 35).code, "TIMEOUT");
}
#[test]
fn interrupted_control_handoffs_keep_uncertainty_and_point_to_control_status() {
    for name in ["control_acquire", "control_release"] {
        let error = timed_out(name, 125);
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details.unwrap()["nextAction"], "control_status");
    }
}
#[tokio::test]
async fn responsive_reads_are_bounded_and_cancellation_preserves_neighbors() {
    let protocol = crate::protocol::tests::ready_protocol();
    let client = Client::default();
    let mut reads = Reads::default();
    for id in 0..8 {
        assert!(
            reads
                .spawn(
                    &json!({"id":id}),
                    Box::pin(std::future::ready(
                        json!({"jsonrpc":"2.0","id":id,"result":{}})
                    )),
                    8
                )
                .is_none()
        );
    }
    let overloaded = reads
        .spawn(
            &json!({"id":8}),
            Box::pin(std::future::ready(
                json!({"jsonrpc":"2.0","id":8,"result":{}}),
            )),
            8,
        )
        .unwrap();
    assert_eq!(overloaded["error"]["code"], -32000);
    assert!(reads.cancel(&json!(0)));
    let mut completed = std::collections::HashSet::new();
    while !reads.is_empty() {
        if let Some(response) = reads.next().await.unwrap() {
            assert_ne!(response["id"], 0);
            assert!(completed.insert(response["id"].as_u64().unwrap()));
            assert_eq!(response["result"], json!({}));
        }
    }
    assert_eq!(completed.len(), 7);
    assert!(
        reads
            .spawn(
                &json!({"id":0}),
                Box::pin(std::future::ready(
                    json!({"jsonrpc":"2.0","id":0,"result":{}})
                )),
                8
            )
            .is_none()
    );
    assert_eq!(reads.next().await.unwrap().unwrap()["id"], 0);
    assert!(protocol.responsive_request(&json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"act","arguments":{}}}), &client).is_none());
}
