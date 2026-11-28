//! Explicit owned Chrome execution acceptance in both display modes.
use super::*;
use crate::browser::{
    evaluation::EvaluationValue,
    page::{Load, Navigation},
};

async fn send(session: &Session, request: Request) -> Value {
    let operation = settled(
        session,
        session.submit(Uuid::new_v4(), request, 30000).unwrap(),
    )
    .await;
    assert_eq!(
        operation.outcome,
        Outcome::Succeeded,
        "{operation:?}; status: {}",
        session.status()
    );
    operation.result.unwrap()
}
fn evaluate(scope: Reference, expression: &str, return_handle: bool) -> Request {
    Request::Evaluate {
        scope,
        expression: expression.into(),
        is_function: None,
        argument: EvaluationValue::default(),
        return_handle,
    }
}
fn reference(value: &Value, key: &str) -> Reference {
    serde_json::from_value(value[key].clone()).unwrap()
}

#[tokio::test]
async fn native_developer_frontend_owns_its_attachment_and_window() {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let bundle = crate::browser::preparation::prepare_chrome(|_| {})
        .await
        .unwrap();
    for mode in [Mode::Headless, Mode::Headed] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("state");
        crate::artifacts::directory(&directory).unwrap();
        let session = Session::launch_connection(
            bundle.clone(),
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
        let result = std::panic::AssertUnwindSafe(async {
            let ready = successful(&session, session.startup()).await;
            let page = reference(&ready, "page");
            let context = reference(&ready,"context");
            let opened = send(
                &session,
                Request::DeveloperOpen {
                    page,
                    panel: Some("lighthouse".into()),
                },
            )
            .await;
            assert_eq!(opened["models"]["nativeFrontend"], true, "{opened}");
            assert!(
                opened["models"]["modules"]["panels/lighthouse/lighthouse.js"].is_array(),
                "{opened}"
            );
            assert!(
                opened["models"]["modules"]["models/trace/trace.js"].is_array(),
                "{opened}"
            );
            let developer = reference(&opened, "developer");
            send(&session, Request::DeveloperClose { developer }).await;
            send(&session,Request::TraceStart{context,options:crate::browser::tracing::Options{screenshots:false,cpu_sampling:true,..Default::default()}}).await;
            send(&session,Request::ProfileStart{page,options:crate::browser::profiling::Options{javascript_coverage:true,css_coverage:true,..Default::default()}}).await;
            send(&session,evaluate(page,"(()=>{performance.mark('Acme-start');document.head.insertAdjacentHTML('beforeend','<style>body{color:rgb(1,2,3)}.unused{color:red}</style>');globalThis.acmeBurn=function acmeBurn(){let total=0;for(let i=0;i<2000000;i++)total+=Math.sqrt(i);return total};acmeBurn();performance.measure('Acme-investigation','Acme-start');return getComputedStyle(document.body).color})()",false)).await;
            assert_eq!(send(&session,Request::ProfileStatus{page}).await["state"],"recording");
            let profile=send(&session,Request::ProfileStop{page}).await;
            let reports=profile["reports"].as_array().unwrap();
            assert_eq!(reports.len(),3,"{profile}");
            for report in reports{
                let value:Value=serde_json::from_slice(&std::fs::read(report["attachment"]["path"].as_str().unwrap()).unwrap()).unwrap();
                match report["format"].as_str().unwrap(){
                    "v8-cpu-profile"=>{assert!(!value["nodes"].as_array().unwrap().is_empty());assert!(value["endTime"].as_f64().unwrap()>value["startTime"].as_f64().unwrap());},
                    "v8-precise-javascript-coverage"=>assert!(value["result"].as_array().unwrap().iter().any(|script|script["functions"].as_array().unwrap().iter().any(|function|function["functionName"]=="acmeBurn"))),
                    "cdp-css-rule-usage"=>assert!(value["ruleUsage"].as_array().unwrap().iter().any(|rule|rule["used"]==true)),
                    format=>panic!("Unexpected profile format {format}")
                }
                send(&session,Request::DeveloperEvidenceRemove{evidence_id:serde_json::from_value(report["evidenceId"].clone()).unwrap()}).await;
            }
            let trace=send(&session,Request::TraceStop { file_path: None, analysis_developer: None, analysis_page: None }).await;
            let bytes=std::fs::read(trace["report"]["attachment"]["path"].as_str().unwrap()).unwrap();
            let data:Value=serde_json::from_slice(&bytes).unwrap();
            assert!(data["traceEvents"].as_array().unwrap().iter().any(|event|event["name"]=="Acme-investigation"));
            assert_eq!(trace["report"]["complete"],true);
            assert_eq!(trace["report"]["playwrightTraceViewer"],false);
            assert_eq!(send(&session,Request::TraceStatus{}).await["state"],"complete");
            use crate::browser::debugging::{Action as Debug,Exceptions};
            send(&session,Request::PageDebugger{page,action:Debug::Enable{exceptions:Exceptions::None}}).await;
            let running=session.submit(Uuid::new_v4(),evaluate(page,"(()=>{const secret=41;debugger;return secret+1})()",false),30000).unwrap();
            let deadline=Instant::now()+Duration::from_secs(10);
            let pause=loop{
                let status=session.status();
                if let Some(pause)=status["debuggers"].as_array().unwrap().iter().find_map(|d|d["pauseId"].as_str()){break Uuid::parse_str(pause).unwrap();}
                assert!(Instant::now()<deadline,"Debugger pause was not observed");tokio::time::sleep(Duration::from_millis(25)).await;
            };
            assert_eq!(send(&session,Request::PageDebugger{page,action:Debug::Status{include_frames:true}}).await["paused"],true);
            let local=send(&session,Request::PageDebugger{page,action:Debug::Evaluate{pause_id:pause,frame:0,expression:"secret+1".into()}}).await;
            assert_eq!(local["value"]["value"],42,"{local}");
            send(&session,Request::PageDebugger{page,action:Debug::Resume{pause_id:pause}}).await;
            assert_eq!(successful(&session,running).await["value"]["value"],42);
            let stale=settled(&session,session.submit(Uuid::new_v4(),Request::PageDebugger{page,action:Debug::Resume{pause_id:pause}},30000).unwrap()).await;
            assert_eq!(stale.code.as_deref(),Some("STALE_DEBUGGER_PAUSE"));
            send(&session,Request::PageDebugger{page,action:Debug::Disable}).await;
            send(&session,Request::DeveloperEvidenceRemove{evidence_id:serde_json::from_value(trace["report"]["evidenceId"].clone()).unwrap()}).await;
            assert_eq!(session.status()["phase"], "ready");
        })
        .catch_unwind()
        .await;
        successful(&session, session.disconnect(30000).unwrap()).await;
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }
}

#[tokio::test]
async fn native_main_world_evaluation_handles_and_workers_preserve_lifetimes() {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let bundle = crate::browser::preparation::prepare_chrome(|_| {})
        .await
        .unwrap();
    for mode in [Mode::Headless, Mode::Headed] {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("state");
        crate::artifacts::directory(&private).unwrap();
        let session = Session::launch_connection(
            bundle.clone(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            LaunchOptions {
                media_fixtures: Default::default(),
                proxy: None,
                engine: Engine::Chrome,
                mode,
                context: ContextOptions::default(),
            },
            &private,
        )
        .unwrap();
        let exercise=std::panic::AssertUnwindSafe(async{
            let ready=successful(&session,session.startup()).await;
            let page=reference(&ready,"page");
            send(&session,Request::PageNavigate{page,navigation:Navigation::SetContent{html:"<script>globalThis.Acme=17;globalThis.acmeWorker=new Worker(URL.createObjectURL(new Blob(['self.onmessage=()=>{}'],{type:'application/javascript'})));</script>".into(),wait_until:Load::Load}}).await;
            assert_eq!(send(&session,evaluate(page,"Acme",false)).await["value"],json!({"kind":"json","value":17}));
            let graph=send(&session,evaluate(page,"()=>{const x={value:-0,big:12345678901234567890n};x.self=x;return x}",false)).await;
            assert_eq!(graph["value"]["entries"][0]["value"],json!({"kind":"number","value":"negative_zero"}));
            assert_eq!(graph["value"]["entries"][2]["value"]["kind"],"reference");
            let parent=reference(&send(&session,evaluate(page,"({child:{name:'Acme'},count:1})",true)).await,"handle");
            let child=reference(&send(&session,Request::HandleProperty{handle:parent,name:"child".into()}).await,"handle");
            let keys=send(&session,Request::HandleProperties{handle:parent,query:Default::default()}).await;
            assert_eq!(keys["properties"]["names"],json!(["child","count"]));
            send(&session,Request::HandleDispose{handle:parent}).await;
            assert_eq!(send(&session,Request::HandleRead{handle:child}).await["value"]["entries"][0]["value"]["value"],"Acme");
            let arg=Request::Evaluate{scope:page,expression:"value=>value.name".into(),is_function:Some(true),argument:EvaluationValue::Handle{handle:child},return_handle:false};
            assert_eq!(send(&session,arg).await["value"]["value"],"Acme");
            let deadline=Instant::now()+Duration::from_secs(10);
            let worker=loop{
                let workers=send(&session,Request::WorkerList{page}).await;
                if let Some(worker)=workers["workers"].as_array().unwrap().first(){break reference(worker,"worker");}
                assert!(Instant::now()<deadline,"Dedicated worker not observed");
                tokio::time::sleep(Duration::from_millis(25)).await;
            };
            assert_eq!(send(&session,evaluate(worker,"()=>6*7",false)).await["value"]["value"],42);
            let exception=settled(&session,session.submit(Uuid::new_v4(),evaluate(page,"(()=>{throw Error('Acme exception')})()",false),30000).unwrap()).await;
            assert_eq!(exception.code.as_deref(),Some("JAVASCRIPT_EXCEPTION"));
            assert_eq!(exception.result.unwrap()["dispatchCompleted"],true);
            send(&session,Request::PageNavigate{page,navigation:Navigation::SetContent{html:"<p>Acme replacement</p>".into(),wait_until:Load::Load}}).await;
            let stale=settled(&session,session.submit(Uuid::new_v4(),Request::HandleRead{handle:child},30000).unwrap()).await;
            assert_eq!(stale.code.as_deref(),Some("STALE_REFERENCE"));
        }).catch_unwind().await;
        successful(&session, session.disconnect(30000).unwrap()).await;
        if let Err(panic) = exercise {
            std::panic::resume_unwind(panic);
        }
    }
}
