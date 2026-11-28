//! Real SDK replies must settle after an owned paused context closes.
use super::*;
use crate::browser::{
    contract::{ContextOptions, Engine, LaunchOptions, Mode},
    dispatcher::Event,
    events::Item,
    objects::ObjectRef,
};
use futures_util::FutureExt;
use serde_json::json;
use std::time::Duration;

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}
async fn channel(
    connection: &Connection,
    owner: &ObjectRef,
    method: &str,
    params: Value,
    field: &str,
    kind: &str,
) -> ObjectRef {
    let reply = connection
        .retained(owner, method, params, deadline(), false)
        .await
        .unwrap()
        .unwrap();
    connection
        .resolve(reply[field]["guid"].as_str().unwrap(), kind)
        .await
        .unwrap()
}
#[tokio::test]
async fn sdk_closed_context_replies_to_original_paused_call() {
    let cache = PathBuf::from(
        std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select the private browser cache"),
    );
    let root = tempfile::Builder::new()
        .prefix("tr-sdk-paused-close-")
        .tempdir()
        .unwrap()
        .keep();
    for mode in [Mode::Headless, Mode::Headed] {
        let bundle = DiagnosticBundle::inspect(
            &cache.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
            &cache.join("playwright-core-1.64.1/package"),
            &cache.join("browsers"),
            deadline(),
        )
        .await
        .unwrap();
        let mut driver = Driver::start(bundle, deadline()).await.unwrap();
        let mut events = driver.take_events().unwrap();
        let connection = driver.connection().clone();
        let result=std::panic::AssertUnwindSafe(async {
            let browser_type=connection.bootstrap().unwrap().browser_types["firefox"].clone();
            let browser=channel(&connection,&browser_type,"launch",LaunchOptions{engine:Engine::Firefox,mode,proxy:None,context:Default::default(),media_fixtures:Default::default()}.wire(),"browser","Browser").await;
            for cycle in 0..2 {
                let context=channel(&connection,&browser,"newContext",ContextOptions::default().wire(),"context","BrowserContext").await;
                let page=channel(&connection,&context,"newPage",json!({}),"page","Page").await;
                let initializer=connection.initializer(&page).await.unwrap().unwrap();
                let frame=connection.resolve(initializer["mainFrame"]["guid"].as_str().unwrap(),"Frame").await.unwrap();
                let initializer=connection.initializer(&context).await.unwrap().unwrap();
                let debugger=connection.resolve(initializer["debugger"]["guid"].as_str().unwrap(),"Debugger").await.unwrap();
                connection.retained(&frame,"setContent",json!({"html":"<script>globalThis.acmeClosedClicks=0</script><button id=Acme onclick='acmeClosedClicks++'>Acme</button>","waitUntil":"load"}),deadline(),false).await.unwrap();
                if cycle == 1 { connection.retained(&debugger,"enable",json!({}),deadline(),true).await.unwrap(); }
                connection.retained(&debugger,"requestPause",json!({}),deadline(),true).await.unwrap();
                let caller_connection=connection.clone();let caller_frame=frame.clone();
                let caller=tokio::spawn(async move {
                    caller_connection.retained(&caller_frame,"click",json!({"selector":"#Acme","strict":true,"force":false}),deadline(),false).await
                });
                tokio::time::timeout(Duration::from_secs(10),async {
                    loop {
                        match events.next().await.expect("Owned driver event stream") {
                            Item::Event(Event::Message{object,method,params}) if object==debugger && method=="pausedStateChanged" && params.as_ref().is_some_and(|params|params["pausedDetails"].is_object())=>break,
                            Item::Barrier(reply)=>{let _=reply.send(());},
                            Item::Gap{dropped}=>panic!("Pause ownership cannot survive an unobserved gap: {dropped}"),
                            _=>{}
                        }
                    }
                }).await.expect("Actual SDK pause");
                assert!(!caller.is_finished(),"The paused original request must still be owned");
                let count=connection.retained_internal(&frame,"evaluateExpression",json!({"expression":"acmeClosedClicks","isFunction":false,"arg":crate::browser::values::argument(&Value::Null).unwrap()}),deadline(),false).await.unwrap().unwrap();
                assert_eq!(crate::browser::values::result(&count["value"]).unwrap(),json!(0));
                driver.observe_descendants().await.unwrap();
                connection.retained(&context,"close",json!({}),deadline(),true).await.unwrap();
                let reply=tokio::time::timeout(Duration::from_secs(5),caller).await.expect("A real original SDK reply must follow closed-scope cleanup").unwrap();
                let error=match reply {Err(connection::Error::Remote(error))=>error,other=>panic!("Expected a real closed-target reply, not local retirement or loss: {other:?}")};
                assert!(error.error.to_string().contains("Target page, context or browser has been closed"),"{error:?}");
                assert_eq!(connection.status().pending,0);
                std::fs::write(root.join(format!("{mode:?}-{cycle}.json")),serde_json::to_vec_pretty(&json!({"context":context.guid(),"originalReply":error.error,"pending":connection.status().pending,"effectsBeforeClose":0})).unwrap()).unwrap();
            }
        }).catch_unwind().await;
        let original = driver.shutdown(deadline()).await;
        std::fs::write(
            root.join(format!("{mode:?}-shutdown.json")),
            serde_json::to_vec_pretty(
                &json!({"originalSucceeded":original.is_ok(),"shutdown":driver.shutdown_report()}),
            )
            .unwrap(),
        )
        .unwrap();
        if original.is_err() {
            loop {
                let retry = driver
                    .shutdown(Instant::now() + Duration::from_secs(10))
                    .await;
                std::fs::write(
                    root.join(format!("{mode:?}-cleanup-retry.json")),
                    serde_json::to_vec_pretty(
                        &json!({"succeeded":retry.is_ok(),"shutdown":driver.shutdown_report()}),
                    )
                    .unwrap(),
                )
                .unwrap();
                if retry.is_ok() {
                    break;
                }
                eprintln!(
                    "Cleanup pending; retaining SDK driver {:?}; evidence {}",
                    driver.pid(),
                    root.display()
                );
            }
        }
        assert!(driver.pid().is_none());
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
        assert!(
            original.is_ok(),
            "Original shutdown failed; retained evidence {}",
            root.display()
        );
    }
    eprintln!("Real closed-context SDK reply evidence: {}", root.display());
}
