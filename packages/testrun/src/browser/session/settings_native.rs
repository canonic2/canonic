//! Owned localhost acceptance for environment settings and concurrent media.
use super::page_native::{execute, successful};
use super::*;
use crate::browser::{
    contract::{ContextOptions, Engine, Mode},
    page::{Load, Navigation},
    settings::{
        ColorScheme, ContextSetting, Contrast, ForcedColors, Geolocation, Media, MediaOptions,
        Permission, ReducedMotion,
    },
};

#[tokio::test]
async fn firefox_webkit_settings_remain_scoped_and_media_survives_overrides_in_both_modes() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let router = axum::Router::new()
        .route(
            "/",
            axum::routing::get(|| async {
                axum::response::Html("<!doctype html><h1>Acme settings</h1>")
            }),
        )
        .route(
            "/probe",
            axum::routing::get(|| async { ([("cache-control", "no-store")], "Acme online") }),
        );
    let (stop, stopping) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopping.await;
            })
            .await
            .unwrap();
    });
    let matrix = tokio::spawn(async move {
        for engine in [Engine::Firefox, Engine::Webkit] {
            for mode in [Mode::Headless, Mode::Headed] {
                let root = tempfile::tempdir().unwrap();
                let private = root.path().join("state");
                crate::artifacts::directory(&private).unwrap();
                let session = Session::launch(
                    super::native::bundle().await,
                    Uuid::new_v4(),
                    Uuid::new_v4(),
                    LaunchOptions {
                        media_fixtures: Default::default(),
                        proxy: None,
                        engine,
                        mode,
                        context: ContextOptions::default(),
                    },
                    &private,
                )
                .unwrap();
                let exercising =
                    tokio::spawn(exercise(session.clone(), engine, origin.clone())).await;
                let disconnect = session.disconnect(30000).unwrap();
                assert_eq!(
                    session
                        .wait(disconnect, Instant::now() + Duration::from_secs(35))
                        .await
                        .unwrap()
                        .outcome,
                    Outcome::Succeeded
                );
                if let Err(error) = exercising {
                    let evidence = root.keep();
                    panic!("{engine:?}/{mode:?}: {error}; retained fixture evidence: {evidence:?}");
                }
            }
        }
    })
    .await;
    stop.send(()).unwrap();
    server.await.unwrap();
    matrix.unwrap();
}

pub(super) async fn js(session: &Session, page: Reference, expression: &str) -> Value {
    let observed = successful(
        session,
        Request::Evaluate {
            scope: page,
            expression: format!("async () => JSON.stringify(await ({expression})())"),
            is_function: Some(true),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await;
    serde_json::from_str(observed["value"]["value"].as_str().unwrap()).unwrap()
}
async fn configure(session: &Session, context: Reference, setting: ContextSetting) -> Value {
    successful(session, Request::ContextConfigure { context, setting }).await
}
fn media_reset() -> MediaOptions {
    MediaOptions {
        media: Some(Media::NoOverride),
        color_scheme: Some(ColorScheme::NoOverride),
        reduced_motion: Some(ReducedMotion::NoOverride),
        forced_colors: Some(ForcedColors::NoOverride),
        contrast: Some(Contrast::NoOverride),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "One ordered owned-context fixture verifies network, permission and media restoration across the same resources"
)]
async fn exercise(session: Session, engine: Engine, origin: String) {
    let startup = session
        .wait(session.startup(), Instant::now() + Duration::from_secs(45))
        .await
        .unwrap();
    assert_eq!(startup.outcome, Outcome::Succeeded);
    creation_storage_and_event_workflows(&session, &origin).await;
    let started = startup.result.unwrap();
    let context: Reference = serde_json::from_value(started["context"].clone()).unwrap();
    let page: Reference = serde_json::from_value(started["page"].clone()).unwrap();
    successful(
        &session,
        Request::PageNavigate {
            page,
            navigation: Navigation::Goto {
                url: origin.clone(),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    let sibling_context: Reference = serde_json::from_value(
        successful(
            &session,
            Request::ContextCreate {
                options: ContextOptions::default(),
            },
        )
        .await["context"]
            .clone(),
    )
    .unwrap();
    let sibling: Reference = serde_json::from_value(
        successful(
            &session,
            Request::PageCreate {
                context: sibling_context,
                select: false,
            },
        )
        .await["page"]
            .clone(),
    )
    .unwrap();
    successful(
        &session,
        Request::PageNavigate {
            page: sibling,
            navigation: Navigation::Goto {
                url: origin.clone(),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    const FETCH: &str = "async () => { try { const response=await fetch('/probe?nonce='+crypto.randomUUID(),{cache:'no-store'}); return await response.text(); } catch { return 'blocked'; } }";
    assert_eq!(js(&session, page, FETCH).await, "Acme online");
    configure(&session, context, ContextSetting::Offline { offline: true }).await;
    assert_eq!(js(&session, page, "() => navigator.onLine").await, false);
    assert_eq!(js(&session, page, FETCH).await, "blocked");
    assert_eq!(js(&session, sibling, FETCH).await, "Acme online");
    configure(
        &session,
        context,
        ContextSetting::Offline { offline: false },
    )
    .await;
    assert_eq!(js(&session, page, FETCH).await, "Acme online");
    let permission_baseline = js(
        &session,
        page,
        "async () => (await navigator.permissions.query({name:'geolocation'})).state",
    )
    .await;
    configure(
        &session,
        context,
        ContextSetting::PermissionsGrant {
            origin: origin.clone(),
            permissions: vec![Permission::Geolocation],
        },
    )
    .await;
    configure(
        &session,
        context,
        ContextSetting::Geolocation {
            geolocation: Some(Geolocation {
                latitude: 12.5,
                longitude: -34.25,
                accuracy: 5.0,
            }),
        },
    )
    .await;
    let location=js(&session,page,"() => new Promise((resolve,reject) => navigator.geolocation.getCurrentPosition(position => resolve({latitude:position.coords.latitude,longitude:position.coords.longitude,accuracy:position.coords.accuracy}),reject,{timeout:5000,maximumAge:0}))").await;
    for (field, expected) in [("latitude", 12.5), ("longitude", -34.25), ("accuracy", 5.0)] {
        assert_eq!(location[field].as_f64(), Some(expected), "{location}");
    }
    assert_eq!(
        js(
            &session,
            page,
            "async () => (await navigator.permissions.query({name:'geolocation'})).state"
        )
        .await,
        "granted"
    );
    configure(
        &session,
        context,
        ContextSetting::Geolocation { geolocation: None },
    )
    .await;
    configure(&session, context, ContextSetting::PermissionsClear {}).await;
    let cleared_in_document = js(
        &session,
        page,
        "async () => (await navigator.permissions.query({name:'geolocation'})).state",
    )
    .await;
    let fresh_permission_page: Reference = serde_json::from_value(
        successful(
            &session,
            Request::PageCreate {
                context,
                select: false,
            },
        )
        .await["page"]
            .clone(),
    )
    .unwrap();
    successful(
        &session,
        Request::PageNavigate {
            page: fresh_permission_page,
            navigation: Navigation::Goto {
                url: origin.clone(),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    let fresh_permission = js(
        &session,
        fresh_permission_page,
        "async () => (await navigator.permissions.query({name:'geolocation'})).state",
    )
    .await;
    assert_eq!(
        fresh_permission, permission_baseline,
        "immediate={cleared_in_document}; fresh document={fresh_permission}"
    );
    successful(
        &session,
        Request::PageClose {
            page: fresh_permission_page,
        },
    )
    .await;
    successful(
        &session,
        Request::PageNavigate {
            page,
            navigation: Navigation::Reload {
                wait_until: Load::Load,
            },
        },
    )
    .await;
    assert_eq!(
        js(
            &session,
            page,
            "async () => (await navigator.permissions.query({name:'geolocation'})).state"
        )
        .await,
        permission_baseline,
        "before navigation={cleared_in_document}; fresh document={fresh_permission}"
    );
    let unsupported = if engine == Engine::Firefox {
        Permission::Camera
    } else {
        Permission::PersistentStorage
    };
    let refused = execute(
        &session,
        Request::ContextConfigure {
            context,
            setting: ContextSetting::PermissionsGrant {
                origin,
                permissions: vec![unsupported],
            },
        },
        5000,
    )
    .await;
    assert_eq!(refused.code.as_deref(), Some("PERMISSION_UNSUPPORTED"));
    let baseline = successful(
        &session,
        Request::PageEmulateMedia {
            page,
            options: media_reset(),
        },
    )
    .await["observed"]
        .clone();
    let sibling_baseline = successful(
        &session,
        Request::PageEmulateMedia {
            page: sibling,
            options: media_reset(),
        },
    )
    .await["observed"]
        .clone();
    let preview: Reference = serde_json::from_value(
        successful(
            &session,
            Request::PreviewStart {
                page,
                options: Default::default(),
            },
        )
        .await["preview"]
            .clone(),
    )
    .unwrap();
    let old = super::preview_native::latest(&session, preview, 0).await;
    let recording: Reference = serde_json::from_value(
        successful(
            &session,
            Request::RecordingStart {
                page,
                options: Default::default(),
            },
        )
        .await["recording"]
            .clone(),
    )
    .unwrap();
    let emulated = successful(
        &session,
        Request::PageEmulateMedia {
            page,
            options: MediaOptions {
                media: Some(Media::Screen),
                color_scheme: Some(ColorScheme::Dark),
                reduced_motion: Some(ReducedMotion::Reduce),
                forced_colors: Some(ForcedColors::None),
                contrast: Some(Contrast::More),
            },
        },
    )
    .await;
    assert_eq!(emulated["previewRecovery"]["succeeded"], true);
    for field in ["screen", "dark", "reducedMotion", "moreContrast"] {
        assert_eq!(emulated["observed"][field], true, "{field}: {emulated}");
    }
    assert_eq!(emulated["observed"]["forcedColors"], false);
    let printed = successful(
        &session,
        Request::PageEmulateMedia {
            page,
            options: MediaOptions {
                media: Some(Media::Print),
                ..Default::default()
            },
        },
    )
    .await;
    assert_eq!(printed["observed"]["print"], true, "{printed}");
    assert_eq!(printed["observed"]["screen"], false, "{printed}");
    let forced = successful(
        &session,
        Request::PageEmulateMedia {
            page,
            options: MediaOptions {
                forced_colors: Some(ForcedColors::Active),
                ..Default::default()
            },
        },
    )
    .await;
    assert_eq!(forced["observed"]["forcedColors"], true, "{forced}");
    let fresh = super::preview_native::latest(
        &session,
        preview,
        old.metadata["seq"].as_str().unwrap().parse().unwrap(),
    )
    .await;
    assert_ne!(fresh.metadata["sourceEpoch"], old.metadata["sourceEpoch"]);
    assert_eq!(
        successful(
            &session,
            Request::PageEmulateMedia {
                page: sibling,
                options: media_reset()
            }
        )
        .await["observed"],
        sibling_baseline
    );
    assert_eq!(
        successful(
            &session,
            Request::PageEmulateMedia {
                page,
                options: media_reset()
            }
        )
        .await["observed"],
        baseline
    );
    let movie = successful(&session, Request::RecordingStop { recording }).await;
    assert_eq!(movie["status"], "partial", "{movie}");
    assert_eq!(movie["verification"]["decodable"], true, "{movie}");
    successful(&session, Request::PreviewStop { preview }).await;
    successful(
        &session,
        Request::ContextClose {
            context: sibling_context,
        },
    )
    .await;
}

async fn new_page(session: &Session, context: Reference, origin: &str) -> Reference {
    let page = serde_json::from_value(
        successful(
            session,
            Request::PageCreate {
                context,
                select: false,
            },
        )
        .await["page"]
            .clone(),
    )
    .unwrap();
    successful(
        session,
        Request::PageNavigate {
            page,
            navigation: Navigation::Goto {
                url: origin.into(),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    page
}

async fn creation_storage_and_event_workflows(session: &Session, origin: &str) {
    let context: Reference = serde_json::from_value(
        successful(
            session,
            Request::ContextCreate {
                options: ContextOptions {
                    locale: Some("en-GB".into()),
                    timezone_id: Some("UTC".into()),
                    user_agent: Some("Acme-Testrun/1.0".into()),
                    color_scheme: Some(ColorScheme::Dark),
                    reduced_motion: Some(ReducedMotion::Reduce),
                    ..Default::default()
                },
            },
        )
        .await["context"]
            .clone(),
    )
    .unwrap();
    let page = new_page(session, context, origin).await;
    let environment = js(session, page, "() => ({language:navigator.language,userAgent:navigator.userAgent,timeZone:Intl.DateTimeFormat().resolvedOptions().timeZone,dark:matchMedia('(prefers-color-scheme: dark)').matches,reducedMotion:matchMedia('(prefers-reduced-motion: reduce)').matches})").await;
    assert_eq!(
        environment,
        json!({"language":"en-GB","userAgent":"Acme-Testrun/1.0","timeZone":"UTC","dark":true,"reducedMotion":true})
    );
    js(session, page, "async () => { localStorage.setItem('acme','private-state'); document.cookie='acme=private-cookie; path=/'; const db=await new Promise((resolve,reject)=>{const r=indexedDB.open('Acme',1);r.onupgradeneeded=()=>r.result.createObjectStore('state');r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);}); await new Promise((resolve,reject)=>{const t=db.transaction('state','readwrite'); t.objectStore('state').put('private-idb','key');t.oncomplete=resolve;t.onerror=()=>reject(t.error);});db.close();return true; }").await;
    let exported = successful(
        session,
        Request::StorageExport {
            context,
            options: crate::browser::storage::Options {
                indexed_db: true,
                ..Default::default()
            },
        },
    )
    .await;
    assert!(!exported.to_string().contains("private-state"));
    assert!(!exported.to_string().contains("private-cookie"));
    assert!(!exported.to_string().contains("private-idb"));
    let snapshot = serde_json::from_value(exported["snapshot"]["reference"].clone()).unwrap();
    let restored = serde_json::from_value(
        successful(
            session,
            Request::ContextCreate {
                options: ContextOptions::default(),
            },
        )
        .await["context"]
            .clone(),
    )
    .unwrap();
    successful(
        session,
        Request::StorageApply {
            context: restored,
            snapshot,
            mode: crate::browser::storage::ApplyMode::Replace,
            install_authenticator: false,
        },
    )
    .await;
    let restored_page = new_page(session, restored, origin).await;
    let values = js(session, restored_page, "async () => { const db=await new Promise((resolve,reject)=>{const r=indexedDB.open('Acme',1);r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);});const value=await new Promise((resolve,reject)=>{const r=db.transaction('state').objectStore('state').get('key');r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);});db.close();return {local:localStorage.getItem('acme'),cookie:document.cookie,idb:value}; }").await;
    assert_eq!(values["local"], "private-state");
    assert_eq!(values["idb"], "private-idb");
    assert!(
        values["cookie"]
            .as_str()
            .unwrap()
            .split(';')
            .any(|cookie| cookie.trim() == "acme=private-cookie")
    );
    page_event_workflow(session, restored, restored_page).await;
    successful(session, Request::StorageForget { snapshot }).await;
    successful(session, Request::ContextClose { context }).await;
    successful(session, Request::ContextClose { context: restored }).await;
}

async fn page_event_workflow(session: &Session, context: Reference, page: Reference) {
    use crate::browser::activity::{Condition, Event};
    let history = successful(
        session,
        Request::EventsRead {
            context,
            cursor: None,
            limit: 100,
        },
    )
    .await;
    let after = history["latestCursor"].as_u64().unwrap();
    let created = successful(
        session,
        Request::PageCreate {
            context,
            select: false,
        },
    )
    .await;
    let created_page: Reference = serde_json::from_value(created["page"].clone()).unwrap();
    let matched = successful(
        session,
        Request::EventsWait {
            context,
            after,
            conditions: vec![Condition {
                event: Event::PageCreated,
                page: Some(created_page),
            }],
        },
    )
    .await;
    assert_eq!(matched["event"]["resource"], json!(created_page));
    let after = matched["nextCursor"].as_u64().unwrap();
    successful(session, Request::PageClose { page: created_page }).await;
    let closed = successful(
        session,
        Request::EventsWait {
            context,
            after,
            conditions: vec![Condition {
                event: Event::PageClosed,
                page: Some(created_page),
            }],
        },
    )
    .await;
    assert_eq!(closed["event"]["page"], json!(created_page));
    assert_eq!(
        js(session, page, "() => localStorage.getItem('acme')").await,
        "private-state"
    );
}
