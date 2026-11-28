//! Native cascade precedence and inherited CSS in both Chrome display modes.
use super::*;
use crate::browser::page::{Load, Locator, Navigation};
async fn send(session: &Session, request: Request) -> Value {
    successful(
        session,
        session.submit(Uuid::new_v4(), request, 30000).unwrap(),
    )
    .await
}
async fn diagnostic(session: &Session, request: Request) -> Value {
    match session.submit(Uuid::new_v4(), request, 5000) {
        Ok(id) => serde_json::json!(settled(session, id).await),
        Err(error) => serde_json::json!({"submissionError":error.code}),
    }
}
#[tokio::test]
async fn native_css_matches_cascade_and_inheritance_in_both_modes() {
    run_css(1).await;
}
#[tokio::test]
async fn native_css_frontend_transition_repeated_owned_launches() {
    run_css(5).await;
}
async fn run_css(repetitions: usize) {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let bundle = crate::browser::preparation::prepare_chrome(|_| {})
        .await
        .unwrap();
    for repetition in 0..repetitions {
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
            let exercise=std::panic::AssertUnwindSafe(async{
            let ready=successful(&session,session.startup()).await;let page=serde_json::from_value(ready["page"].clone()).unwrap();
            send(&session,Request::PageNavigate{page,navigation:Navigation::SetContent{html:"<!doctype html><title>Acme CSS</title><style>@layer base { #probe {color:rgb(1,2,3);--acme:low} } @media (min-width:1px) { #probe {color:rgb(4,5,6)} } body {font-family:Acme}</style><div id=probe style='color:rgb(7,8,9)'>Acme</div>".into(),wait_until:Load::Load}}).await;
            let opened=send(&session,Request::DeveloperOpen{page,panel:Some("elements".into())}).await;let developer=serde_json::from_value(opened["developer"].clone()).unwrap();
            let query=|offset,limit|Request::PageCss{developer,page,frame:None,locator:Locator::Css{selector:"#probe".into()},offset,limit};
            let before=session.status();
            let operation=settled(&session,session.submit(Uuid::new_v4(),query(0,50),30000).unwrap()).await;
            if operation.outcome!=Outcome::Succeeded{
                let document=diagnostic(&session,Request::Evaluate{scope:page,expression:"JSON.stringify({url:location.href,readyState:document.readyState,html:document.documentElement.outerHTML})".into(),is_function:Some(false),argument:Default::default(),return_handle:false}).await;
                let isolated=diagnostic(&session,Request::PageRead{page,frame:None,locator:Some(Locator::Css{selector:"html".into()}),read:crate::browser::page::Read::Dom,max_bytes:8192}).await;
                panic!("CSS failed in {mode:?}, repetition {repetition}: {operation:?}; main-world document: {document}; isolated DOM: {isolated}; before: {before}; after: {}",session.status());
            }
            let result=operation.result.unwrap();let rules=result["result"]["items"].as_array().unwrap();
            assert!(rules.iter().any(|rule|rule["declarations"].as_array().unwrap().iter().any(|p|p["name"]=="color"&&p["state"]=="Active")),"{result}");
            assert!(rules.iter().any(|rule|rule["declarations"].as_array().unwrap().iter().any(|p|p["name"]=="color"&&p["state"]=="Overloaded")),"{result}");
            assert!(rules.iter().any(|rule|rule["inherited"]==true&&rule["declarations"].as_array().unwrap().iter().any(|p|p["name"]=="font-family")),"{result}");
            assert!(rules.iter().any(|rule|rule["wrappers"].as_array().unwrap().iter().any(|w|w["kind"]=="layer")),"{result}");
            assert!(rules.iter().any(|rule|rule["declarations"].as_array().unwrap().iter().any(|p|p["name"]=="--acme")),"{result}");
            let page_two=send(&session,query(1,1)).await;assert_eq!(page_two["result"]["items"][0],rules[1]);
            send(&session,Request::DeveloperClose{developer}).await;
            let stale=settled(&session,session.submit(Uuid::new_v4(),query(0,10),30000).unwrap()).await;assert_eq!(stale.code.as_deref(),Some("STALE_REFERENCE"));
        }).catch_unwind().await;
            successful(&session, session.disconnect(30000).unwrap()).await;
            if let Err(panic) = exercise {
                std::panic::resume_unwind(panic);
            }
        }
    }
}
