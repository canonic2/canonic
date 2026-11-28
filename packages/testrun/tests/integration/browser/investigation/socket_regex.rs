//! Installed ASCII socket regexes: flags, real relay, Page selection and native nonmatches.
use super::*;

async fn invalid(mcp: &mut Mcp, target: &str, context: &Value) {
    let request = json!({"operation":"web_socket_route_add_matcher","scope":context,"matcher":{"kind":"regex","pattern":"(?=Acme"}});
    let response = mcp
        .request(
            "tools/call",
            json!({"name":"browser_send","arguments":{"targetId":target,"request":request}}),
        )
        .await;
    let result = &response["result"]["structuredContent"];
    let code = if result["ok"] == true {
        let status=mcp.tool("browser_status",json!({"targetId":target,"operationId":result["data"]["operation"]["id"],"waitMs":30000})).await;
        assert_eq!(status["operation"]["outcome"], "failed", "{status}");
        status["operation"]["code"].clone()
    } else {
        result["code"].clone()
    };
    assert_eq!(code, "WEBSOCKET_REGEX_INVALID", "{response}");
    let listed = operation(
        mcp,
        target,
        json!({"operation":"web_socket_route_list","scope":context}),
    )
    .await;
    assert!(listed["routes"].as_array().unwrap().is_empty());
}
#[tokio::test]
async fn installed_step_four_socket_regex_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready().await;
            let context = &ready["browser"]["selectedContext"];
            let result = std::panic::AssertUnwindSafe(async {
                invalid(&mut mcp, target, context).await;
                let regex =
                    socket_protocols::matched_urls(&fixture, &mut mcp, target, context, "regex")
                        .await;
                let pattern = socket_protocols::matched_urls(
                    &fixture,
                    &mut mcp,
                    target,
                    context,
                    "url_pattern",
                )
                .await;
                json!({"regex":regex,"urlPattern":pattern})
            })
            .catch_unwind()
            .await;
            mcp.tool("disconnect", json!({"targetId":target})).await;
            match result {
                Ok(latency) => eprintln!("Socket regex latency {engine} {mode}: {latency}"),
                Err(panic) => {
                    fixture.cli(&["disconnect"]).await;
                    mcp.tool("disconnect", json!({})).await;
                    mcp.stop().await;
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
}
