//! Application tool schemas, one-use invocations, result handles and document fences.
use super::investigation::Site;
use super::*;
use serde_json::json;
pub(super) const HTML: &str = r#"<!doctype html><title>Acme tools</title><button id=probe>Acme</button><script>
globalThis.acmeToolCalls=0;globalThis.acmeToolValue=0;globalThis.acmeNodeCalls=0;globalThis.acmeNestedCalls=0;
addEventListener('devtoolstooldiscovery',event=>event.respondWith({name:'Acme',description:'Application diagnostics',tools:[
{name:'increment',description:'Increment the application value',inputSchema:{type:'object',definitions:{amount:{type:'integer',minimum:1,maximum:100}},properties:{amount:{$ref:'#/definitions/amount'}},required:['amount'],additionalProperties:false},execute:({amount})=>{acmeToolCalls++;acmeToolValue+=amount;return acmeToolValue;}},
{name:'node',description:'Return the application button',inputSchema:{type:'object',additionalProperties:false},execute:()=>{acmeNodeCalls++;return document.querySelector('#probe')}},
{name:'element',description:'Read an owned application element',inputSchema:{type:'object',properties:{element:{'x-mcp-type':'HTMLElement'}},required:['element'],additionalProperties:false},execute:({element})=>{acmeToolCalls++;return element.tagName;}},
{name:'nested',description:'Read nested document elements',inputSchema:{type:'object',definitions:{node:{'x-mcp-type':'HTMLElement'}},properties:{payload:{type:'object',properties:{nodes:{type:'array',items:{$ref:'#/definitions/node'}}},required:['nodes'],additionalProperties:false},plain:{type:'object'}},required:['payload','plain'],additionalProperties:false},execute:({payload,plain})=>{acmeNestedCalls++;return {tags:payload.nodes.map(node=>node.tagName),plain:!!plain.elementHandle};}},
{name:'dialog',description:'Exercise an application callback',inputSchema:{type:'object',additionalProperties:false},execute:async()=>{alert('Acme tool dialog');return 'Acme resolved';}},
{name:'explode',description:'Mutate then throw',inputSchema:{type:'object',additionalProperties:false},execute:()=>{acmeToolCalls++;throw Error('Acme tool exception');}}
]}));</script>"#;
macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run(Engine::$engine, Mode::$mode).await;
        }
    };
}
cell!(item_four_developer_tools_chrome_headless, Chrome, Headless);
cell!(item_four_developer_tools_chrome_headed, Chrome, Headed);
cell!(
    item_four_developer_tools_firefox_headless,
    Firefox,
    Headless
);
cell!(item_four_developer_tools_firefox_headed, Firefox, Headed);
cell!(item_four_developer_tools_webkit_headless, Webkit, Headless);
cell!(item_four_developer_tools_webkit_headed, Webkit, Headed);

async fn annotated_child_result(session: &Session, page: Reference) -> Reference {
    successful(session,Request::Evaluate {scope:page,expression:"new Promise(resolve=>{const frame=document.createElement('iframe');frame.id='acme-tools-child';frame.src='/tools';frame.onload=()=>resolve(true);document.body.append(frame)})".into(),is_function:Some(false),argument:Default::default(),return_handle:false}).await;
    let entered = successful(
        session,
        Request::FrameEnter {
            page,
            frame: None,
            locator: Locator::Css {
                selector: "#acme-tools-child".into(),
            },
        },
    )
    .await;
    let frame = serde_json::from_value(entered["frame"].clone()).unwrap();
    let discovered = successful(session, Request::DeveloperToolsDiscover { scope: frame }).await;
    let registry = serde_json::from_value(discovered["registry"].clone()).unwrap();
    let tool_id = discovered["metadata"]["groups"][0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "node")
        .unwrap()["toolId"]
        .as_str()
        .unwrap()
        .to_owned();
    let result = successful(
        session,
        Request::DeveloperToolCall {
            registry,
            tool_id,
            parameters: json!({}),
            return_handle: true,
        },
    )
    .await;
    let handle = serde_json::from_value(result["handle"].clone()).unwrap();
    successful(session, Request::DeveloperToolsRelease { registry }).await;
    let projected = successful(session, Request::DeveloperToolResultRead { handle }).await;
    assert_eq!(projected["frame"], json!(frame));
    let locator = serde_json::from_value(projected["annotations"][0]["locator"].clone()).unwrap();
    let snapshot = successful(
        session,
        Request::PageSnapshot {
            page,
            frame: Some(frame),
            locator: Some(locator),
        },
    )
    .await;
    assert_eq!(snapshot["frame"], json!(frame));
    assert!(
        snapshot["value"].as_str().unwrap().contains("Acme"),
        "{snapshot}"
    );
    handle
}
#[expect(
    clippy::too_many_lines,
    reason = "One owned workflow keeps schema, callback and document fences under the same teardown"
)]
async fn run(engine: Engine, mode: Mode) {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let site = Site::start().await;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("state");
    crate::artifacts::directory(&directory).unwrap();
    let bundle = if engine == Engine::Chrome {
        Bundle::Chrome(
            super::super::preparation::prepare_chrome(|_| {})
                .await
                .unwrap(),
        )
    } else {
        Bundle::Playwright(
            super::super::preparation::prepare(engine, |_| {})
                .await
                .unwrap(),
        )
    };
    let session = Session::launch_connection(
        bundle,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        LaunchOptions {
            media_fixtures: Default::default(),
            proxy: None,
            engine,
            mode,
            context: Default::default(),
        },
        &directory,
    )
    .unwrap();
    let exercise = std::panic::AssertUnwindSafe(async {
        let ready = settled(&session, session.startup()).await;
        assert_eq!(ready.outcome, Outcome::Succeeded, "{ready:?}");
        let ready = ready.result.unwrap();
        let page = serde_json::from_value(ready["page"].clone()).unwrap();
        let context = serde_json::from_value(ready["context"].clone()).unwrap();
        successful(
            &session,
            Request::PageNavigate {
                page,
                navigation: Navigation::Goto {
                    url: format!("{}/tools", site.url),
                    wait_until: Load::Load,
                },
            },
        )
        .await;
        let discovered =
            successful(&session, Request::DeveloperToolsDiscover { scope: page }).await;
        let registry = serde_json::from_value(discovered["registry"].clone()).unwrap();
        let tools = discovered["metadata"]["groups"][0]["tools"]
            .as_array()
            .unwrap();
        assert_eq!(tools.len(), 6, "{discovered}");
        let increment = tools
            .iter()
            .find(|tool| tool["name"] == "increment")
            .unwrap()["toolId"]
            .as_str()
            .unwrap()
            .to_owned();
        let node = tools.iter().find(|tool| tool["name"] == "node").unwrap()["toolId"]
            .as_str()
            .unwrap()
            .to_owned();
        let invalid = settled(
            &session,
            session
                .submit(
                    Uuid::new_v4(),
                    Request::DeveloperToolCall {
                        registry,
                        tool_id: increment.clone(),
                        parameters: json!({"amount":"wrong"}),
                        return_handle: false,
                    },
                    30000,
                )
                .unwrap(),
        )
        .await;
        assert_eq!(
            invalid.code.as_deref(),
            Some("DEVELOPER_TOOL_ARGUMENTS_INVALID"),
            "{invalid:?}"
        );
        let count = successful(
            &session,
            Request::Evaluate {
                scope: page,
                expression: "acmeToolCalls".into(),
                is_function: Some(false),
                argument: Default::default(),
                return_handle: false,
            },
        )
        .await;
        assert_eq!(count["value"]["value"], 0);
        let result = successful(
            &session,
            Request::DeveloperToolCall {
                registry,
                tool_id: increment.clone(),
                parameters: json!({"amount":2}),
                return_handle: false,
            },
        )
        .await;
        assert_eq!(result["value"]["value"], 2, "{result}");
        let explode = tools.iter().find(|tool| tool["name"] == "explode").unwrap()["toolId"]
            .as_str()
            .unwrap()
            .to_owned();
        let failed = settled(
            &session,
            session
                .submit(
                    Uuid::new_v4(),
                    Request::DeveloperToolCall {
                        registry,
                        tool_id: explode,
                        parameters: json!({}),
                        return_handle: false,
                    },
                    30000,
                )
                .unwrap(),
        )
        .await;
        assert_ne!(failed.outcome, Outcome::Succeeded, "{failed:?}");
        assert_eq!(
            failed.result.as_ref().unwrap()["potentiallyMutating"],
            true,
            "{failed:?}"
        );
        let calls = successful(
            &session,
            Request::Evaluate {
                scope: page,
                expression: "acmeToolCalls".into(),
                is_function: Some(false),
                argument: Default::default(),
                return_handle: false,
            },
        )
        .await;
        assert_eq!(calls["value"]["value"], 2);
        let object = successful(
            &session,
            Request::DeveloperToolCall {
                registry,
                tool_id: node,
                parameters: json!({}),
                return_handle: true,
            },
        )
        .await;
        let handle = serde_json::from_value(object["handle"].clone()).unwrap();
        for _ in 0..2 {
            let projected = successful(&session, Request::DeveloperToolResultRead { handle }).await;
            assert_eq!(projected["annotations"].as_array().unwrap().len(), 1);
            assert_eq!(projected["originalToolReplayed"], false);
            let locator =
                serde_json::from_value(projected["annotations"][0]["locator"].clone()).unwrap();
            let snapshot = successful(
                &session,
                Request::PageSnapshot {
                    page,
                    frame: None,
                    locator: Some(locator),
                },
            )
            .await;
            assert!(
                snapshot["value"].as_str().unwrap().contains("Acme"),
                "{snapshot}"
            );
        }
        assert_eq!(
            successful(
                &session,
                Request::Evaluate {
                    scope: page,
                    expression: "acmeNodeCalls".into(),
                    is_function: Some(false),
                    argument: Default::default(),
                    return_handle: false
                }
            )
            .await["value"]["value"],
            1
        );
        let tag = successful(
            &session,
            Request::HandleProperty {
                handle,
                name: "tagName".into(),
            },
        )
        .await;
        let tag = serde_json::from_value(tag["handle"].clone()).unwrap();
        assert_eq!(
            successful(&session, Request::HandleRead { handle: tag }).await["value"]["value"],
            "BUTTON"
        );
        successful(&session, Request::HandleDispose { handle: tag }).await;
        let element_tool = tools.iter().find(|tool| tool["name"] == "element").unwrap()["toolId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            successful(
                &session,
                Request::DeveloperToolCall {
                    registry,
                    tool_id: element_tool.clone(),
                    parameters: json!({"element":{"elementHandle":handle}}),
                    return_handle: false,
                }
            )
            .await["value"]["value"],
            "BUTTON"
        );
        let nested_tool=tools.iter().find(|tool|tool["name"]=="nested").unwrap()["toolId"].as_str().unwrap().to_owned();
        let nested=successful(&session,Request::DeveloperToolCall{registry,tool_id:nested_tool.clone(),parameters:json!({"payload":{"nodes":[{"elementHandle":handle},{"elementHandle":handle}]},"plain":{"elementHandle":handle}}),return_handle:false}).await;
        assert_eq!(nested["value"]["entries"][0]["value"]["items"][0]["value"],"BUTTON","{nested}");
        assert_eq!(nested["value"]["entries"][1]["value"]["value"],true,"{nested}");
        let other = successful(
            &session,
            Request::PageCreate {
                context,
                select: false,
            },
        )
        .await;
        let other = serde_json::from_value(other["page"].clone()).unwrap();
        let foreign = successful(
            &session,
            Request::Evaluate {
                scope: other,
                expression: "document.documentElement".into(),
                is_function: Some(false),
                argument: Default::default(),
                return_handle: true,
            },
        )
        .await;
        let refused = settled(
            &session,
            session
                .submit(
                    Uuid::new_v4(),
                    Request::DeveloperToolCall {
                        registry,
                        tool_id: element_tool.clone(),
                        parameters: json!({"element":{"elementHandle":foreign["handle"]}}),
                        return_handle: false,
                    },
                    30000,
                )
                .unwrap(),
        )
        .await;
        assert_eq!(
            refused.code.as_deref(),
            Some("FOREIGN_HANDLE"),
            "{refused:?}"
        );
        successful(&session, Request::PageClose { page: other }).await;
        successful(
            &session,
            Request::Evaluate {
                scope: page,
                expression: "document.querySelector('#probe').remove()".into(),
                is_function: Some(false),
                argument: Default::default(),
                return_handle: false,
            },
        )
        .await;
        let detached = settled(
            &session,
            session
                .submit(
                    Uuid::new_v4(),
                    Request::DeveloperToolCall {
                        registry,
                        tool_id: element_tool,
                        parameters: json!({"element":{"elementHandle":handle}}),
                        return_handle: false,
                    },
                    30000,
                )
                .unwrap(),
        )
        .await;
        let nested_detached=settled(&session,session.submit(Uuid::new_v4(),Request::DeveloperToolCall{registry,tool_id:nested_tool,parameters:json!({"payload":{"nodes":[{"elementHandle":handle}]},"plain":{}}),return_handle:false},30000).unwrap()).await;
        assert_eq!(nested_detached.code,detached.code,"{nested_detached:?}");
        let nested_calls=successful(&session,Request::Evaluate{scope:page,expression:"acmeNestedCalls".into(),is_function:Some(false),argument:Default::default(),return_handle:false}).await;
        assert_eq!(nested_calls["value"]["value"],1,"{nested_calls}");
        assert_eq!(
            detached.code.as_deref(),
            Some(if engine == Engine::Chrome {
                "JAVASCRIPT_EXCEPTION"
            } else {
                "BROWSER_METHOD_FAILED"
            }),
            "{detached:?}"
        );
        assert_eq!(
            successful(
                &session,
                Request::Evaluate {
                    scope: page,
                    expression: "acmeToolCalls".into(),
                    is_function: Some(false),
                    argument: Default::default(),
                    return_handle: false
                }
            )
            .await["value"]["value"],
            3
        );
        successful(&session, Request::HandleDispose { handle }).await;
        let dialog = tools.iter().find(|tool| tool["name"] == "dialog").unwrap()["toolId"]
            .as_str()
            .unwrap()
            .to_owned();
        let pending = session
            .submit(
                Uuid::new_v4(),
                Request::DeveloperToolCall {
                    registry,
                    tool_id: dialog,
                    parameters: json!({}),
                    return_handle: false,
                },
                30000,
            )
            .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        let callback = loop {
            let status = session.status();
            let callbacks = status["callbacks"]
                .as_array()
                .or_else(|| status["resources"].as_array())
                .unwrap();
            if let Some(dialog) = callbacks.iter().find(|item| item["kind"] == "dialog") {
                break serde_json::from_value(dialog["reference"].clone()).unwrap();
            }
            assert!(Instant::now() < until, "Tool dialog unavailable: {status}");
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        successful(
            &session,
            Request::CallbackResolve {
                callback,
                response: crate::browser::contract::CallbackResponse::Dialog {
                    accept: true,
                    prompt_text: None,
                },
            },
        )
        .await;
        let resolved = settled(&session, pending).await;
        assert_eq!(resolved.outcome, Outcome::Succeeded, "{resolved:?}");
        assert_eq!(resolved.result.unwrap()["value"]["value"], "Acme resolved");
        successful(&session, Request::DeveloperToolsRelease { registry }).await;
        successful(&session, Request::DeveloperToolsRelease { registry }).await;
        let discovered =
            successful(&session, Request::DeveloperToolsDiscover { scope: page }).await;
        let registry = serde_json::from_value(discovered["registry"].clone()).unwrap();
        let child_result = annotated_child_result(&session, page).await;
        successful(
            &session,
            Request::PageNavigate {
                page,
                navigation: Navigation::Goto {
                    url: format!("{}/audit", site.url),
                    wait_until: Load::Load,
                },
            },
        )
        .await;
        let stale = settled(
            &session,
            session
                .submit(
                    Uuid::new_v4(),
                    Request::DeveloperToolCall {
                        registry,
                        tool_id: increment,
                        parameters: json!({"amount":2}),
                        return_handle: false,
                    },
                    30000,
                )
                .unwrap(),
        )
        .await;
        assert_eq!(stale.code.as_deref(), Some("STALE_REFERENCE"), "{stale:?}");
        let stale_result = settled(
            &session,
            session
                .submit(
                    Uuid::new_v4(),
                    Request::DeveloperToolResultRead {
                        handle: child_result,
                    },
                    30000,
                )
                .unwrap(),
        )
        .await;
        assert_eq!(
            stale_result.code.as_deref(),
            Some("STALE_REFERENCE"),
            "{stale_result:?}"
        );
        successful(&session, Request::DeveloperToolsRelease { registry }).await;
    })
    .catch_unwind()
    .await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = exercise {
        std::panic::resume_unwind(panic);
    }
}
