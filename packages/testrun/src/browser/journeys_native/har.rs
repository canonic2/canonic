//! Callback HAR replay owns imported bytes and never continues a missing POST implicitly.
use super::*;
use crate::browser::contract::{CallbackResponse, HarNotFound};
use serde_json::json;
use std::sync::atomic::Ordering;
macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run(Engine::$engine, Mode::$mode).await;
        }
    };
}
cell!(item_four_har_chrome_headless, Chrome, Headless);
cell!(item_four_har_chrome_headed, Chrome, Headed);
cell!(item_four_har_firefox_headless, Firefox, Headless);
cell!(item_four_har_firefox_headed, Firefox, Headed);
cell!(item_four_har_webkit_headless, Webkit, Headless);
cell!(item_four_har_webkit_headed, Webkit, Headed);

async fn run(engine: Engine, mode: Mode) {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let site = super::investigation::Site::start().await;
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    crate::artifacts::directory(&state).unwrap();
    let input = root.path().join("input");
    crate::artifacts::directory(&input).unwrap();
    let path = input.join("source.har");
    let url = format!("{}/har-write", site.url);
    let end = format!("{}/har-end", site.url);
    let entry = |url: &str,
                 method: &str,
                 body: Option<&str>,
                 status: u16,
                 headers: Value,
                 content: Value| {
        let mut request = json!({"url":url,"method":method,"headers":[]});
        if let Some(body) = body {
            request["postData"] = json!({"mimeType":"text/plain","text":body});
        }
        json!({"request":request,"response":{"status":status,"headers":headers,"content":content}})
    };
    let mut value = json!({"log":{"version":"1.2","creator":{"name":"Acme","version":"1"},"entries":[
        entry(&url,"POST",Some("Acme request secret"),201,json!([{"name":"Content-Type","value":"application/octet-stream"},{"name":"Content-Length","value":"123"},{"name":"Content-Encoding","value":"gzip"},{"name":"X-Acme","value":"first"},{"name":"X-Acme","value":"second"}]),json!({"size":3,"encoding":"base64","text":"AP+A"})),
        entry(&url,"POST",Some("redirect"),302,json!([{"name":"Location","value":"/har-end"}]),json!({"size":0,"text":""})),
        entry(&end,"GET",None,200,json!([]),json!({"text":"redirected"}))
    ]}});
    let start = format!("{}/har-document-start", site.url);
    let document = format!("{}/har-document", site.url);
    let html = "<!doctype html><title>Acme HAR</title><script>const data=Promise.all(Array.from({length:12},(_,i)=>fetch('/har-data?part='+i).then(r=>r.json())));const blocked=fetch('/har-write',{method:'POST',body:'missing'}).then(()=>false,()=>true);alert('Acme automatic HAR');Promise.all([data,blocked]).then(([data,blocked])=>globalThis.AcmeHar={ok:data.every(v=>v.ok),count:data.length,blocked});</script>";
    let entries = value["log"]["entries"].as_array_mut().unwrap();
    entries.push(entry(
        &start,
        "GET",
        None,
        302,
        json!([{"name":"Location","value":"/har-document"}]),
        json!({"text":""}),
    ));
    entries.push(entry(
        &document,
        "GET",
        None,
        200,
        json!([{"name":"Content-Type","value":"text/html"}]),
        json!({"text":html}),
    ));
    for index in 0..12 {
        entries.push(entry(
            &format!("{}/har-data?part={index}", site.url),
            "GET",
            None,
            200,
            json!([{"name":"Content-Type","value":"application/json"}]),
            json!({"text":"{\"ok\":true}"}),
        ));
    }
    crate::artifacts::write(&path, &serde_json::to_vec(&value).unwrap()).unwrap();
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
        &state,
    )
    .unwrap();
    let result=std::panic::AssertUnwindSafe(async{
        let ready=settled(&session,session.startup()).await;assert_eq!(ready.outcome,Outcome::Succeeded,"{ready:?}");let ready=ready.result.unwrap();
        let page=serde_json::from_value(ready["page"].clone()).unwrap();
        successful(&session,Request::PageNavigate{page,navigation:Navigation::Goto{url:format!("{}/audit",site.url),wait_until:Load::Load}}).await;
        let registered=successful(&session,Request::HarRegister{path:path.to_string_lossy().into_owned()}).await;
        let archive_id=serde_json::from_value(registered["archiveId"].clone()).unwrap();
        assert!(registered["reports"][0]["attachment"]["path"].as_str().is_some());
        std::fs::write(&path,b"changed source after registration").unwrap();
        successful(&session,Request::PageRouteSet{page,url:Some(url.clone()),authentication_challenges:false}).await;
        for (index,(body,not_found,expected,redirects)) in [
            ("Acme request secret",HarNotFound::Abort,Some((201,vec![0,255,128])),0),
            ("redirect",HarNotFound::Abort,Some((200,b"redirected".to_vec())),1),
            ("missing",HarNotFound::Abort,None,0),
            ("missing",HarNotFound::Continue,Some((200,b"Acme network write".to_vec())),0),
        ].into_iter().enumerate(){
            let expression=format!("async()=>{{try{{const r=await fetch({},{{method:'POST',body:{}}});return JSON.stringify({{status:r.status,bytes:Array.from(new Uint8Array(await r.arrayBuffer())),header:r.headers.get('x-acme')}})}}catch{{return JSON.stringify({{blocked:true}})}}}}",json!(url),json!(body));
            let pending=session.submit(Uuid::new_v4(),Request::Evaluate{scope:page,expression,is_function:Some(true),argument:Default::default(),return_handle:false},30000).unwrap();
            let until=Instant::now()+Duration::from_secs(10);
            let callback=loop{
                let status=session.status();let resources=status["callbacks"].as_array().or_else(||status["resources"].as_array()).unwrap();
                if let Some(route)=resources.iter().find(|r|r["kind"]=="route"){break serde_json::from_value(route["reference"].clone()).unwrap();}
                assert!(Instant::now()<until,"HAR callback unavailable: {status}");tokio::time::sleep(Duration::from_millis(25)).await;
            };
            if index==0{
                let invalid=settled(&session,session.submit(Uuid::new_v4(),Request::CallbackResolve{callback,response:CallbackResponse::RouteHar{archive_id:Uuid::new_v4(),not_found:HarNotFound::Abort}},30000).unwrap()).await;
                assert_eq!(invalid.code.as_deref(),Some("UNKNOWN_HAR_ARCHIVE"),"{invalid:?}");
            }
            let resolved=successful(&session,Request::CallbackResolve{callback,response:CallbackResponse::RouteHar{archive_id,not_found}}).await;
            assert_eq!(resolved["har"]["matched"],index<2);if index<2{assert_eq!(resolved["har"]["redirectsFollowed"],redirects);}
            let completed=settled(&session,pending).await;assert_eq!(completed.outcome,Outcome::Succeeded,"{completed:?}");
            let response:Value=serde_json::from_str(completed.result.unwrap()["value"]["value"].as_str().unwrap()).unwrap();
            if let Some((status,bytes))=expected{assert_eq!(response["status"],status);assert_eq!(response["bytes"],json!(bytes));if index==0{let value=response["header"].as_str().unwrap();assert!(value.contains("first")&&value.contains("second"),"{response}");}}
            else{assert_eq!(response["blocked"],true);}
            assert_eq!(site.posts.load(Ordering::SeqCst),usize::from(index==3),"Unexpected network POST after HAR case {index}");
        }
        let conflict=settled(&session,session.submit(Uuid::new_v4(),Request::PageHarSet{page,archive_id:Some(archive_id),not_found:HarNotFound::Abort, url_filter: None,},30000).unwrap()).await;
        assert_eq!(conflict.code.as_deref(),Some("MANUAL_ROUTE_ACTIVE"),"{conflict:?}");
        successful(&session,Request::PageRouteSet{page,url:None,authentication_challenges:false}).await;
        automatic(&session,page,archive_id,&start,&document).await;
        assert_eq!(site.posts.load(Ordering::SeqCst),1,"Automatic HAR replay dispatched a missing POST");
        successful(&session,Request::HarRemove{archive_id}).await;
        assert!(successful(&session,Request::HarList{}).await["archives"].as_array().unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(),"changed source after registration");
    }).catch_unwind().await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

async fn automatic(
    session: &Session,
    page: Reference,
    archive_id: Uuid,
    start: &str,
    document: &str,
) {
    let enabled = successful(
        session,
        Request::PageHarSet {
            page,
            archive_id: Some(archive_id),
            not_found: HarNotFound::Abort,
            url_filter: None,
        },
    )
    .await;
    assert_eq!(enabled["rule"]["phase"], "active");
    for (request, code) in [
        (Request::HarRemove { archive_id }, "HAR_ARCHIVE_IN_USE"),
        (
            Request::PageRouteSet {
                page,
                url: None,
                authentication_challenges: false,
            },
            "HAR_ROUTE_ACTIVE",
        ),
    ] {
        let result = settled(
            session,
            session.submit(Uuid::new_v4(), request, 30000).unwrap(),
        )
        .await;
        assert_eq!(result.code.as_deref(), Some(code), "{result:?}");
    }
    let navigation = session
        .submit(
            Uuid::new_v4(),
            Request::PageNavigate {
                page,
                navigation: Navigation::Goto {
                    url: start.into(),
                    wait_until: Load::Load,
                },
            },
            30000,
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let dialog = loop {
        let status = session.status();
        let resources = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        if let Some(dialog) = resources
            .iter()
            .find(|resource| resource["kind"] == "dialog")
        {
            break serde_json::from_value(dialog["reference"].clone()).unwrap();
        }
        assert!(
            Instant::now() < until,
            "Automatic HAR dialog unavailable: {status}; navigation: {:?}",
            session.operation(navigation)
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert!(
        !session.operation(navigation).unwrap().settled,
        "Navigation finished before its dialog was resolved"
    );
    successful(
        session,
        Request::CallbackResolve {
            callback: dialog,
            response: CallbackResponse::Dialog {
                accept: true,
                prompt_text: None,
            },
        },
    )
    .await;
    let navigation = settled(session, navigation).await;
    assert_eq!(navigation.outcome, Outcome::Succeeded, "{navigation:?}");
    let result=successful(session,Request::Evaluate{scope:page,expression:"async()=>{for(let i=0;i<500;i++){if(globalThis.AcmeHar)return JSON.stringify({...AcmeHar,url:location.href});await new Promise(resolve=>setTimeout(resolve,20));}throw Error('Acme automatic HAR did not settle')}".into(),is_function:Some(true),argument:Default::default(),return_handle:false}).await;
    let result: Value = serde_json::from_str(result["value"]["value"].as_str().unwrap()).unwrap();
    assert_eq!(
        result,
        json!({"ok":true,"count":12,"blocked":true,"url":document})
    );
    let rules = successful(session, Request::HarList {}).await;
    assert_eq!(rules["routes"][0]["phase"], "active");
    assert_eq!(rules["routes"][0]["failures"], 0, "{rules}");
    let stop = session
        .submit(
            Uuid::new_v4(),
            Request::PageHarSet {
                page,
                archive_id: None,
                not_found: HarNotFound::Abort,
                url_filter: None,
            },
            30000,
        )
        .unwrap();
    let stopped = settled(session, stop).await;
    let failures = match session {
        Session::Playwright(session) => session.private_failures(),
        Session::Chrome(_) => Vec::new(),
    };
    if stopped.outcome != Outcome::Succeeded {
        assert_eq!(
            session.status()["engine"],
            "firefox",
            "{stopped:?}; {failures:?}"
        );
        assert_eq!(stopped.code.as_deref(), Some("BROWSER_METHOD_FAILED"));
        assert!(
            failures
                .iter()
                .any(|error| error.contains("resetInterceptionWithURI")
                    && error.contains("_interceptedChannel is undefined")),
            "{failures:?}"
        );
        assert_eq!(stopped.result.as_ref().unwrap()["cleanupPending"], true);
        let retained = successful(session, Request::HarList {}).await;
        assert_eq!(retained["routes"][0]["phase"], "stopping");
        let remove = session
            .submit(Uuid::new_v4(), Request::HarRemove { archive_id }, 30000)
            .unwrap();
        assert_eq!(
            settled(session, remove).await.code.as_deref(),
            Some("HAR_ARCHIVE_IN_USE")
        );
        successful(session, Request::PageClose { page }).await;
    }
    assert!(
        successful(session, Request::HarList {}).await["routes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
