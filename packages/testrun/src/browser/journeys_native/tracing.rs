//! Real Playwright trace chunks, groups and private ZIP evidence in owned cells.
use super::*;
use crate::browser::tracing::Options;
use std::io::{Cursor, Read};
macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run(Engine::$engine, Mode::$mode).await;
        }
    };
}
cell!(item_four_trace_archive_firefox_headless, Firefox, Headless);
cell!(item_four_trace_archive_firefox_headed, Firefox, Headed);
cell!(item_four_trace_archive_webkit_headless, Webkit, Headless);
cell!(item_four_trace_archive_webkit_headed, Webkit, Headed);

async fn run(engine: Engine, mode: Mode) {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("state");
    crate::artifacts::directory(&directory).unwrap();
    let bundle = Bundle::Playwright(
        super::super::preparation::prepare(engine, |_| {})
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
            engine,
            mode,
            context: Default::default(),
        },
        &directory,
    )
    .unwrap();
    let exercise=std::panic::AssertUnwindSafe(async{
        let ready=settled(&session,session.startup()).await;assert_eq!(ready.outcome,Outcome::Succeeded,"{ready:?}");let ready=ready.result.unwrap();
        let context=serde_json::from_value(ready["context"].clone()).unwrap();let page=serde_json::from_value(ready["page"].clone()).unwrap();
        let started=successful(&session,Request::TraceStart{context,options:Options{snapshots:true.into(),screenshots:true,..Default::default()}}).await;assert_eq!(started["format"],"playwright-trace-zip");
        successful(&session,Request::TraceGroupStart{name:"Acme failing journey".into(),location:None}).await;
        successful(&session,Request::PageNavigate{page,navigation:Navigation::SetContent{html:"<title>Acme trace</title><button id=count onclick='this.textContent=\"Acme changed\"'>Acme</button>".into(),wait_until:Load::Load}}).await;
        successful(&session,action(page,Locator::Css{selector:"#count".into()},Action::Click{})).await;
        successful(&session,Request::TraceGroupEnd{}).await;
        let first=successful(&session,Request::TraceChunkStop{}).await;let report=&first["reports"][0];
        inspect(report,"Acme failing journey");
        successful(&session,Request::TraceChunkStart{title:"Acme second chunk".into(),name:None}).await;
        successful(&session,Request::Evaluate{scope:page,expression:"console.log('Acme private trace');42".into(),is_function:Some(false),argument:Default::default(),return_handle:false}).await;
        let stopped=successful(&session,Request::TraceStop { file_path: None, analysis_developer: None, analysis_page: None }).await;assert_eq!(stopped["active"],false);assert_eq!(stopped["reports"].as_array().unwrap().len(),2);
        inspect(&stopped["reports"][1],"Acme second chunk");
        assert_eq!(successful(&session,Request::TraceStop { file_path: None, analysis_developer: None, analysis_page: None }).await,stopped);
        let evidence=successful(&session,Request::DeveloperEvidenceList{}).await;assert_eq!(evidence["evidence"].as_array().unwrap().len(),2);
        for report in evidence["evidence"].as_array().unwrap(){successful(&session,Request::DeveloperEvidenceRemove{evidence_id:serde_json::from_value(report["evidenceId"].clone()).unwrap()}).await;}
        successful(&session,Request::TraceStart{context,options:Options{max_duration_ms:Some(1000),..Default::default()}}).await;
        let until=Instant::now()+Duration::from_secs(10);loop{let status=successful(&session,Request::TraceStatus{}).await;if status["active"]==false{assert_eq!(status["stopReason"],"duration-limit");assert_eq!(status["reports"].as_array().unwrap().len(),1);break;}assert!(Instant::now()<until,"{status}");tokio::time::sleep(Duration::from_millis(25)).await;}
    }).catch_unwind().await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = exercise {
        std::panic::resume_unwind(panic);
    }
}
fn inspect(report: &Value, expected: &str) {
    assert_eq!(report["format"], "playwright-trace-zip");
    assert_eq!(report["complete"], true);
    let bytes = std::fs::read(report["attachment"]["path"].as_str().unwrap()).unwrap();
    assert!(bytes.len() <= 32 * 1024 * 1024);
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut trace = String::new();
    archive
        .by_name("trace.trace")
        .unwrap()
        .read_to_string(&mut trace)
        .unwrap();
    assert!(
        trace.contains(expected),
        "Missing trace group/title {expected}"
    );
    assert!(
        trace
            .lines()
            .all(|line| serde_json::from_str::<Value>(line).is_ok())
    );
    assert!(archive.by_name("trace.network").is_ok());
}
