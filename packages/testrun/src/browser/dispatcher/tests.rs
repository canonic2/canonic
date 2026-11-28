//! Pinned wire fixtures, independent of a browser process.

use super::*;

pub(crate) fn creates() -> Vec<Value> {
    let mut messages = Vec::new();
    for name in ["chromium", "firefox", "webkit"] {
        messages.push(created(
            name,
            "BrowserType",
            json!({"name":name,"executablePath":"/private/acme/browser"}),
        ));
    }
    messages.push(created("android", "Android", json!({})));
    messages.push(created("electron", "Electron", json!({})));
    messages.push(created(
        "playwright",
        "Playwright",
        json!({
            "chromium":{"guid":"chromium"},"firefox":{"guid":"firefox"},"webkit":{"guid":"webkit"},
            "android":{"guid":"android"},"electron":{"guid":"electron"}
        }),
    ));
    messages
}

pub(crate) fn created(guid: &str, kind: &str, initializer: Value) -> Value {
    json!({"guid":"","method":"__create__","params":{"guid":guid,"type":kind,"initializer":initializer}})
}

pub(crate) fn method_error(id: impl serde::Serialize) -> Value {
    json!({"id":id,"error":{"error":{"name":"Error","message":"Acme fixture refusal"}},"log":[]})
}

fn ready() -> Dispatcher {
    let mut dispatcher = Dispatcher::default();
    let root = dispatcher.root().unwrap();
    let (id, _) = dispatcher
        .admit(&root, "initialize", json!({"sdkLanguage":"javascript"}))
        .unwrap();
    for message in creates() {
        dispatcher.dispatch(message).unwrap();
    }
    dispatcher
        .dispatch(json!({"id":id,"result":{"playwright":{"guid":"playwright"}}}))
        .unwrap();
    dispatcher
}

#[test]
fn bootstrap_validates_named_types_without_inventing_parent_relationships() {
    let dispatcher = ready();
    let bootstrap = dispatcher.bootstrap().unwrap();
    assert_eq!(bootstrap.browser_types.len(), 3);
    for name in ["chromium", "firefox", "webkit"] {
        assert_eq!(
            dispatcher
                .initializer(&bootstrap.browser_types[name])
                .unwrap()
                .unwrap()["name"],
            name
        );
    }
    assert_eq!(dispatcher.pending_count(), 0);
}

#[test]
fn admission_rejects_invalid_or_unready_work_without_consuming_ids() {
    let mut dispatcher = Dispatcher::default();
    let root = dispatcher.root().unwrap();
    assert!(matches!(
        dispatcher.admit(&root, "other", json!({})),
        Err(Error::NotReady)
    ));
    assert!(
        dispatcher
            .admit(&root, "initialize", json!({"sdkLanguage":"rust"}))
            .is_err()
    );
    let (id, wire) = dispatcher
        .admit(&root, "initialize", json!({"sdkLanguage":"javascript"}))
        .unwrap();
    assert_eq!(id, 1);
    assert_eq!(wire["id"], "1");
    assert_eq!(wire["guid"], "");
    assert_eq!(wire["metadata"], json!({}));
    assert!(matches!(
        dispatcher.admit(&root, "initialize", json!({"sdkLanguage":"javascript"})),
        Err(Error::NotReady)
    ));
}

#[test]
fn string_wire_ids_settle_the_original_owner_and_reject_noncanonical_aliases() {
    let mut dispatcher = ready();
    let browser_type = dispatcher.bootstrap().unwrap().browser_types["chromium"].clone();
    let (id, wire) = dispatcher
        .admit(&browser_type, "launch", json!({}))
        .unwrap();
    assert_eq!(wire["id"], id.to_string());
    for alias in [
        format!("0{id}"),
        format!("+{id}"),
        format!(" {id}"),
        "9007199254740992".into(),
    ] {
        assert!(matches!(
            dispatcher.dispatch(method_error(alias)),
            Err(Error::Message)
        ));
        assert_eq!(dispatcher.pending_count(), 1);
    }
    assert!(
        matches!(dispatcher.dispatch(method_error(wire["id"].clone())).unwrap(),Incoming::Reply{id:seen,..} if seen==id)
    );
    assert_eq!(dispatcher.pending_count(), 0);
    assert!(
        matches!(dispatcher.dispatch(method_error(wire["id"].clone())).unwrap(),Incoming::Event(Event::LateReply{id:seen}) if seen==id)
    );
}

#[test]
fn out_of_order_results_and_creation_before_reply_match_one_pending_call() {
    let mut dispatcher = ready();
    let browser_type = dispatcher.bootstrap().unwrap().browser_types["chromium"].clone();
    let (a, _) = dispatcher
        .admit(&browser_type, "launch", json!({}))
        .unwrap();
    let (b, _) = dispatcher
        .admit(&browser_type, "launch", json!({}))
        .unwrap();
    dispatcher
        .dispatch(created(
            "browser",
            "Browser",
            json!({"name":"chromium","browserName":"chromium","version":"156"}),
        ))
        .unwrap();
    assert!(
        matches!(dispatcher.dispatch(method_error(b)).unwrap(),Incoming::Reply { id,result:Err(_) } if id==b)
    );
    assert!(
        matches!(dispatcher.dispatch(json!({"id":a,"result":{"browser":{"guid":"browser"}}})).unwrap(),Incoming::Reply { id,result:Ok(_) } if id==a)
    );
    assert_eq!(dispatcher.pending_count(), 0);
    assert!(
        matches!(dispatcher.dispatch(method_error(a)).unwrap(),Incoming::Event(Event::LateReply {id}) if id==a)
    );
    assert!(matches!(
        dispatcher.dispatch(method_error(a + 100)),
        Err(Error::UnknownReply)
    ));
}

#[test]
fn malformed_results_events_and_initializers_fail_without_partial_publication() {
    let mut dispatcher = ready();
    assert!(
        dispatcher
            .dispatch(created("bad", "BrowserType", json!({"name":1})))
            .is_err()
    );
    assert!(dispatcher.resolve("bad", "BrowserType").is_err());
    assert!(
        dispatcher
            .dispatch(created("unknown", "InventedType", json!({})))
            .is_err()
    );
    assert!(
        dispatcher
            .dispatch(json!({"guid":"chromium","method":"unknown","params":{}}))
            .is_err()
    );
    let browser_type = dispatcher.bootstrap().unwrap().browser_types["chromium"].clone();
    let (id, _) = dispatcher
        .admit(&browser_type, "launch", json!({}))
        .unwrap();
    assert!(
        dispatcher
            .dispatch(json!({"id":id,"result":{"browser":{"guid":"chromium"}}}))
            .is_err()
    );
    assert_eq!(dispatcher.pending_count(), 1);
    dispatcher.close();
    assert!(matches!(
        dispatcher.admit(&browser_type, "launch", json!({})),
        Err(Error::Closed)
    ));
}

#[test]
fn callback_slots_remain_available_when_normal_pending_calls_are_full() {
    let mut dispatcher = ready();
    let browser_type = dispatcher.bootstrap().unwrap().browser_types["chromium"].clone();
    for _ in 0..120 {
        dispatcher
            .admit(&browser_type, "launch", json!({}))
            .unwrap();
    }
    assert!(matches!(
        dispatcher.admit(&browser_type, "launch", json!({})),
        Err(Error::Capacity)
    ));
    dispatcher
        .dispatch(created(
            "dialog",
            "Dialog",
            json!({"type":"alert","message":"Acme","defaultValue":""}),
        ))
        .unwrap();
    let dialog = dispatcher.resolve("dialog", "Dialog").unwrap();
    assert!(matches!(
        dispatcher.admit_responsive(&browser_type, "launch", json!({})),
        Err(Error::Method)
    ));
    dispatcher
        .dispatch(created(
            "har-request",
            "Request",
            json!({
                "url":"https://example.com/start","resourceType":"document","method":"GET",
                "headers":[],"isNavigationRequest":true
            }),
        ))
        .unwrap();
    dispatcher
        .dispatch(created(
            "har-route",
            "Route",
            json!({"request":{"guid":"har-request"}}),
        ))
        .unwrap();
    let route = dispatcher.resolve("har-route", "Route").unwrap();
    let (id, message) = dispatcher
        .admit_responsive(
            &route,
            "redirectNavigationRequest",
            json!({"url":"https://example.com/end"}),
        )
        .unwrap();
    assert_eq!(message["method"], "redirectNavigationRequest");
    dispatcher.dispatch(json!({"id":id,"result":{}})).unwrap();
    for _ in 0..8 {
        dispatcher
            .admit_responsive(&dialog, "dismiss", json!({}))
            .unwrap();
    }
    assert_eq!(dispatcher.pending_count(), 128);
    assert!(matches!(
        dispatcher.admit_responsive(&dialog, "dismiss", json!({})),
        Err(Error::Capacity)
    ));
}

#[test]
fn wire_adoption_and_disposal_release_initializers_and_fence_reused_guids() {
    let mut dispatcher = ready();
    dispatcher
        .dispatch(created(
            "dialog",
            "Dialog",
            json!({"type":"alert","message":"Acme","defaultValue":""}),
        ))
        .unwrap();
    let dialog = dispatcher.resolve("dialog", "Dialog").unwrap();
    dispatcher
        .dispatch(json!({"guid":"playwright","method":"__adopt__","params":{"guid":"dialog"}}))
        .unwrap();
    let before = dispatcher.initializer_bytes;
    dispatcher
        .dispatch(json!({"guid":"dialog","method":"__dispose__","params":{"reason":"gc"}}))
        .unwrap();
    assert!(dispatcher.initializer_bytes < before);
    assert!(dispatcher.initializer(&dialog).is_err());
    dispatcher
        .dispatch(created(
            "dialog",
            "Dialog",
            json!({"type":"alert","message":"Acme","defaultValue":""}),
        ))
        .unwrap();
    assert_ne!(dispatcher.resolve("dialog", "Dialog").unwrap(), dialog);
}

#[test]
fn frame_load_states_follow_validated_loadstate_events() {
    let mut dispatcher = ready();
    dispatcher
        .dispatch(created(
            "states-frame",
            "Frame",
            json!({"url":"about:blank","name":"","loadStates":["load"]}),
        ))
        .unwrap();
    let frame = dispatcher.resolve("states-frame", "Frame").unwrap();
    let states = |dispatcher: &Dispatcher| {
        dispatcher.initializer(&frame).unwrap().unwrap()["loadStates"].clone()
    };
    for event in [json!({"add":"networkidle"}), json!({"add":"networkidle"})] {
        dispatcher
            .dispatch(json!({"guid":"states-frame","method":"loadstate","params":event}))
            .unwrap();
    }
    assert_eq!(states(&dispatcher), json!(["load", "networkidle"]));
    for removed in ["load", "networkidle"] {
        dispatcher
            .dispatch(
                json!({"guid":"states-frame","method":"loadstate","params":{"remove":removed}}),
            )
            .unwrap();
    }
    assert_eq!(states(&dispatcher), json!([]));
    assert!(
        dispatcher
            .dispatch(json!({"guid":"states-frame","method":"loadstate","params":{"add":"idle"}}))
            .is_err(),
        "states outside the pinned schema are refused"
    );
}

#[test]
fn native_releases_have_reserved_capacity_but_new_down_events_do_not() {
    let mut dispatcher = ready();
    dispatcher
        .dispatch(created(
            "input-frame",
            "Frame",
            json!({"url":"about:blank","name":"","loadStates":[]}),
        ))
        .unwrap();
    dispatcher
        .dispatch(created(
            "input-page",
            "Page",
            json!({"mainFrame":{"guid":"input-frame"},"isClosed":false}),
        ))
        .unwrap();
    let page = dispatcher.resolve("input-page", "Page").unwrap();
    for _ in 0..120 {
        dispatcher
            .admit(&page, "keyboardDown", json!({"key":"Shift"}))
            .unwrap();
    }
    assert!(matches!(
        dispatcher.admit_responsive(&page, "keyboardDown", json!({"key":"Shift"})),
        Err(Error::Method)
    ));
    assert!(matches!(
        dispatcher.admit_responsive(&page, "mouseDown", json!({"button":"left"})),
        Err(Error::Method)
    ));
    dispatcher
        .admit_responsive(&page, "keyboardUp", json!({"key":"Shift"}))
        .unwrap();
    dispatcher
        .admit_responsive(&page, "mouseUp", json!({"button":"left"}))
        .unwrap();
}

#[test]
fn fetched_response_cleanup_uses_the_reserved_lane_without_granting_fetch() {
    let mut dispatcher = ready();
    dispatcher
        .dispatch(created("cleanup-tracing", "Tracing", json!({})))
        .unwrap();
    dispatcher
        .dispatch(created(
            "cleanup-request",
            "APIRequestContext",
            json!({"tracing":{"guid":"cleanup-tracing"}}),
        ))
        .unwrap();
    let request = dispatcher
        .resolve("cleanup-request", "APIRequestContext")
        .unwrap();
    assert!(matches!(
        dispatcher.admit_responsive(&request, "fetch", json!({"url":"https://example.com"})),
        Err(Error::Method)
    ));
    dispatcher
        .admit_responsive(
            &request,
            "disposeAPIResponse",
            json!({"fetchUid":"Acme-response"}),
        )
        .unwrap();
}

#[test]
fn mismatched_browser_names_cannot_complete_bootstrap() {
    let mut dispatcher = Dispatcher::default();
    let root = dispatcher.root().unwrap();
    dispatcher
        .admit(&root, "initialize", json!({"sdkLanguage":"javascript"}))
        .unwrap();
    for mut message in creates() {
        if message["params"]["guid"] == "firefox" {
            message["params"]["initializer"]["name"] = json!("chromium");
        }
        dispatcher.dispatch(message).unwrap();
    }
    assert!(matches!(
        dispatcher.dispatch(json!({"id":1,"result":{"playwright":{"guid":"playwright"}}})),
        Err(Error::Bootstrap)
    ));
    assert!(dispatcher.bootstrap().is_none());
}

#[test]
fn disposal_of_a_bootstrap_type_cannot_leave_the_connection_ready() {
    let mut dispatcher = ready();
    assert!(matches!(
        dispatcher.dispatch(json!({"guid":"firefox","method":"__dispose__","params":{}})),
        Err(Error::Bootstrap)
    ));
}

#[test]
fn optional_method_error_details_use_the_official_client_default_without_losing_the_epoch() {
    let mut dispatcher = ready();
    let id = dispatcher.next_id;
    dispatcher.next_id += 1;
    // Exercise the response boundary directly with the same method metadata
    // that a frame binding registers, without requiring a document fixture.
    dispatcher.pending.insert(
        id,
        Pending {
            kind: "Frame".into(),
            method: "expect".into(),
        },
    );
    let Incoming::Reply {
        result: Err(error), ..
    } = dispatcher.dispatch(method_error(id)).unwrap()
    else {
        panic!("Expected a method failure, not epoch loss");
    };
    assert!(error.details.is_none());
    assert!(dispatcher.bootstrap().is_some());
    assert_eq!(dispatcher.pending_count(), 0);
}
