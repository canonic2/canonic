//! Context/page initialization, future descendants and immutable existing effects.
use super::investigation::Site;
use super::*;
use crate::browser::diagnostics::{DiagnosticKind, Query};
use crate::browser::evaluation::EvaluationValue;
use serde_json::json;

pub(super) const HTML: &str = r#"<!doctype html><title>Acme initialization</title><script>
globalThis.acmeInitObserved={context:globalThis.AcmeContextInit??null,page:globalThis.AcmePageInit??null,contextRuns:globalThis.AcmeContextRuns??0,pageRuns:globalThis.AcmePageRuns??0,hook:!!globalThis.__REACT_DEVTOOLS_GLOBAL_HOOK__};
const receipt={acmeInit:true,observed:acmeInitObserved};
if(parent!==self)parent.postMessage(receipt,'*');else if(opener)opener.postMessage(receipt,'*');
</script><p>Acme initialization</p>"#;
macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run(Engine::$engine, Mode::$mode).await;
        }
    };
}
cell!(item_four_context_scripts_chrome_headless, Chrome, Headless);
cell!(item_four_context_scripts_chrome_headed, Chrome, Headed);
cell!(
    item_four_context_scripts_firefox_headless,
    Firefox,
    Headless
);
cell!(item_four_context_scripts_firefox_headed, Firefox, Headed);
cell!(item_four_context_scripts_webkit_headless, Webkit, Headless);
cell!(item_four_context_scripts_webkit_headed, Webkit, Headed);

async fn evaluate(session: &Session, page: Reference, expression: &str, argument: Value) -> Value {
    successful(
        session,
        Request::Evaluate {
            scope: page,
            expression: expression.into(),
            is_function: Some(true),
            argument: EvaluationValue::Json { value: argument },
            return_handle: false,
        },
    )
    .await["value"]["value"]
        .clone()
}
async fn observed(session: &Session, page: Reference) -> Value {
    serde_json::from_str(
        evaluate(
            session,
            page,
            "()=>JSON.stringify(globalThis.acmeInitObserved)",
            Value::Null,
        )
        .await
        .as_str()
        .unwrap(),
    )
    .unwrap()
}
async fn navigate(session: &Session, page: Reference, url: String) {
    successful(
        session,
        Request::PageNavigate {
            page,
            navigation: Navigation::Goto {
                url,
                wait_until: Load::Load,
            },
        },
    )
    .await;
}

#[expect(
    clippy::too_many_lines,
    reason = "One owned workflow correlates future documents, descendants and removal across the same script owners"
)]
async fn run(engine: Engine, mode: Mode) {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    std::env::var_os("TESTRUN_FRAMEWORK_FIXTURE")
        .expect("Select the pinned private React fixture directory");
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
    let exercise=std::panic::AssertUnwindSafe(async {
        let ready=settled(&session,session.startup()).await;assert_eq!(ready.outcome,Outcome::Succeeded,"{ready:?}");let ready=ready.result.unwrap();
        let context=serde_json::from_value(ready["context"].clone()).unwrap();let page=serde_json::from_value(ready["page"].clone()).unwrap();
        successful(&session,Request::DiagnosticsStart{context}).await;
        let registered=successful(&session,Request::ContextInitScriptAdd{context,source:"globalThis.AcmeContextInit='Acme context';globalThis.AcmeContextRuns=(globalThis.AcmeContextRuns??0)+1;console.log('Acme context initialization:'+location.hostname+':'+(parent!==self))".into()}).await;
        let context_script=serde_json::from_value(registered["scriptId"].clone()).unwrap();
        let prepared=successful(&session,Request::ContextFrameworkPrepare{context}).await;
        let hook_script=serde_json::from_value(prepared["scriptId"].clone()).unwrap();
        let registered=successful(&session,Request::PageInitScriptAdd{page,source:"globalThis.AcmePageInit='Acme page';globalThis.AcmePageRuns=(globalThis.AcmePageRuns??0)+1".into()}).await;
        let page_script=serde_json::from_value(registered["scriptId"].clone()).unwrap();
        assert_eq!(evaluate(&session,page,"()=>JSON.stringify([typeof AcmeContextInit,typeof AcmePageInit,typeof __REACT_DEVTOOLS_GLOBAL_HOOK__])",Value::Null).await,json!("[\"undefined\",\"undefined\",\"undefined\"]"));
        assert_eq!(successful(&session,Request::ContextInitScriptList{context}).await["scripts"].as_array().unwrap().len(),2);
        navigate(&session,page,format!("{}/init-script-app",site.url)).await;
        assert_eq!(observed(&session,page).await,json!({"context":"Acme context","page":"Acme page","contextRuns":1,"pageRuns":1,"hook":true}));
        let frame=evaluate(&session,page,r#"async url=>new Promise((resolve,reject)=>{
          const frame=document.createElement('iframe');
          const listener=event=>{if(event.source===frame.contentWindow&&event.data?.acmeInit){clearTimeout(timer);removeEventListener('message',listener);globalThis.acmeInitFrame=frame;resolve(JSON.stringify(event.data.observed));}};
          const timer=setTimeout(()=>{removeEventListener('message',listener);frame.remove();reject(Error('Acme initialized frame unavailable'));},10000);
          addEventListener('message',listener);frame.src=url;document.body.append(frame);
        })"#,json!(format!("{}/init-script-app",site.url.replace("127.0.0.1","localhost")))).await;
        assert_eq!(serde_json::from_str::<Value>(frame.as_str().unwrap()).unwrap(),json!({"context":"Acme context","page":"Acme page","contextRuns":1,"pageRuns":1,"hook":true}));
        if engine==Engine::Chrome {assert!(session.status()["targetStartup"]["resumeAcknowledgements"]["frames"].as_u64().unwrap()>=1);}
        let until=Instant::now()+Duration::from_secs(5);
        let frame_console=loop {
            let history=successful(&session,Request::DiagnosticsRead{context,query:Query{kind:Some(DiagnosticKind::Console),include_text:true,limit:100,..Default::default()}}).await;
            if let Some(event)=history["events"].as_array().unwrap().iter().find(|event|event["data"]["text"]=="Acme context initialization:localhost:true") {break event.clone();}
            assert!(Instant::now()<until,"Missing frame initialization diagnostics: {history}");
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        if engine==Engine::Chrome {
            assert_eq!(frame_console["page"],json!(page));
            assert_eq!(frame_console["data"]["subscriptionBoundary"],if frame_console["data"]["attachmentSetupPending"]==true {json!("owned-target-startup-pending")}else{Value::Null});
        }
        evaluate(&session,page,"()=>{acmeInitFrame.remove();delete globalThis.acmeInitFrame;return true;}",Value::Null).await;
        let popup=evaluate(&session,page,r#"async url=>new Promise((resolve,reject)=>{
          let popup;const listener=event=>{if(event.source===popup&&event.data?.acmeInit){clearTimeout(timer);removeEventListener('message',listener);popup.close();resolve(JSON.stringify(event.data.observed));}};
          const timer=setTimeout(()=>{removeEventListener('message',listener);popup?.close();reject(Error('Acme initialized popup unavailable'));},10000);
          addEventListener('message',listener);popup=open(url);
          if(!popup){clearTimeout(timer);removeEventListener('message',listener);reject(Error('Acme popup refused'));}
        })"#,json!(format!("{}/init-script-app",site.url))).await;
        // The pinned WebKit adapter explicitly evaluates its bootstrap during
        // popup adoption as well as installing it for the initial navigation.
        // Its retained popup global observes both runs; the application still
        // sees the context source before it starts. Document expandos have
        // different initialization timing on Chrome/Firefox and cannot serve
        // as a common execution counter.
        let popup_runs=if engine==Engine::Webkit{2}else{1};
        assert_eq!(serde_json::from_str::<Value>(popup.as_str().unwrap()).unwrap(),json!({"context":"Acme context","page":null,"contextRuns":popup_runs,"pageRuns":0,"hook":true}));
        let sibling=successful(&session,Request::PageCreate{context,select:false}).await;
        let sibling=serde_json::from_value(sibling["page"].clone()).unwrap();
        navigate(&session,sibling,format!("{}/init-script-app",site.url)).await;
        assert_eq!(observed(&session,sibling).await,json!({"context":"Acme context","page":null,"contextRuns":1,"pageRuns":0,"hook":true}));
        let other=successful(&session,Request::ContextCreate{options:Default::default()}).await;
        let other=serde_json::from_value(other["context"].clone()).unwrap();
        let other_page=successful(&session,Request::PageCreate{context:other,select:false}).await;
        let other_page=serde_json::from_value(other_page["page"].clone()).unwrap();
        navigate(&session,other_page,format!("{}/init-script-app",site.url)).await;
        assert_eq!(observed(&session,other_page).await,json!({"context":null,"page":null,"contextRuns":0,"pageRuns":0,"hook":false}));
        successful(&session,Request::ContextClose{context:other}).await;
        navigate(&session,sibling,format!("{}/framework",site.url)).await;
        assert_eq!(evaluate(&session,sibling,"async()=>{const until=Date.now()+10000;while(!document.querySelector('#count')){if(Date.now()>until)throw Error('Acme React did not mount');await new Promise(resolve=>setTimeout(resolve,25));}return true;}",Value::Null).await,true);
        let opened=successful(&session,Request::FrameworkOpen{scope:sibling}).await;
        let registry=serde_json::from_value(opened["registry"].clone()).unwrap();
        let tree=successful(&session,Request::FrameworkQuery{registry,query:serde_json::from_value(json!({"kind":"tree","limit":100})).unwrap()}).await;
        assert!(!tree["report"]["items"].as_array().unwrap().is_empty(),"{tree}");
        successful(&session,Request::FrameworkRelease{registry}).await;
        successful(&session,Request::ContextInitScriptRemove{context,script_id:hook_script}).await;
        successful(&session,Request::ContextInitScriptRemove{context,script_id:context_script}).await;
        assert_eq!(evaluate(&session,sibling,"()=>AcmeContextInit",Value::Null).await,"Acme context");
        navigate(&session,page,format!("{}/init-script-app",site.url)).await;
        assert_eq!(observed(&session,page).await,json!({"context":null,"page":"Acme page","contextRuns":0,"pageRuns":1,"hook":false}));
        successful(&session,Request::PageInitScriptRemove{page,script_id:page_script}).await;
        navigate(&session,page,format!("{}/init-script-app",site.url)).await;
        assert_eq!(observed(&session,page).await,json!({"context":null,"page":null,"contextRuns":0,"pageRuns":0,"hook":false}));
        assert!(successful(&session,Request::ContextInitScriptList{context}).await["scripts"].as_array().unwrap().is_empty());
        assert!(successful(&session,Request::PageInitScriptList{page}).await["scripts"].as_array().unwrap().is_empty());
    }).catch_unwind().await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = exercise {
        if let Session::Playwright(driver) = &session {
            eprintln!(
                "Context script private failures: {:?}",
                driver.private_failures()
            );
        }
        std::panic::resume_unwind(panic);
    }
}
