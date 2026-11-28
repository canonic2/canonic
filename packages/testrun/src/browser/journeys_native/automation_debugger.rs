//! Real automation pauses preserve pending actions, source identities and siblings.
use super::*;
use crate::browser::automation_debugger::Action as Debug;
use crate::browser::automation_debugger::Location;
macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run(Engine::$engine, Mode::$mode).await;
        }
    };
}
cell!(
    item_four_automation_debugger_firefox_headless,
    Firefox,
    Headless
);
cell!(
    item_four_automation_debugger_firefox_headed,
    Firefox,
    Headed
);
cell!(
    item_four_automation_debugger_webkit_headless,
    Webkit,
    Headless
);
cell!(item_four_automation_debugger_webkit_headed, Webkit, Headed);
async fn debug(session: &Session, context: Reference, action: Debug) -> Value {
    successful(session, Request::ContextDebugger { context, action }).await
}
async fn pause(session: &Session, context: Reference, old: Option<Uuid>, pending: Uuid) -> Uuid {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let status = debug(
            session,
            context,
            Debug::Status {
                include_details: false,
            },
        )
        .await;
        if status["paused"] == true {
            let pause = serde_json::from_value(status["pauseId"].clone()).unwrap();
            if Some(pause) != old {
                return pause;
            }
        }
        if Instant::now() >= until {
            let calls = settled(
                session,
                session
                    .submit(
                        Uuid::new_v4(),
                        Request::ContextDebugger {
                            context,
                            action: Debug::Calls {
                                offset: 0,
                                limit: 32,
                                include_details: true,
                            },
                        },
                        5000,
                    )
                    .unwrap(),
            )
            .await;
            panic!(
                "Automation pause unavailable: {status}; input: {:?}; calls: {calls:?}",
                session.operation(pending)
            );
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
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
        let ready=settled(&session,session.startup()).await;assert_eq!(ready.outcome,Outcome::Succeeded,"{ready:?}");let ready=ready.result.unwrap();let context=serde_json::from_value(ready["context"].clone()).unwrap();let page=serde_json::from_value(ready["page"].clone()).unwrap();
        successful(&session,Request::PageNavigate{page,navigation:Navigation::SetContent{html:"<script>globalThis.AcmeClicks=0</script><button id=Acme-private-action onclick='AcmeClicks++'>Acme</button>".into(),wait_until:Load::Load}}).await;
        let click=||action(page,Locator::Css{selector:"#Acme-private-action".into()},Action::Click{});
        assert_eq!(debug(&session,context,Debug::Enable).await["callCollectionEnabled"],true);
        let probe=successful(&session,Request::Evaluate{scope:page,expression:"6*7".into(),is_function:Some(false),argument:Default::default(),return_handle:false}).await;
        assert_eq!(probe["value"]["value"],42);
        let calls=debug(&session,context,Debug::Calls{offset:0,limit:32,include_details:true}).await;
        assert!(!calls["calls"].as_array().unwrap().is_empty(),"An enabled debugger must observe an ordinary evaluation: {calls}");
        successful(&session,Request::TraceStart{context,options:crate::browser::tracing::Options{snapshots:true.into(),..Default::default()}}).await;
        debug(&session,context,Debug::Pause).await;
        let first=session.submit(Uuid::new_v4(),click(),30000).unwrap();let first_pause=pause(&session,context,None,first).await;
        assert!(!session.operation(first).unwrap().settled);
        let metadata=debug(&session,context,Debug::Status{include_details:false}).await;assert!(!metadata.to_string().contains("Acme-private-action"));assert_eq!(metadata["javascriptDebugger"],false);
        let private=debug(&session,context,Debug::Status{include_details:true}).await;assert!(private["details"].is_object());
        let trace=successful(&session,Request::TraceStop { file_path: None, analysis_developer: None, analysis_page: None }).await;
        assert!(!trace["reports"].as_array().expect("Trace reports").is_empty(),"{trace}");
        assert_eq!(debug(&session,context,Debug::Status{include_details:false}).await["pauseId"],serde_json::json!(first_pause));
        debug(&session,context,Debug::Next{pause_id:first_pause}).await;
        let until=Instant::now()+Duration::from_secs(10);
        loop {
            let status=debug(&session,context,Debug::Status{include_details:false}).await;
            if session.operation(first).unwrap().settled{break;}
            if status["paused"]==true&&status["pauseId"]!=serde_json::json!(first_pause){debug(&session,context,Debug::Resume{pause_id:serde_json::from_value(status["pauseId"].clone()).unwrap()}).await;break;}
            assert!(Instant::now()<until,"Stepped call did not settle or pause: {status}");tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(settled(&session,first).await.outcome,Outcome::Succeeded);
        if debug(&session,context,Debug::Status{include_details:false}).await["pauseRequested"]!=true{debug(&session,context,Debug::Pause).await;}
        let second=session.submit(Uuid::new_v4(),click(),30000).unwrap();let second_pause=pause(&session,context,Some(first_pause),second).await;
        let stale=settled(&session,session.submit(Uuid::new_v4(),Request::ContextDebugger{context,action:Debug::Resume{pause_id:first_pause}},30000).unwrap()).await;assert_eq!(stale.code.as_deref(),Some("STALE_AUTOMATION_PAUSE"));
        debug(&session,context,Debug::Resume{pause_id:second_pause}).await;assert_eq!(settled(&session,second).await.outcome,Outcome::Succeeded);
        let count=successful(&session,Request::Evaluate{scope:page,expression:"AcmeClicks".into(),is_function:Some(false),argument:Default::default(),return_handle:false}).await;assert_eq!(count["value"]["value"],2);
        let until=Instant::now()+Duration::from_secs(10);
        let private=loop{
            let calls=debug(&session,context,Debug::Calls{offset:0,limit:32,include_details:true}).await;
            if calls["calls"].as_array().unwrap().iter().any(|call|call["status"]=="success"&&call["details"]["title"].as_str().is_some_and(|title|title.to_ascii_lowercase().contains("click"))){break calls;}
            assert!(Instant::now()<until,"Automation call updates did not arrive: {calls}");tokio::time::sleep(Duration::from_millis(25)).await;
        };
        assert!(private["calls"].as_array().unwrap().iter().any(|call|!call["logs"].as_array().unwrap().is_empty()),"{private}");
        let metadata=debug(&session,context,Debug::Calls{offset:0,limit:32,include_details:false}).await;
        assert!(metadata["calls"].as_array().unwrap().iter().all(|call|call.get("details").is_none()&&call.get("logs").is_none()));
        assert!(!metadata.to_string().contains("Acme-private"));
        source_run_to(&session,context,page).await;
        let sibling=successful(&session,Request::ContextCreate{options:Default::default()}).await;let sibling=serde_json::from_value(sibling["context"].clone()).unwrap();let other=successful(&session,Request::PageCreate{context:sibling,select:false}).await;let other=serde_json::from_value(other["page"].clone()).unwrap();
        debug(&session,context,Debug::Pause).await;let closing=session.submit(Uuid::new_v4(),click(),30000).unwrap();pause(&session,context,None,closing).await;
        successful(&session,Request::ContextClose{context}).await;let cancelled=tokio::time::timeout(Duration::from_secs(5),settled(&session,closing)).await.expect("A closed context must release its paused automation call without waiting for the input deadline");assert_ne!(cancelled.outcome,Outcome::Succeeded);
        let still_live=successful(&session,Request::Evaluate{scope:other,expression:"6*7".into(),is_function:Some(false),argument:Default::default(),return_handle:false}).await;assert_eq!(still_live["value"]["value"],42);
    }).catch_unwind().await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = exercise {
        std::panic::resume_unwind(panic);
    }
}

async fn source_run_to(session: &Session, context: Reference, page: Reference) {
    let source_file = "src/Acme-journey.rs";
    let target = Location {
        file: "Acme-journey.rs".into(),
        line: Some(30),
        column: Some(3),
    };
    let click = |file: &str, line, column| {
        action(
            page,
            Locator::Css {
                selector: "#Acme-private-action".into(),
            },
            Action::AtSource {
                source: Location {
                    file: file.into(),
                    line: Some(line),
                    column: Some(column),
                },
                action: Box::new(Action::Click {}),
            },
        )
    };
    debug(session, context, Debug::Pause).await;
    let first = session
        .submit(Uuid::new_v4(), click(source_file, 10, 3), 30000)
        .unwrap();
    let first_pause = pause(session, context, None, first).await;
    debug(
        session,
        context,
        Debug::RunTo {
            pause_id: first_pause,
            location: target.clone(),
        },
    )
    .await;
    assert_eq!(settled(session, first).await.outcome, Outcome::Succeeded);
    for (file, line, column) in [
        (source_file, 20, 3),
        ("src/Acme-other.rs", 30, 3),
        (source_file, 30, 4),
    ] {
        successful(session, click(file, line, column)).await;
        assert_ne!(
            debug(
                session,
                context,
                Debug::Status {
                    include_details: false
                }
            )
            .await["paused"],
            true
        );
    }
    let count = successful(
        session,
        Request::Evaluate {
            scope: page,
            expression: "AcmeClicks".into(),
            is_function: Some(false),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await;
    assert_eq!(count["value"]["value"], 6);
    let matching = session
        .submit(Uuid::new_v4(), click(source_file, 30, 3), 30000)
        .unwrap();
    let matching_pause = pause(session, context, Some(first_pause), matching).await;
    assert!(!session.operation(matching).unwrap().settled);
    let metadata = debug(
        session,
        context,
        Debug::Status {
            include_details: false,
        },
    )
    .await;
    assert!(!metadata.to_string().contains("Acme-journey"));
    let private = debug(
        session,
        context,
        Debug::Status {
            include_details: true,
        },
    )
    .await;
    assert_eq!(
        private["details"]["location"],
        serde_json::json!({"file":source_file,"line":30,"column":3})
    );
    let stale = settled(
        session,
        session
            .submit(
                Uuid::new_v4(),
                Request::ContextDebugger {
                    context,
                    action: Debug::RunTo {
                        pause_id: first_pause,
                        location: target,
                    },
                },
                30000,
            )
            .unwrap(),
    )
    .await;
    assert_eq!(stale.code.as_deref(), Some("STALE_AUTOMATION_PAUSE"));
    debug(
        session,
        context,
        Debug::Resume {
            pause_id: matching_pause,
        },
    )
    .await;
    assert_eq!(settled(session, matching).await.outcome, Outcome::Succeeded);
    let count = successful(
        session,
        Request::Evaluate {
            scope: page,
            expression: "AcmeClicks".into(),
            is_function: Some(false),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await;
    assert_eq!(count["value"]["value"], 7);
    let calls = debug(
        session,
        context,
        Debug::Calls {
            offset: 0,
            limit: 32,
            include_details: true,
        },
    )
    .await;
    assert!(
        calls["calls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|call| call["status"] == "success"
                && call["details"]["location"]
                    == serde_json::json!({"file":source_file,"line":30,"column":3})),
        "{calls}"
    );
    let metadata = debug(
        session,
        context,
        Debug::Calls {
            offset: 0,
            limit: 32,
            include_details: false,
        },
    )
    .await;
    assert!(!metadata.to_string().contains("Acme-journey"));
}
