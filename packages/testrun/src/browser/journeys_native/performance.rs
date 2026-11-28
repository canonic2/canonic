//! Native analysis of a recorded navigation and trusted interaction in both Chrome modes.
use super::investigation::Site;
use super::*;
use crate::browser::{performance::Query, tracing::Options};
use serde_json::json;
#[tokio::test]
async fn item_four_performance_chrome_headless() {
    run(Mode::Headless).await;
}
#[tokio::test]
async fn item_four_performance_chrome_headed() {
    run(Mode::Headed).await;
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
    let result=std::panic::AssertUnwindSafe(async{
        let ready=settled(&session,session.startup()).await;assert_eq!(ready.outcome,Outcome::Succeeded,"{ready:?}");let ready=ready.result.unwrap();
        let page=serde_json::from_value(ready["page"].clone()).unwrap();let context=serde_json::from_value(ready["context"].clone()).unwrap();
        successful(&session,Request::PageSelect{page}).await;
        successful(&session,Request::TraceStart{context,options:Options{screenshots:false,cpu_sampling:false,..Default::default()}}).await;
        successful(&session,Request::PageNavigate{page,navigation:Navigation::Goto{url:format!("{}/performance",site.url),wait_until:Load::Load}}).await;
        successful(&session,Request::Evaluate{scope:page,expression:"()=>new Promise(resolve=>{const finish=()=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(true)));if(performance.getEntriesByType('paint').some(entry=>entry.name==='first-contentful-paint'))return finish();const observer=new PerformanceObserver(list=>{if(list.getEntries().some(entry=>entry.name==='first-contentful-paint')){observer.disconnect();finish();}});observer.observe({type:'paint',buffered:true});})".into(),is_function:Some(true),argument:Default::default(),return_handle:false}).await;
        successful(&session,action(page,Locator::Css{selector:"#work".into()},Action::Click{})).await;
        successful(&session,Request::Evaluate{scope:page,expression:"()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(true))))".into(),is_function:Some(true),argument:Default::default(),return_handle:false}).await;
        let trace=successful(&session,Request::TraceStop { file_path: None, analysis_developer: None, analysis_page: None }).await;let report=&trace["report"];assert_eq!(report["complete"],true);
        let evidence_id=serde_json::from_value(report["evidenceId"].clone()).unwrap();
        let opened=successful(&session,Request::DeveloperOpen{page,panel:Some("timeline".into())}).await;let developer=serde_json::from_value(opened["developer"].clone()).unwrap();
        let loaded=successful(&session,Request::PerformanceLoad{developer,evidence_id}).await;
        assert!(loaded["summary"]["data"]["eventCount"].as_u64().unwrap()>0,"{loaded}");
        let sets=loaded["summary"]["data"]["insightSets"].as_array().unwrap();
        let set=sets.iter().find(|set|set["url"]==format!("{}/performance",site.url)&&set["navigationObserved"]==true).expect("Recorded navigation insight set");
        let insight_set_id=set["insightSetId"].as_str().unwrap().to_owned();let analysis_id=serde_json::from_value(loaded["analysisId"].clone()).unwrap();
        let vitals=successful(&session,Request::PerformanceQuery{developer,analysis_id,query:Query::Vitals{insight_set_id:insight_set_id.clone()}}).await;
        let metrics=&vitals["result"]["data"];assert_eq!(metrics["fieldData"],false);assert_eq!(metrics["pageLifetime"],false);
        assert_eq!(metrics["LCP"]["available"],true,"{vitals}");assert!(metrics["LCP"]["value"].as_f64().unwrap()>0.0,"{vitals}");
        assert_eq!(metrics["INP"]["available"],true,"{vitals}");assert!(metrics["INP"]["value"].as_f64().unwrap()>0.0,"{vitals}");
        let timings=successful(&session,Request::PerformanceQuery{developer,analysis_id,query:Query::Timings{offset:0,limit:100}}).await;
        assert!(timings["result"]["data"]["items"].as_array().unwrap().iter().any(|metric|metric["metricName"]=="FCP"&&metric["value"].as_f64().is_some_and(|value|value>0.0)),"{timings}");
        let insights=successful(&session,Request::PerformanceQuery{developer,analysis_id,query:Query::Insights{insight_set_id:insight_set_id.clone(),insight:None,offset:0,limit:100}}).await;
        let first=insights["result"]["data"]["items"].as_array().unwrap().first().unwrap()["name"].as_str().unwrap().to_owned();
        let detail=successful(&session,Request::PerformanceQuery{developer,analysis_id,query:Query::Insights{insight_set_id,insight:Some(first),offset:0,limit:10}}).await;assert!(!detail["result"]["data"]["insight"].is_null());
        successful(&session,Request::PerformanceRelease{developer,analysis_id}).await;
        let stale=settled(&session,session.submit(Uuid::new_v4(),Request::PerformanceQuery{developer,analysis_id,query:Query::Summary{}},30000).unwrap()).await;assert_eq!(stale.code.as_deref(),Some("UNKNOWN_PERFORMANCE_ANALYSIS"));
        successful(&session,Request::PerformanceLoad{developer,evidence_id}).await;
        successful(&session,Request::DeveloperClose{developer}).await;assert!(session.status()["performance"]["analysis"].is_null());
        let evidence=successful(&session,Request::DeveloperEvidenceList{}).await;assert!(evidence["evidence"].as_array().unwrap().iter().any(|item|item["evidenceId"]==json!(evidence_id)));
        assert_eq!(successful(&session,Request::Evaluate{scope:page,expression:"document.title".into(),is_function:Some(false),argument:Default::default(),return_handle:false}).await["value"]["value"],"Acme performance");
    }).catch_unwind().await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
