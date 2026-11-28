//! Native CDP wire and transport fixtures over owned in-memory WebSockets.

use super::*;
#[path = "tests/retirement.rs"]
mod retirement;

#[tokio::test(start_paused = true)]
async fn native_frame_ack_is_independent_of_decoder_and_input_replies() {
    let (connection, mut events, mut peer) = fixture().await;
    connection
        .frame_lane()
        .attach("acme-page".into(), uuid::Uuid::new_v4())
        .unwrap();
    let mut frames = connection.frame_lane().claim().unwrap();
    send(&mut peer,json!({"sessionId":"acme-page","method":"Page.screencastFrame","params":{"data":"AA==","sessionId":7,"metadata":{"offsetTop":0,"pageScaleFactor":1,"deviceWidth":959,"deviceHeight":599,"scrollOffsetX":0,"scrollOffsetY":0,"timestamp":1}}})).await;
    let ack = command(&mut peer).await;
    assert_eq!(ack["method"], "Page.screencastFrameAck");
    assert_eq!(ack["params"]["sessionId"], 7);
    // Leave ACK outstanding; ordinary input must retain its own response lane.
    let input = connection.clone();
    let input = tokio::spawn(async move {
        input
            .call(
                Some("acme-page"),
                "Input.insertText",
                json!({"text":"Acme"}),
                Lane::Ordinary,
                deadline(),
            )
            .await
    });
    let typing = command(&mut peer).await;
    assert_eq!(typing["method"], "Input.insertText");
    send(
        &mut peer,
        json!({"id":typing["id"],"sessionId":"acme-page","result":{}}),
    )
    .await;
    input.await.unwrap().unwrap();
    assert!(
        events.try_next().is_none(),
        "JPEG payload entered lifecycle queue"
    );
    let frame = frames.next().await.unwrap();
    assert_eq!(frame.params["sessionId"], 7);
    tokio::time::advance(Duration::from_secs(6)).await;
    assert!(frames.next().await.is_none());
    assert_eq!(connection.frame_lane().failure(), Some("MEDIA_ACK_FAILED"));
    assert!(
        connection.status().open,
        "A media ACK failure must preserve browser input"
    );
    send(
        &mut peer,
        json!({"id":ack["id"],"sessionId":"acme-page","result":{}}),
    )
    .await;
    connection.close(deadline()).await.unwrap();
}

#[test]
fn control_input_admits_only_release_phases() {
    assert!(control_method(
        "Network.setCacheDisabled",
        &json!({"cacheDisabled":false})
    ));
    for params in [
        json!({"cacheDisabled":true}),
        json!({}),
        json!({"cacheDisabled":false,"extra":true}),
    ] {
        assert!(!control_method("Network.setCacheDisabled", &params));
    }
    for (method, allowed, refused) in [
        ("Input.dispatchKeyEvent", "keyUp", "keyDown"),
        ("Input.dispatchMouseEvent", "mouseReleased", "mousePressed"),
        ("Input.dispatchTouchEvent", "touchEnd", "touchStart"),
    ] {
        assert!(control_method(method, &json!({"type":allowed})));
        assert!(!control_method(method, &json!({"type":refused})));
        assert!(!control_method(method, &json!({})));
    }
    assert!(control_method(
        "Input.dispatchTouchEvent",
        &json!({"type":"touchCancel"})
    ));
    assert!(!control_method(
        "Runtime.evaluate",
        &json!({"expression":"1"})
    ));
}
use crate::browser::cdp::{
    Event, EventItem, MAX_COMMAND_BYTES, MAX_CRITICAL_EVENTS, MAX_EVENT_BYTES, MAX_EVENTS,
    events::Buffer, wire::Incoming,
};
use futures_util::{SinkExt, StreamExt, poll};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{
    io::DuplexStream,
    time::{Instant, timeout},
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::Role},
};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

type Peer = WebSocketStream<DuplexStream>;

async fn fixture() -> (Connection, Events, Peer) {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let client = WebSocketStream::from_raw_socket(client, Role::Client, Some(config())).await;
    let server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
    let (connection, events) = Connection::start(client);
    (connection, events, server)
}

async fn command(peer: &mut Peer) -> Value {
    let message = timeout(Duration::from_secs(5), peer.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let Message::Text(text) = message else {
        panic!("Expected CDP text command")
    };
    serde_json::from_str(text.as_str()).unwrap()
}

async fn send(peer: &mut Peer, value: Value) {
    peer.send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

fn version_result(product: &str) -> Value {
    json!({"protocolVersion":"1.3","product":product,"revision":"@acme-fixture","userAgent":"Acme QA","jsVersion":"fixture"})
}

#[tokio::test]
async fn bootstrap_verifies_exact_identity_before_enabling_discovery() {
    for (product, user_agent, mac) in [
        ("Chrome/156.0.8078.47", "Acme Windows NT 10.0", false),
        (
            "HeadlessChrome/156.0.8078.47",
            "Acme X11 Linux x86_64",
            false,
        ),
        (
            "Chrome/156.0.8078.47",
            "Acme Macintosh; Intel Mac OS X",
            true,
        ),
    ] {
        let (connection, _, mut peer) = fixture().await;
        let bootstrap = connection.clone();
        let bootstrap = tokio::spawn(async move {
            crate::browser::cdp::Bootstrap::initialize(&bootstrap, "156.0.8078.47", deadline())
                .await
        });
        let version = command(&mut peer).await;
        assert_eq!(version["method"], "Browser.getVersion");
        let mut identity = version_result(product);
        identity["userAgent"] = json!(user_agent);
        send(&mut peer, json!({"id":version["id"],"result":identity})).await;
        let discovery = command(&mut peer).await;
        assert_eq!(discovery["method"], "Target.setDiscoverTargets");
        assert_eq!(discovery["params"], json!({"discover":true}));
        send(&mut peer, json!({"id":discovery["id"],"result":{}})).await;
        assert_eq!(bootstrap.await.unwrap().unwrap().identity.product, product);
        assert_eq!(connection.uses_mac_keyboard(), mac);
        connection.close(deadline()).await.unwrap();
    }
}

#[tokio::test]
async fn bootstrap_discards_edge_pid_only_for_verified_edge_target_metadata() {
    use crate::browser::{cdp::Bootstrap, preparation::Channel};

    let build = "158.0.4370.33";
    for (channel, product, pid, accepted) in [
        (Channel::Msedge, format!("Edg/{build}"), json!(0), true),
        (Channel::Chrome, format!("Chrome/{build}"), json!(0), false),
        (Channel::Msedge, format!("Edg/{build}"), json!("0"), false),
    ] {
        let (connection, mut events, mut peer) = fixture().await;
        let bootstrap = connection.clone();
        let bootstrap = tokio::spawn(async move {
            Bootstrap::initialize_channel(&bootstrap, build, Some(channel), deadline()).await
        });
        let version = command(&mut peer).await;
        send(
            &mut peer,
            json!({"id":version["id"],"result":version_result(&product)}),
        )
        .await;
        let discovery = command(&mut peer).await;
        assert_eq!(discovery["method"], "Target.setDiscoverTargets");
        let target = json!({
            "targetId":"acme-page", "type":"page", "title":"Acme", "url":"about:blank",
            "attached":false, "canAccessOpener":false, "pid":pid,
            "futureMetadata":{"targetId":"foreign-acme"}
        });
        send(
            &mut peer,
            json!({"method":"Target.targetCreated","params":{"targetInfo":target}}),
        )
        .await;
        if !accepted {
            assert!(matches!(
                bootstrap.await.unwrap(),
                Err(Error::Lost {
                    reason: Loss::Protocol,
                    ..
                })
            ));
            continue;
        }
        send(&mut peer, json!({"id":discovery["id"],"result":{}})).await;
        assert_eq!(bootstrap.await.unwrap().unwrap().identity.product, product);
        assert!(matches!(events.try_next(), Some(EventItem::Event(event))
            if event.params["targetInfo"].get("pid").is_none()
                && event.params["targetInfo"].get("futureMetadata").is_none()));

        let list = connection.clone();
        let list = tokio::spawn(async move {
            list.call(
                None,
                "Target.getTargets",
                json!({}),
                Lane::Ordinary,
                deadline(),
            )
            .await
        });
        let request = command(&mut peer).await;
        send(
            &mut peer,
            json!({"id":request["id"],"result":{"targetInfos":[target]}}),
        )
        .await;
        let result = list.await.unwrap().unwrap();
        assert_eq!(result["targetInfos"][0]["targetId"], "acme-page");
        assert!(result["targetInfos"][0].get("pid").is_none());
        assert!(result["targetInfos"][0].get("futureMetadata").is_none());
        connection.close(deadline()).await.unwrap();
    }
}

#[tokio::test]
async fn mismatched_bootstrap_and_unmapped_commands_do_not_enable_control() {
    let (connection, _, mut peer) = fixture().await;
    assert!(matches!(
        connection
            .call(None, "Acme.unknown", json!({}), Lane::Ordinary, deadline())
            .await,
        Err(Error::Schema(_))
    ));
    assert_eq!(connection.status().pending, 0);
    let bootstrap = connection.clone();
    let bootstrap = tokio::spawn(async move {
        crate::browser::cdp::Bootstrap::initialize(&bootstrap, "156.0.8078.47", deadline()).await
    });
    let version = command(&mut peer).await;
    send(
        &mut peer,
        json!({"id":version["id"],"result":version_result("Chrome/155.0.0.0")}),
    )
    .await;
    assert!(matches!(bootstrap.await.unwrap(), Err(Error::Identity)));
    assert_eq!(connection.status().pending, 0);
    connection.close(deadline()).await.unwrap();
}

#[test]
fn endpoint_requires_explicit_numeric_loopback_browser_identity() {
    for endpoint in [
        "ws://127.0.0.1:9222/devtools/browser/acme-123",
        "ws://[::1]:49212/devtools/browser/acme-123",
    ] {
        assert!(endpoint_valid(endpoint), "{endpoint}");
    }
    for endpoint in [
        "ws://localhost:9222/devtools/browser/acme",
        "ws://example.com:9222/devtools/browser/acme",
        "ws://0.0.0.0:9222/devtools/browser/acme",
        "ws://127.0.0.1/devtools/browser/acme",
        "ws://127.0.0.1:0/devtools/browser/acme",
        "wss://127.0.0.1:9222/devtools/browser/acme",
        "ws://127.0.0.1:9222/devtools/page/acme",
        "ws://127.0.0.1:9222/devtools/browser/",
        "ws://user@127.0.0.1:9222/devtools/browser/acme",
        "ws://127.0.0.1:9222/devtools/browser/acme?token=private",
        "ws://127.0.0.1:9222/devtools/browser/acme#fragment",
    ] {
        assert!(!endpoint_valid(endpoint), "{endpoint}");
    }
}

#[test]
fn wire_preserves_session_omission_and_rejects_invalid_commands_before_encoding() {
    let browser: Value = serde_json::from_str(
        &wire::command(1, None, "Browser.getVersion", json!({}), false).unwrap(),
    )
    .unwrap();
    assert!(browser.get("sessionId").is_none());
    let scoped: Value = serde_json::from_str(
        &wire::command(2, Some("acme-session"), "Runtime.enable", json!({}), false).unwrap(),
    )
    .unwrap();
    assert_eq!(scoped["sessionId"], "acme-session");
    for (session, method, params) in [
        (Some(""), "Runtime.enable", json!({})),
        (None, "Runtime", json!({})),
        (None, "Runtime.enable.extra", json!({})),
        (None, "Runtime.enable", Value::Null),
    ] {
        assert!(matches!(
            wire::command(1, session, method, params, false),
            Err(Error::InvalidCommand)
        ));
    }
    assert!(matches!(
        wire::command(
            1,
            None,
            "Runtime.evaluate",
            json!({"expression":"x".repeat(MAX_COMMAND_BYTES)}),
            false,
        ),
        Err(Error::CommandBounds)
    ));
}

#[test]
fn fulfillment_encoding_has_a_separate_bounded_budget() {
    assert!(
        wire::command(
            1,
            Some("acme"),
            "Fetch.fulfillRequest",
            json!({"body":"A".repeat(1_398_104)}),
            false,
        )
        .is_ok()
    );
    assert!(matches!(
        wire::command(
            1,
            Some("acme"),
            "Fetch.fulfillRequest",
            json!({"body":"A".repeat(2 * MAX_COMMAND_BYTES)}),
            false,
        ),
        Err(Error::CommandBounds)
    ));
}

#[test]
fn malformed_reply_shapes_are_terminal_without_echoing_private_data() {
    for value in [
        json!([]),
        json!({"id":0,"result":{}}),
        json!({"id":1,"result":null}),
        json!({"id":1}),
        json!({"id":1,"result":{},"error":{"code":-1,"message":"private"}}),
        json!({"id":1,"result":{},"sessionId":null}),
        json!({"id":1,"error":{"message":"private"}}),
        json!({"method":"Target.targetCreated","params":[]}),
        json!({"id":1,"result":{},"unexpected":"private"}),
    ] {
        assert!(matches!(
            wire::incoming(&value.to_string()),
            Err(Loss::Protocol)
        ));
    }
    assert!(matches!(wire::incoming("{private"), Err(Loss::Protocol)));
    let Incoming::Reply { result: Err(error), .. } = wire::incoming(
        r#"{"id":1,"error":{"code":-32000,"message":"private application value","data":{"detail":"private"}}}"#
    ).unwrap() else { panic!("Expected remote error") };
    assert!(!error.to_string().contains("private application value"));
    assert!(!error.to_string().contains("detail"));
}

#[tokio::test]
async fn handshake_uses_the_supplied_stream_and_admits_no_public_endpoint_discovery() {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let accept = tokio::spawn(async move {
        let mut peer = tokio_tungstenite::accept_async(server).await.unwrap();
        let request = command(&mut peer).await;
        assert_eq!(request["method"], "Browser.getVersion");
        send(
            &mut peer,
            json!({"id":request["id"],"result":version_result("Chrome/fixture")}),
        )
        .await;
        // Wait for transport closure, so the fixture owns its entire lifetime.
        let _ = peer.next().await;
    });
    let (connection, _) = Connection::open(
        client,
        "ws://127.0.0.1:49212/devtools/browser/acme",
        deadline(),
    )
    .await
    .unwrap();
    let value = connection
        .call(
            None,
            "Browser.getVersion",
            json!({}),
            Lane::Ordinary,
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(value["product"], "Chrome/fixture");
    connection.close(deadline()).await.unwrap();
    timeout(Duration::from_secs(5), accept)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn replies_route_out_of_order_and_validate_flattened_sessions() {
    let (connection, _, mut peer) = fixture().await;
    let first = connection.clone();
    let first = tokio::spawn(async move {
        first
            .call(
                Some("page-a"),
                "Runtime.evaluate",
                json!({"expression":"'a'"}),
                Lane::Ordinary,
                deadline(),
            )
            .await
    });
    let a = command(&mut peer).await;
    let second = connection.clone();
    let second = tokio::spawn(async move {
        second
            .call(
                Some("page-b"),
                "Runtime.evaluate",
                json!({"expression":"'b'"}),
                Lane::Ordinary,
                deadline(),
            )
            .await
    });
    let b = command(&mut peer).await;
    assert_ne!(a["id"], b["id"]);
    send(
        &mut peer,
        json!({"id":b["id"],"sessionId":"page-b","result":{"result":{"type":"string","value":"b"}}}),
    )
    .await;
    send(
        &mut peer,
        json!({"id":a["id"],"sessionId":"page-a","result":{"result":{"type":"string","value":"a"}}}),
    )
    .await;
    assert_eq!(second.await.unwrap().unwrap()["result"]["value"], "b");
    assert_eq!(first.await.unwrap().unwrap()["result"]["value"], "a");
    assert_eq!(connection.status().retained_replies, 0);
    connection.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn wrong_session_and_unknown_response_ids_lose_the_connection() {
    for wrong_session in [true, false] {
        let (connection, _, mut peer) = fixture().await;
        let call = connection.clone();
        let call = tokio::spawn(async move {
            call.call(
                Some("page-a"),
                "Runtime.enable",
                json!({}),
                Lane::Ordinary,
                deadline(),
            )
            .await
        });
        let request = command(&mut peer).await;
        let id = if wrong_session {
            request["id"].clone()
        } else {
            json!(9999)
        };
        let session = if wrong_session { "page-b" } else { "page-a" };
        send(&mut peer, json!({"id":id,"sessionId":session,"result":{}})).await;
        assert!(matches!(
            call.await.unwrap(),
            Err(Error::Lost {
                reason: Loss::Protocol,
                dispatch: Dispatch::PossiblyDispatched
            })
        ));
        connection.close(deadline()).await.unwrap();
    }
}

#[tokio::test]
async fn browser_routing_error_without_session_preserves_the_request_owner_and_neighbor_calls() {
    let (connection, _, mut peer) = fixture().await;
    let call = connection.clone();
    let call = tokio::spawn(async move {
        call.call(
            Some("page-a"),
            "Runtime.releaseObjectGroup",
            json!({"objectGroup":"console"}),
            Lane::Control,
            deadline(),
        )
        .await
    });
    let request = command(&mut peer).await;
    send(
        &mut peer,
        json!({"id":request["id"],"error":{"code":-32000,"message":"Acme attachment closed"}}),
    )
    .await;
    assert!(matches!(call.await.unwrap(), Err(Error::Remote(_))));
    assert!(connection.status().open);
    let neighbor = connection.clone();
    let neighbor = tokio::spawn(async move {
        neighbor
            .call(
                Some("page-b"),
                "Runtime.enable",
                json!({}),
                Lane::Ordinary,
                deadline(),
            )
            .await
    });
    let next = command(&mut peer).await;
    assert_ne!(next["id"], request["id"]);
    assert_eq!(next["method"], "Runtime.enable");
    send(
        &mut peer,
        json!({"id":next["id"],"sessionId":"page-b","result":{}}),
    )
    .await;
    neighbor.await.unwrap().unwrap();
    connection.close(deadline()).await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn late_browser_routing_error_retains_its_original_attachment_without_replay() {
    let (connection, _, mut peer) = fixture().await;
    let call = connection.clone();
    let expires = Instant::now() + Duration::from_millis(100);
    let call = tokio::spawn(async move {
        call.call(
            Some("page-a"),
            "Runtime.releaseObjectGroup",
            json!({"objectGroup":"console"}),
            Lane::Control,
            expires,
        )
        .await
    });
    let request = command(&mut peer).await;
    tokio::time::advance(Duration::from_millis(101)).await;
    assert!(matches!(
        call.await.unwrap(),
        Err(Error::Timeout {
            dispatch: Dispatch::PossiblyDispatched
        })
    ));
    send(
        &mut peer,
        json!({"id":request["id"],"error":{"code":-32000,"message":"Acme attachment closed"}}),
    )
    .await;
    timeout(Duration::from_secs(1), async {
        while connection.status().retained_replies == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let retained = connection.take_late_reply().unwrap();
    assert_eq!(retained.session_id.as_deref(), Some("page-a"));
    assert!(retained.result.is_err());
    assert!(connection.status().open);
    assert!(
        timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    connection.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn sessionless_success_conflicting_error_and_unknown_error_id_remain_protocol_failures() {
    for (session, remote_error, known_id) in [
        (None, false, true),
        (Some("page-b"), true, true),
        (None, true, false),
    ] {
        let (connection, _, mut peer) = fixture().await;
        let call = connection.clone();
        let call = tokio::spawn(async move {
            call.call(
                Some("page-a"),
                "Runtime.enable",
                json!({}),
                Lane::Ordinary,
                deadline(),
            )
            .await
        });
        let request = command(&mut peer).await;
        let mut packet =
            json!({"id":if known_id {request["id"].clone()} else {json!(9999)},"result":{}});
        if let Some(session) = session {
            packet["sessionId"] = json!(session);
        }
        if remote_error {
            packet.as_object_mut().unwrap().remove("result");
            packet["error"] = json!({"code":-32000,"message":"Acme attachment closed"});
        }
        send(&mut peer, packet).await;
        assert!(matches!(
            call.await.unwrap(),
            Err(Error::Lost {
                reason: Loss::Protocol,
                dispatch: Dispatch::PossiblyDispatched
            })
        ));
        connection.close(deadline()).await.unwrap();
    }
}

#[tokio::test]
async fn cancelled_queued_work_is_skipped_and_ids_are_never_reused() {
    let (connection, _, mut peer) = fixture().await;
    let mut cancelled = Box::pin(connection.call(
        None,
        "Browser.getVersion",
        json!({}),
        Lane::Ordinary,
        deadline(),
    ));
    assert!(poll!(&mut cancelled).is_pending());
    drop(cancelled);
    // No await has let the writer run before cancellation.
    let survivor = connection.clone();
    let survivor = tokio::spawn(async move {
        survivor
            .call(
                None,
                "Browser.getVersion",
                json!({}),
                Lane::Ordinary,
                deadline(),
            )
            .await
    });
    let request = command(&mut peer).await;
    assert_eq!(request["id"], 2);
    send(
        &mut peer,
        json!({"id":2,"result":version_result("Chrome/fixture")}),
    )
    .await;
    survivor.await.unwrap().unwrap();
    connection.close(deadline()).await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn dispatched_timeout_retains_the_late_allocation_reply_without_replay() {
    let (connection, _, mut peer) = fixture().await;
    let call = connection.clone();
    let expires = Instant::now() + Duration::from_millis(100);
    let call = tokio::spawn(async move {
        call.call(
            None,
            "Target.createBrowserContext",
            json!({}),
            Lane::Ordinary,
            expires,
        )
        .await
    });
    let request = command(&mut peer).await;
    tokio::time::advance(Duration::from_millis(101)).await;
    assert!(matches!(
        call.await.unwrap(),
        Err(Error::Timeout {
            dispatch: Dispatch::PossiblyDispatched
        })
    ));
    assert_eq!(connection.status().pending, 1);
    send(
        &mut peer,
        json!({"id":request["id"],"result":{"browserContextId":"owned-acme"}}),
    )
    .await;
    timeout(Duration::from_secs(1), async {
        while connection.status().retained_replies == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let retained = connection.take_late_reply().unwrap();
    assert_eq!(retained.id, request["id"].as_u64().unwrap());
    assert_eq!(retained.method, "Target.createBrowserContext");
    assert_eq!(retained.result.unwrap()["browserContextId"], "owned-acme");
    assert_eq!(connection.status().pending, 0);
    assert!(connection.take_late_reply().is_none());
    connection.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn cancellation_after_reply_notification_preserves_the_unread_result() {
    let (connection, _, mut peer) = fixture().await;
    let mut call = Box::pin(connection.call(
        None,
        "Target.createBrowserContext",
        json!({}),
        Lane::Ordinary,
        deadline(),
    ));
    assert!(poll!(&mut call).is_pending());
    let request = command(&mut peer).await;
    // Deliver synchronously, then drop before the waiting future can consume it.
    connection
        .0
        .shared
        .incoming(
            &json!({"id":request["id"],"result":{"browserContextId":"owned-acme"}}).to_string(),
        )
        .unwrap();
    assert!(connection.take_late_reply().is_none());
    drop(call);
    assert_eq!(
        connection.take_late_reply().unwrap().result.unwrap()["browserContextId"],
        "owned-acme"
    );
    connection.close(deadline()).await.unwrap();
}

#[tokio::test]
async fn full_ordinary_admission_preserves_control_capacity_and_control_priority() {
    let (connection, _, mut peer) = fixture().await;
    let mut calls = Vec::new();
    for _ in 0..MAX_PENDING - CONTROL_RESERVE {
        let mut call = Box::pin(connection.call(
            None,
            "Browser.getVersion",
            json!({}),
            Lane::Ordinary,
            deadline(),
        ));
        assert!(poll!(&mut call).is_pending());
        calls.push(call);
    }
    assert!(matches!(
        connection
            .call(
                None,
                "Browser.getVersion",
                json!({}),
                Lane::Ordinary,
                deadline()
            )
            .await,
        Err(Error::Busy)
    ));
    assert!(matches!(
        connection
            .call(
                None,
                "Runtime.evaluate",
                json!({}),
                Lane::Control,
                deadline()
            )
            .await,
        Err(Error::ControlMethod)
    ));
    let mut close_target = Box::pin(connection.call(
        None,
        "Target.closeTarget",
        json!({"targetId":"acme"}),
        Lane::Control,
        deadline(),
    ));
    assert!(poll!(&mut close_target).is_pending());
    let request = command(&mut peer).await;
    assert_eq!(request["method"], "Target.closeTarget");
    send(
        &mut peer,
        json!({"id":request["id"],"result":{"success":true}}),
    )
    .await;
    close_target.await.unwrap();
    drop(calls);
    connection.close(deadline()).await.unwrap();
}

fn event(method: &str) -> Event {
    Event {
        method: method.into(),
        session_id: None,
        params: json!({}),
    }
}

#[tokio::test]
async fn diagnostics_cannot_evict_critical_events_and_report_loss() {
    let buffer = Buffer::default();
    let mut events = Events(buffer.clone());
    buffer.push(event("Target.targetDestroyed"), 64).unwrap();
    for _ in 0..MAX_EVENTS {
        buffer.push(event("Network.dataReceived"), 64).unwrap();
    }
    buffer.close();
    assert!(matches!(
        events.next().await,
        Some(EventItem::Gap { dropped: 1 })
    ));
    assert!(
        matches!(events.next().await, Some(EventItem::Event(Event { method, .. })) if method == "Target.targetDestroyed")
    );
    let mut remaining = 0;
    while events.next().await.is_some() {
        remaining += 1;
    }
    assert_eq!(remaining, MAX_EVENTS - 1);
}

#[test]
fn critical_event_overflow_and_oversize_are_explicit_connection_losses() {
    let buffer = Buffer::default();
    for _ in 0..MAX_CRITICAL_EVENTS {
        buffer.push(event("Target.targetDestroyed"), 64).unwrap();
    }
    assert_eq!(
        buffer.push(event("Target.detachedFromTarget"), 64),
        Err(Loss::EventCapacity)
    );
    let buffer = Buffer::default();
    assert_eq!(
        buffer.push(event("Fetch.requestPaused"), MAX_EVENT_BYTES + 1),
        Err(Loss::EventCapacity)
    );
    assert!(
        buffer
            .push(event("Network.dataReceived"), MAX_EVENT_BYTES + 1)
            .is_ok()
    );
}

#[tokio::test]
async fn peer_loss_and_repeated_close_report_dispatch_without_replaying_calls() {
    let (connection, mut events, mut peer) = fixture().await;
    let call = connection.clone();
    let call = tokio::spawn(async move {
        call.call(
            None,
            "Browser.getVersion",
            json!({}),
            Lane::Ordinary,
            deadline(),
        )
        .await
    });
    command(&mut peer).await;
    drop(peer);
    assert!(matches!(
        call.await.unwrap(),
        Err(Error::Lost {
            dispatch: Dispatch::PossiblyDispatched,
            ..
        })
    ));
    connection.close(deadline()).await.unwrap();
    connection.close(deadline()).await.unwrap();
    assert!(events.next().await.is_none());
    assert!(matches!(
        connection
            .call(
                None,
                "Browser.getVersion",
                json!({}),
                Lane::Ordinary,
                deadline()
            )
            .await,
        Err(Error::Lost {
            dispatch: Dispatch::NotDispatched,
            ..
        })
    ));
}

#[tokio::test]
async fn dropping_the_last_connection_handle_closes_its_owned_stream() {
    let (connection, mut events, mut peer) = fixture().await;
    drop(connection);
    timeout(Duration::from_secs(5), async {
        assert!(events.next().await.is_none());
        assert!(!matches!(peer.next().await, Some(Ok(Message::Text(_)))));
    })
    .await
    .unwrap();
}

#[tokio::test]
#[expect(
    clippy::result_large_err,
    reason = "Tungstenite requires its HTTP error response as the handshake callback's error type"
)]
async fn diagnostic_react_native_transport_has_separate_schema_and_refuses_sessions_and_control_evaluation()
 {
    let (client, server) = tokio::io::duplex(65536);
    let peer = tokio::spawn(async move {
        tokio_tungstenite::accept_hdr_async(
            server,
            |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                assert_eq!(request.headers()["origin"], "http://127.0.0.1:18081");
                Ok(response)
            },
        )
        .await
        .unwrap()
    });
    let (connection, mut events) = Connection::open_react_native(
        client,
        "ws://127.0.0.1:18081/inspector/debug?device=acme&page=1",
        deadline(),
    )
    .await
    .unwrap();
    let mut peer = peer.await.unwrap();
    for (session, method, params, lane) in [
        (Some("foreign"), "Runtime.enable", json!({}), Lane::Ordinary),
        (None, "Debugger.resume", json!({}), Lane::Ordinary),
        (
            None,
            "Runtime.evaluate",
            json!({"expression":"arbitrary()","returnByValue":true,"contextId":1}),
            Lane::Ordinary,
        ),
        (
            None,
            "Runtime.evaluate",
            json!({"expression":"1","returnByValue":true,"contextId":1}),
            Lane::Control,
        ),
    ] {
        assert!(
            connection
                .call(session, method, params, lane, deadline())
                .await
                .is_err()
        );
    }
    assert_eq!(connection.status().pending, 0);
    let call = connection.clone();
    let call = tokio::spawn(async move {
        call.call(
            None,
            "HeapProfiler.startSampling",
            json!({"samplingInterval":32768,"stackDepth":32}),
            Lane::Ordinary,
            deadline(),
        )
        .await
    });
    let request = command(&mut peer).await;
    assert_eq!(request["method"], "HeapProfiler.startSampling");
    send(&mut peer,json!({"method":"ReactNativeApplication.metadataUpdated","params":{"integrationName":"owned fixture"}})).await;
    send(&mut peer, json!({"id":request["id"],"result":{}})).await;
    call.await.unwrap().unwrap();
    assert!(
        matches!(events.next().await,Some(EventItem::Event(Event{method,..})) if method=="ReactNativeApplication.metadataUpdated")
    );
    send(
        &mut peer,
        json!({"method":"Runtime.executionContextsCleared","params":{},"sessionId":"foreign"}),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(1), async {
        while connection.status().open {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(connection.status().loss, Some(Loss::Protocol));
    connection.close(deadline()).await.unwrap();
    let (chrome, _, _) = fixture().await;
    assert!(matches!(
        chrome
            .call(
                None,
                "ReactNativeApplication.enable",
                json!({}),
                Lane::Ordinary,
                deadline()
            )
            .await,
        Err(Error::Schema(_))
    ));
    chrome.close(deadline()).await.unwrap();
}
