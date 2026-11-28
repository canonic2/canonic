//! Installed immediate selector evaluation across page, frame and element roots.
use super::*;

fn request(scope: &Value, selector: &str, expression: &str, all: bool) -> Value {
    json!({"operation":"selector_evaluate","scope":scope,"options":{"locator":{"kind":"css","selector":selector},"expression":expression,"isFunction":true,"all":all}})
}
async fn evaluate(
    mcp: &mut Mcp,
    target: &str,
    scope: &Value,
    expression: &str,
    retain: bool,
) -> Value {
    operation(mcp,target,json!({"operation":"evaluate","scope":scope,"expression":expression,"isFunction":true,"returnHandle":retain})).await
}
async fn retained_and_waiting(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    page: &Value,
    frame: &Value,
    root: &Value,
) {
    let mut handles = Vec::new();
    for (scope, selector, expected) in [(page, "#root", 2), (frame, ".item", 0), (root, ".item", 0)]
    {
        let mut call = request(scope, selector, "async element=>element", false);
        call["options"]["returnHandle"] = json!(true);
        let result = if scope == page {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &call.to_string()])
                .await;
            completed(mcp, target, submitted).await
        } else {
            operation(mcp, target, call).await
        };
        let node = result["handle"].clone();
        assert_eq!(
            operation(
                mcp,
                target,
                request(&node, ".item", "elements=>elements.length", true)
            )
            .await["value"]["value"],
            expected
        );
        handles.push(node);
    }
    for (selector, expected) in [(".item", 2), (".missing", 0)] {
        let mut call = request(root, selector, "elements=>elements", true);
        call["options"]["returnHandle"] = json!(true);
        let array = operation(mcp, target, call).await["handle"].clone();
        assert_eq!(
            evaluate(mcp, target, &array, "array=>array.length", false).await["value"]["value"],
            expected
        );
        handles.push(array);
    }
    let mut cyclic = request(
        page,
        "#root",
        "element=>{const result={name:'Acme retained',element};result.self=result;return result;}",
        false,
    );
    cyclic["options"]["returnHandle"] = json!(true);
    let object = operation(mcp, target, cyclic).await["handle"].clone();
    assert_eq!(
        evaluate(
            mcp,
            target,
            &object,
            "value=>value.self===value && value.element.id==='root'",
            false
        )
        .await["value"]["value"],
        true
    );
    handles.push(object);
    let mut primitive = request(page, "#root", "element=>null", false);
    primitive["options"]["returnHandle"] = json!(true);
    let primitive = operation(mcp, target, primitive).await["handle"].clone();
    assert_eq!(
        evaluate(mcp, target, &primitive, "value=>value===null", false).await["value"]["value"],
        true
    );
    handles.push(primitive);
    let mut detached = request(page, "#root", "element=>element.cloneNode(true)", false);
    detached["options"]["returnHandle"] = json!(true);
    let detached = operation(mcp, target, detached).await["handle"].clone();
    let mut call = request(
        &detached,
        ".item",
        "element=>++globalThis.AcmeWaitingCalls",
        false,
    );
    call["options"]["waitForAttachment"] = json!(true);
    super::collectors::refused(mcp, target, call, "SELECTOR_ROOT_DETACHED", true).await;
    handles.push(detached);
    for scope in [page, frame, root] {
        evaluate(mcp,target,scope,"root=>{globalThis.AcmeWaitingCalls=0;setTimeout(()=>{const element=document.createElement('span');element.className='acme-delayed';element.textContent='Acme delayed';(root instanceof Element?root:document.body).append(element);},200);return true;}",false).await;
        let mut call = request(
            scope,
            ".acme-delayed",
            "async element=>{globalThis.AcmeWaitingCalls++;return element;}",
            false,
        );
        call["options"]["waitForAttachment"] = json!(true);
        call["options"]["waitTimeoutMs"] = json!(3000);
        call["options"]["returnHandle"] = json!(true);
        let node = operation(mcp, target, call).await["handle"].clone();
        assert_eq!(
            evaluate(mcp, target, &node, "element=>element.textContent", false).await["value"]["value"],
            "Acme delayed"
        );
        assert_eq!(
            evaluate(mcp, target, scope, "()=>AcmeWaitingCalls", false).await["value"]["value"],
            1
        );
        evaluate(mcp, target, &node, "element=>element.remove()", false).await;
        handles.push(node);
        let mut timeout = request(
            scope,
            ".acme-never",
            "element=>++globalThis.AcmeWaitingCalls",
            false,
        );
        timeout["options"]["waitForAttachment"] = json!(true);
        timeout["options"]["waitTimeoutMs"] = json!(50);
        super::collectors::refused(mcp, target, timeout, "SELECTOR_WAIT_EXPIRED", true).await;
        assert_eq!(
            evaluate(mcp, target, scope, "()=>AcmeWaitingCalls", false).await["value"]["value"],
            1
        );
    }
    let mut ambiguous = request(
        root,
        ".item",
        "element=>++globalThis.AcmeWaitingCalls",
        false,
    );
    ambiguous["options"]["waitForAttachment"] = json!(true);
    super::collectors::refused(mcp, target, ambiguous, "AMBIGUOUS_LOCATOR", true).await;
    let acted=mcp.tool("act",json!({"targetId":target,"actions":[{"action":"selector_evaluate","scope":page,"options":{"locator":{"kind":"css","selector":"#root"},"expression":"async element=>element","isFunction":true,"waitForAttachment":true,"waitTimeoutMs":1000,"returnHandle":true}}]})).await;
    let acted = completed(mcp, target, acted).await;
    assert_eq!(acted["completedActions"], 1);
    handles.push(acted["results"][0]["handle"].clone());
    for handle in handles {
        operation(
            mcp,
            target,
            json!({"operation":"handle_dispose","handle":handle}),
        )
        .await;
        super::collectors::refused(
            mcp,
            target,
            json!({"operation":"handle_read","handle":handle}),
            "STALE_REFERENCE",
            true,
        )
        .await;
    }
}
#[expect(
    clippy::too_many_lines,
    reason = "One owned workflow compares page/frame/element selection and releases its handles after stale/foreign failures"
)]
async fn run(
    fixture: &Fixture,
    mcp: &mut Mcp,
    target: &str,
    context: &Value,
    page: &Value,
    base: &str,
) {
    operation(
        mcp,
        target,
        json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":base}}),
    )
    .await;
    evaluate(mcp,target,page,r#"async()=>{
        globalThis.AcmeCalls=0;
        document.body.innerHTML='<section id="root"><span class="item">Acme first</span><span class="item">Acme second</span></section><span class="item">Acme outside</span><div id="shadow"></div><iframe id="child"></iframe>';
        document.querySelector('#shadow').attachShadow({mode:'open'}).innerHTML='<button aria-label="Acme shadow">Acme shadow</button>';
        const child=document.querySelector('#child');
        await new Promise(resolve=>{child.onload=resolve;child.srcdoc='<span class="item">Acme child</span>';});
        return true;
    }"#,false).await;
    let started = std::time::Instant::now();
    let mut first = request(
        page,
        ".item",
        "async (element,arg)=>{globalThis.AcmeCalls++;return element.textContent+arg;}",
        false,
    );
    first["options"]["argument"] = json!({"kind":"json","value":"!"});
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &first.to_string()])
        .await;
    assert_eq!(
        completed(mcp, target, submitted).await["value"]["value"],
        "Acme first!"
    );
    let fresh_ms = started.elapsed().as_secs_f64() * 1000.0;
    let retained = std::time::Instant::now();
    assert_eq!(
        operation(
            mcp,
            target,
            request(page, ".item", "elements=>elements.length", true)
        )
        .await["value"]["value"],
        3
    );
    let retained_all_ms = retained.elapsed().as_secs_f64() * 1000.0;
    let mut role = request(page, "button", "element=>element.textContent", false);
    role["options"]["locator"] =
        json!({"kind":"role","role":"button","name":"Acme shadow","exact":true});
    assert_eq!(
        operation(mcp, target, role).await["value"]["value"],
        "Acme shadow"
    );
    assert_eq!(
        operation(
            mcp,
            target,
            request(page, ".missing", "elements=>elements.length", true)
        )
        .await["value"]["value"],
        0
    );
    super::collectors::refused(
        mcp,
        target,
        request(page, ".missing", "element=>++globalThis.AcmeCalls", false),
        "ELEMENT_NOT_FOUND",
        true,
    )
    .await;
    let mut strict = request(page, ".item", "element=>++globalThis.AcmeCalls", false);
    strict["options"]["strict"] = json!(true);
    super::collectors::refused(mcp, target, strict, "AMBIGUOUS_LOCATOR", true).await;
    let mut invalid = request(page, ".item", "element=>++globalThis.AcmeCalls", true);
    invalid["options"]["strict"] = json!(true);
    super::collectors::refused(mcp, target, invalid, "SELECTOR_EVALUATION_OPTIONS", false).await;
    assert_eq!(
        evaluate(mcp, target, page, "()=>AcmeCalls", false).await["value"]["value"],
        1
    );
    let mut big = request(
        page,
        ".item",
        "async acme(element,arg) { return arg+1n; }",
        false,
    );
    big["options"]["argument"] = json!({"kind":"bigint","value":"41"});
    assert_eq!(
        operation(mcp, target, big).await["value"],
        json!({"kind":"bigint","value":"42"})
    );
    let mut expression = request(page, ".item", "17; 42", false);
    expression["options"]["isFunction"] = json!(false);
    assert_eq!(
        operation(mcp, target, expression).await["value"]["value"],
        42
    );
    let root = evaluate(
        mcp,
        target,
        page,
        "()=>document.querySelector('#root')",
        true,
    )
    .await["handle"]
        .clone();
    let ordinary = evaluate(
        mcp,
        target,
        page,
        "()=>({name:'Acme object',nodeType:1})",
        true,
    )
    .await["handle"]
        .clone();
    assert_eq!(
        operation(
            mcp,
            target,
            request(&root, ".item", "elements=>elements.length", true)
        )
        .await["value"]["value"],
        2
    );
    assert_eq!(
        operation(
            mcp,
            target,
            request(&root, ".item", "element=>element.textContent", false)
        )
        .await["value"]["value"],
        "Acme first"
    );
    assert_eq!(
        operation(
            mcp,
            target,
            request(&root, "#root", "elements=>elements.length", true)
        )
        .await["value"]["value"],
        0
    );
    super::collectors::refused(
        mcp,
        target,
        request(&ordinary, ".item", "element=>42", false),
        "INVALID_SCOPE",
        true,
    )
    .await;
    let frame = operation(
        mcp,
        target,
        json!({"operation":"frame_enter","page":page,"locator":{"kind":"css","selector":"#child"}}),
    )
    .await["frame"]
        .clone();
    let mut snapshot = request(&frame, ".item", "element=>42", false);
    snapshot["options"]["locator"] =
        json!({"kind":"element","frame":frame,"id":uuid::Uuid::new_v4()});
    super::collectors::refused(
        mcp,
        target,
        snapshot,
        "SELECTOR_REFERENCE_UNAVAILABLE",
        false,
    )
    .await;
    assert_eq!(
        operation(
            mcp,
            target,
            request(&frame, ".item", "element=>element.textContent", false)
        )
        .await["value"]["value"],
        "Acme child"
    );
    assert_eq!(
        operation(
            mcp,
            target,
            request(&frame, ".item", "elements=>elements.length", true)
        )
        .await["value"]["value"],
        1
    );
    let mut adopted = request(&frame, ".item", "(element,arg)=>arg.tagName", false);
    adopted["options"]["argument"] = json!({"kind":"handle","handle":root});
    assert_eq!(
        operation(mcp, target, adopted).await["value"]["value"],
        "SECTION"
    );
    let other_page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":false}),
    )
    .await["page"]
        .clone();
    let other_node =
        evaluate(mcp, target, &other_page, "()=>document.body", true).await["handle"].clone();
    let mut foreign = request(&frame, ".item", "(element,arg)=>arg", false);
    foreign["options"]["argument"] = json!({"kind":"handle","handle":other_node});
    super::collectors::refused(mcp, target, foreign, "FOREIGN_HANDLE", true).await;
    operation(
        mcp,
        target,
        json!({"operation":"page_close","page":other_page}),
    )
    .await;
    let acted=mcp.tool("act",json!({"targetId":target,"actions":[
        {"action":"selector_evaluate","scope":page,"options":{"locator":{"kind":"css","selector":"#root"},"expression":"(element,arg)=>element.tagName+arg","isFunction":true,"argument":{"kind":"json","value":"!"},"strict":true,"all":false}},
        {"action":"selector_evaluate","scope":frame,"options":{"locator":{"kind":"css","selector":".item"},"expression":"(elements,arg)=>elements.length+arg","isFunction":true,"argument":{"kind":"json","value":3},"strict":false,"all":true}},
        {"action":"selector_evaluate","scope":root,"options":{"locator":{"kind":"css","selector":".item"},"expression":"(elements,arg)=>elements.length+arg","isFunction":true,"argument":{"kind":"json","value":7},"strict":false,"all":true}}
    ]})).await;
    let acted = completed(mcp, target, acted).await;
    assert_eq!(acted["completedActions"], 3);
    assert_eq!(acted["results"][0]["value"]["value"], "SECTION!");
    assert_eq!(acted["results"][1]["value"]["value"], 4);
    assert_eq!(acted["results"][2]["value"]["value"], 9);
    retained_and_waiting(fixture, mcp, target, page, &frame, &root).await;
    operation(
        mcp,
        target,
        json!({"operation":"handle_dispose","handle":ordinary}),
    )
    .await;
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{base}/other?selector=Acme")}})).await;
    super::collectors::refused(
        mcp,
        target,
        request(&root, ".item", "element=>42", false),
        "STALE_REFERENCE",
        true,
    )
    .await;
    eprintln!(
        "Selector evaluation latency: {}",
        json!({"freshCliSingleMs":fresh_ms,"retainedAllMs":retained_all_ms,"retainedWorkflowWithHandlesFramesAndCleanupMs":retained.elapsed().as_secs_f64()*1000.0})
    );
}

#[tokio::test]
async fn installed_step_four_selector_evaluation_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut cleanup_failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            let ready = fixture.ready_target(target).await;
            let result=std::panic::AssertUnwindSafe(async {
                let page=operation(&mut mcp,target,json!({"operation":"page_create","context":ready["browser"]["selectedContext"],"select":false})).await["page"].clone();
                run(&fixture,&mut mcp,target,&ready["browser"]["selectedContext"],&page,&site.url).await;
                operation(&mut mcp,target,json!({"operation":"page_close","page":page})).await;
                eprintln!("Selector evaluation finished {engine} {mode}");
            }).catch_unwind().await;
            let disconnected = mcp
                .request(
                    "tools/call",
                    json!({"name":"disconnect","arguments":{"targetId":target}}),
                )
                .await;
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("selector-disconnect-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(&disconnected).unwrap(),
            )
            .unwrap();
            if disconnected["result"]["structuredContent"]["ok"] != true {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                cleanup_failures.push(disconnected);
            }
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
    assert!(
        cleanup_failures.is_empty(),
        "Selector disconnects: {cleanup_failures:?}"
    );
}
