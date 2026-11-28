//! Retained live console values, bounded properties and document cleanup in six cells.
use super::investigation::Site;
use super::*;
use crate::browser::{
    diagnostic_arguments::Read as ArgumentRead,
    diagnostics::{DiagnosticKind, Query},
};

macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run(Engine::$engine, Mode::$mode).await;
        }
    };
}
cell!(
    item_four_console_arguments_chrome_headless,
    Chrome,
    Headless
);
cell!(item_four_console_arguments_chrome_headed, Chrome, Headed);
cell!(
    item_four_console_arguments_firefox_headless,
    Firefox,
    Headless
);
cell!(item_four_console_arguments_firefox_headed, Firefox, Headed);
cell!(
    item_four_console_arguments_webkit_headless,
    Webkit,
    Headless
);
cell!(item_four_console_arguments_webkit_headed, Webkit, Headed);
async fn evaluate(session: &Session, page: Reference, expression: &str) -> Value {
    successful(
        session,
        Request::Evaluate {
            scope: page,
            expression: expression.into(),
            is_function: Some(false),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await
}
async fn read(session: &Session, context: Reference, group_id: Uuid, query: ArgumentRead) -> Value {
    successful(
        session,
        Request::DiagnosticsArgumentsRead {
            context,
            group_id,
            query,
        },
    )
    .await
}
async fn captured(
    session: &Session,
    context: Reference,
    page: Reference,
    after: u64,
) -> (Uuid, u64) {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let value = successful(
            session,
            Request::DiagnosticsRead {
                context,
                query: Query {
                    after: Some(after),
                    page: Some(page),
                    kind: Some(DiagnosticKind::Console),
                    ..Default::default()
                },
            },
        )
        .await;
        assert!(!value.to_string().contains("Acme-private-value"), "{value}");
        if let Some(group) = value["events"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|event| event["data"]["arguments"]["groupId"].as_str())
        {
            return (
                Uuid::parse_str(group).unwrap(),
                value["latestSequence"].as_u64().unwrap(),
            );
        }
        assert!(
            Instant::now() < until,
            "Console argument ownership unavailable: {value}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
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
        let context=serde_json::from_value(ready["context"].clone()).unwrap();let page=serde_json::from_value(ready["page"].clone()).unwrap();
        let navigate=||Request::PageNavigate {page,navigation:Navigation::Goto {url:format!("{}/audit",site.url),wait_until:Load::Load}};
        successful(&session,navigate()).await;
        let before=successful(&session,Request::DiagnosticsStart {context}).await;
        successful(&session,Request::DiagnosticsArgumentsSet {context,enabled:true}).await;
        evaluate(&session,page,"globalThis.acmeArgumentCalls=0;globalThis.acmeLogged={name:'Acme-private-value',big:123n,nested:{answer:42}};Object.defineProperty(acmeLogged,'getter',{enumerable:true,get(){acmeArgumentCalls++;return 17}});console.log('Acme object',acmeLogged);true").await;
        let (group,after)=captured(&session,context,page,before["latestSequence"].as_u64().unwrap()).await;
        let list=successful(&session,Request::DiagnosticsArgumentsList {context}).await;
        assert!(!list.to_string().contains("Acme-private-value"));
        let names=read(&session,context,group,ArgumentRead{index:1,properties:Some(Default::default()),..Default::default()}).await;
        assert!(names["value"].to_string().contains("getter"),"{names}");
        assert_eq!(evaluate(&session,page,"acmeArgumentCalls").await["value"]["value"],0);
        evaluate(&session,page,"acmeLogged.nested.answer=43;true").await;
        let value=read(&session,context,group,ArgumentRead{index:1,path:vec!["nested".into()],..Default::default()}).await;
        assert_eq!(value["value"]["entries"][0]["value"]["value"],43,"{value}");
        let getter=read(&session,context,group,ArgumentRead{index:1,path:vec!["getter".into()],..Default::default()}).await;
        assert_eq!(getter["value"]["value"],17);assert_eq!(getter["potentiallyMutating"],true);
        assert_eq!(evaluate(&session,page,"acmeArgumentCalls").await["value"]["value"],1);
        let other=successful(&session,Request::ContextCreate {options:Default::default()}).await;
        let other=serde_json::from_value(other["context"].clone()).unwrap();
        let foreign=settled(&session,session.submit(Uuid::new_v4(),Request::DiagnosticsArgumentsRead {context:other,group_id:group,query:Default::default()},30000).unwrap()).await;
        assert_eq!(foreign.code.as_deref(),Some("DIAGNOSTIC_ARGUMENTS_EXPIRED"),"{foreign:?}");
        successful(&session,Request::ContextClose {context:other}).await;
        evaluate(&session,page,"console.count('Acme retained counter');console.count('Acme retained counter');true").await;
        let disabled=successful(&session,Request::DiagnosticsArgumentsSet {context,enabled:false}).await;
        assert!(disabled["groups"].as_array().unwrap().is_empty(),"{disabled}");
        evaluate(&session,page,"console.count('Acme retained counter');true").await;
        let counts=successful(&session,Request::DiagnosticsRead {context,query:Query{after:Some(after),page:Some(page),kind:Some(DiagnosticKind::Console),include_text:true,..Default::default()}}).await;
        let counts=counts["events"].as_array().unwrap().iter().filter_map(|event|event["data"]["text"].as_str()).filter(|text|text.starts_with("Acme retained counter")).collect::<Vec<_>>();
        assert_eq!(counts.len(),3,"{counts:?}");
        if engine==Engine::Chrome {assert!(counts.last().unwrap().contains('3'),"{counts:?}");}
        let before=successful(&session,Request::DiagnosticsRead {context,query:Default::default()}).await;
        successful(&session,Request::DiagnosticsArgumentsSet {context,enabled:true}).await;
        evaluate(&session,page,"console.log('Acme object',acmeLogged);true").await;
        let (group,_)=captured(&session,context,page,before["latestSequence"].as_u64().unwrap()).await;
        successful(&session,navigate()).await;
        let stale=settled(&session,session.submit(Uuid::new_v4(),Request::DiagnosticsArgumentsRead {context,group_id:group,query:Default::default()},30000).unwrap()).await;
        assert_eq!(stale.code.as_deref(),Some("DIAGNOSTIC_ARGUMENTS_EXPIRED"),"{stale:?}");
        let disabled=successful(&session,Request::DiagnosticsArgumentsSet {context,enabled:false}).await;
        assert!(disabled["groups"].as_array().unwrap().is_empty(),"{disabled}");
    }).catch_unwind().await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = exercise {
        std::panic::resume_unwind(panic);
    }
}
