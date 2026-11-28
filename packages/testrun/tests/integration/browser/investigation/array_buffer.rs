//! ArrayBuffer semantics are compared with the exact pinned SDK before acceptance.
use super::*;

#[tokio::test]
async fn array_buffer_sdk_projection_probe() {
    let fixture = installed_fixture();
    let root = std::path::PathBuf::from(std::env::var_os("TESTRUN_BROWSER_ROOT").unwrap());
    for engine in ["chrome", "firefox", "webkit"] {
        let output = Command::new(root.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"))
            .arg("tests/fixtures/browser-array-buffer-probe.cjs")
            .arg(&root)
            .arg(engine)
            .arg("headless")
            .output()
            .await
            .unwrap();
        std::fs::write(
            fixture
                .workspace
                .path()
                .join(format!("projection-{engine}.json")),
            &output.stdout,
        )
        .unwrap();
        std::fs::write(
            fixture
                .workspace
                .path()
                .join(format!("projection-{engine}.stderr")),
            &output.stderr,
        )
        .unwrap();
        assert!(
            output.status.success(),
            "Pinned {engine}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!(
            "ArrayBuffer projection {engine}: {}; workspace {}",
            String::from_utf8_lossy(&output.stdout),
            fixture.workspace.path().display()
        );
    }
}

async fn reference(fixture: &Fixture, engine: &str, mode: &str) {
    let root = std::path::PathBuf::from(std::env::var_os("TESTRUN_BROWSER_ROOT").unwrap());
    let output = Command::new(root.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"))
        .arg("tests/fixtures/browser-array-buffer.cjs")
        .arg(&root)
        .arg(engine)
        .arg(mode)
        .output()
        .await
        .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("array-buffer-reference-{engine}-{mode}.json")),
        &output.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("array-buffer-reference-{engine}-{mode}.stderr")),
        &output.stderr,
    )
    .unwrap();
    assert!(
        output.status.success(),
        "Pinned {engine}/{mode}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({"inputProjection":true,"resultProjection":true,"resizable":true,"crossRealm":true,"graphCopies":true,"retained":true,"detachedResult":true,"detachedRead":true,"bindingArgument":true,"bindingResolve":true,"bindingReject":true,"navigationFence":true})
    );
}

#[tokio::test]
async fn array_buffer_unchanged_sdk_reference() {
    let fixture = installed_fixture();
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            reference(&fixture, engine, mode).await;
            eprintln!(
                "ArrayBuffer pinned {engine}/{mode}; workspace {}",
                fixture.workspace.path().display()
            );
        }
    }
}

async fn evaluate(mcp: &mut Mcp, target: &str, scope: &Value, expression: &str) -> Value {
    operation(
        mcp,
        target,
        json!({"operation":"evaluate","scope":scope,"expression":expression,"isFunction":true}),
    )
    .await["value"]
        .clone()
}
fn empty(value: &Value) {
    assert_eq!(value["kind"], "object");
    assert_eq!(value["entries"], json!([]));
}
async fn exercise(fixture: &Fixture, mcp: &mut Mcp, target: &str, context: &Value, engine: &str) {
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    let request=json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"()=>{globalThis.acmeBufferReads=0;const b=new ArrayBuffer(4);b.extra=42;Object.defineProperty(b,'getter',{enumerable:true,get(){acmeBufferReads++;throw Error('Acme getter')}});return b}"}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    empty(&completed(mcp, target, submitted).await["value"]);
    assert_eq!(
        evaluate(mcp, target, &page, "()=>acmeBufferReads").await["value"],
        0
    );
    for expression in [
        "()=>new ArrayBuffer(0)",
        "()=>new ArrayBuffer(2,{maxByteLength:8})",
        "()=>{const f=document.createElement('iframe');document.body.append(f);return new f.contentWindow.ArrayBuffer(4)}",
    ] {
        empty(&evaluate(mcp, target, &page, expression).await);
    }
    let graph = evaluate(
        mcp,
        target,
        &page,
        "()=>{const b=new ArrayBuffer(4);return {a:b,b}}",
    )
    .await;
    let entries = graph["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["key"], "a");
    assert_eq!(entries[1]["key"], "b");
    empty(&entries[0]["value"]);
    empty(&entries[1]["value"]);
    assert_ne!(entries[0]["value"]["id"], entries[1]["value"]["id"]);
    let projected=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"value=>({extra:value.extra,arrayBuffer:value instanceof ArrayBuffer})","isFunction":true,"argument":{"kind":"json","value":{"extra":42}}})).await;
    assert_eq!(
        projected["value"]["entries"],
        json!([{"key":"extra","value":{"kind":"json","value":42}},{"key":"arrayBuffer","value":{"kind":"json","value":false}}])
    );
    let parent=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>({buffer:new Uint8Array([0,255,128,65]).buffer})","isFunction":true,"returnHandle":true})).await["handle"].clone();
    let buffer = operation(
        mcp,
        target,
        json!({"operation":"handle_property","handle":parent,"name":"buffer"}),
    )
    .await["handle"]
        .clone();
    operation(
        mcp,
        target,
        json!({"operation":"handle_dispose","handle":parent}),
    )
    .await;
    empty(
        &operation(
            mcp,
            target,
            json!({"operation":"handle_read","handle":buffer}),
        )
        .await["value"],
    );
    let bytes = evaluate(mcp, target, &buffer, "b=>new Uint8Array(b)").await;
    assert_eq!(bytes["kind"], "typed_array");
    assert_eq!(bytes["base64"], "AP+AQQ==");
    evaluate(
        mcp,
        target,
        &buffer,
        "b=>{new Uint8Array(b)[2]=33;return true}",
    )
    .await;
    assert_eq!(
        evaluate(mcp, target, &buffer, "b=>new Uint8Array(b)").await["base64"],
        "AP8hQQ=="
    );
    empty(
        &operation(
            mcp,
            target,
            json!({"operation":"handle_read","handle":buffer}),
        )
        .await["value"],
    );
    for expression in [
        "()=>{const b=new ArrayBuffer(1);structuredClone(b,{transfer:[b]});return b}",
        "()=>{const b=new ArrayBuffer(1);structuredClone(b,{transfer:[b]});return {b}}",
    ] {
        assert_eq!(
            evaluate(mcp, target, &page, expression).await["kind"],
            "undefined"
        );
    }
    let detached=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{const b=new ArrayBuffer(1);structuredClone(b,{transfer:[b]});return b}","isFunction":true,"returnHandle":true})).await["handle"].clone();
    collectors::refused(
        mcp,
        target,
        json!({"operation":"handle_read","handle":detached}),
        if engine == "chrome" {
            "JAVASCRIPT_EXCEPTION"
        } else {
            "BROWSER_METHOD_FAILED"
        },
        true,
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"handle_dispose","handle":detached}),
    )
    .await;
    let binding = operation(
        mcp,
        target,
        json!({"operation":"binding_add","scope":page,"name":"AcmeBufferBridge"}),
    )
    .await["bindingId"]
        .clone();
    let mut disclosed = std::collections::BTreeSet::new();
    for reject in [false, true] {
        let expression = format!(
            "async()=>{{const b=new ArrayBuffer(4);b.extra='Acme private buffer';try{{const v=await AcmeBufferBridge(b);return JSON.stringify({{rejected:false,extra:v.extra,arrayBuffer:v instanceof ArrayBuffer}})}}catch(v){{return JSON.stringify({{rejected:true,extra:v.extra,arrayBuffer:v instanceof ArrayBuffer}})}}}}"
        );
        let held=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":expression,"isFunction":true}})).await;
        let callback =
            super::binding_values::pending(fixture, mcp, target, &binding, &disclosed).await;
        let read = json!({"operation":"binding_read","callback":callback}).to_string();
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &read])
            .await;
        disclosed.insert(submitted["operation"]["id"].as_str().unwrap().to_owned());
        let read = completed(mcp, target, submitted).await;
        assert_eq!(read["private"], true);
        assert_eq!(read["bindingId"], binding);
        assert_eq!(read["arguments"].as_array().unwrap().len(), 1);
        empty(&read["arguments"][0]);
        let extra = if reject { "rejected" } else { "resolved" };
        let response = json!({"kind":if reject{"binding_reject_value"}else{"binding_resolve_value"},"value":{"kind":"json","value":{"extra":extra}}});
        let settled = if reject {
            let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"callback_resolve","callback":callback,"response":response}]})).await;
            completed(mcp, target, submitted).await["results"][0].clone()
        } else {
            operation(
                mcp,
                target,
                json!({"operation":"callback_resolve","callback":callback,"response":response}),
            )
            .await
        };
        assert_eq!(settled["resolved"], true);
        let result = completed(mcp, target, held).await;
        assert_eq!(
            serde_json::from_str::<Value>(result["value"]["value"].as_str().unwrap()).unwrap(),
            json!({"rejected":reject,"extra":extra,"arrayBuffer":false})
        );
    }
    operation(
        mcp,
        target,
        json!({"operation":"binding_remove","scope":page,"bindingId":binding}),
    )
    .await;
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":"data:text/html,<body>Acme replacement</body>","waitUntil":"load"}})).await;
    collectors::refused(
        mcp,
        target,
        json!({"operation":"handle_read","handle":buffer}),
        "STALE_REFERENCE",
        true,
    )
    .await;
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
}

#[tokio::test]
async fn installed_step_four_array_buffer_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    let mut failures = Vec::new();
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            reference(&fixture, engine, mode).await;
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let context = &ready["browser"]["selectedContext"];
            let result =
                std::panic::AssertUnwindSafe(exercise(&fixture, &mut mcp, target, context, engine))
                    .catch_unwind()
                    .await;
            let closed = performance_lifecycle::disconnect_observed(
                &fixture, &mut mcp, target, engine, mode, context,
            )
            .await;
            if closed["result"]["structuredContent"]["ok"] != true {
                failures.push(format!("{engine}/{mode}: {closed}"));
            }
            if let Err(panic) = result {
                mcp.stop().await;
                std::panic::resume_unwind(panic)
            }
            eprintln!(
                "ArrayBuffer projection {engine}/{mode} passed; workspace {}",
                fixture.workspace.path().display()
            );
        }
    }
    mcp.stop().await;
    assert!(
        failures.is_empty(),
        "Original cleanup failures: {failures:?}"
    );
}
