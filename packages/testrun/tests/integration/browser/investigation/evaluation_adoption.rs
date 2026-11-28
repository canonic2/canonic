//! Installed cross-frame nodes and primitive arguments preserve caller and handle ownership.
use super::*;

async fn retain(mcp: &mut Mcp, target: &str, scope: &Value, expression: &str) -> Value {
    operation(mcp,target,json!({"operation":"evaluate","scope":scope,"expression":expression,"isFunction":true,"returnHandle":true})).await["handle"].clone()
}
async fn value(
    mcp: &mut Mcp,
    target: &str,
    scope: &Value,
    expression: &str,
    argument: Value,
) -> Value {
    operation(mcp,target,json!({"operation":"evaluate","scope":scope,"expression":expression,"isFunction":true,"argument":argument})).await["value"].clone()
}
fn argument(handle: &Value) -> Value {
    json!({"kind":"handle","handle":handle})
}

// A library diagnostic exposes private driver causes which public installed
// operation receipts deliberately omit. It is not installed acceptance.
#[tokio::test]
async fn step_four_bigint_handle_diagnostic() {
    use testrun::browser::{
        contract::{ContextOptions, Engine, LaunchOptions, Mode},
        driver::DiagnosticBundle,
        session::Session,
    };
    use tokio::time::Instant;
    use uuid::Uuid;
    let root = std::path::PathBuf::from(std::env::var_os("TESTRUN_BROWSER_ROOT").unwrap());
    let baseline = Command::new(root.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"))
        .arg("tests/fixtures/browser-bigint-baseline.cjs")
        .arg(root.join("playwright-core-1.64.1/package"))
        .arg("firefox")
        .env("PLAYWRIGHT_BROWSERS_PATH", root.join("browsers"))
        .output()
        .await
        .unwrap();
    assert!(
        baseline.status.success(),
        "{}",
        String::from_utf8_lossy(&baseline.stderr)
    );
    let baseline: Value = serde_json::from_slice(&baseline.stdout).unwrap();
    eprintln!("Pinned unmodified Firefox baseline: {baseline}");
    assert_eq!(baseline["ok"], false, "{baseline}");
    assert!(
        baseline["message"]
            .as_str()
            .unwrap()
            .contains("Execution context was destroyed"),
        "{baseline}"
    );
    let webkit = Command::new(root.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"))
        .arg("tests/fixtures/browser-bigint-baseline.cjs")
        .arg(root.join("playwright-core-1.64.1/package"))
        .arg("webkit")
        .env("PLAYWRIGHT_BROWSERS_PATH", root.join("browsers"))
        .output()
        .await
        .unwrap();
    assert!(
        webkit.status.success(),
        "{}",
        String::from_utf8_lossy(&webkit.stderr)
    );
    let webkit: Value = serde_json::from_slice(&webkit.stdout).unwrap();
    eprintln!("Pinned unmodified WebKit baseline: {webkit}");
    assert_eq!(webkit, json!({"ok":true,"value":"undefined"}));
    let bundle = DiagnosticBundle::inspect(
        &root.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
        &root.join("playwright-core-1.64.1/package"),
        &root.join("browsers"),
        Instant::now() + Duration::from_secs(30),
    )
    .await
    .unwrap();
    let workspace = crate::Workspace::new();
    let private = workspace.path().join("bigint-diagnostic");
    std::fs::create_dir(&private).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
    let session = Session::launch(
        bundle,
        Uuid::new_v4(),
        Uuid::new_v4(),
        LaunchOptions {
            media_fixtures: Default::default(),
            proxy: None,
            engine: Engine::Firefox,
            mode: Mode::Headless,
            context: ContextOptions::default(),
        },
        &private,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while session.status()["phase"] == "starting" {
        assert!(Instant::now() < deadline, "{}", session.status());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(session.status()["phase"], "ready", "{}", session.status());
    let page = session.status()["selectedPage"].clone();
    let request=serde_json::from_value(json!({"operation":"evaluate","scope":page,"expression":"()=>123456789012345678901234567890n","isFunction":true,"returnHandle":true})).unwrap();
    let id = session.submit(Uuid::new_v4(), request, 30000).unwrap();
    let result = session
        .wait(id, Instant::now() + Duration::from_secs(35))
        .await
        .unwrap();
    eprintln!(
        "BigInt diagnostic: {result:?}; causes={:?}",
        session.private_failures()
    );
    let id = session.disconnect(30000).unwrap();
    let cleanup = session
        .wait(id, Instant::now() + Duration::from_secs(35))
        .await
        .unwrap();
    assert_eq!(
        cleanup.outcome,
        testrun::browser::lifecycle::Outcome::Succeeded,
        "{cleanup:?}"
    );
    assert_eq!(
        result.outcome,
        testrun::browser::lifecycle::Outcome::Failed,
        "{result:?}"
    );
    assert_eq!(result.code.as_deref(), Some("BROWSER_METHOD_FAILED"));
    assert!(
        session
            .private_failures()
            .iter()
            .any(|cause| cause.contains("Execution context was destroyed"))
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "One correlated workflow checks caller effects and target/source handle lifetime before owned cleanup"
)]
async fn exercise(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    engine: &str,
) -> Value {
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    let html = r#"<!doctype html><script>globalThis.acmeCalls=0;</script><button>Acme parent</button><iframe id=child srcdoc="<!doctype html><script>globalThis.acmeCalls=0;</script><button>Acme child</button>"></iframe>"#;
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":html,"waitUntil":"load"}})).await;
    let frame = operation(
        mcp,
        target,
        json!({"operation":"frame_enter","page":page,"locator":{"kind":"css","selector":"#child"}}),
    )
    .await["frame"]
        .clone();
    if engine == "chrome" {
        value(mcp,target,&page,"()=>{globalThis.acmeAdoptionTimer=setInterval(()=>console.log('Acme adoption housekeeping'),10);return true;}",json!({"kind":"undefined"})).await;
    }
    let parent_node = retain(mcp, target, &page, "()=>document.querySelector('button')").await;
    let child_node = retain(mcp, target, &frame, "()=>document.querySelector('button')").await;
    let child_body = retain(mcp, target, &frame, "()=>document.body").await;
    let child_owner = retain(mcp, target, &frame, "()=>({label:'Acme target'})").await;
    let request=json!({"operation":"evaluate","scope":frame,"expression":"node=>{acmeCalls++;return node.textContent+'|'+document.querySelector('button').textContent+'|'+(node.ownerDocument===document);}","isFunction":true,"argument":argument(&parent_node)}).to_string();
    let began = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let fresh = completed(mcp, target, submitted).await;
    let fresh_ms = began.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(fresh["value"]["value"], "Acme parent|Acme child|false");
    if engine == "chrome" {
        assert_eq!(fresh["argumentCleanupPending"], false);
    }
    let began = std::time::Instant::now();
    assert_eq!(value(mcp,target,&page,"node=>{acmeCalls++;return node.textContent+'|'+document.querySelector('button').textContent;}",argument(&child_node)).await["value"],"Acme child|Acme parent");
    let duplicate =
        json!({"kind":"array","id":1,"items":[argument(&parent_node),argument(&parent_node)]});
    let selected=operation(mcp,target,json!({"operation":"selector_evaluate","scope":frame,"options":{"locator":{"kind":"css","selector":"button"},"expression":"(element,args)=>{acmeCalls++;return element.textContent+'|'+(args[0]===args[1])+'|'+args[0].textContent;}","isFunction":true,"argument":duplicate}})).await;
    assert_eq!(selected["value"]["value"], "Acme child|true|Acme parent");
    let submitted=mcp.tool("act",json!({"targetId":target,"actions":[
        {"action":"evaluate","scope":child_owner,"expression":"(owner,node)=>{acmeCalls++;return owner.label+'|'+node.textContent;}","isFunction":true,"argument":argument(&parent_node)},
        {"action":"selector_evaluate","scope":frame,"options":{"locator":{"kind":"css","selector":"button"},"expression":"(element,node)=>{acmeCalls++;return node;}","isFunction":true,"argument":argument(&parent_node),"returnHandle":true}},
        {"action":"selector_evaluate","scope":child_body,"options":{"locator":{"kind":"css","selector":"button"},"expression":"(elements,node)=>{acmeCalls++;return elements.length+'|'+node.textContent;}","isFunction":true,"argument":argument(&parent_node),"all":true}}
    ]})).await;
    let actions = completed(mcp, target, submitted).await;
    assert_eq!(actions["completedActions"], 3);
    assert_eq!(
        actions["results"][0]["value"]["value"],
        "Acme target|Acme parent"
    );
    assert_eq!(actions["results"][2]["value"]["value"], "1|Acme parent");
    let selected_handle = actions["results"][1]["handle"].clone();
    assert_eq!(
        value(
            mcp,
            target,
            &selected_handle,
            "node=>node.textContent",
            json!({"kind":"undefined"})
        )
        .await["value"],
        "Acme parent"
    );
    let transferred=operation(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"node=>{acmeCalls++;return node;}","isFunction":true,"argument":argument(&child_node),"returnHandle":true})).await["handle"].clone();
    let mut parent_handles = vec![parent_node.clone(), transferred.clone()];
    for (expression, expected) in [
        ("()=>undefined", "undefined|undefined"),
        ("()=>null", "object|null"),
        ("()=>-0", "number|-0"),
        ("()=>42", "number|42"),
        ("()=>false", "boolean|false"),
        ("()=>'Acme café'", "string|Acme café"),
        (
            "()=>123456789012345678901234567890n",
            "bigint|123456789012345678901234567890",
        ),
    ] {
        if engine == "firefox" && expression.contains("123456789012345678901234567890n") {
            // The unchanged pinned Firefox SDK cannot create this handle;
            // require the same explicit failure, then exercise typed BigInt
            // delivery independently. This is not retained-BigInt acceptance.
            collectors::refused(mcp,target,json!({"operation":"evaluate","scope":page,"expression":expression,"isFunction":true,"returnHandle":true}),"BROWSER_METHOD_FAILED",true).await;
            assert_eq!(
                value(
                    mcp,
                    target,
                    &frame,
                    "value=>{acmeCalls++;return typeof value+'|'+String(value);}",
                    json!({"kind":"bigint","value":"123456789012345678901234567890"})
                )
                .await["value"],
                expected
            );
            continue;
        }
        let handle = retain(mcp, target, &page, expression).await;
        let result=value(mcp,target,&frame,"value=>{acmeCalls++;return typeof value+'|'+(Object.is(value,-0)?'-0':String(value));}",argument(&handle)).await;
        if engine == "webkit" && expression.contains("123456789012345678901234567890n") {
            // The unchanged pinned WebKit SDK creates an undefined handle
            // here. Assert that baseline outcome rather than BigInt fidelity.
            assert_eq!(result["value"], "undefined|undefined");
            assert_eq!(
                value(
                    mcp,
                    target,
                    &frame,
                    "value=>typeof value+'|'+String(value)",
                    json!({"kind":"bigint","value":"123456789012345678901234567890"})
                )
                .await["value"],
                expected
            );
        } else {
            assert_eq!(result["value"], expected);
        }
        parent_handles.push(handle);
    }
    let detached=retain(mcp,target,&page,"()=>{const node=document.createElement('span');node.textContent='Acme detached';return node;}").await;
    assert_eq!(
        value(
            mcp,
            target,
            &frame,
            "node=>{acmeCalls++;return node.textContent+'|'+node.isConnected;}",
            argument(&detached)
        )
        .await["value"],
        "Acme detached|false"
    );
    parent_handles.push(detached);
    // Ordinary objects still cannot cross execution contexts; their refusal
    // must not invoke the caller, even when the backend detects it.
    collectors::refused(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"value=>{acmeCalls++;return value}","isFunction":true,"argument":argument(&child_owner)}),"FOREIGN_HANDLE",true).await;
    collectors::refused(mcp,target,json!({"operation":"selector_evaluate","scope":page,"options":{"locator":{"kind":"css","selector":"button"},"expression":"(element,value)=>{acmeCalls++;return value}","isFunction":true,"argument":argument(&child_owner)}}),"FOREIGN_HANDLE",true).await;
    assert_eq!(
        value(
            mcp,
            target,
            &page,
            "()=>acmeCalls",
            json!({"kind":"undefined"})
        )
        .await["value"],
        2
    );
    assert_eq!(
        value(
            mcp,
            target,
            &frame,
            "()=>acmeCalls",
            json!({"kind":"undefined"})
        )
        .await["value"],
        13
    );
    let other = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    collectors::refused(mcp,target,json!({"operation":"evaluate","scope":other,"expression":"()=>{globalThis.acmeForeignCalled=true;return 0}","isFunction":true,"argument":argument(&parent_node)}),"FOREIGN_HANDLE",true).await;
    assert_eq!(
        value(
            mcp,
            target,
            &other,
            "()=>globalThis.acmeForeignCalled===undefined",
            json!({"kind":"undefined"})
        )
        .await["value"],
        true
    );
    operation(mcp, target, json!({"operation":"page_close","page":other})).await;
    operation(
        mcp,
        target,
        json!({"operation":"handle_dispose","handle":child_node}),
    )
    .await;
    assert_eq!(
        value(
            mcp,
            target,
            &transferred,
            "node=>node.textContent",
            json!({"kind":"undefined"})
        )
        .await["value"],
        "Acme child"
    );
    value(mcp,target,&page,"()=>new Promise(resolve=>{const frame=document.querySelector('iframe');frame.addEventListener('load',()=>resolve(true),{once:true});frame.srcdoc='<!doctype html><p>Acme replacement</p>';})",json!({"kind":"undefined"})).await;
    collectors::refused(
        mcp,
        target,
        json!({"operation":"handle_read","handle":child_body}),
        "STALE_REFERENCE",
        true,
    )
    .await;
    collectors::refused(mcp,target,json!({"operation":"evaluate","scope":page,"expression":"value=>{acmeCalls++;return value}","isFunction":true,"argument":argument(&child_body)}),"STALE_REFERENCE",true).await;
    assert_eq!(
        value(
            mcp,
            target,
            &transferred,
            "node=>node.textContent",
            json!({"kind":"undefined"})
        )
        .await["value"],
        "Acme child",
        "The acknowledged target-world result outlives its source frame's document"
    );
    for handle in parent_handles {
        operation(
            mcp,
            target,
            json!({"operation":"handle_dispose","handle":handle}),
        )
        .await;
    }
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
    json!({"freshCliCrossFrameArgumentMs":fresh_ms,"retainedWorkflowWithLifetimesMs":began.elapsed().as_secs_f64()*1000.0})
}

#[tokio::test]
async fn installed_step_four_evaluation_adoption_checkpoint() {
    let fixture = installed_fixture();
    let mut mcp = Mcp::start(&fixture).await;
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let began = std::time::Instant::now();
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let connect_ms = began.elapsed().as_secs_f64() * 1000.0;
            let context = &ready["browser"]["selectedContext"];
            let result =
                std::panic::AssertUnwindSafe(exercise(&fixture, &mut mcp, target, context, engine))
                    .catch_unwind()
                    .await;
            let began = std::time::Instant::now();
            let disconnect = mcp
                .request(
                    "tools/call",
                    json!({"name":"disconnect","arguments":{"targetId":target}}),
                )
                .await;
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("adoption-disconnect-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(&disconnect).unwrap(),
            )
            .unwrap();
            let cleanup_ms = began.elapsed().as_secs_f64() * 1000.0;
            if disconnect["result"]["structuredContent"]["ok"] != true {
                fixture.cli(&["disconnect"]).await;
            }
            assert_eq!(
                disconnect["result"]["structuredContent"]["ok"], true,
                "{disconnect}"
            );
            match result {
                Ok(metrics) => eprintln!(
                    "Evaluation adoption {engine} {mode}: {metrics}; connectMs={connect_ms}; cleanupMs={cleanup_ms}"
                ),
                Err(panic) => {
                    fixture.cli(&["disconnect"]).await;
                    mcp.stop().await;
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.stop().await;
}
