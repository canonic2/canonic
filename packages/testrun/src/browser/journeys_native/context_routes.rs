//! Context interception, page precedence and scope-specific callback ownership.
use super::investigation::Site;
use super::*;
use crate::browser::{contract::CallbackResponse, routing::Header};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;

macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run(Engine::$engine, Mode::$mode).await;
        }
    };
}
cell!(item_four_context_routes_chrome_headless, Chrome, Headless);
cell!(item_four_context_routes_chrome_headed, Chrome, Headed);
cell!(item_four_context_routes_firefox_headless, Firefox, Headless);
cell!(item_four_context_routes_firefox_headed, Firefox, Headed);
cell!(item_four_context_routes_webkit_headless, Webkit, Headless);
cell!(item_four_context_routes_webkit_headed, Webkit, Headed);

fn reference(value: &Value, key: &str) -> Reference {
    serde_json::from_value(value[key].clone()).unwrap()
}
fn evaluate(page: Reference, expression: String) -> Request {
    Request::Evaluate {
        scope: page,
        expression,
        is_function: Some(false),
        argument: Default::default(),
        return_handle: false,
    }
}
async fn reply(session: &Session, scope: Reference, text: &str) {
    let until = Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = session.status();
        let items = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        if let Some(callback) = items.iter().find(|callback| callback["kind"] == "route") {
            assert_eq!(callback["policyScope"], json!(scope), "{status}");
            break reference(callback, "reference");
        }
        assert!(
            Instant::now() < until,
            "Context route unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    successful(
        session,
        Request::CallbackResolve {
            callback,
            response: CallbackResponse::RouteFulfillBinary {
                status: 200,
                body_base64: STANDARD.encode(text),
                headers: vec![
                    Header {
                        name: "Content-Type".into(),
                        value: "text/plain".into(),
                    },
                    Header {
                        name: "Access-Control-Allow-Origin".into(),
                        value: "*".into(),
                    },
                ],
            },
        },
    )
    .await;
}
async fn fetch(session: &Session, page: Reference, url: &str, scope: Reference, text: &str) {
    let waiting = session
        .submit(
            Uuid::new_v4(),
            evaluate(
                page,
                format!("fetch({}).then(response=>response.text())", json!(url)),
            ),
            30000,
        )
        .unwrap();
    reply(session, scope, text).await;
    let result = settled(session, waiting).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
    assert_eq!(result.result.unwrap()["value"]["value"], text);
}
async fn continue_to_network(session: &Session, page: Reference, url: &str, fallback: bool) {
    let pending = session
        .submit(
            Uuid::new_v4(),
            evaluate(
                page,
                format!("fetch({}).then(response=>response.text())", json!(url)),
            ),
            30000,
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = session.status();
        let items = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        if let Some(callback) = items.iter().find(|r| r["kind"] == "route") {
            assert_eq!(callback["policyScope"], json!(page));
            break reference(callback, "reference");
        }
        assert!(
            Instant::now() < until,
            "Terminal route unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    successful(
        session,
        Request::CallbackResolve {
            callback,
            response: if fallback {
                CallbackResponse::RouteFallback {}
            } else {
                CallbackResponse::RouteContinue {}
            },
        },
    )
    .await;
    loop {
        let status = session.status();
        let items = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        assert!(
            !items.iter().any(|r| r["kind"] == "route"),
            "Terminal continuation yielded to another policy: {status}"
        );
        if let Some(operation) = session.operation(pending)
            && operation.settled
        {
            assert_eq!(operation.outcome, Outcome::Succeeded, "{operation:?}");
            assert_eq!(operation.result.unwrap()["value"]["value"], "Acme network");
            break;
        }
        assert!(
            Instant::now() < until,
            "Terminal continuation did not reach the network: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
async fn begin_fallback(session: &Session, page: Reference, url: &str) {
    successful(
        session,
        evaluate(
            page,
            format!(
                "globalThis.AcmeFallbackResult=fetch({}).then(response=>response.text());true",
                json!(url)
            ),
        ),
    )
    .await;
    let until = Instant::now() + Duration::from_secs(10);
    let original = loop {
        let status = session.status();
        let items = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        if let Some(callback) = items.iter().find(|r| r["kind"] == "route") {
            assert_eq!(callback["policyScope"], json!(page));
            break reference(callback, "reference");
        }
        assert!(
            Instant::now() < until,
            "Fallback route unavailable: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    successful(
        session,
        Request::CallbackResolve {
            callback: original,
            response: CallbackResponse::RouteFallback {},
        },
    )
    .await;
    let status = session.status();
    let items = status["callbacks"]
        .as_array()
        .or_else(|| status["resources"].as_array())
        .unwrap();
    assert!(items.iter().all(|r| r["reference"] != json!(original)));
}
async fn fallback_to_context(session: &Session, page: Reference, context: Reference, url: &str) {
    begin_fallback(session, page, url).await;
    let removal = session
        .submit(
            Uuid::new_v4(),
            Request::PageRouteSet {
                page,
                url: None,
                authentication_challenges: false,
            },
            30000,
        )
        .unwrap();
    let refused = settled(session, removal).await;
    assert_eq!(refused.outcome, Outcome::Failed);
    assert_eq!(refused.code.as_deref(), Some("ROUTE_CALLBACKS_PENDING"));
    reply(session, context, "Acme fallback policy").await;
    let result = successful(session, evaluate(page, "AcmeFallbackResult".into())).await;
    assert_eq!(result["value"]["value"], "Acme fallback policy");
}
async fn fallback_with_overrides(
    session: &Session,
    page: Reference,
    context: Reference,
    site: &Site,
    url: &str,
) {
    use crate::browser::routing::Overrides;
    use std::sync::atomic::Ordering;
    let before = site.posts.load(Ordering::SeqCst);
    successful(session,evaluate(page,format!("globalThis.AcmeOverrideResult=fetch({},{{headers:{{'x-remove':'old'}}}}).then(r=>r.json());true",json!(url)))).await;
    let until = Instant::now() + Duration::from_secs(10);
    let original = loop {
        let status = session.status();
        let items = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        if let Some(callback) = items.iter().find(|item| item["kind"] == "route") {
            assert_eq!(callback["policyScope"], json!(page));
            break reference(callback, "reference");
        }
        assert!(Instant::now() < until, "Missing override route: {status}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let handoff = successful(
        session,
        Request::CallbackResolve {
            callback: original,
            response: CallbackResponse::RouteFallbackWith {
                overrides: Overrides {
                    url: Some(format!("{}/rewrite-target?private=Acme", site.url)),
                    method: Some("POST".into()),
                    headers: Some(vec![Header {
                        name: "x-acme".into(),
                        value: "new".into(),
                    }]),
                    body_base64: Some("AP+A".into()),
                },
            },
        },
    )
    .await;
    assert_eq!(site.posts.load(Ordering::SeqCst), before);
    let next = reference(&handoff, "nextCallback");
    let status = session.status();
    let items = status["callbacks"]
        .as_array()
        .or_else(|| status["resources"].as_array())
        .unwrap();
    assert!(
        items
            .iter()
            .any(|item| item["reference"] == json!(next) && item["policyScope"] == json!(context)),
        "{status}"
    );
    successful(
        session,
        Request::CallbackResolve {
            callback: next,
            response: CallbackResponse::RouteContinue {},
        },
    )
    .await;
    let result = successful(
        session,
        evaluate(page, "AcmeOverrideResult.then(JSON.stringify)".into()),
    )
    .await;
    let value: Value = serde_json::from_str(result["value"]["value"].as_str().unwrap()).unwrap();
    assert_eq!(
        value,
        json!({"target":true,"header":"new","removed":true,"bytes":[0,255,128]})
    );
    assert_eq!(site.posts.load(Ordering::SeqCst), before + 1);
}
async fn navigate(session: &Session, page: Reference, url: &str) {
    successful(
        session,
        Request::PageNavigate {
            page,
            navigation: Navigation::Goto {
                url: url.into(),
                wait_until: Load::Load,
            },
        },
    )
    .await;
}
fn descendant(page: Reference, url: &str, popup: bool) -> Request {
    let create = if popup {
        format!(
            "target=open({});if(!target)reject(Error('Acme popup refused'));",
            json!(url)
        )
    } else {
        format!(
            "const frame=document.createElement('iframe');frame.src={};document.body.append(frame);target=frame.contentWindow;globalThis.AcmeRouteChild=frame;",
            json!(url)
        )
    };
    evaluate(
        page,
        format!(
            r#"new Promise((resolve,reject)=>{{
        let target;const listener=event=>{{if(event.source===target&&event.data?.kind==='Acme routed frame'){{clearTimeout(timer);removeEventListener('message',listener);resolve(event.data.text);}}}};
        const timer=setTimeout(()=>{{removeEventListener('message',listener);reject(Error('Acme descendant route unavailable'));}},10000);addEventListener('message',listener);{create}
        globalThis.AcmeRoutePopup={popup}?target:null;
    }})"#
        ),
    )
}
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
    let exercise=std::panic::AssertUnwindSafe(async {
        let ready=settled(&session,session.startup()).await;assert_eq!(ready.outcome,Outcome::Succeeded,"{ready:?}");let ready=ready.result.unwrap();
        let context=reference(&ready,"context");let page=reference(&ready,"page");
        navigate(&session,page,&format!("{}/audit",site.url)).await;
        let direct=format!("{}/frame-probe",site.url);
        successful(&session,Request::ContextRouteSet{context,url:Some(direct.clone()),authentication_challenges:false}).await;
        successful(&session,Request::PageRouteSet{page,url:Some(direct.clone()),authentication_challenges:false}).await;
        continue_to_network(&session,page,&direct,false).await;
        fallback_to_context(&session,page,context,&direct).await;
        fallback_with_overrides(&session,page,context,&site,&direct).await;
        successful(&session,Request::ContextRouteSet{context,url:None,authentication_challenges:false}).await;
        continue_to_network(&session,page,&direct,true).await;
        let cross=site.url.replace("127.0.0.1","localhost");let probe=format!("{cross}/frame-probe");let app=format!("{cross}/route-frame");
        successful(&session,Request::ContextRouteSet{context,url:Some(probe.clone()),authentication_challenges:false}).await;
        successful(&session,Request::PageRouteSet{page,url:Some(probe.clone()),authentication_challenges:false}).await;
        fetch(&session,page,&probe,page,"Acme page policy").await;
        successful(&session,Request::PageRouteSet{page,url:None,authentication_challenges:false}).await;
        fetch(&session,page,&probe,context,"Acme context policy").await;
        for popup in [false,true] {
            let waiting=session.submit(Uuid::new_v4(),descendant(page,&app,popup),30000).unwrap();
            reply(&session,context,"Acme descendant policy").await;
            let result=settled(&session,waiting).await;assert_eq!(result.outcome,Outcome::Succeeded,"{result:?}");assert_eq!(result.result.unwrap()["value"]["value"],"Acme descendant policy");
            successful(&session,evaluate(page,if popup {"AcmeRoutePopup.close();true"}else{"AcmeRouteChild.remove();true"}.into())).await;
        }
        let sibling=successful(&session,Request::PageCreate{context,select:false}).await;let sibling=reference(&sibling,"page");
        navigate(&session,sibling,&app).await;reply(&session,context,"Acme sibling policy").await;
        assert_eq!(successful(&session,evaluate(sibling,"AcmeFrameRoute".into())).await["value"]["value"],"Acme sibling policy");
        let other=successful(&session,Request::ContextCreate{options:Default::default()}).await;let other=reference(&other,"context");
        let foreign=successful(&session,Request::PageCreate{context:other,select:false}).await;let foreign=reference(&foreign,"page");
        navigate(&session,foreign,&app).await;
        assert_eq!(successful(&session,evaluate(foreign,"AcmeFrameRoute".into())).await["value"]["value"],"Acme network");
        successful(&session,Request::ContextClose{context:other}).await;
        successful(&session,Request::PageRouteSet{page,url:Some(probe.clone()),authentication_challenges:false}).await;
        successful(&session,Request::ContextRouteSet{context,url:None,authentication_challenges:false}).await;
        fetch(&session,page,&probe,page,"Acme retained page policy").await;
        navigate(&session,sibling,&app).await;
        assert_eq!(successful(&session,evaluate(sibling,"AcmeFrameRoute".into())).await["value"]["value"],"Acme network");
        successful(&session,Request::PageRouteSet{page,url:None,authentication_challenges:false}).await;
        exercise_har(&session,context,page,sibling,&app,&probe,root.path()).await;
    }).catch_unwind().await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = exercise {
        if let Session::Playwright(driver) = &session {
            eprintln!(
                "Context route private failures: {:?}",
                driver.private_failures()
            );
        }
        std::panic::resume_unwind(panic);
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "One context replay workflow verifies future descendants, archive retention, page precedence and independent removal"
)]
async fn exercise_har(
    session: &Session,
    context: Reference,
    page: Reference,
    sibling: Reference,
    app: &str,
    probe: &str,
    directory: &Path,
) {
    use crate::browser::contract::HarNotFound;
    let input = directory.join("context-har-input");
    crate::artifacts::directory(&input).unwrap();
    let path = input.join("source.har");
    crate::artifacts::write(&path,&serde_json::to_vec(&json!({"log":{"version":"1.2","entries":[{
        "request":{"url":probe,"method":"GET","headers":[]},"response":{"status":200,"headers":[{"name":"Content-Type","value":"text/plain"},{"name":"Access-Control-Allow-Origin","value":"*"}],"content":{"text":"Acme context HAR"}}
    }]}})).unwrap()).unwrap();
    let registered = successful(
        session,
        Request::HarRegister {
            path: path.to_string_lossy().into_owned(),
        },
    )
    .await;
    let archive_id = serde_json::from_value(registered["archiveId"].clone()).unwrap();
    let replay = successful(
        session,
        Request::ContextHarSet {
            context,
            archive_id: Some(archive_id),
            not_found: HarNotFound::Continue,
            url_filter: None,
        },
    )
    .await;
    assert_eq!(replay["rule"]["scopeKind"], "context");
    assert_eq!(replay["rule"]["context"], json!(context));
    let removal = settled(
        session,
        session
            .submit(Uuid::new_v4(), Request::HarRemove { archive_id }, 30000)
            .unwrap(),
    )
    .await;
    assert_eq!(removal.code.as_deref(), Some("HAR_ARCHIVE_IN_USE"));
    let fresh = successful(
        session,
        Request::PageCreate {
            context,
            select: false,
        },
    )
    .await;
    let fresh = reference(&fresh, "page");
    for page in [fresh, sibling] {
        navigate(session, page, app).await;
        assert_eq!(
            successful(session, evaluate(page, "AcmeFrameRoute".into())).await["value"]["value"],
            "Acme context HAR"
        );
    }
    for popup in [false, true] {
        let result = successful(session, descendant(page, app, popup)).await;
        assert_eq!(result["value"]["value"], "Acme context HAR");
        successful(
            session,
            evaluate(
                page,
                if popup {
                    "AcmeRoutePopup.close();true"
                } else {
                    "AcmeRouteChild.remove();true"
                }
                .into(),
            ),
        )
        .await;
    }
    let other = successful(
        session,
        Request::ContextCreate {
            options: Default::default(),
        },
    )
    .await;
    let other = reference(&other, "context");
    let foreign = successful(
        session,
        Request::PageCreate {
            context: other,
            select: false,
        },
    )
    .await;
    let foreign = reference(&foreign, "page");
    navigate(session, foreign, app).await;
    assert_eq!(
        successful(session, evaluate(foreign, "AcmeFrameRoute".into())).await["value"]["value"],
        "Acme network"
    );
    successful(session, Request::ContextClose { context: other }).await;
    successful(
        session,
        Request::PageRouteSet {
            page,
            url: Some(probe.into()),
            authentication_challenges: false,
        },
    )
    .await;
    fetch(session, page, probe, page, "Acme page over context HAR").await;
    begin_fallback(session, page, probe).await;
    let replayed = successful(session, evaluate(page, "AcmeFallbackResult".into())).await;
    assert_eq!(replayed["value"]["value"], "Acme context HAR");
    successful(
        session,
        Request::ContextHarSet {
            context,
            archive_id: None,
            not_found: HarNotFound::Abort,
            url_filter: None,
        },
    )
    .await;
    fetch(session, page, probe, page, "Acme page after context HAR").await;
    navigate(session, fresh, app).await;
    assert_eq!(
        successful(session, evaluate(fresh, "AcmeFrameRoute".into())).await["value"]["value"],
        "Acme network"
    );
    successful(session, Request::HarRemove { archive_id }).await;
    successful(
        session,
        Request::PageRouteSet {
            page,
            url: None,
            authentication_challenges: false,
        },
    )
    .await;
}
