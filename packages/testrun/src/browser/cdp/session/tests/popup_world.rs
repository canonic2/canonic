//! Private native event evidence for the installed adopted-popup missing world.
use super::*;
use futures_util::FutureExt;
use std::os::unix::fs::PermissionsExt;

async fn query(session: &Session, request: Value) -> Operation {
    let id = session
        .submit(
            Uuid::new_v4(),
            serde_json::from_value(request).unwrap(),
            30000,
        )
        .unwrap();
    session
        .wait(id, Instant::now() + Duration::from_secs(35))
        .await
        .unwrap()
}

#[tokio::test]
async fn native_popup_world_diagnostic() {
    let directory = tempfile::Builder::new()
        .prefix("tr-popup-world-")
        .tempdir_in("/private/tmp")
        .unwrap()
        .keep();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    eprintln!("Native popup diagnostic workspace {}", directory.display());
    let bundle = crate::browser::preparation::prepare_chrome(|_| {})
        .await
        .unwrap();
    let session = Session::launch_connection(
        bundle,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        LaunchOptions {
            media_fixtures: Default::default(),
            proxy: None,
            engine: crate::browser::contract::Engine::Chrome,
            mode: crate::browser::contract::Mode::Headless,
            context: Default::default(),
        },
        &directory,
    )
    .unwrap();
    let diagnostic=std::panic::AssertUnwindSafe(async {
    let startup=session.wait(session.0.startup,Instant::now()+Duration::from_secs(35)).await.unwrap();
    assert_eq!(startup.outcome,Outcome::Succeeded,"{startup:?}");
    let context=session.status()["selectedContext"].clone();
    let page=query(&session,json!({"operation":"page_create","context":context,"select":true})).await.result.unwrap()["page"].clone();
    let content=query(&session,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":"<!doctype html><button id=adopt>Acme</button><iframe id=child srcdoc='<body>Acme child</body>'></iframe>","waitUntil":"load"}})).await;
    assert_eq!(content.outcome,Outcome::Succeeded,"{content:?}");
    let handle=query(&session,json!({"operation":"evaluate","scope":page,"expression":"()=>document.querySelector('#adopt')","isFunction":true,"returnHandle":true})).await.result.unwrap()["handle"].clone();
    for expression in ["()=>{globalThis.acmePopup=window.open('about:blank');return !!acmePopup}","()=>{acmePopup.document.body.append(acmePopup.document.adoptNode(document.querySelector('#adopt')))}"] {
        let result=query(&session,json!({"operation":"evaluate","scope":page,"expression":expression,"isFunction":true})).await;
        assert_eq!(result.outcome,Outcome::Succeeded,"{result:?}");
    }
    let owner=query(&session,json!({"operation":"handle_owner_frame","handle":handle})).await;
    assert_eq!(owner.outcome,Outcome::Succeeded,"{owner:?}");
    let owner=owner.result.unwrap();
    let before=session.status();
    let result=query(&session,json!({"operation":"evaluate","scope":owner["page"],"expression":"()=>!!document.querySelector('#adopt')","isFunction":true})).await;
    let after=session.status();
    std::fs::write(directory.join("popup-world.json"),serde_json::to_vec_pretty(&json!({"owner":owner,"before":before,"result":{"outcome":result.outcome,"code":result.code,"value":result.result},"after":after})).unwrap()).unwrap();
    eprintln!("Native popup diagnostic: outcome={:?}, code={:?}; evidence {}",result.outcome,result.code,directory.display());
    }).catch_unwind().await;
    let id = session.disconnect(30000).unwrap();
    let cleanup = session
        .wait(id, Instant::now() + Duration::from_secs(35))
        .await
        .unwrap();
    std::fs::write(
        directory.join("cleanup.json"),
        serde_json::to_vec_pretty(&session.status()).unwrap(),
    )
    .unwrap();
    assert_eq!(cleanup.outcome, Outcome::Succeeded, "{cleanup:?}");
    if let Err(panic) = diagnostic {
        std::panic::resume_unwind(panic);
    }
}
