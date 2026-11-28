//! Installed React profile options and retained cleanup/data-read ownership.
use super::*;

async fn query(mcp: &mut Mcp, target: &str, registry: &Value, query: Value) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"framework_query","registry":registry,"query":query}),
    )
    .await["report"]
        .clone()
}
async fn failed(mcp: &mut Mcp, target: &str, registry: &Value, query: Value) {
    let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"framework_query","registry":registry,"query":query}})).await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(status["operation"]["settled"], true, "{status}");
    assert_eq!(status["operation"]["outcome"], "failed", "{status}");
    assert!(
        matches!(
            status["operation"]["code"].as_str(),
            Some("JAVASCRIPT_EXCEPTION" | "BROWSER_METHOD_FAILED")
        ),
        "{status}"
    );
}
async fn evaluate(
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    expression: &str,
    argument: Value,
) -> Value {
    operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":expression,"isFunction":true,"argument":{"kind":"json","value":argument}})).await["value"]["value"].clone()
}
async fn probe(mcp: &mut Mcp, target: &str, page: &Value) -> Value {
    serde_json::from_str(evaluate(mcp,target,page,"()=>JSON.stringify({starts:AcmeProfileProbe.starts,stops:AcmeProfileProbe.stops,reads:AcmeProfileProbe.reads})",json!(null)).await.as_str().unwrap()).unwrap()
}
#[expect(
    clippy::too_many_lines,
    reason = "One installed workflow correlates native profiler effects, retained reports and recovery counters before releasing its document"
)]
async fn exercise(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    url: &str,
) -> Value {
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    let prepared = operation(
        mcp,
        target,
        json!({"operation":"framework_prepare","page":page}),
    )
    .await;
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{url}/framework"),"waitUntil":"load"}})).await;
    evaluate(mcp,target,&page,"async()=>{const until=Date.now()+10000;while(!document.querySelector('#count')){if(Date.now()>until)throw Error('Acme React did not mount');await new Promise(resolve=>setTimeout(resolve,25));}return true;}",json!(null)).await;
    let registry = operation(
        mcp,
        target,
        json!({"operation":"framework_open","scope":page}),
    )
    .await["registry"]
        .clone();
    let other = operation(
        mcp,
        target,
        json!({"operation":"framework_open","scope":page}),
    )
    .await["registry"]
        .clone();
    evaluate(mcp,target,&page,r#"()=>{const hook=window.__REACT_DEVTOOLS_GLOBAL_HOOK__;const rendererId=Array.from(hook.rendererInterfaces.keys()).find(id=>hook.getFiberRoots(id).size&&!hook.renderers.get(id).rendererPackageName.includes('react-server-dom'));const ri=hook.rendererInterfaces.get(rendererId);const original={start:ri.startProfiling,stop:ri.stopProfiling,read:ri.getProfilingData};const probe={starts:0,stops:0,reads:0,failRead:false,failStart:false};ri.startProfiling=function(...args){probe.starts++;const result=original.start.apply(this,args);if(probe.failStart){probe.failStart=false;throw Error('Acme uncertain profile start');}return result;};ri.stopProfiling=function(...args){probe.stops++;return original.stop.apply(this,args);};ri.getProfilingData=function(...args){probe.reads++;if(probe.failRead){probe.failRead=false;throw Error('Acme profile data unavailable');}return original.read.apply(this,args);};probe.restore=()=>{ri.startProfiling=original.start;ri.stopProfiling=original.stop;ri.getProfilingData=original.read;};globalThis.AcmeProfileProbe=probe;return true;}"#,json!(null)).await;
    let mut fresh_ms = 0.0;
    let mut retained_start = None;
    for (index, flags) in [
        json!({}),
        json!({"recordChangeDescriptions":false,"recordTimeline":false}),
        json!({"recordChangeDescriptions":true,"recordTimeline":true}),
        json!({"recordChangeDescriptions":false,"recordTimeline":true}),
        json!({"recordChangeDescriptions":true,"recordTimeline":false}),
    ]
    .into_iter()
    .enumerate()
    {
        let mut options = json!({"kind":"renders_start","durationMs":30000});
        options
            .as_object_mut()
            .unwrap()
            .extend(flags.as_object().unwrap().clone());
        let expected = json!({"recordChangeDescriptions":flags.get("recordChangeDescriptions").cloned().unwrap_or(json!(true)),"recordTimeline":flags.get("recordTimeline").cloned().unwrap_or(json!(false))});
        let started = if index == 0 {
            let request =
                json!({"operation":"framework_query","registry":registry,"query":options})
                    .to_string();
            let start = std::time::Instant::now();
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &request])
                .await;
            let result = completed(mcp, target, submitted).await["report"].clone();
            fresh_ms = start.elapsed().as_secs_f64() * 1000.0;
            retained_start = Some(std::time::Instant::now());
            result
        } else {
            let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"framework_query","registry":registry,"query":options}]})).await;
            let result = completed(mcp, target, submitted).await;
            assert_eq!(result["completedActions"], 1);
            result["results"][0]["report"].clone()
        };
        assert_eq!(started["options"], expected);
        assert_eq!(
            query(mcp, target, &registry, json!({"kind":"renders_status"})).await["options"],
            expected
        );
        if index == 0 {
            failed(mcp, target, &other, json!({"kind":"renders_start"})).await;
        }
        evaluate(mcp,target,&page,"async expected=>{document.querySelector('#count').click();const until=Date.now()+10000;while(document.querySelector('#count').textContent!=='Count '+expected){if(Date.now()>until)throw Error('Acme React commit missing');await new Promise(resolve=>setTimeout(resolve,25));}return true;}",json!(index+1)).await;
        let stopped = query(mcp, target, &registry, json!({"kind":"renders_stop"})).await;
        assert_eq!(stopped["options"], expected);
        let commits = stopped["data"]["dataForRoots"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|root| root["commitData"].as_array().unwrap())
            .collect::<Vec<_>>();
        assert!(!commits.is_empty(), "{stopped}");
        for commit in commits {
            if expected["recordChangeDescriptions"] == true {
                assert!(commit["changeDescriptions"].is_array(), "{stopped}");
            } else {
                assert!(commit["changeDescriptions"].is_null(), "{stopped}");
            }
        }
        if expected["recordTimeline"] == true {
            assert!(stopped["data"]["timelineData"].is_object(), "{stopped}");
            assert!(
                stopped["data"]["timelineData"]["componentMeasures"]
                    .as_array()
                    .is_some_and(|rows| !rows.is_empty()),
                "{stopped}"
            );
        }
        assert_eq!(
            query(mcp, target, &registry, json!({"kind":"renders_stop"})).await,
            stopped
        );
    }
    collectors::refused(mcp,target,json!({"operation":"framework_query","registry":registry,"query":{"kind":"renders_status","recordTimeline":false}}),"FRAMEWORK_QUERY_INVALID",false).await;
    let before = probe(mcp, target, &page).await;
    assert_eq!(before, json!({"starts":5,"stops":5,"reads":5}));
    evaluate(
        mcp,
        target,
        &page,
        "()=>{AcmeProfileProbe.failRead=true;return true;}",
        json!(null),
    )
    .await;
    query(
        mcp,
        target,
        &registry,
        json!({"kind":"renders_start","recordChangeDescriptions":false,"recordTimeline":false}),
    )
    .await;
    failed(mcp, target, &registry, json!({"kind":"renders_stop"})).await;
    let status = query(mcp, target, &registry, json!({"kind":"renders_status"})).await;
    assert_eq!(status["recording"], false);
    assert_eq!(status["phase"], "stopped");
    assert_eq!(status["cleanupPending"], true);
    failed(mcp, target, &other, json!({"kind":"renders_start"})).await;
    assert_eq!(
        probe(mcp, target, &page).await,
        json!({"starts":6,"stops":6,"reads":6})
    );
    let recovered = query(mcp, target, &registry, json!({"kind":"renders_stop"})).await;
    assert_eq!(recovered["elapsedMs"], status["elapsedMs"]);
    assert_eq!(
        query(mcp, target, &registry, json!({"kind":"renders_stop"})).await,
        recovered
    );
    assert_eq!(
        probe(mcp, target, &page).await,
        json!({"starts":6,"stops":6,"reads":7})
    );
    evaluate(
        mcp,
        target,
        &page,
        "()=>{AcmeProfileProbe.failRead=true;return true;}",
        json!(null),
    )
    .await;
    query(mcp, target, &registry, json!({"kind":"renders_start"})).await;
    failed(mcp, target, &registry, json!({"kind":"renders_stop"})).await;
    let stopped_counts = probe(mcp, target, &page).await;
    operation(
        mcp,
        target,
        json!({"operation":"framework_release","registry":registry}),
    )
    .await;
    assert_eq!(probe(mcp, target, &page).await, stopped_counts);
    query(
        mcp,
        target,
        &other,
        json!({"kind":"renders_start","recordTimeline":false}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"framework_release","registry":other}),
    )
    .await;
    assert_eq!(
        probe(mcp, target, &page).await,
        json!({"starts":8,"stops":8,"reads":8})
    );
    let uncertain = operation(
        mcp,
        target,
        json!({"operation":"framework_open","scope":page}),
    )
    .await["registry"]
        .clone();
    let contender = operation(
        mcp,
        target,
        json!({"operation":"framework_open","scope":page}),
    )
    .await["registry"]
        .clone();
    evaluate(
        mcp,
        target,
        &page,
        "()=>{AcmeProfileProbe.failStart=true;return true;}",
        json!(null),
    )
    .await;
    failed(mcp, target, &uncertain, json!({"kind":"renders_start"})).await;
    let status = query(mcp, target, &uncertain, json!({"kind":"renders_status"})).await;
    assert_eq!(status["phase"], "start-uncertain");
    assert!(status["recording"].is_null());
    assert_eq!(status["cleanupPending"], true);
    failed(mcp, target, &contender, json!({"kind":"renders_start"})).await;
    operation(
        mcp,
        target,
        json!({"operation":"framework_release","registry":uncertain}),
    )
    .await;
    query(mcp,target,&contender,json!({"kind":"renders_start","durationMs":100,"recordChangeDescriptions":false,"recordTimeline":false})).await;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let status = query(mcp, target, &contender, json!({"kind":"renders_status"})).await;
        if status["phase"] == "complete" {
            break;
        }
        assert!(std::time::Instant::now() < until, "{status}");
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let expired = query(mcp, target, &contender, json!({"kind":"renders_stop"})).await;
    assert_eq!(
        expired["options"],
        json!({"recordChangeDescriptions":false,"recordTimeline":false})
    );
    assert_eq!(
        query(mcp, target, &contender, json!({"kind":"renders_stop"})).await,
        expired
    );
    operation(
        mcp,
        target,
        json!({"operation":"framework_release","registry":contender}),
    )
    .await;
    assert_eq!(
        probe(mcp, target, &page).await,
        json!({"starts":10,"stops":10,"reads":9})
    );
    let retained_ms = retained_start.unwrap().elapsed().as_secs_f64() * 1000.0;
    evaluate(
        mcp,
        target,
        &page,
        "()=>{AcmeProfileProbe.restore();delete globalThis.AcmeProfileProbe;return true;}",
        json!(null),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"page_init_script_remove","page":page,"scriptId":prepared["scriptId"]}),
    )
    .await;
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    json!({"freshCliProfileStartMs":fresh_ms,"retainedProfileWorkflowWithInspectionMs":retained_ms})
}

#[tokio::test]
async fn installed_step_four_framework_profile_options_checkpoint() {
    let site = Site::start().await;
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let start = std::time::Instant::now();
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready().await;
            let connect_ms = start.elapsed().as_secs_f64() * 1000.0;
            let result = std::panic::AssertUnwindSafe(exercise(
                &fixture,
                &mut mcp,
                target,
                &ready["browser"]["selectedContext"],
                &site.url,
            ))
            .catch_unwind()
            .await;
            let start = std::time::Instant::now();
            mcp.tool("disconnect", json!({"targetId":target})).await;
            let cleanup_ms = start.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(metrics) => eprintln!(
                    "Framework profile {engine} {mode}: {metrics}; connectMs={connect_ms}; cleanupMs={cleanup_ms}"
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
