//! Explicit private Chrome lifecycle acceptance; never selects an installed browser.

use super::*;

#[path = "native_css.rs"]
mod css;
#[path = "native_evaluation.rs"]
mod evaluation;
#[path = "native_heap.rs"]
mod heap;
#[path = "native_media_fixtures.rs"]
mod media_fixtures;
#[path = "native_targets.rs"]
mod targets;
use crate::browser::contract::{Engine, Mode};
use futures_util::FutureExt;

async fn settled(session: &Session, operation: Uuid) -> Operation {
    let result = session
        .wait(operation, Instant::now() + Duration::from_secs(35))
        .await
        .unwrap();
    assert!(result.settled, "{result:?}");
    result
}
async fn successful(session: &Session, operation: Uuid) -> Value {
    let result = settled(session, operation).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{result:?}");
    result.result.unwrap()
}

#[tokio::test]
async fn browser_process_loss_fences_old_references_and_releases_owned_descendants() {
    assert!(crate::browser::processes::initial_host());
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select an explicit private browser cache");
    let bundle = crate::browser::preparation::prepare_chrome(|_| {})
        .await
        .unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let private = temporary.path().join("state");
    crate::artifacts::directory(&private).unwrap();
    let owner = Uuid::new_v4();
    let generation = Uuid::new_v4();
    let options = LaunchOptions {
        media_fixtures: Default::default(),
        proxy: None,
        engine: Engine::Chrome,
        mode: Mode::Headless,
        context: ContextOptions::default(),
    };
    let first = Session::launch_connection(
        bundle.clone(),
        Uuid::new_v4(),
        owner,
        generation,
        options.clone(),
        &private,
    )
    .unwrap();
    let result = std::panic::AssertUnwindSafe(async {
        successful(&first, first.startup()).await;
        let page: Reference =
            serde_json::from_value(first.status()["selectedPage"].clone()).unwrap();
        let pid = first
            .0
            .shared
            .native_pid
            .load(std::sync::atomic::Ordering::SeqCst);
        assert_ne!(pid, 0);
        let mut witnesses = crate::browser::processes::Tracker::default();
        witnesses.observe(pid).unwrap();
        assert!(witnesses.count() >= 2);
        // SAFETY: this PID belongs to the fixture's retained, unreaped Child.
        // The session is held and no disconnect/reaper runs before this signal.
        assert_eq!(
            unsafe { libc::kill(i32::try_from(pid).unwrap(), libc::SIGKILL) },
            0
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            while first.status()["phase"] != "lost" {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            first
                .submit(Uuid::new_v4(), Request::PageClose { page }, 30000)
                .err()
                .unwrap()
                .code,
            "SESSION_NOT_READY"
        );
        (page, witnesses)
    })
    .catch_unwind()
    .await;
    successful(&first, first.disconnect(30000).unwrap()).await;
    let (page, witnesses) = match result {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    assert!(witnesses.released().unwrap());
    let replacement =
        Session::launch_connection(bundle, Uuid::new_v4(), owner, generation, options, &private)
            .unwrap();
    let result = std::panic::AssertUnwindSafe(async {
        successful(&replacement, replacement.startup()).await;
        let rejected = replacement
            .submit(Uuid::new_v4(), Request::PageClose { page }, 30000)
            .unwrap();
        assert_eq!(
            settled(&replacement, rejected).await.code.as_deref(),
            Some("FOREIGN_REFERENCE")
        );
    })
    .catch_unwind()
    .await;
    successful(&replacement, replacement.disconnect(30000).unwrap()).await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
async fn private_chrome_connections_isolate_references_and_release_both_modes() {
    assert!(
        crate::browser::processes::initial_host(),
        "Chrome lifecycle acceptance requires macOS 15 x64"
    );
    let cache = std::env::var_os("TESTRUN_BROWSER_ROOT")
        .expect("Select an explicit private TESTRUN_BROWSER_ROOT");
    assert!(Path::new(&cache).is_absolute());
    let bundle = crate::browser::preparation::prepare_chrome(|_| {})
        .await
        .unwrap();
    for mode in [Mode::Headless, Mode::Headed] {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("private");
        crate::artifacts::directory(&directory).unwrap();
        let owner = Uuid::new_v4();
        let generation = Uuid::new_v4();
        let options = LaunchOptions {
            media_fixtures: Default::default(),
            proxy: None,
            engine: Engine::Chrome,
            mode,
            context: ContextOptions::default(),
        };
        let first = Session::launch_connection(
            bundle.clone(),
            Uuid::new_v4(),
            owner,
            generation,
            options.clone(),
            &directory,
        )
        .unwrap();
        let second = Session::launch_connection(
            bundle.clone(),
            Uuid::new_v4(),
            owner,
            generation,
            options,
            &directory,
        )
        .unwrap();
        let exercise = std::panic::AssertUnwindSafe(isolate_connections(&first, &second))
            .catch_unwind()
            .await;
        let (first_closed, second_closed) = tokio::join!(
            settled(&first, first.disconnect(30000).unwrap()),
            settled(&second, second.disconnect(30000).unwrap())
        );
        assert_eq!(first_closed.outcome, Outcome::Succeeded, "{first_closed:?}");
        assert_eq!(
            second_closed.outcome,
            Outcome::Succeeded,
            "{second_closed:?}"
        );
        assert_eq!(first.status()["phase"], "closed");
        assert_eq!(second.status()["phase"], "closed");
        for entry in std::fs::read_dir(&directory).unwrap() {
            assert!(!entry.unwrap().path().join("chrome-profile").exists());
        }
        if let Err(panic) = exercise {
            std::panic::resume_unwind(panic);
        }
    }
}

async fn isolate_connections(first: &Session, second: &Session) {
    let (first_ready, second_ready) = tokio::join!(
        settled(first, first.startup()),
        settled(second, second.startup())
    );
    assert_eq!(first_ready.outcome, Outcome::Succeeded, "{first_ready:?}");
    assert_eq!(second_ready.outcome, Outcome::Succeeded, "{second_ready:?}");
    let foreign_page: Reference =
        serde_json::from_value(second.status()["selectedPage"].clone()).unwrap();
    let refused = first
        .submit(
            Uuid::new_v4(),
            Request::PageClose { page: foreign_page },
            30000,
        )
        .unwrap();
    assert_eq!(
        settled(first, refused).await.code.as_deref(),
        Some("FOREIGN_REFERENCE")
    );
    assert_eq!(second.status()["phase"], "ready");
    let context = successful(
        first,
        first
            .submit(
                Uuid::new_v4(),
                Request::ContextCreate {
                    options: ContextOptions::default(),
                },
                30000,
            )
            .unwrap(),
    )
    .await;
    let context: Reference = serde_json::from_value(context["context"].clone()).unwrap();
    successful(
        first,
        first
            .submit(
                Uuid::new_v4(),
                Request::PageCreate {
                    context,
                    select: true,
                },
                30000,
            )
            .unwrap(),
    )
    .await;
    assert_eq!(first.status()["selectedContext"], json!(context));
    successful(
        first,
        first
            .submit(Uuid::new_v4(), Request::ContextClose { context }, 30000)
            .unwrap(),
    )
    .await;
    assert!(first.status()["selectedPage"].is_null());
}

#[tokio::test]
async fn native_dialogs_and_waits_allow_independent_controls() {
    use crate::browser::page::{Action, Condition, Load, Locator, Navigation};
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let bundle = crate::browser::preparation::prepare_chrome(|_| {})
        .await
        .unwrap();
    for mode in [Mode::Headless, Mode::Headed] {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("state");
        crate::artifacts::directory(&private).unwrap();
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
                context: ContextOptions::default(),
            },
            &private,
        )
        .unwrap();
        let result=std::panic::AssertUnwindSafe(async {
            let ready=successful(&session,session.startup()).await;
            let page:Reference=serde_json::from_value(ready["page"].clone()).unwrap();
            successful(&session,session.submit(Uuid::new_v4(),Request::PageNavigate{page,navigation:Navigation::SetContent{html:"<button onclick='document.querySelector(\"#result\").textContent=prompt(\"Acme prompt\")'>Prompt</button><p id=result>Waiting</p>".into(),wait_until:Load::Load}},30000).unwrap()).await;
            let waiting=session.submit(Uuid::new_v4(),Request::PageWait{page,frame:None,locator:Some(Locator::Css{selector:"#result".into()}),condition:Condition::Text{value:"Acme answer".into(),contains:false}},30000).unwrap();
            let click=session.submit(Uuid::new_v4(),Request::PageAction{page,frame:None,locator:Locator::Role{role:"button".into(),name:Some("Prompt".into()),exact:true},action:Action::Click{},force:false,postcondition:None},30000).unwrap();
            let callback=tokio::time::timeout(Duration::from_secs(20),async {
                loop {if let Some(value)=session.status()["callbacks"].as_array().and_then(|items|items.first()){break serde_json::from_value::<Reference>(value["reference"].clone()).unwrap();}tokio::time::sleep(Duration::from_millis(10)).await;}
            }).await.unwrap_or_else(|error|panic!("Dialog not observed: {error:?}; click={:?}; wait={:?}; status={}",session.operation(click),session.operation(waiting),session.status()));
            successful(&session,session.submit(Uuid::new_v4(),Request::CallbackResolve{callback,response:contract::CallbackResponse::Dialog{accept:true,prompt_text:Some("Acme answer".into())}},30000).unwrap()).await;
            successful(&session,click).await;successful(&session,waiting).await;
            let duplicate=session.submit(Uuid::new_v4(),Request::CallbackResolve{callback,response:contract::CallbackResponse::Dialog{accept:false,prompt_text:None}},30000).unwrap();
            assert_eq!(settled(&session,duplicate).await.code.as_deref(),Some("STALE_REFERENCE"));
        }).catch_unwind().await;
        successful(&session, session.disconnect(30000).unwrap()).await;
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }
}
