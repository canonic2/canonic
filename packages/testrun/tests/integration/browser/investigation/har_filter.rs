//! Installed capture admission preserves independent diagnostic history.
use super::*;

#[tokio::test]
async fn installed_step_four_har_filter_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready().await;
            let context = &ready["browser"]["selectedContext"];
            let page = &ready["browser"]["selectedPage"];
            let result = std::panic::AssertUnwindSafe(async {
                operation(&mut mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url}})).await;
                operation(&mut mcp,target,json!({"operation":"diagnostics_start","context":context})).await;
                installed_har_capture(&fixture,&mut mcp,&site.url,target,page,context,true).await;
                operation(&mut mcp,target,json!({"operation":"diagnostics_stop","context":context})).await;
            }).catch_unwind().await;
            let cleanup = std::time::Instant::now();
            mcp.tool("disconnect", json!({"targetId":target})).await;
            eprintln!(
                "HAR filter cleanup {engine} {mode}: {:?}",
                cleanup.elapsed()
            );
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
}
