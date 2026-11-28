//! Native context creation options, environment setters, media and throttling
//! follow the pinned SDK's Chromium frame-session calls on owned attachments.
use super::investigation::owned_page;
use super::*;

fn second_page_info() -> Value {
    json!({"targetId":"page-two","type":"page","title":"Acme","url":"about:blank","attached":false,"canAccessOpener":false,"browserContextId":"context-acme"})
}

/// Serves a second page allocation up to its emulation, returning each
/// emulation command in order until `Target.setAutoAttach`.
async fn serve_second_page(peer: &mut Peer) -> Vec<Value> {
    let info = second_page_info();
    let create = command(peer, "Target.createTarget").await;
    send(
        peer,
        json!({"method":"Target.targetCreated","params":{"targetInfo":info}}),
    )
    .await;
    reply(peer, &create, json!({"targetId":"page-two"})).await;
    let get = command(peer, "Target.getTargetInfo").await;
    reply(peer, &get, json!({"targetInfo":info})).await;
    let attach = command(peer, "Target.attachToTarget").await;
    reply(peer, &attach, json!({"sessionId":"session-two"})).await;
    let enable = command(peer, "Page.enable").await;
    send(peer, json!({"method":"Target.attachedToTarget","params":{"sessionId":"session-two","targetInfo":info,"waitingForDebugger":false}})).await;
    reply(peer, &enable, json!({})).await;
    let tree = command(peer, "Page.getFrameTree").await;
    reply(peer, &tree, json!({"frameTree":{"frame":{"id":"frame-two","loaderId":"loader-two","url":"about:blank","domainAndRegistry":"","securityOrigin":"://","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}})).await;
    let enable = command(peer, "Runtime.enable").await;
    send(peer, json!({"method":"Runtime.executionContextCreated","sessionId":"session-two","params":{"context":{"id":2,"origin":"://","name":"","uniqueId":"world-two","auxData":{"frameId":"frame-two","isDefault":true}}}})).await;
    reply(peer, &enable, json!({})).await;
    let mut commands = Vec::new();
    loop {
        let message = tokio::time::timeout_at(deadline(), peer.next())
            .await
            .expect("emulation command")
            .unwrap()
            .unwrap();
        let Message::Text(message) = message else {
            panic!("Expected CDP command");
        };
        let value: Value = serde_json::from_str(&message).unwrap();
        if value["method"] == "Target.setAutoAttach" {
            reply(peer, &value, json!({})).await;
            break;
        }
        assert_eq!(value["sessionId"], "session-two");
        let result = if value["method"] == "Page.addScriptToEvaluateOnNewDocument" {
            json!({"identifier":"acme-block"})
        } else {
            json!({})
        };
        reply(peer, &value, result).await;
        commands.push(value);
    }
    let initialization = command(peer, "Page.addScriptToEvaluateOnNewDocument").await;
    reply(peer, &initialization, json!({"identifier":"acme-two"})).await;
    let chooser = command(peer, "Page.setInterceptFileChooserDialog").await;
    reply(peer, &chooser, json!({})).await;
    commands
}

async fn execute(mut worker: Worker, request: Value) -> (Worker, Result<Value, Error>) {
    let request = serde_json::from_value(request).unwrap();
    let result = worker
        .execute_one(Uuid::new_v4(), request, deadline())
        .await;
    (worker, result)
}

#[tokio::test]
async fn creation_options_apply_the_pinned_chromium_page_overrides_in_order() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    let options: ContextOptions = serde_json::from_value(json!({"viewport":{"width":390,"height":664},"screen":{"width":390,"height":844},"deviceScaleFactor":3,"isMobile":true,"userAgent":"Mozilla/5.0 (Linux; Android 14) Acme","locale":"de-DE","timezoneId":"Europe/Berlin","javaScriptEnabled":false,"bypassCSP":true,"ignoreHTTPSErrors":true,"offline":true,"colorScheme":"dark","geolocation":{"latitude":52.5,"longitude":13.4},"serviceWorkers":"block"})).unwrap();
    worker.context_layout.insert(context, options);
    let task = tokio::spawn(execute(
        worker,
        json!({"operation":"page_create","context":context}),
    ));
    let commands = serve_second_page(&mut peer).await;
    let (_worker, result) = task.await.unwrap();
    result.unwrap();
    let methods = commands
        .iter()
        .map(|command| command["method"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        [
            "Security.setIgnoreCertificateErrors",
            "Emulation.setDeviceMetricsOverride",
            "Emulation.setFocusEmulationEnabled",
            "Page.setBypassCSP",
            "Emulation.setScriptExecutionDisabled",
            "Emulation.setUserAgentOverride",
            "Emulation.setLocaleOverride",
            "Emulation.setTimezoneOverride",
            "Emulation.setGeolocationOverride",
            "Emulation.setEmulatedMedia",
            "Network.enable",
            "Network.emulateNetworkConditions",
            "Page.addScriptToEvaluateOnNewDocument",
        ]
    );
    assert_eq!(
        commands[1]["params"],
        json!({"mobile":true,"width":390,"height":664,"screenWidth":390,"screenHeight":844,"deviceScaleFactor":3.0,"screenOrientation":{"angle":0,"type":"portraitPrimary"}})
    );
    assert_eq!(commands[2]["params"], json!({"enabled":true}));
    assert_eq!(
        commands[5]["params"],
        json!({"userAgent":"Mozilla/5.0 (Linux; Android 14) Acme","acceptLanguage":"de-DE","userAgentMetadata":{"mobile":true,"model":"","architecture":"arm","platform":"Android","platformVersion":"14"}})
    );
    assert_eq!(
        commands[8]["params"],
        json!({"latitude":52.5,"longitude":13.4,"accuracy":0.0})
    );
    assert_eq!(
        commands[9]["params"]["features"][0],
        json!({"name":"prefers-color-scheme","value":"dark"})
    );
    assert_eq!(
        commands[11]["params"],
        json!({"offline":true,"latency":0,"downloadThroughput":-1,"uploadThroughput":-1})
    );
    assert_eq!(
        commands[12]["params"]["source"],
        crate::browser::settings::SERVICE_WORKER_BLOCK
    );
    assert!(commands[12]["params"].get("worldName").is_none());
}

/// A popup is discovered paused by browser-level auto-attach rather than
/// created; it is emulated before it resumes, and its locale override already
/// active in the opener's renderer is kept as the pinned `emulateLocale` does.
#[tokio::test]
async fn paused_popup_is_emulated_before_resume_and_keeps_a_shared_locale() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    worker.startup.enabled = true;
    worker.context_layout.insert(
        context,
        ContextOptions {
            locale: Some("de-DE".into()),
            ..ContextOptions::default()
        },
    );
    let info = json!({"targetId":"page-popup","type":"page","title":"Acme","url":"about:blank","attached":true,"canAccessOpener":true,"openerId":"page-acme","browserContextId":"context-acme"});
    worker
        .apply(EventItem::Event(Event {
            method: "Target.attachedToTarget".into(),
            session_id: None,
            params: json!({"sessionId":"session-popup","targetInfo":info,"waitingForDebugger":true}),
        }))
        .unwrap();
    let task = tokio::spawn(async move {
        worker.reconcile(Uuid::new_v4(), deadline()).await.unwrap();
        worker
    });
    let mut methods = Vec::new();
    loop {
        let message = tokio::time::timeout_at(deadline(), peer.next())
            .await
            .expect("popup setup command")
            .unwrap()
            .unwrap();
        let Message::Text(message) = message else {
            panic!("Expected CDP command");
        };
        let value: Value = serde_json::from_str(&message).unwrap();
        assert_eq!(value["sessionId"], "session-popup", "{value}");
        let method = value["method"].as_str().unwrap().to_owned();
        match method.as_str() {
            "Page.getFrameTree" => reply(&mut peer, &value, json!({"frameTree":{"frame":{"id":"frame-popup","loaderId":"loader-popup","url":"about:blank","domainAndRegistry":"","securityOrigin":"://","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}})).await,
            "Page.addScriptToEvaluateOnNewDocument" => {
                reply(&mut peer, &value, json!({"identifier":"acme-popup"})).await;
            }
            "Emulation.setLocaleOverride" => {
                send(&mut peer, json!({"id":value["id"],"sessionId":"session-popup","error":{"code":-32000,"message":"Another locale override is already in effect"}})).await;
            }
            _ => reply(&mut peer, &value, json!({})).await,
        }
        methods.push(method.clone());
        if method == "Runtime.runIfWaitingForDebugger" {
            break;
        }
    }
    let worker = task.await.unwrap();
    let position = |name: &str| {
        methods
            .iter()
            .position(|method| method == name)
            .unwrap_or_else(|| panic!("{name} missing from {methods:?}"))
    };
    let resume = position("Runtime.runIfWaitingForDebugger");
    for override_method in [
        "Emulation.setFocusEmulationEnabled",
        "Emulation.setUserAgentOverride",
        "Emulation.setLocaleOverride",
        "Emulation.setEmulatedMedia",
    ] {
        assert!(position(override_method) < resume, "{methods:?}");
    }
    assert!(worker.index.target_reference("page-popup").is_some());
    assert_eq!(worker.startup_summary()["pausedTargets"], 0);
}

#[tokio::test]
async fn default_context_sends_only_the_sdk_media_baseline_and_metrics() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    worker
        .context_layout
        .insert(context, ContextOptions::default());
    let task = tokio::spawn(execute(
        worker,
        json!({"operation":"page_create","context":context}),
    ));
    let commands = serve_second_page(&mut peer).await;
    task.await.unwrap().1.unwrap();
    assert_eq!(commands.len(), 3);
    assert_eq!(commands[0]["params"]["mobile"], false);
    assert_eq!(commands[1]["method"], "Emulation.setFocusEmulationEnabled");
    assert_eq!(commands[2]["method"], "Emulation.setEmulatedMedia");
    assert_eq!(commands[2]["params"]["media"], "");
    assert_eq!(commands[2]["params"]["features"][0]["value"], "light");
}

#[tokio::test]
async fn environment_setters_store_options_and_apply_to_existing_attachments() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    worker
        .context_layout
        .insert(context, ContextOptions::default());
    let task = tokio::spawn(execute(
        worker,
        json!({"operation":"context_configure","context":context,"setting":{"kind":"offline","offline":true}}),
    ));
    let enable = command(&mut peer, "Network.enable").await;
    assert_eq!(enable["sessionId"], "session-acme");
    reply(&mut peer, &enable, json!({})).await;
    let offline = command(&mut peer, "Network.emulateNetworkConditions").await;
    assert_eq!(offline["params"]["offline"], true);
    reply(&mut peer, &offline, json!({})).await;
    let (worker, result) = task.await.unwrap();
    let receipt = result.unwrap();
    assert_eq!(receipt["configuredAttachments"], 1);
    assert_eq!(receipt["backend"], "chrome_cdp");
    assert_eq!(worker.context_layout[&context].offline, Some(true));

    let task = tokio::spawn(execute(
        worker,
        json!({"operation":"page_emulate_throttling","page":page,"options":{"network":"slow-3g","cpuRate":4}}),
    ));
    let enable = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let throttled = command(&mut peer, "Network.emulateNetworkConditions").await;
    assert_eq!(
        throttled["params"],
        json!({"offline":true,"latency":2000.0,"downloadThroughput":50000.0,"uploadThroughput":50000.0})
    );
    reply(&mut peer, &throttled, json!({})).await;
    let cpu = command(&mut peer, "Emulation.setCPUThrottlingRate").await;
    assert_eq!(cpu["params"]["rate"], 4.0);
    reply(&mut peer, &cpu, json!({})).await;
    let (worker, result) = task.await.unwrap();
    assert_eq!(
        result.unwrap()["effective"],
        json!({"cpuRate":4.0,"network":"slow-3g"})
    );

    // Clearing context offline keeps the page's own preset.
    let task = tokio::spawn(execute(
        worker,
        json!({"operation":"context_configure","context":context,"setting":{"kind":"offline","offline":false}}),
    ));
    let enable = command(&mut peer, "Network.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let online = command(&mut peer, "Network.emulateNetworkConditions").await;
    assert_eq!(online["params"]["offline"], false);
    assert_eq!(online["params"]["latency"], 2000.0);
    reply(&mut peer, &online, json!({})).await;
    let (worker, result) = task.await.unwrap();
    result.unwrap();

    let task = tokio::spawn(execute(
        worker,
        json!({"operation":"context_configure","context":context,"setting":{"kind":"geolocation","geolocation":null}}),
    ));
    let geolocation = command(&mut peer, "Emulation.setGeolocationOverride").await;
    assert_eq!(geolocation["params"], json!({}));
    reply(&mut peer, &geolocation, json!({})).await;
    task.await.unwrap().1.unwrap();
}

#[tokio::test]
async fn permission_grants_accumulate_per_normalized_origin_and_clear_resets_them() {
    let (worker, mut peer, _root, context, _page) = owned_page().await;
    let task = tokio::spawn(execute(
        worker,
        json!({"operation":"context_configure","context":context,"setting":{"kind":"permissions_grant","origin":"https://EXAMPLE.com:443/","permissions":["camera"]}}),
    ));
    let grant = command(&mut peer, "Browser.grantPermissions").await;
    assert_eq!(
        grant["params"],
        json!({"browserContextId":"context-acme","origin":"https://example.com","permissions":["videoCapture"]})
    );
    reply(&mut peer, &grant, json!({})).await;
    let (worker, result) = task.await.unwrap();
    result.unwrap();
    let task = tokio::spawn(execute(
        worker,
        json!({"operation":"context_configure","context":context,"setting":{"kind":"permissions_grant","origin":"https://example.com","permissions":["geolocation","local-network-access"]}}),
    ));
    let grant = command(&mut peer, "Browser.grantPermissions").await;
    assert_eq!(
        grant["params"]["permissions"],
        json!([
            "geolocation",
            "localNetworkAccess",
            "localNetwork",
            "loopbackNetwork",
            "videoCapture"
        ])
    );
    send(
        &mut peer,
        json!({"id":grant["id"],"error":{"code":-32602,"message":"Unknown permission type"}}),
    )
    .await;
    let fallback = command(&mut peer, "Browser.grantPermissions").await;
    assert_eq!(
        fallback["params"]["permissions"],
        json!(["geolocation", "localNetworkAccess", "videoCapture"])
    );
    reply(&mut peer, &fallback, json!({})).await;
    let (worker, result) = task.await.unwrap();
    result.unwrap();
    let task = tokio::spawn(execute(
        worker,
        json!({"operation":"context_configure","context":context,"setting":{"kind":"permissions_clear"}}),
    ));
    let reset = command(&mut peer, "Browser.resetPermissions").await;
    assert_eq!(reset["params"], json!({"browserContextId":"context-acme"}));
    reply(&mut peer, &reset, json!({})).await;
    let (worker, result) = task.await.unwrap();
    result.unwrap();
    assert!(!worker.emulation.granted.contains_key(&context));
}

#[test]
fn unsupported_chrome_permissions_and_throttling_bounds_refuse_before_dispatch() {
    let reference = |value: Value| serde_json::from_value::<Request>(value).unwrap();
    let context = json!({"connectionId":Uuid::nil(),"id":Uuid::nil(),"generation":Uuid::nil()});
    assert_eq!(
        supported(&reference(json!({"operation":"context_configure","context":context,"setting":{"kind":"permissions_grant","origin":"https://example.com","permissions":["push"]}}))).unwrap_err().code,
        "PERMISSION_UNSUPPORTED"
    );
    assert_eq!(
        supported(&reference(
            json!({"operation":"page_emulate_throttling","page":context,"options":{"cpuRate":21}})
        ))
        .unwrap_err()
        .code,
        "INVALID_THROTTLING_OPTIONS"
    );
    assert_eq!(
        supported(&reference(
            json!({"operation":"page_emulate_throttling","page":context,"options":{}})
        ))
        .unwrap_err()
        .code,
        "EMPTY_THROTTLING_OPTIONS"
    );
}

#[tokio::test]
async fn invalid_timezone_maps_the_pinned_message() {
    let (mut worker, mut peer, _root, context, _page) = owned_page().await;
    worker.context_layout.insert(
        context,
        ContextOptions {
            timezone_id: Some("Acme/Nowhere".into()),
            ..ContextOptions::default()
        },
    );
    let task = tokio::spawn(execute(
        worker,
        json!({"operation":"page_create","context":context}),
    ));
    let info = second_page_info();
    let create = command(&mut peer, "Target.createTarget").await;
    send(
        &mut peer,
        json!({"method":"Target.targetCreated","params":{"targetInfo":info}}),
    )
    .await;
    reply(&mut peer, &create, json!({"targetId":"page-two"})).await;
    let get = command(&mut peer, "Target.getTargetInfo").await;
    reply(&mut peer, &get, json!({"targetInfo":info})).await;
    let attach = command(&mut peer, "Target.attachToTarget").await;
    reply(&mut peer, &attach, json!({"sessionId":"session-two"})).await;
    let enable = command(&mut peer, "Page.enable").await;
    send(&mut peer, json!({"method":"Target.attachedToTarget","params":{"sessionId":"session-two","targetInfo":info,"waitingForDebugger":false}})).await;
    reply(&mut peer, &enable, json!({})).await;
    let tree = command(&mut peer, "Page.getFrameTree").await;
    reply(&mut peer, &tree, json!({"frameTree":{"frame":{"id":"frame-two","loaderId":"loader-two","url":"about:blank","domainAndRegistry":"","securityOrigin":"://","mimeType":"text/html","secureContextType":"Secure","crossOriginIsolatedContextType":"NotIsolated","gatedAPIFeatures":[]}}})).await;
    let enable = command(&mut peer, "Runtime.enable").await;
    reply(&mut peer, &enable, json!({})).await;
    let metrics = command(&mut peer, "Emulation.setDeviceMetricsOverride").await;
    reply(&mut peer, &metrics, json!({})).await;
    let focus = command(&mut peer, "Emulation.setFocusEmulationEnabled").await;
    reply(&mut peer, &focus, json!({})).await;
    let timezone = command(&mut peer, "Emulation.setTimezoneOverride").await;
    send(&mut peer, json!({"id":timezone["id"],"sessionId":"session-two","error":{"code":-32000,"message":"Invalid timezone ID"}})).await;
    let (_worker, result) = task.await.unwrap();
    assert_eq!(result.unwrap_err().code, "INVALID_TIMEZONE");
}

#[tokio::test]
async fn page_media_merges_observes_and_rolls_back_failed_dispatch() {
    let (mut worker, mut peer, _root, context, page) = owned_page().await;
    worker
        .context_layout
        .insert(context, ContextOptions::default());
    let task = tokio::spawn(execute(
        worker,
        json!({"operation":"page_emulate_media","page":page,"options":{"colorScheme":"dark"}}),
    ));
    let world = command(&mut peer, "Page.createIsolatedWorld").await;
    send(&mut peer, json!({"method":"Runtime.executionContextCreated","sessionId":"session-acme","params":{"context":{"id":41,"origin":"://","name":page::WORLD,"uniqueId":"Acme-isolated","auxData":{"frameId":"frame-acme","isDefault":false}}}})).await;
    reply(&mut peer, &world, json!({"executionContextId":41})).await;
    let media = command(&mut peer, "Emulation.setEmulatedMedia").await;
    assert_eq!(media["params"]["features"][0]["value"], "dark");
    send(&mut peer, json!({"id":media["id"],"sessionId":"session-acme","error":{"code":-32000,"message":"Acme failure"}})).await;
    let restored = command(&mut peer, "Emulation.setEmulatedMedia").await;
    assert_eq!(restored["params"]["features"][0]["value"], "light");
    reply(&mut peer, &restored, json!({})).await;
    let (worker, result) = task.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "BROWSER_METHOD_FAILED");
    let partial = error.partial.unwrap();
    assert_eq!(partial["rolledBack"], true);
    assert_eq!(partial["restoredAttachments"], 1);
    assert_eq!(partial["stateMayHaveChanged"], false);
    assert!(!worker.emulation.page_media.contains_key(&page));

    let task = tokio::spawn(execute(
        worker,
        json!({"operation":"page_emulate_media","page":page,"options":{"media":"print"}}),
    ));
    let world = command(&mut peer, "Page.createIsolatedWorld").await;
    send(&mut peer, json!({"method":"Runtime.executionContextCreated","sessionId":"session-acme","params":{"context":{"id":42,"origin":"://","name":page::WORLD,"uniqueId":"Acme-isolated-two","auxData":{"frameId":"frame-acme","isDefault":false}}}})).await;
    reply(&mut peer, &world, json!({"executionContextId":42})).await;
    let media = command(&mut peer, "Emulation.setEmulatedMedia").await;
    assert_eq!(media["params"]["media"], "print");
    reply(&mut peer, &media, json!({})).await;
    let observed = command(&mut peer, "Runtime.evaluate").await;
    assert_eq!(observed["params"]["uniqueContextId"], "Acme-isolated-two");
    reply(
        &mut peer,
        &observed,
        json!({"result":{"type":"object","value":{"print":true,"screen":false}}}),
    )
    .await;
    let (_worker, result) = task.await.unwrap();
    let receipt = result.unwrap();
    assert_eq!(receipt["observed"]["print"], true);
    assert_eq!(receipt["source"], "chrome-cdp-media-emulation");
}
