//! Installed worker authority across frame replacement and context-owned worker lifetimes.
use super::*;
use tokio::io::AsyncWriteExt;

struct WorkersSite {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for WorkersSite {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl WorkersSite {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept()=>{
                        let (mut stream,_)=accepted.unwrap();
                        tasks.spawn(async move {
                            let mut bytes=Vec::new();let mut chunk=[0;1024];
                            while bytes.len()<8192&&!bytes.windows(4).any(|part|part==b"\r\n\r\n") {
                                let count=stream.read(&mut chunk).await.unwrap();if count==0{return;}bytes.extend_from_slice(&chunk[..count]);
                            }
                            let request=String::from_utf8_lossy(&bytes);let path=request.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap();
                            let (mime,body)=match path {
                                "/dedicated.js"=>("application/javascript","self.AcmeIdentity={label:self.name,count:0,guardCalls:0};self.postMessage(self.name);"),
                                "/shared.js"=>("application/javascript","self.AcmeIdentity={label:'Acme shared',count:84};onconnect=event=>{const port=event.ports[0];port.start();port.postMessage('Acme shared ready');};"),
                                "/service.js"=>("application/javascript","self.AcmeIdentity={label:'Acme service',count:42};addEventListener('install',event=>event.waitUntil(self.skipWaiting()));addEventListener('activate',event=>event.waitUntil(self.clients.claim()));addEventListener('message',event=>{if(event.data==='hold'){event.waitUntil(new Promise(resolve=>self.AcmeRelease=resolve));event.ports[0].postMessage('Acme service held');}else if(event.data==='release'){self.AcmeRelease?.();event.ports[0].postMessage('Acme service released');}});"),
                                "/child"=>("text/html","<!doctype html><title>Acme child</title><p>Acme child document</p>"),
                                _=>("text/html","<!doctype html><title>Acme workers</title><p>Acme parent</p><iframe id=child src='/child'></iframe>"),
                            };
                            let response=format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",body.len());
                            let _=stream.write_all(response.as_bytes()).await;
                        });
                    }
                    done=tasks.join_next(),if !tasks.is_empty()=>{done.unwrap().unwrap();}
                }
            }
        });
        Self { url, task }
    }
}
async fn evaluate(
    mcp: &mut Mcp,
    target: &str,
    scope: &Value,
    expression: &str,
    argument: Option<Value>,
    retain: bool,
) -> Value {
    let mut request = json!({"operation":"evaluate","scope":scope,"expression":expression,"isFunction":true,"returnHandle":retain});
    if let Some(argument) = argument {
        request["argument"] = argument;
    }
    operation(mcp, target, request).await
}
async fn value(mcp: &mut Mcp, target: &str, scope: &Value, expression: &str) -> Value {
    evaluate(mcp, target, scope, expression, None, false).await["value"]["value"].clone()
}
async fn refused(mcp: &mut Mcp, target: &str, request: Value, code: &str) {
    let submitted = mcp
        .tool("browser_send", json!({"targetId":target,"request":request}))
        .await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(status["operation"]["code"], code, "{status}");
}
async fn create_worker(mcp: &mut Mcp, target: &str, scope: &Value, label: &str) {
    let expression = format!(
        "()=>new Promise(resolve=>{{const worker=globalThis.AcmeOwnedWorker=new Worker(new URL('/dedicated.js',location.href).href,{{name:{}}});worker.onmessage=event=>resolve(event.data);}})",
        json!(label)
    );
    assert_eq!(value(mcp, target, scope, &expression).await, label);
}
async fn named_workers(
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    url: &str,
    labels: &[&str],
) -> Vec<Value> {
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let listed = operation(mcp, target, json!({"operation":"worker_list","page":page})).await;
        let candidates = listed["workers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|worker| {
                worker["url"] == format!("{url}/dedicated.js")
                    && worker["executionAvailable"] != false
            })
            .collect::<Vec<_>>();
        if candidates.len() == labels.len() {
            let mut found = std::collections::BTreeMap::new();
            for candidate in candidates {
                let worker = &candidate["worker"];
                let label = value(mcp, target, worker, "()=>self.AcmeIdentity.label").await;
                found.insert(label.as_str().unwrap().to_owned(), worker.clone());
            }
            if labels.iter().all(|label| found.contains_key(*label)) {
                return labels.iter().map(|label| found[*label].clone()).collect();
            }
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Owned dedicated workers not observed: {listed}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn read(mcp: &mut Mcp, target: &str, handle: &Value) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"handle_read","handle":handle}),
    )
    .await
}
async fn native_context_workers(
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
    url: &str,
) {
    let peer = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":peer,"navigation":{"kind":"goto","url":format!("{url}/peer"),"waitUntil":"load"}})).await;
    for owner in [page, &peer] {
        assert_eq!(value(mcp,target,owner,"()=>new Promise(resolve=>{const worker=globalThis.AcmeShared=new SharedWorker('/shared.js','Acme shared');worker.port.onmessage=event=>resolve(event.data);worker.port.start();})").await,"Acme shared ready");
    }
    assert_eq!(value(mcp,target,page,"async()=>{const registration=await navigator.serviceWorker.register('/service.js');await navigator.serviceWorker.ready;return await new Promise(resolve=>{const channel=new MessageChannel();channel.port1.onmessage=event=>resolve(event.data);registration.active.postMessage('hold',[channel.port2]);});}").await,"Acme service held");
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    let records = loop {
        let listed = operation(
            mcp,
            target,
            json!({"operation":"context_worker_list","context":context}),
        )
        .await;
        let selected = [("shared", "shared.js"), ("service", "service.js")]
            .iter()
            .map(|(kind, path)| {
                listed["workers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|worker| {
                        worker["kind"] == *kind
                            && worker["url"] == format!("{url}/{path}")
                            && worker["executionAvailable"] == true
                    })
                    .cloned()
            })
            .collect::<Option<Vec<_>>>();
        if let Some(records) = selected {
            break records;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "Context workers unavailable: {listed}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let mut retained = Vec::new();
    for (record, label) in records.iter().zip(["Acme shared", "Acme service"]) {
        assert!(record["page"].is_null());
        assert_eq!(record["context"], *context);
        let result = evaluate(
            mcp,
            target,
            &record["worker"],
            "()=>self.AcmeIdentity",
            None,
            true,
        )
        .await;
        assert_eq!(result["worker"], record["worker"]);
        assert!(result["frame"].is_null());
        retained.push((result["handle"].clone(), record["worker"].clone(), label));
    }
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    for (handle, worker, label) in &retained {
        assert_eq!(
            evaluate(mcp, target, handle, "owner=>owner.label", None, false).await["value"]["value"],
            *label
        );
        assert_eq!(read(mcp, target, handle).await["worker"], *worker);
        operation(
            mcp,
            target,
            json!({"operation":"handle_dispose","handle":handle}),
        )
        .await;
    }
    assert_eq!(value(mcp,target,&peer,"async()=>{const registration=await navigator.serviceWorker.getRegistration('/');return await new Promise(resolve=>{const channel=new MessageChannel();channel.port1.onmessage=event=>resolve(event.data);registration.active.postMessage('release',[channel.port2]);});}").await,"Acme service released");
    operation(mcp, target, json!({"operation":"page_close","page":peer})).await;
}
#[expect(
    clippy::too_many_lines,
    reason = "One worker authority workflow correlates retained values, frame transitions and close/replacement witnesses before exact cleanup"
)]
async fn cell(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    engine: &str,
    site: &WorkersSite,
) -> Value {
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{}/page",site.url),"waitUntil":"load"}})).await;
    let frame = operation(
        mcp,
        target,
        json!({"operation":"frame_enter","page":page,"locator":{"kind":"css","selector":"#child"}}),
    )
    .await["frame"]
        .clone();
    create_worker(mcp, target, &page, "Acme main").await;
    create_worker(mcp, target, &frame, "Acme child").await;
    let workers = named_workers(mcp, target, &page, &site.url, &["Acme main", "Acme child"]).await;
    let main = &workers[0];
    let child = &workers[1];
    let request=json!({"operation":"evaluate","scope":main,"expression":"()=>++AcmeIdentity.count","isFunction":true}).to_string();
    let start = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let result = completed(mcp, target, submitted).await;
    assert_eq!(result["value"]["value"], 1);
    assert_eq!(result["worker"], *main);
    assert!(result["frame"].is_null());
    let fresh_ms = start.elapsed().as_secs_f64() * 1000.0;
    let retained_start = std::time::Instant::now();
    let mut handles = Vec::new();
    let mut properties = Vec::new();
    for worker in [main, child] {
        let result = evaluate(mcp, target, worker, "()=>self.AcmeIdentity", None, true).await;
        assert_eq!(result["worker"], *worker);
        assert!(result["frame"].is_null());
        let handle = result["handle"].clone();
        let property = operation(
            mcp,
            target,
            json!({"operation":"handle_property","handle":handle,"name":"label"}),
        )
        .await["handle"]
            .clone();
        assert_eq!(read(mcp, target, &property).await["worker"], *worker);
        handles.push(handle);
        properties.push(property);
    }
    refused(mcp,target,json!({"operation":"evaluate","scope":main,"expression":"value=>{AcmeIdentity.guardCalls++;return value;}","isFunction":true,"argument":{"kind":"handle","handle":handles[1]}}),"FOREIGN_HANDLE").await;
    refused(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"value=>value","isFunction":true,"argument":{"kind":"handle","handle":handles[0]}}),"FOREIGN_HANDLE").await;
    assert_eq!(
        value(mcp, target, main, "()=>AcmeIdentity.guardCalls").await,
        0
    );
    let document = evaluate(
        mcp,
        target,
        &frame,
        "()=>({label:'Acme document'})",
        None,
        true,
    )
    .await["handle"]
        .clone();
    assert_eq!(value(mcp,target,&page,"()=>new Promise(resolve=>{const frame=document.querySelector('#child');frame.onload=()=>resolve(true);frame.src='/child?Acme=replacement';})").await,true);
    let remaining = named_workers(mcp, target, &page, &site.url, &["Acme main"]).await;
    assert_eq!(remaining[0], *main);
    assert_eq!(
        read(mcp, target, &properties[0]).await["value"]["value"],
        "Acme main"
    );
    assert_eq!(
        evaluate(mcp, target, &handles[0], "owner=>owner.count", None, false).await["value"]["value"],
        1
    );
    for handle in [&handles[1], &properties[1], &document] {
        refused(
            mcp,
            target,
            json!({"operation":"handle_read","handle":handle}),
            "STALE_REFERENCE",
        )
        .await;
    }
    refused(
        mcp,
        target,
        json!({"operation":"evaluate","scope":child,"expression":"()=>42","isFunction":true}),
        "STALE_REFERENCE",
    )
    .await;
    let next_frame = operation(
        mcp,
        target,
        json!({"operation":"frame_enter","page":page,"locator":{"kind":"css","selector":"#child"}}),
    )
    .await["frame"]
        .clone();
    create_worker(mcp, target, &next_frame, "Acme replacement").await;
    let current = named_workers(
        mcp,
        target,
        &page,
        &site.url,
        &["Acme main", "Acme replacement"],
    )
    .await;
    assert_eq!(current[0], *main);
    assert_ne!(current[1], *child);
    let result=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"evaluate","scope":main,"expression":"arg=>arg.label+'|'+AcmeIdentity.label","isFunction":true,"argument":{"kind":"handle","handle":handles[0]}}]})).await;
    let result = completed(mcp, target, result).await;
    assert_eq!(
        result["results"][0]["value"]["value"],
        "Acme main|Acme main"
    );
    let inventory = operation(
        mcp,
        target,
        json!({"operation":"context_worker_list","context":context}),
    )
    .await;
    if engine == "chrome" {
        native_context_workers(mcp, target, context, &page, &site.url).await;
        refused(
            mcp,
            target,
            json!({"operation":"handle_read","handle":handles[0]}),
            "STALE_REFERENCE",
        )
        .await;
    } else {
        assert_eq!(inventory["availableKinds"], json!(["dedicated"]));
        assert_eq!(inventory["unsupportedKinds"], json!(["shared", "service"]));
        for handle in [&handles[0], &properties[0]] {
            operation(
                mcp,
                target,
                json!({"operation":"handle_dispose","handle":handle}),
            )
            .await;
        }
        operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    }
    json!({"freshCliWorkerEvalMs":fresh_ms,"retainedWorkerWorkflowMs":retained_start.elapsed().as_secs_f64()*1000.0,"contextWorkerKinds":if engine=="chrome"{json!(["shared","service"])}else{json!(["dedicated"])}})
}
#[tokio::test]
async fn installed_step_four_worker_lifetime_checkpoint() {
    let fixture = installed_fixture();
    let site = WorkersSite::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let start = std::time::Instant::now();
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let connect_ms = start.elapsed().as_secs_f64() * 1000.0;
            let result = std::panic::AssertUnwindSafe(cell(
                &fixture,
                &mut mcp,
                target,
                &ready["browser"]["selectedContext"],
                engine,
                &site,
            ))
            .catch_unwind()
            .await;
            let start = std::time::Instant::now();
            mcp.tool("disconnect", json!({"targetId":target})).await;
            let cleanup_ms = start.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(metrics) => eprintln!(
                    "Worker lifetime {engine} {mode}: {metrics}; connectMs={connect_ms}; cleanupMs={cleanup_ms}"
                ),
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
