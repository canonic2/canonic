//! Installed native trace deadlines, terminal receipts and per-reload cache policy.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub(super) struct CacheSite {
    pub(super) url: String,
    pub(super) assets: Arc<AtomicUsize>,
    pub(super) pages: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for CacheSite {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl CacheSite {
    pub(super) async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let assets = Arc::new(AtomicUsize::new(0));
        let counts = assets.clone();
        let pages = Arc::new(AtomicUsize::new(0));
        let page_counts = pages.clone();
        let task = tokio::spawn(async move {
            let mut calls = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (mut stream, _) = accepted.unwrap();
                        let counts = counts.clone();
                        let page_counts = page_counts.clone();
                        calls.spawn(async move {
                            let mut head = Vec::new();
                            while !head.ends_with(b"\r\n\r\n") && head.len() < 8192 {
                                match stream.read_u8().await { Ok(byte) => head.push(byte), Err(_) => return }
                            }
                            let (body, mime, cache) = if head.starts_with(b"GET /asset.js ") {
                                counts.fetch_add(1, Ordering::SeqCst);
                                ("performance.mark('Acme-trace-loaded');", "text/javascript", "public, max-age=3600, immutable")
                            } else {
                                if head.starts_with(b"GET / ") || head.starts_with(b"GET /?") { page_counts.fetch_add(1, Ordering::SeqCst); }
                                ("<!doctype html><title>Acme trace</title><script src='/asset.js'></script><p>Acme</p>", "text/html", "no-store")
                            };
                            let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nCache-Control: {cache}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
                        });
                    },
                    _ = calls.join_next(), if !calls.is_empty() => {},
                }
            }
        });
        Self {
            url,
            assets,
            pages,
            task,
        }
    }
}
fn artifact(stopped: &Value) {
    let report = &stopped["report"];
    assert_eq!(report["complete"], true, "{stopped}");
    let bytes = std::fs::read(report["attachment"]["path"].as_str().unwrap()).unwrap();
    let trace: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(!trace["traceEvents"].as_array().unwrap().is_empty());
}
async fn terminal(mcp: &mut Mcp, target: &str) -> Value {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let status = operation(mcp, target, json!({"operation":"trace_status"})).await;
        if status["active"] == false {
            assert_eq!(status["state"], "complete", "{status}");
            return status;
        }
        assert!(std::time::Instant::now() < until, "{status}");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
#[tokio::test]
async fn installed_step_four_native_trace_lifecycle_checkpoint() {
    let site = CacheSite::start().await;
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        let connected = fixture
            .cli(&["browser", "connect", "--engine", "chrome", "--mode", mode])
            .await;
        let target = connected["targetId"].as_str().unwrap();
        let ready = fixture.ready_target(target).await;
        let context = ready["browser"]["selectedContext"].clone();
        let result = std::panic::AssertUnwindSafe(async {
            let page = operation(&mut mcp, target, json!({"operation":"page_create","context":context,"select":true})).await["page"].clone();
            operation(&mut mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url,"waitUntil":"load"}})).await;
            let warm = site.assets.load(Ordering::SeqCst);
            assert!(warm > 0);
            let request = json!({"operation":"trace_start","context":context,"options":{"maxDurationMs":1000,"reload":{"ignoreCache":true},"cpuSampling":true}}).to_string();
            let submitted = fixture.cli(&["browser","send","--target",target,&request]).await;
            let started = completed(&mut mcp,target,submitted).await;
            assert!(started["reload"].is_object(), "{started}");
            assert_eq!(site.assets.load(Ordering::SeqCst), warm + 1);
            operation(&mut mcp,target,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"()=>new Promise(resolve=>setTimeout(()=>resolve(42),2500))"})).await;
            let status = terminal(&mut mcp,target).await;
            assert_eq!(status["state"], "complete", "{status}");
            assert_eq!(status["active"], false);
            assert_eq!(status["report"]["traceId"], started["traceId"]);
            let stopped = operation(&mut mcp,target,json!({"operation":"trace_stop"})).await;
            artifact(&stopped);
            assert_eq!(stopped["report"]["stopReason"], "duration");
            assert_eq!(operation(&mut mcp,target,json!({"operation":"trace_stop"})).await, stopped);
            let developer = operation(&mut mcp,target,json!({"operation":"developer_open","page":page,"panel":"timeline"})).await["developer"].clone();
            let loaded = operation(&mut mcp,target,json!({"operation":"performance_load","developer":developer,"evidenceId":stopped["report"]["evidenceId"]})).await;
            assert!(loaded["summary"]["data"]["insightSets"].as_array().unwrap().iter().any(|set|set["url"] == format!("{}/",site.url) && set["navigationObserved"] == true), "{loaded}");
            operation(&mut mcp,target,json!({"operation":"performance_release","developer":developer,"analysisId":loaded["analysisId"]})).await;
            operation(&mut mcp,target,json!({"operation":"developer_close","developer":developer})).await;
            let cached = site.assets.load(Ordering::SeqCst);
            let submitted = mcp.tool("act",json!({"targetId":target,"actions":[{"action":"trace_start","context":context,"options":{"maxDurationMs":30000,"reload":{"ignoreCache":false}}}]})).await;
            let action = completed(&mut mcp,target,submitted).await;
            assert_eq!(action["completedActions"], 1);
            let second = &action["results"][0];
            assert_eq!(second["state"], "recording");
            assert_eq!(second["options"]["maxDurationMs"], 30000);
            assert_eq!(second["options"]["reload"]["ignoreCache"], false);
            assert!(second["reload"].is_object(), "{second}");
            assert_ne!(second["traceId"], started["traceId"]);
            assert_eq!(site.assets.load(Ordering::SeqCst), cached, "reload cache bypass must not persist");
            let manual = operation(&mut mcp,target,json!({"operation":"trace_stop"})).await;
            artifact(&manual);
            assert_eq!(manual["report"]["stopReason"], "requested");
            let other_context = operation(&mut mcp,target,json!({"operation":"context_create"})).await["context"].clone();
            collectors::refused(&mut mcp,target,json!({"operation":"trace_start","context":other_context,"options":{"page":page}}),"INVALID_SCOPE",true).await;
            operation(&mut mcp,target,json!({"operation":"context_close","context":other_context})).await;
            collectors::refused(&mut mcp,target,json!({"operation":"trace_start","context":context,"options":{"autoStop":false,"maxDurationMs":5000}}),"TRACE_DURATION_CONFLICT",false).await;
            let other_page = operation(&mut mcp,target,json!({"operation":"page_create","context":context,"select":true})).await["page"].clone();
            let request = json!({"operation":"trace_start","context":context,"options":{"page":page,"autoStop":true,"reload":{}}}).to_string();
            let began = std::time::Instant::now();
            let submitted = fixture.cli(&["browser","send","--target",target,&request]).await;
            let automatic = completed(&mut mcp,target,submitted).await;
            let fresh_ms = began.elapsed().as_secs_f64()*1000.0;
            assert_eq!(automatic["metadataPage"],page);
            assert_eq!(automatic["options"]["autoStop"],true);
            assert_eq!(automatic["reload"]["page"],page);
            assert_eq!(mcp.tool("browser_status",json!({"targetId":target})).await["browser"]["selectedPage"],other_page);
            operation(&mut mcp,target,json!({"operation":"evaluate","scope":other_page,"isFunction":true,"expression":"()=>new Promise(resolve=>setTimeout(()=>resolve(42),6000))"})).await;
            terminal(&mut mcp,target).await;
            let automatic = operation(&mut mcp,target,json!({"operation":"trace_stop"})).await;
            artifact(&automatic);
            assert_eq!(automatic["report"]["stopReason"],"auto-stop");
            assert_eq!(automatic["report"]["metadataPage"],page);
            assert!(automatic["report"]["elapsedMs"].as_u64().unwrap()>=5000);
            assert_eq!(operation(&mut mcp,target,json!({"operation":"trace_stop"})).await,automatic);
            let retained_start = std::time::Instant::now();
            let submitted = mcp.tool("act",json!({"targetId":target,"actions":[{"action":"trace_start","context":context,"options":{"page":page,"autoStop":false}}]})).await;
            let action = completed(&mut mcp,target,submitted).await;
            assert_eq!(action["completedActions"],1);
            let explicit_manual = &action["results"][0];
            assert_eq!(explicit_manual["metadataPage"],page);
            assert_eq!(explicit_manual["options"]["autoStop"],false);
            assert!(explicit_manual["durationRemainingMs"].is_null());
            operation(&mut mcp,target,json!({"operation":"evaluate","scope":other_page,"isFunction":true,"expression":"()=>new Promise(resolve=>setTimeout(()=>resolve(42),6000))"})).await;
            assert_eq!(operation(&mut mcp,target,json!({"operation":"trace_status"})).await["state"],"recording");
            let explicit_manual = operation(&mut mcp,target,json!({"operation":"trace_stop"})).await;
            artifact(&explicit_manual);
            assert_eq!(explicit_manual["report"]["stopReason"],"requested");
            assert_eq!(mcp.tool("browser_status",json!({"targetId":target})).await["browser"]["selectedPage"],other_page);
            eprintln!("Explicit page/boolean stop {mode}: freshCliStartReloadMs={fresh_ms}; retainedManualWorkflowWithInspectionMs={}",retained_start.elapsed().as_secs_f64()*1000.0);
            let submitted = mcp.tool("act",json!({"targetId":target,"actions":[{"action":"trace_start","context":context,"options":{"page":page,"autoStop":true}}]})).await;
            let action = completed(&mut mcp,target,submitted).await;
            assert_eq!(action["completedActions"],1);
            assert_eq!(action["results"][0]["options"]["autoStop"],true);
            assert_eq!(action["results"][0]["metadataPage"],page);
            terminal(&mut mcp,target).await;
            let action_automatic = operation(&mut mcp,target,json!({"operation":"trace_stop"})).await;
            artifact(&action_automatic);
            assert_eq!(action_automatic["report"]["stopReason"],"auto-stop");
            operation(&mut mcp,target,json!({"operation":"page_close","page":other_page})).await;
            collectors::refused(&mut mcp,target,json!({"operation":"trace_start","context":context,"options":{"page":other_page}}),"STALE_REFERENCE",true).await;
            operation(&mut mcp,target,json!({"operation":"trace_start","context":context,"options":{"maxDurationMs":1000}})).await;
            terminal(&mut mcp,target).await;
            let idle = operation(&mut mcp,target,json!({"operation":"trace_stop"})).await;
            artifact(&idle);
            assert_eq!(idle["report"]["stopReason"], "duration");
            for receipt in [&stopped, &manual, &automatic, &explicit_manual, &action_automatic, &idle] {
                operation(&mut mcp,target,json!({"operation":"developer_evidence_remove","evidenceId":receipt["report"]["evidenceId"]})).await;
            }
            operation(&mut mcp,target,json!({"operation":"page_close","page":page})).await;
            eprintln!("Native trace lifecycle {mode} passed; workspace {}", fixture.workspace.path().display());
        }).catch_unwind().await;
        mcp.tool("disconnect", json!({"targetId":target})).await;
        if let Err(panic) = result {
            fixture.cli(&["disconnect"]).await;
            mcp.tool("disconnect", json!({})).await;
            mcp.stop().await;
            std::panic::resume_unwind(panic);
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
}
