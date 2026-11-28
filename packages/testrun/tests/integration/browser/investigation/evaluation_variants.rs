//! Generic frame and bound-handle evaluation through installed CLI, MCP and act.
use super::*;

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
async fn exercise(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    engine: &str,
) -> Value {
    let site = Site::start().await;
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    if engine != "chrome" {
        for options in [json!({"page":page}), json!({"autoStop":false})] {
            collectors::refused(
                mcp,
                target,
                json!({"operation":"trace_start","context":context,"options":options}),
                "TRACE_NATIVE_OPTIONS_UNSUPPORTED",
                false,
            )
            .await;
        }
    }
    let html = r#"<!doctype html><script>globalThis.acmeScope='parent';globalThis.acmeCalls=0;</script><button>Acme parent</button><iframe id=child srcdoc="<!doctype html><script>globalThis.acmeScope='child';globalThis.acmeCalls=0;</script><button>Acme child</button>"></iframe>"#;
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":html,"waitUntil":"load"}})).await;
    let frame = operation(
        mcp,
        target,
        json!({"operation":"frame_enter","page":page,"locator":{"kind":"css","selector":"#child"}}),
    )
    .await["frame"]
        .clone();
    let mut all_handles = Vec::new();
    let mut owners = Vec::new();
    let mut chains = Vec::new();
    let mut nulls = Vec::new();
    let mut fresh_ms = 0.0;
    let mut retained_start = None;
    for (scope, label) in [(&page, "parent"), (&frame, "child")] {
        let sources = [
            "value=>{globalThis.acmeCalls++;return acmeScope+'|'+typeof value+'|'+(value===null)}",
            "function acme(value){globalThis.acmeCalls++;return acmeScope+'|'+typeof value+'|'+(value===null)}",
            "acme(value){globalThis.acmeCalls++;return acmeScope+'|'+typeof value+'|'+(value===null)}",
            "async acme(value){globalThis.acmeCalls++;return acmeScope+'|'+typeof value+'|'+(value===null)}",
        ];
        for (index, source) in sources.iter().enumerate() {
            let result = if label == "parent" && index == 0 {
                let request = json!({"operation":"evaluate","scope":scope,"expression":source,"isFunction":true}).to_string();
                let start = std::time::Instant::now();
                let submitted = fixture
                    .cli(&["browser", "send", "--target", target, &request])
                    .await;
                let result = completed(mcp, target, submitted).await;
                fresh_ms = start.elapsed().as_secs_f64() * 1000.0;
                retained_start = Some(std::time::Instant::now());
                result
            } else {
                evaluate(mcp, target, scope, source, None, false).await
            };
            assert_eq!(
                result["value"]["value"],
                format!("{label}|undefined|false"),
                "{result}"
            );
            if label == "child" {
                assert_eq!(result["frame"], frame);
            }
        }
        for (argument, expected) in [
            (
                json!({"kind":"json","value":null}),
                format!("{label}|object|true"),
            ),
            (
                json!({"kind":"undefined"}),
                format!("{label}|undefined|false"),
            ),
        ] {
            let result = evaluate(mcp, target, scope, sources[0], Some(argument), false).await;
            assert_eq!(result["value"]["value"], expected);
        }
        for (argument, expected) in [
            (None, "undefined"),
            (Some(json!({"kind":"json","value":null})), "object"),
        ] {
            let mut request = json!({"operation":"selector_evaluate","scope":scope,"options":{"locator":{"kind":"css","selector":"button"},"expression":"async acme(element,arg){return element.textContent+'|'+typeof arg;}","isFunction":true}});
            if let Some(argument) = argument {
                request["options"]["argument"] = argument;
            }
            assert_eq!(
                operation(mcp, target, request).await["value"]["value"],
                format!("Acme {label}|{expected}")
            );
        }
        let result = operation(mcp,target,json!({"operation":"evaluate","scope":scope,"expression":"globalThis.acmeCalls++; 42","isFunction":false})).await;
        assert_eq!(result["value"]["value"], 42);
        let function = operation(mcp,target,json!({"operation":"evaluate","scope":scope,"expression":"function acme(){globalThis.acmeCalls++;}","isFunction":false,"returnHandle":true})).await["handle"].clone();
        assert_eq!(
            evaluate(
                mcp,
                target,
                &function,
                "(owner,arg)=>typeof owner+'|'+typeof arg",
                None,
                false
            )
            .await["value"]["value"],
            "function|undefined"
        );
        all_handles.push(function);
        let owner = evaluate(mcp,target,scope,"async acme(value){globalThis.acmeCalls++;const shared={tag:acmeScope};return {shared,a:shared,missing:value,explicit:null};}",None,true).await["handle"].clone();
        let value = evaluate(mcp,target,&owner,"async acme(owner,arg){globalThis.acmeCalls++;return owner.a===owner.shared && owner.missing===undefined && owner.explicit===null && arg===undefined;}",None,false).await;
        assert_eq!(value["value"]["value"], true);
        let chain = evaluate(mcp,target,&owner,"async acme(owner,arg){globalThis.acmeCalls++;return {from:owner.shared,owner,argument:arg};}",Some(json!({"kind":"handle","handle":owner})),true).await["handle"].clone();
        assert_eq!(evaluate(mcp,target,&chain,"(value,other)=>value.argument===other && value.owner===other && value.from===other.shared",Some(json!({"kind":"handle","handle":owner})),false).await["value"]["value"],true);
        let undefined =
            evaluate(mcp, target, scope, "value=>value", None, true).await["handle"].clone();
        assert_eq!(
            operation(
                mcp,
                target,
                json!({"operation":"handle_read","handle":undefined})
            )
            .await["value"],
            json!({"kind":"undefined"})
        );
        let null_handle = evaluate(
            mcp,
            target,
            &owner,
            "(owner,arg)=>arg",
            Some(json!({"kind":"json","value":null})),
            true,
        )
        .await["handle"]
            .clone();
        assert_eq!(
            operation(
                mcp,
                target,
                json!({"operation":"handle_read","handle":null_handle})
            )
            .await["value"],
            json!({"kind":"json","value":null})
        );
        let element = evaluate(
            mcp,
            target,
            scope,
            "()=>document.querySelector('button')",
            None,
            true,
        )
        .await["handle"]
            .clone();
        assert_eq!(
            evaluate(
                mcp,
                target,
                &element,
                "async acme(element,arg){return element.textContent+'|'+typeof arg;}",
                None,
                false
            )
            .await["value"]["value"],
            format!("Acme {label}|undefined")
        );
        let count = operation(mcp,target,json!({"operation":"evaluate","scope":scope,"expression":"acmeCalls","isFunction":false})).await;
        assert_eq!(
            count["value"]["value"], 10,
            "Caller expressions must execute exactly once: {count}"
        );
        all_handles.extend([
            owner.clone(),
            chain.clone(),
            undefined,
            null_handle.clone(),
            element,
        ]);
        owners.push(owner);
        chains.push(chain);
        nulls.push(null_handle);
    }
    // The shared adapter must preserve omission independently of explicit null.
    for (argument, expected) in [
        (None, "undefined"),
        (Some(json!({"kind":"json","value":null})), "object"),
    ] {
        let mut action = json!({"action":"evaluate","scope":frame,"expression":"async acme(value){return typeof value;}","isFunction":true});
        let mut retained = json!({"action":"evaluate","scope":frame,"expression":"async acme(value){return value;}","isFunction":true,"returnHandle":true});
        let mut selector = json!({"action":"selector_evaluate","scope":frame,"options":{"locator":{"kind":"css","selector":"button"},"expression":"async acme(element,value){return typeof value;}","isFunction":true}});
        if let Some(argument) = argument {
            action["argument"] = argument.clone();
            retained["argument"] = argument.clone();
            selector["options"]["argument"] = argument;
        }
        let submitted = mcp
            .tool(
                "act",
                json!({"targetId":target,"actions":[action,retained,selector]}),
            )
            .await;
        let result = completed(mcp, target, submitted).await;
        assert_eq!(result["completedActions"], 3);
        assert_eq!(result["results"][0]["value"]["value"], expected);
        assert_eq!(result["results"][2]["value"]["value"], expected);
        let handle = result["results"][1]["handle"].clone();
        let read = operation(
            mcp,
            target,
            json!({"operation":"handle_read","handle":handle}),
        )
        .await;
        assert_eq!(
            read["value"],
            if expected == "undefined" {
                json!({"kind":"undefined"})
            } else {
                json!({"kind":"json","value":null})
            }
        );
        all_handles.push(handle);
    }
    collectors::refused(mcp,target,json!({"operation":"evaluate","scope":frame,"expression":"()=>{globalThis.acmeCalls++;return 0}","isFunction":true,"argument":{"kind":"handle","handle":owners[0]}}),"FOREIGN_HANDLE",true).await;
    assert_eq!(operation(mcp,target,json!({"operation":"evaluate","scope":frame,"expression":"acmeCalls","isFunction":false})).await["value"]["value"],10);
    operation(
        mcp,
        target,
        json!({"operation":"handle_dispose","handle":owners[0]}),
    )
    .await;
    all_handles.retain(|handle| handle != &owners[0]);
    collectors::refused(
        mcp,
        target,
        json!({"operation":"handle_read","handle":owners[0]}),
        "STALE_REFERENCE",
        true,
    )
    .await;
    assert_eq!(evaluate(mcp,target,&chains[0],"value=>value.argument===value.owner && value.from===value.owner.shared && value.owner.explicit===null",None,false).await["value"]["value"],true);
    for handle in all_handles {
        if handle != chains[1] && handle != nulls[0] {
            operation(
                mcp,
                target,
                json!({"operation":"handle_dispose","handle":handle}),
            )
            .await;
        }
    }
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":site.url,"waitUntil":"load"}})).await;
    for handle in [&chains[1], &nulls[0]] {
        collectors::refused(
            mcp,
            target,
            json!({"operation":"handle_read","handle":handle}),
            "STALE_REFERENCE",
            true,
        )
        .await;
    }
    collectors::refused(
        mcp,
        target,
        json!({"operation":"evaluate","scope":frame,"expression":"()=>42","isFunction":true}),
        "STALE_REFERENCE",
        true,
    )
    .await;
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    json!({"freshCliEvaluateMs":fresh_ms,"retainedFrameHandleWorkflowMs":retained_start.expect("Fresh CLI evaluation precedes retained work").elapsed().as_secs_f64()*1000.0})
}

#[tokio::test]
async fn installed_step_four_evaluation_variants_checkpoint() {
    let fixture = installed_fixture();
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
            let result = std::panic::AssertUnwindSafe(exercise(
                &fixture,
                &mut mcp,
                target,
                &ready["browser"]["selectedContext"],
                engine,
            ))
            .catch_unwind()
            .await;
            let start = std::time::Instant::now();
            mcp.tool("disconnect", json!({"targetId":target})).await;
            let cleanup_ms = start.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(metrics) => eprintln!(
                    "Frame/handle evaluation {engine} {mode}: {metrics}; connectMs={connect_ms}; cleanupMs={cleanup_ms}"
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
