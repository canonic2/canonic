//! Establish the pinned public serializer before choosing corrections.
use super::*;

#[tokio::test]
async fn object_projection_sdk_probe() {
    let fixture = installed_fixture();
    let root = std::path::PathBuf::from(std::env::var_os("TESTRUN_BROWSER_ROOT").unwrap());
    for engine in ["chrome", "firefox", "webkit"] {
        let output = Command::new(root.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"))
            .arg("tests/fixtures/browser-object-projection.cjs")
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
                .join(format!("object-projection-{engine}.json")),
            &output.stdout,
        )
        .unwrap();
        std::fs::write(
            fixture
                .workspace
                .path()
                .join(format!("object-projection-{engine}.stderr")),
            &output.stderr,
        )
        .unwrap();
        assert!(
            output.status.success(),
            "Pinned {engine}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!(
            "Object projection {engine}: {}; workspace {}",
            String::from_utf8_lossy(&output.stdout),
            fixture.workspace.path().display()
        );
    }
}

fn projected(value: &Value, expected: &Value) {
    if expected == &json!({"AcmeUndefined":true}) {
        assert_eq!(value["kind"], "undefined");
        return;
    }
    if let Some(fields) = expected.as_object() {
        assert_eq!(value["kind"], "object", "{value}");
        let entries = value["entries"].as_array().unwrap();
        assert_eq!(entries.len(), fields.len(), "{value}");
        for (key, expected) in fields {
            let entry = entries.iter().find(|entry| entry["key"] == *key).unwrap();
            projected(&entry["value"], expected);
        }
    } else {
        assert_eq!(value["kind"], "json", "{value}");
        assert_eq!(&value["value"], expected);
    }
}
fn cases() -> Vec<(&'static str, Value)> {
    vec![
        (
            "()=>Object.assign(Object.create({inherited:13}),{own:42})",
            json!({"own":42}),
        ),
        (
            "()=>new(class Acme{constructor(){this.own=42}get inherited(){throw Error('Acme prototype getter')}})",
            json!({"own":42}),
        ),
        ("()=>new Map([['Acme',42]])", json!({})),
        (
            "()=>Object.assign(new Map([['Acme',42]]),{own:13})",
            json!({"own":13}),
        ),
        (
            "()=>Object.assign(new Set([42]),{own:13})",
            json!({"own":13}),
        ),
        ("()=>function Acme(){}", json!({"AcmeUndefined":true})),
        ("()=>Symbol('Acme')", json!({"AcmeUndefined":true})),
        (
            "()=>({fn:function Acme(){},symbol:Symbol('Acme'),own:42})",
            json!({"fn":{"AcmeUndefined":true},"symbol":{"AcmeUndefined":true},"own":42}),
        ),
        (
            "()=>({get refused(){throw Error('Acme getter')},own:42})",
            json!({"own":42}),
        ),
        (
            "()=>({toJSON(){throw Error('Acme toJSON')},own:42})",
            json!({"toJSON":{},"own":42}),
        ),
        (
            "()=>Object.defineProperty({},'toJSON',{value(){return {own:42}}})",
            json!({"own":42}),
        ),
        (
            "()=>Object.defineProperty({},'toJSON',{value(){throw Error('Acme toJSON')}})",
            json!({}),
        ),
        (
            "()=>Object.defineProperty({},'toJSON',{value(){return this}})",
            json!({"AcmeUndefined":true}),
        ),
    ]
}
async fn reference(fixture: &Fixture, engine: &str, mode: &str) {
    let root = std::path::PathBuf::from(std::env::var_os("TESTRUN_BROWSER_ROOT").unwrap());
    let output = Command::new(root.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"))
        .arg("tests/fixtures/browser-object-projection.cjs")
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
            .join(format!("object-reference-{engine}-{mode}.json")),
        &output.stdout,
    )
    .unwrap();
    std::fs::write(
        fixture
            .workspace
            .path()
            .join(format!("object-reference-{engine}-{mode}.stderr")),
        &output.stderr,
    )
    .unwrap();
    assert!(
        output.status.success(),
        "Pinned {engine}/{mode}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["nestedKeys"], json!(["fn", "symbol", "own"]));
    assert_eq!(report["bindingKeys"], json!(["map", "fn", "symbol", "own"]));
    assert_eq!(report["mapIdentity"]["same"], true);
}
#[tokio::test]
async fn object_projection_unchanged_sdk_reference() {
    let fixture = installed_fixture();
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            reference(&fixture, engine, mode).await;
            eprintln!(
                "Pinned object projection {engine}/{mode}; workspace {}",
                fixture.workspace.path().display()
            );
        }
    }
}
async fn exercise(fixture: &Fixture, mcp: &mut Mcp, target: &str, context: &Value) {
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    for (index, (expression, expected)) in cases().iter().enumerate() {
        let request =
            json!({"operation":"evaluate","scope":page,"expression":expression,"isFunction":true});
        let value = match index % 3 {
            0 => {
                let submitted = fixture
                    .cli(&["browser", "send", "--target", target, &request.to_string()])
                    .await;
                completed(mcp, target, submitted).await["value"].clone()
            }
            1 => operation(mcp, target, request).await["value"].clone(),
            _ => {
                let submitted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"evaluate","scope":page,"expression":expression,"isFunction":true}]})).await;
                completed(mcp, target, submitted).await["results"][0]["value"].clone()
            }
        };
        projected(&value, expected);
        let handle=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":expression,"isFunction":true,"returnHandle":true})).await["handle"].clone();
        let read = operation(
            mcp,
            target,
            json!({"operation":"handle_read","handle":handle}),
        )
        .await;
        projected(&read["value"], expected);
        operation(
            mcp,
            target,
            json!({"operation":"handle_dispose","handle":handle}),
        )
        .await;
    }
    let graph=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>{const map=Object.assign(new Map([['Acme',42]]),{own:13});return {a:map,b:map}}","isFunction":true})).await["value"].clone();
    let entries = graph["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["key"], "a");
    projected(&entries[0]["value"], &json!({"own":13}));
    assert_eq!(entries[1]["key"], "b");
    assert_eq!(
        entries[1]["value"],
        json!({"kind":"reference","id":entries[0]["value"]["id"]})
    );
    let parent=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"()=>({map:Object.assign(new Map([['Acme',42]]),{own:13})})","isFunction":true,"returnHandle":true})).await["handle"].clone();
    let map = operation(
        mcp,
        target,
        json!({"operation":"handle_property","handle":parent,"name":"map"}),
    )
    .await["handle"]
        .clone();
    operation(
        mcp,
        target,
        json!({"operation":"handle_dispose","handle":parent}),
    )
    .await;
    projected(
        &operation(mcp, target, json!({"operation":"handle_read","handle":map})).await["value"],
        &json!({"own":13}),
    );
    assert_eq!(operation(mcp,target,json!({"operation":"evaluate","scope":map,"expression":"map=>map instanceof Map && map.size===1","isFunction":true})).await["value"]["value"],true);
    let binding = operation(
        mcp,
        target,
        json!({"operation":"binding_add","scope":page,"name":"AcmeObjectBridge"}),
    )
    .await["bindingId"]
        .clone();
    let held=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"evaluate","scope":page,"expression":"async()=>AcmeObjectBridge({map:Object.assign(new Map([['Acme private',42]]),{own:13}),fn:function Acme(){},symbol:Symbol('Acme'),get refused(){throw Error('Acme getter')},own:42})","isFunction":true}})).await;
    let mut disclosed = std::collections::BTreeSet::new();
    let callback = super::binding_values::pending(fixture, mcp, target, &binding, &disclosed).await;
    let submitted = fixture
        .cli(&[
            "browser",
            "send",
            "--target",
            target,
            &json!({"operation":"binding_read","callback":callback}).to_string(),
        ])
        .await;
    disclosed.insert(submitted["operation"]["id"].as_str().unwrap().to_owned());
    let read = completed(mcp, target, submitted).await;
    assert_eq!(read["private"], true);
    assert_eq!(read["bindingId"], binding);
    assert_eq!(read["arguments"].as_array().unwrap().len(), 1);
    projected(
        &read["arguments"][0],
        &json!({"map":{"own":13},"fn":{"AcmeUndefined":true},"symbol":{"AcmeUndefined":true},"own":42}),
    );
    operation(mcp,target,json!({"operation":"callback_resolve","callback":callback,"response":{"kind":"binding_resolve_value","value":{"kind":"json","value":{"accepted":true}}}})).await;
    projected(
        &completed(mcp, target, held).await["value"],
        &json!({"accepted":true}),
    );
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
        json!({"operation":"handle_read","handle":map}),
        "STALE_REFERENCE",
        true,
    )
    .await;
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
}
#[tokio::test]
async fn installed_step_four_object_projection_checkpoint() {
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
                std::panic::AssertUnwindSafe(exercise(&fixture, &mut mcp, target, context))
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
                "Object projection {engine}/{mode} passed; workspace {}",
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
