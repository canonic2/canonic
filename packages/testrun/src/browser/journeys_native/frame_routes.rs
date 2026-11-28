//! Owned cross-process renderer routing and automatic HAR replay.
use super::investigation::Site;
use super::*;
use crate::browser::contract::{CallbackResponse, HarNotFound};
use serde_json::json;

#[tokio::test]
async fn item_four_frame_routes_chrome_headless() {
    run(Mode::Headless).await;
}
#[tokio::test]
async fn item_four_frame_routes_chrome_headed() {
    run(Mode::Headed).await;
}

fn evaluate(page: Reference, expression: &str) -> Request {
    Request::Evaluate {
        scope: page,
        expression: expression.into(),
        is_function: Some(false),
        argument: Default::default(),
        return_handle: false,
    }
}
fn frame(page: Reference, url: &str) -> Request {
    evaluate(
        page,
        &format!(
            r#"new Promise((resolve,reject)=>{{
        const frame=document.createElement('iframe');globalThis.acmeRouteFrame=frame;
        const listener=event=>{{if(event.source===frame.contentWindow&&event.data?.kind==='Acme routed frame'){{clearTimeout(timer);removeEventListener('message',listener);resolve(event.data.text);}}}};
        const timer=setTimeout(()=>{{removeEventListener('message',listener);reject(Error('Acme frame route unavailable'));}},10000);
        addEventListener('message',listener);frame.src={};document.body.append(frame);
    }})"#,
            json!(url)
        ),
    )
}
async fn pending(session: &Session) -> Reference {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let status = session.status();
        if let Some(callback) = status["callbacks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|callback| callback["kind"] == "route")
        {
            return serde_json::from_value(callback["reference"].clone()).unwrap();
        }
        assert!(Instant::now() < until, "Frame route unavailable: {status}");
        tokio::time::sleep(Duration::from_millis(25)).await;
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
        let ready=settled(&session,session.startup()).await;assert_eq!(ready.outcome,Outcome::Succeeded,"{ready:?}");
        let page:Reference=serde_json::from_value(ready.result.unwrap()["page"].clone()).unwrap();
        successful(&session,Request::PageNavigate{page,navigation:Navigation::Goto{url:format!("{}/audit",site.url),wait_until:Load::Load}}).await;
        let cross=site.url.replace("127.0.0.1","localhost");let probe=format!("{cross}/frame-probe");
        successful(&session,Request::PageRouteSet{page,url:Some(probe.clone()),authentication_challenges:false}).await;
        let waiting=session.submit(Uuid::new_v4(),frame(page,&format!("{cross}/route-frame")),30000).unwrap();
        let callback=pending(&session).await;
        successful(&session,Request::CallbackResolve{callback,response:CallbackResponse::RouteFulfill{status:Some(200),body:Some("Acme intercepted frame".into()),json:None,content_type:Some("text/plain".into()),headers:vec![],path:None,file:None}}).await;
        let result=settled(&session,waiting).await;assert_eq!(result.outcome,Outcome::Succeeded,"{result:?}");assert_eq!(result.result.unwrap()["value"]["value"],"Acme intercepted frame");
        assert!(session.status()["targetStartup"]["resumeAcknowledgements"]["frames"].as_u64().unwrap()>=1);
        successful(&session,Request::PageRouteSet{page,url:None,authentication_challenges:false}).await;
        successful(&session,evaluate(page,"acmeRouteFrame.remove();true")).await;
        let input=root.path().join("har-input");crate::artifacts::directory(&input).unwrap();
        let path=input.join("frame.har");
        crate::artifacts::write(&path,&serde_json::to_vec(&json!({"log":{"version":"1.2","entries":[{"request":{"url":probe,"method":"GET","headers":[]},"response":{"status":200,"headers":[],"content":{"text":"Acme HAR frame"}}}]}})).unwrap()).unwrap();
        let archive=successful(&session,Request::HarRegister{path:path.to_string_lossy().into_owned()}).await;
        let archive_id=serde_json::from_value(archive["archiveId"].clone()).unwrap();
        successful(&session,Request::PageHarSet{page,archive_id:Some(archive_id),not_found:HarNotFound::Continue, url_filter: None,}).await;
        let replayed=successful(&session,frame(page,&format!("{cross}/route-frame"))).await;
        assert_eq!(replayed["value"]["value"],"Acme HAR frame");
        successful(&session,Request::PageHarSet{page,archive_id:None,not_found:HarNotFound::Abort, url_filter: None,}).await;
        successful(&session,evaluate(page,"acmeRouteFrame.remove();true")).await;
        let network=successful(&session,frame(page,&format!("{cross}/route-frame"))).await;
        assert_eq!(network["value"]["value"],"Acme network");
        successful(&session,Request::HarRemove{archive_id}).await;
    }).catch_unwind().await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = exercise {
        std::panic::resume_unwind(panic);
    }
}
