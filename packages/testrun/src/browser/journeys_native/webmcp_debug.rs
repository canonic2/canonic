//! Owned Chrome debugging-tool registration, asynchronous results and cancellation.
use super::investigation::Site;
use super::*;
use crate::browser::webmcp_debug::Action;
use serde_json::{Value, json};

#[tokio::test]
async fn item_four_webmcp_debug_chrome_headless() {
    run(Mode::Headless).await;
}
#[tokio::test]
async fn item_four_webmcp_debug_chrome_headed() {
    run(Mode::Headed).await;
}
async fn action(session: &Session, scope: Reference, action: Action) -> Value {
    successful(session, Request::WebMcpDebug { scope, action }).await
}
async fn evaluate(session: &Session, scope: Reference, expression: &str) -> Value {
    successful(
        session,
        Request::Evaluate {
            scope,
            expression: expression.into(),
            is_function: Some(false),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await
}
async fn result(session: &Session, scope: Reference, invocation: &Value) -> Value {
    let invocation_id = serde_json::from_value(invocation["invocationId"].clone()).unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let value = action(session, scope, Action::Read { invocation_id }).await;
        if value["result"].is_object() {
            return value;
        }
        assert!(Instant::now() < until, "Invocation did not settle: {value}");
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
        let ready=settled(&session,session.startup()).await;
        assert_eq!(ready.outcome,Outcome::Succeeded,"{ready:?}");
        let page=serde_json::from_value(ready.result.unwrap()["page"].clone()).unwrap();
        successful(&session,Request::PageNavigate {page,navigation:Navigation::Goto {url:format!("{}/audit",site.url),wait_until:Load::Load}}).await;
        let registered=evaluate(&session,page,r#"(()=>{
          globalThis.acmeWebMcpValue=0;
          globalThis.acmeWebMcpRegistration=new AbortController();
          document.modelContext.registerTool({name:'acme-debug',description:'Increment Acme diagnostics',inputSchema:{type:'object',properties:{amount:{type:'integer'}},required:['amount'],additionalProperties:false},annotations:{debugging:true},execute:async({amount})=>({content:[{type:'text',text:String(acmeWebMcpValue+=amount)}]})},{signal:acmeWebMcpRegistration.signal});
          document.modelContext.registerTool({name:'acme-pending',description:'Pending Acme diagnostics',inputSchema:{type:'object'},annotations:{debugging:true},execute:()=>new Promise(()=>{})});
          document.modelContext.registerTool({name:'acme-user',description:'User tool',inputSchema:{type:'object'},execute:async()=>({content:[]})});
          return true;
        })()"#).await;
        assert_eq!(registered["value"]["value"],true);
        let discovered=action(&session,page,Action::Enable {}).await;
        assert_eq!(discovered["tools"].as_array().unwrap().len(),2,"{discovered}");
        evaluate(&session,page,"new Promise(resolve=>{const child=document.createElement('iframe');child.src='/audit';child.onload=()=>resolve(true);document.body.append(child)})").await;
        let status=session.status();
        let resources=status["resources"].as_array().unwrap();
        let main=resources.iter().find(|resource|resource["kind"]=="frame"&&resource["parent"]==json!(page)).expect("Owned main frame");
        let child=resources.iter().find(|resource|resource["kind"]=="frame"&&resource["parent"]==main["reference"]).expect("Owned child frame");
        let child=serde_json::from_value(child["reference"].clone()).unwrap();
        evaluate(&session,child,"document.modelContext.registerTool({name:'acme-debug',description:'Acme child diagnostic',inputSchema:{type:'object'},annotations:{debugging:true},execute:()=>({value:'Acme child'})});true").await;
        let child_tools=action(&session,child,Action::Enable {}).await;
        assert_eq!(child_tools["tools"].as_array().unwrap().len(),1,"{child_tools}");
        let child_invocation=action(&session,child,Action::Invoke {name:"acme-debug".into(),parameters:json!({})}).await;
        assert_eq!(result(&session,child,&child_invocation).await["result"]["output"]["value"],"Acme child");
        action(&session,child,Action::Disable {}).await;
        assert_eq!(action(&session,page,Action::Status {}).await["tools"].as_array().unwrap().len(),2);
        let invocation=action(&session,page,Action::Invoke {name:"acme-debug".into(),parameters:json!({"amount":2})}).await;
        let completed=result(&session,page,&invocation).await;
        assert_eq!(completed["state"],"completed","{completed}");
        assert!(completed["result"]["output"].to_string().contains('2'),"{completed}");
        assert_eq!(evaluate(&session,page,"acmeWebMcpValue").await["value"]["value"],2);
        let pending=action(&session,page,Action::Invoke {name:"acme-pending".into(),parameters:json!({})}).await;
        let invocation_id=serde_json::from_value(pending["invocationId"].clone()).unwrap();
        action(&session,page,Action::Cancel {invocation_id}).await;
        let canceled=result(&session,page,&pending).await;
        assert_eq!(canceled["state"],"canceled","{canceled}");
        evaluate(&session,page,"acmeWebMcpRegistration.abort();true").await;
        let status=action(&session,page,Action::Status {}).await;
        assert_eq!(status["tools"].as_array().unwrap().len(),1,"{status}");
        successful(&session,Request::PageNavigate {page,navigation:Navigation::Goto {url:format!("{}/audit",site.url),wait_until:Load::Load}}).await;
        let invocation_id=serde_json::from_value(invocation["invocationId"].clone()).unwrap();
        let stale=action(&session,page,Action::Read {invocation_id}).await;
        assert_eq!(stale["state"],"document_lost","{stale}");
        assert!(stale["result"].is_null());
        action(&session,page,Action::Disable {}).await;
        action(&session,page,Action::Disable {}).await;
    }).catch_unwind().await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = exercise {
        std::panic::resume_unwind(panic);
    }
}
