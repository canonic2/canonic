//! Installed SDK replies have durable witnesses without private error payloads.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct FailureSite {
    url: String,
    held: Arc<AtomicUsize>,
    reset: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for FailureSite {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl FailureSite {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let held = Arc::new(AtomicUsize::new(0));
        let reset = Arc::new(AtomicUsize::new(0));
        let held_count = held.clone();
        let reset_count = reset.clone();
        let task = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept(),if requests.len()<32=>{
                        let (mut stream,_)=accepted.unwrap();
                        let held=held_count.clone();let reset=reset_count.clone();
                        requests.spawn(async move {
                            let mut head=Vec::new();let mut chunk=[0;1024];
                            while head.len()<8192&&!head.windows(4).any(|part|part==b"\r\n\r\n") {
                                let count=stream.read(&mut chunk).await.unwrap();
                                if count==0 {return;} head.extend_from_slice(&chunk[..count]);
                            }
                            if head.starts_with(b"GET /held?") {
                                held.fetch_add(1,Ordering::SeqCst);
                                // Hold the owned transport beyond the SDK navigation timeout.
                                // No response bytes or server-side timeout can explain its reply.
                                let _=stream.read(&mut chunk).await;
                            } else {
                                assert!(head.starts_with(b"GET /reset?"));
                                reset.fetch_add(1,Ordering::SeqCst);
                                // A real loopback connection closes without HTTP response bytes.
                            }
                        });
                    },
                    Some(result)=requests.join_next(),if !requests.is_empty()=>{result.unwrap();},
                }
            }
        });
        Self {
            url,
            held,
            reset,
            task,
        }
    }
}

pub(super) fn witness(fixture: &Fixture, target: &str, operation: &Value) -> Value {
    let connection = target.strip_prefix("browser:").unwrap();
    let path = fixture
        .workspace
        .pool_directory()
        .join("browser")
        .join(format!("browser-{connection}"));
    let mut found = vec![];
    for entry in std::fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let bytes = std::fs::read(path).unwrap();
            assert!(
                !String::from_utf8_lossy(&bytes).contains("AcmePrivateFailure"),
                "Private URLs and errors must not enter lifecycle journals"
            );
            let record: Value = serde_json::from_slice(&bytes).unwrap();
            if record["operationId"] == operation["id"] && record["settled"] == true {
                found.push(record);
            }
        }
    }
    assert_eq!(
        found.len(),
        1,
        "Exactly one durable settlement must correlate to this operation"
    );
    found.pop().unwrap()
}

#[tokio::test]
async fn installed_step_four_failure_evidence_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = vec![];
    for mode in ["headless", "headed"] {
        for engine in ["firefox", "webkit"] {
            let site = FailureSite::start().await;
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result=std::panic::AssertUnwindSafe(async {
                for (path,code,kind,outcome) in [("held","OUTCOME_UNKNOWN","timeout","unknown"),("reset","BROWSER_METHOD_FAILED","method-error","failed")] {
                    let page=operation(&mut mcp,target,json!({"operation":"page_create","context":ready["browser"]["selectedContext"],"select":false})).await["page"].clone();
                    let submitted=mcp.tool("browser_send",json!({"targetId":target,"timeoutMs":3000,"request":{"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/{path}?AcmePrivateFailure=1",site.url),"waitUntil":"load"}}})).await;
                    let status=mcp.tool("browser_status",json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000})).await;
                    let settled=&status["operation"];
                    let record=witness(&fixture,target,settled);
                    std::fs::write(fixture.workspace.path().join(format!("failure-evidence-{engine}-{mode}-{path}.json")),serde_json::to_vec_pretty(&json!({"operation":settled,"journal":record})).unwrap()).unwrap();
                    assert_eq!(settled["settled"],true,"{status}");
                    assert_eq!(settled["outcome"],outcome,"{status}");
                    assert_eq!(settled["code"],code,"{status}");
                    assert_eq!(record["backendFailure"],json!({"source":"sdk-reply","kind":kind,"dispatch":"replied"}),"A public unknown result cannot substitute for an observed backend reply: {record}");
                    assert!(record.get("result").is_none());
                    assert!(settled.get("backendFailure").is_none(),"Private journal evidence does not alter public result schemas");
                    operation(&mut mcp,target,json!({"operation":"page_close","page":page})).await;
                }
                assert_eq!(site.held.load(Ordering::SeqCst),1);
                assert!(site.reset.load(Ordering::SeqCst)>=1);
            }).catch_unwind().await;
            let closed = performance_lifecycle::disconnect_observed(
                &fixture,
                &mut mcp,
                target,
                engine,
                mode,
                &ready["browser"]["selectedContext"],
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                failures.push(format!("{engine}/{mode}: {closed}"));
            }
            if let Err(panic) = result {
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
            eprintln!(
                "Installed failure evidence {engine}/{mode}: SDK timeout, method error, private journal correlation and original cleanup"
            );
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
