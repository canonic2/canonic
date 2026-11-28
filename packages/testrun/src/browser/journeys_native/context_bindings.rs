//! Scoped startup bindings, callback responsiveness and independent removal.
use super::investigation::Site;
use super::*;
use crate::browser::{contract::CallbackResponse, evaluation::EvaluationValue};
use serde_json::json;

pub(super) const HTML: &str = r#"<!doctype html><title>Acme context bindings</title><script>
const before={context:typeof globalThis.AcmeContextBridge,page:typeof globalThis.AcmePageBridge,init:globalThis.AcmeBindingAtInit??null};
const publish=value=>{globalThis.acmeBindingReceipt={...before,value};if(parent!==self)parent.postMessage({kind:'Acme binding',receipt:acmeBindingReceipt},'*');else if(opener)opener.postMessage({kind:'Acme binding',receipt:acmeBindingReceipt},'*');return value;};
globalThis.acmeBindingReady=typeof AcmeContextBridge==='function'?AcmeContextBridge('Acme private startup',parent!==self,!!opener).then(publish):Promise.resolve(publish(null));
</script><p>Acme context bindings</p>"#;

macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run(Engine::$engine, Mode::$mode).await;
        }
    };
}
cell!(item_four_context_bindings_chrome_headless, Chrome, Headless);
cell!(item_four_context_bindings_chrome_headed, Chrome, Headed);
cell!(
    item_four_context_bindings_firefox_headless,
    Firefox,
    Headless
);
cell!(item_four_context_bindings_firefox_headed, Firefox, Headed);
cell!(item_four_context_bindings_webkit_headless, Webkit, Headless);
cell!(item_four_context_bindings_webkit_headed, Webkit, Headed);

fn reference(value: &Value, key: &str) -> Reference {
    serde_json::from_value(value[key].clone()).unwrap()
}
fn evaluate(scope: Reference, expression: &str, argument: Value) -> Request {
    Request::Evaluate {
        scope,
        expression: expression.into(),
        is_function: Some(true),
        argument: EvaluationValue::Json { value: argument },
        return_handle: false,
    }
}
async fn pending_binding(session: &Session) -> Reference {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let status = session.status();
        let callbacks = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        if let Some(callback) = callbacks
            .iter()
            .find(|callback| callback["kind"] == "binding")
        {
            assert!(!callback.to_string().contains("Acme private"));
            return reference(callback, "reference");
        }
        assert!(
            Instant::now() < until,
            "Binding callback unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn reply(session: &Session, binding: Uuid, payload: &str, scope: Option<(bool, bool)>) {
    let callback = pending_binding(session).await;
    let args = successful(session, Request::BindingRead { callback }).await;
    assert_eq!(args["bindingId"], json!(binding));
    assert_eq!(args["arguments"][0]["value"], payload);
    if let Some((frame, popup)) = scope {
        assert_eq!(args["arguments"][1]["value"], frame);
        assert_eq!(args["arguments"][2]["value"], popup);
    }
    successful(
        session,
        Request::CallbackResolve {
            callback,
            response: CallbackResponse::BindingResolve { value: json!(42) },
        },
    )
    .await;
}
async fn navigate(session: &Session, page: Reference, url: &str) {
    successful(
        session,
        Request::PageNavigate {
            page,
            navigation: Navigation::Goto {
                url: format!("{url}/context-binding-app"),
                wait_until: Load::Load,
            },
        },
    )
    .await;
}
async fn receipt(session: &Session, page: Reference) -> Value {
    let value = successful(
        session,
        evaluate(
            page,
            "async()=>{await acmeBindingReady;return JSON.stringify(acmeBindingReceipt)}",
            Value::Null,
        ),
    )
    .await;
    serde_json::from_str(value["value"]["value"].as_str().unwrap()).unwrap()
}
fn expected(page: bool) -> Value {
    json!({"context":"function","page":if page {"function"}else{"undefined"},"init":"function","value":42})
}

#[expect(
    clippy::too_many_lines,
    reason = "One owned six-cell journey correlates context/page registrations, descendant callbacks and their teardown"
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
        let ready = settled(&session,session.startup()).await;
        assert_eq!(ready.outcome,Outcome::Succeeded,"{ready:?}");
        let ready = ready.result.unwrap();
        let context = reference(&ready,"context");
        let page = reference(&ready,"page");
        let script = successful(&session,Request::ContextInitScriptAdd { context, source:"globalThis.AcmeBindingAtInit=typeof globalThis.AcmeContextBridge".into() }).await;
        let script_id = serde_json::from_value(script["scriptId"].clone()).unwrap();
        let binding = successful(&session,Request::BindingAdd { scope: context, name:"AcmeContextBridge".into() }).await;
        let binding = serde_json::from_value(binding["bindingId"].clone()).unwrap();
        let page_binding = successful(&session,Request::BindingAdd { scope: page, name:"AcmePageBridge".into() }).await;
        let page_binding = serde_json::from_value(page_binding["bindingId"].clone()).unwrap();
        navigate(&session,page,&site.url).await;
        let callback = pending_binding(&session).await;
        let removing = settled(&session,session.submit(Uuid::new_v4(),Request::BindingRemove { scope: context, binding_id: binding },30000).unwrap()).await;
        assert_eq!(removing.code.as_deref(),Some("BINDING_CALLS_PENDING"));
        let args = successful(&session,Request::BindingRead { callback }).await;
        assert_eq!(args["bindingId"],json!(binding));
        assert_eq!(args["arguments"][0]["value"],"Acme private startup");
        successful(&session,Request::CallbackResolve { callback, response:CallbackResponse::BindingResolve { value:json!(42) } }).await;
        assert_eq!(receipt(&session,page).await,expected(true));
        navigate(&session,page,&site.url).await;
        reply(&session,binding,"Acme private startup",Some((false,false))).await;
        assert_eq!(receipt(&session,page).await,expected(true));
        let call = session.submit(Uuid::new_v4(),evaluate(page,"()=>AcmePageBridge('Acme private page')",Value::Null),30000).unwrap();
        reply(&session,page_binding,"Acme private page",None).await;
        let result = settled(&session,call).await;
        assert_eq!(result.outcome,Outcome::Succeeded,"{result:?}");
        assert_eq!(result.result.unwrap()["value"]["value"],42);
        let frame = session.submit(Uuid::new_v4(),evaluate(page,r#"async url=>new Promise((resolve,reject)=>{
            const frame=document.createElement('iframe');const listener=event=>{if(event.source===frame.contentWindow&&event.data?.kind==='Acme binding'){clearTimeout(timer);removeEventListener('message',listener);frame.remove();resolve(JSON.stringify(event.data.receipt));}};
            const timer=setTimeout(()=>{removeEventListener('message',listener);frame.remove();reject(Error('Acme frame binding unavailable'));},10000);addEventListener('message',listener);frame.src=url;document.body.append(frame);
        })"#,json!(format!("{}/context-binding-app",site.url.replace("127.0.0.1","localhost")))),30000).unwrap();
        reply(&session,binding,"Acme private startup",Some((true,false))).await;
        let frame = settled(&session,frame).await;
        assert_eq!(frame.outcome,Outcome::Succeeded,"{frame:?}");
        assert_eq!(serde_json::from_str::<Value>(frame.result.unwrap()["value"]["value"].as_str().unwrap()).unwrap(),expected(true));
        let popup = session.submit(Uuid::new_v4(),evaluate(page,r#"async url=>new Promise((resolve,reject)=>{
            let popup;const listener=event=>{if(event.source===popup&&event.data?.kind==='Acme binding'){clearTimeout(timer);removeEventListener('message',listener);popup.close();resolve(JSON.stringify(event.data.receipt));}};
            const timer=setTimeout(()=>{removeEventListener('message',listener);popup?.close();reject(Error('Acme popup binding unavailable'));},10000);addEventListener('message',listener);popup=open(url);if(!popup){clearTimeout(timer);removeEventListener('message',listener);reject(Error('Acme popup refused'));}
        })"#,json!(format!("{}/context-binding-app",site.url))),30000).unwrap();
        reply(&session,binding,"Acme private startup",Some((false,true))).await;
        let popup = settled(&session,popup).await;
        assert_eq!(popup.outcome,Outcome::Succeeded,"{popup:?}");
        assert_eq!(serde_json::from_str::<Value>(popup.result.unwrap()["value"]["value"].as_str().unwrap()).unwrap(),expected(false));
        let sibling = successful(&session,Request::PageCreate { context, select:false }).await;
        let sibling = reference(&sibling,"page");
        navigate(&session,sibling,&site.url).await;
        reply(&session,binding,"Acme private startup",Some((false,false))).await;
        assert_eq!(receipt(&session,sibling).await,expected(false));
        let other = successful(&session,Request::ContextCreate { options:Default::default() }).await;
        let other = reference(&other,"context");
        let other_page = successful(&session,Request::PageCreate { context:other, select:false }).await;
        let other_page = reference(&other_page,"page");
        navigate(&session,other_page,&site.url).await;
        assert_eq!(receipt(&session,other_page).await,json!({"context":"undefined","page":"undefined","init":null,"value":null}));
        successful(&session,Request::ContextClose { context:other }).await;
        successful(&session,Request::ContextInitScriptRemove { context, script_id }).await;
        successful(&session,Request::BindingRemove { scope:context, binding_id:binding }).await;
        assert!(successful(&session,Request::BindingList { scope:context }).await["bindings"].as_array().unwrap().is_empty());
        assert_eq!(successful(&session,evaluate(page,"()=>typeof AcmeContextBridge",Value::Null)).await["value"]["value"],"undefined");
        assert_eq!(successful(&session,evaluate(page,"()=>typeof AcmePageBridge",Value::Null)).await["value"]["value"],"function");
        assert_eq!(successful(&session,evaluate(sibling,"()=>typeof AcmeContextBridge",Value::Null)).await["value"]["value"],"undefined");
        successful(&session,Request::BindingRemove { scope:page, binding_id:page_binding }).await;
        navigate(&session,page,&site.url).await;
        assert_eq!(receipt(&session,page).await,json!({"context":"undefined","page":"undefined","init":null,"value":null}));
    }).catch_unwind().await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = exercise {
        if let Session::Playwright(driver) = &session {
            eprintln!(
                "Context binding private failures: {:?}",
                driver.private_failures()
            );
        }
        std::panic::resume_unwind(panic);
    }
}
