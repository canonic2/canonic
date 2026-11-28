//! New target setup must progress while application code awaits its descendants.
use super::investigation::Site;
use super::*;
use crate::browser::{
    diagnostics::{DiagnosticKind, Query},
    evaluation::EvaluationValue,
};
use serde_json::json;

#[tokio::test]
async fn item_four_startup_chrome_headless() {
    for _ in 0..2 {
        run(Mode::Headless).await;
    }
}
#[tokio::test]
async fn item_four_startup_chrome_headed() {
    // Every fresh browser must survive asynchronous descendant teardown.
    for _ in 0..3 {
        run(Mode::Headed).await;
    }
}

async fn run(mode: Mode) {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let site = Site::start().await;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("state");
    crate::artifacts::directory(&directory).unwrap();
    let bundle = Bundle::Chrome(
        super::super::preparation::prepare_chrome(|_| {})
            .await
            .unwrap(),
    );
    let session = Session::launch_connection(
        bundle,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        LaunchOptions {
            media_fixtures: Default::default(),
            proxy: None,
            engine: Engine::Chrome,
            mode,
            context: Default::default(),
        },
        &directory,
    )
    .unwrap();
    let exercise=std::panic::AssertUnwindSafe(async {
        let ready=settled(&session,session.startup()).await;
        assert_eq!(ready.outcome,Outcome::Succeeded,"{ready:?}");let ready=ready.result.unwrap();
        let page=serde_json::from_value(ready["page"].clone()).unwrap();let context=serde_json::from_value(ready["context"].clone()).unwrap();
        successful(&session,Request::PageNavigate{page,navigation:Navigation::Goto{url:format!("{}/audit",site.url),wait_until:Load::Load}}).await;
        successful(&session,Request::DiagnosticsStart{context}).await;
        let result=successful(&session,Request::Evaluate{
            scope:page,is_function:Some(true),return_handle:false,
            argument:EvaluationValue::Json{value:json!(format!("{}/startup-frame",site.url.replace("127.0.0.1","localhost")))},
            expression:r#"async frameUrl=>{
              const received=label=>new Promise((resolve,reject)=>{
                const listener=event=>{if(event.data===label){clearTimeout(timer);removeEventListener('message',listener);resolve();}};
                const timer=setTimeout(()=>{removeEventListener('message',listener);reject(Error('Acme descendant did not start: '+label));},10000);
                addEventListener('message',listener);
              });
              const popupReady=received('Acme popup');const popup=open('/startup-popup');
              if(!popup)throw Error('Acme popup unavailable');await popupReady;
              const frameReady=received('Acme frame');const frame=document.createElement('iframe');frame.src=frameUrl;document.body.append(frame);await frameReady;
              const worker=new Worker('/startup-worker');await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('Acme worker did not start')),10000);worker.onmessage=event=>{if(event.data==='Acme worker'){clearTimeout(timer);resolve();}};});
              worker.terminate();frame.remove();popup.close();return 'Acme startup complete';
            }"#.into(),
        }).await;
        assert_eq!(result["value"]["value"],"Acme startup complete");
        let history=successful(&session,Request::DiagnosticsRead{context,query:Query{kind:Some(DiagnosticKind::Console),include_text:true,..Default::default()}}).await;
        for text in ["Acme startup popup","Acme startup frame","Acme startup worker"] {
            assert!(history["events"].as_array().unwrap().iter().any(|event|event["data"]["text"]==text),"Missing {text}: {history}");
        }
        let startup = session.status()["targetStartup"].clone();
        assert_eq!(startup["pausedTargets"],0,"{startup}");
        for (kind,minimum) in [("pages",2),("frames",1),("workers",1)] {
            assert!(startup["resumeAcknowledgements"][kind].as_u64().unwrap()>=minimum,"Missing native {kind} startup receipt: {startup}");
        }
    }).catch_unwind().await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = exercise {
        std::panic::resume_unwind(panic);
    }
}
