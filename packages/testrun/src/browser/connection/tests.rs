//! Real in-memory duplex pipes exercise the complete connection task and writer.

use super::*;
use crate::browser::dispatcher::{
    Event,
    tests::{created, creates, method_error},
};
use std::time::Duration;
use tokio::io::{DuplexStream, ReadHalf, WriteHalf};

pub(crate) struct Fixture {
    reader: transport::Reader<ReadHalf<DuplexStream>>,
    writer: transport::Writer<WriteHalf<DuplexStream>>,
    host_writer_dropped: Arc<AtomicBool>,
}

struct ObservedWriter {
    inner: WriteHalf<DuplexStream>,
    dropped: Arc<AtomicBool>,
}

impl AsyncWrite for ObservedWriter {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.inner).poll_write(cx, bytes)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl Drop for ObservedWriter {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}

impl Fixture {
    pub(crate) async fn request(&mut self) -> Value {
        self.request_named("request").await
    }
    async fn request_named(&mut self, stage: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(5), self.reader.read())
            .await
            .unwrap_or_else(|_| panic!("No complete driver request for {stage}"))
            .unwrap()
            .unwrap()
    }
    pub(crate) async fn send(&mut self, message: &Value) {
        self.writer.write(message).await.unwrap();
    }
}

pub(crate) async fn fixture(capacity: usize) -> (Connection, Events, Fixture) {
    let (host, peer) = tokio::io::duplex(capacity);
    let (read, write) = tokio::io::split(host);
    let host_writer_dropped = Arc::new(AtomicBool::new(false));
    let write = ObservedWriter {
        inner: write,
        dropped: host_writer_dropped.clone(),
    };
    let (peer_read, peer_write) = tokio::io::split(peer);
    let mut peer = Fixture {
        reader: transport::Reader::new(peer_read),
        writer: transport::Writer::new(peer_write),
        host_writer_dropped,
    };
    let driver = async {
        let initialize = peer.request_named("initialize").await;
        assert_eq!(initialize["params"]["sdkLanguage"], "javascript");
        for message in creates() {
            peer.send(&message).await;
        }
        peer.send(&json!({"id":initialize["id"],"result":{"playwright":{"guid":"playwright"}}}))
            .await;
        peer
    };
    let (connected, peer) = tokio::join!(
        Connection::open(read, write, super::super::DRIVER_VERSION, deadline()),
        driver
    );
    let (connection, events) = connected.unwrap();
    assert!(connection.status().ready);
    (connection, events, peer)
}

#[tokio::test]
async fn server_attachment_uses_websocket_frames_and_disconnect_drops_only_transport() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::{
        WebSocketStream,
        tungstenite::{Message, protocol::Role},
    };

    let (wire, peer) = tokio::io::duplex(4096);
    let wire: Box<dyn crate::browser::attachment::SocketIo> = Box::new(wire);
    let socket = WebSocketStream::from_raw_socket(wire, Role::Client, None).await;
    let mut peer = WebSocketStream::from_raw_socket(peer, Role::Server, None).await;
    let server = async {
        let initialize = peer.next().await.unwrap().unwrap().into_text().unwrap();
        let initialize: Value = serde_json::from_str(&initialize).unwrap();
        assert_eq!(initialize["method"], "initialize");
        for message in creates() {
            peer.send(Message::Text(message.to_string().into()))
                .await
                .unwrap();
        }
        peer.send(Message::Text(
            json!({"id":initialize["id"],"result":{"playwright":{"guid":"playwright"}}})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    };
    let (client, ()) = tokio::join!(Connection::open_server(socket, deadline()), server);
    client.unwrap().0.close().await.unwrap();
    let message = tokio::time::timeout_at(deadline(), peer.next())
        .await
        .unwrap();
    assert!(
        !matches!(message, Some(Ok(Message::Text(_)))),
        "{message:?}"
    );
}

async fn created_event(events: &mut Events, expected: &str) -> ObjectRef {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Item::Event(Event::Created { object, kind })) = events.next().await
                && kind == expected
            {
                return object;
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn a_pending_operation_does_not_block_events_queries_or_dialog_response() {
    let (connection, mut events, mut peer) = fixture(4096).await;
    let browser_type = connection.bootstrap().unwrap().browser_types["chromium"].clone();
    let mut launch = Box::pin(connection.call(&browser_type, "launch", json!({}), deadline()));
    assert!(futures_util::poll!(launch.as_mut()).is_pending());
    let request = peer.request().await;
    peer.send(&created(
        "dialog",
        "Dialog",
        json!({"type":"alert","message":"Acme","defaultValue":""}),
    ))
    .await;
    let dialog = created_event(&mut events, "Dialog").await;
    assert_eq!(connection.status().pending, 1);
    assert_eq!(
        connection.initializer(&dialog).await.unwrap().unwrap()["message"],
        "Acme"
    );
    let mut response = Box::pin(connection.respond(&dialog, "accept", json!({}), deadline()));
    assert!(futures_util::poll!(response.as_mut()).is_pending());
    let accept = peer.request().await;
    assert_eq!(accept["method"], "accept");
    peer.send(&json!({"id":accept["id"],"result":{}})).await;
    assert_eq!(response.await.unwrap(), Some(json!({})));
    assert!(futures_util::poll!(launch.as_mut()).is_pending());
    peer.send(&method_error(request["id"].clone())).await;
    assert!(matches!(launch.await, Err(Error::Remote(_))));
    connection.close().await.unwrap();
}

#[tokio::test]
async fn queued_cancellation_never_writes_the_cancelled_command() {
    let (connection, _events, mut peer) = fixture(8).await;
    let browser_type = connection.bootstrap().unwrap().browser_types["chromium"].clone();
    let mut first =
        Box::pin(connection.call(&browser_type, "launch", json!({"slowMo":111}), deadline()));
    assert!(futures_util::poll!(first.as_mut()).is_pending());
    let mut cancelled =
        Box::pin(connection.call(&browser_type, "launch", json!({"slowMo":222}), deadline()));
    assert!(futures_util::poll!(cancelled.as_mut()).is_pending());
    drop(cancelled);
    let mut third =
        Box::pin(connection.call(&browser_type, "launch", json!({"slowMo":333}), deadline()));
    assert!(futures_util::poll!(third.as_mut()).is_pending());
    let a = peer.request_named("first launch").await;
    let c = peer.request_named("third launch after cancellation").await;
    assert_eq!(a["params"]["slowMo"], 111);
    assert_eq!(c["params"]["slowMo"], 333);
    assert!(a["metadata"]["timeout"].as_u64().unwrap() > 0);
    peer.send(&method_error(c["id"].clone())).await;
    peer.send(&method_error(a["id"].clone())).await;
    assert!(matches!(first.await, Err(Error::Remote(_))));
    assert!(matches!(third.await, Err(Error::Remote(_))));
    connection.close().await.unwrap();
}

#[tokio::test]
async fn abandoned_dispatched_calls_keep_ownership_until_a_terminal_reply() {
    let (connection, mut events, mut peer) = fixture(4096).await;
    let browser_type = connection.bootstrap().unwrap().browser_types["chromium"].clone();
    let mut call = Box::pin(connection.call(&browser_type, "launch", json!({}), deadline()));
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    let request = peer.request().await;
    let id = request["id"].as_str().unwrap().parse::<u64>().unwrap();
    drop(call);
    assert_eq!(connection.status().pending, 1);
    peer.send(&method_error(id)).await;
    loop {
        if let Some(Item::Settled {
            id: seen,
            succeeded: false,
        }) = events.next().await
            && seen == id
        {
            break;
        }
    }
    assert_eq!(connection.status().pending, 0);
    // Duplicate retired replies become diagnostics and cannot settle a new call.
    let mut next = Box::pin(connection.call(&browser_type, "launch", json!({}), deadline()));
    assert!(futures_util::poll!(next.as_mut()).is_pending());
    let current = peer.request().await;
    peer.send(&method_error(id)).await;
    peer.send(&method_error(current["id"].clone())).await;
    assert!(matches!(next.await, Err(Error::Remote(_))));
    connection.close().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn timeout_preserves_dispatched_ownership_and_later_terminal_evidence() {
    let (connection, mut events, mut peer) = fixture(4096).await;
    let browser_type = connection.bootstrap().unwrap().browser_types["chromium"].clone();
    let mut call = Box::pin(connection.call(
        &browser_type,
        "launch",
        json!({}),
        Instant::now() + Duration::from_secs(10),
    ));
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    let request = peer.request().await;
    tokio::time::advance(Duration::from_secs(11)).await;
    assert!(matches!(
        call.await,
        Err(Error::Timeout {
            dispatch: Dispatch::PossiblyDispatched
        })
    ));
    assert_eq!(connection.status().pending, 1);
    peer.send(&method_error(request["id"].clone())).await;
    loop {
        if matches!(
            events.next().await,
            Some(Item::Settled {
                succeeded: false,
                ..
            })
        ) {
            break;
        }
    }
    assert_eq!(connection.status().pending, 0);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn driver_loss_and_malformed_messages_fail_waiters_without_replay() {
    for malformed in [false, true] {
        let (connection, _events, mut peer) = fixture(4096).await;
        let browser_type = connection.bootstrap().unwrap().browser_types["chromium"].clone();
        let mut call = Box::pin(connection.call(&browser_type, "launch", json!({}), deadline()));
        assert!(futures_util::poll!(call.as_mut()).is_pending());
        let _ = peer.request().await;
        if malformed {
            peer.send(&json!({"id":9999,"result":{}})).await;
        } else {
            drop(peer);
        }
        assert!(matches!(
            call.await,
            Err(Error::Lost {
                dispatch: Dispatch::PossiblyDispatched
            })
        ));
        assert!(!connection.status().ready);
        assert_eq!(
            connection.status().loss,
            Some(if malformed { Loss::Protocol } else { Loss::Eof })
        );
        assert!(connection.bootstrap().is_err());
        assert!(matches!(
            connection
                .call(&browser_type, "launch", json!({}), deadline())
                .await,
            Err(Error::Lost {
                dispatch: Dispatch::NotDispatched
            })
        ));
    }
}

#[tokio::test]
async fn slow_event_consumers_do_not_block_replies_and_loss_is_explicit() {
    let (connection, mut events, mut peer) = fixture(65536).await;
    peer.send(&created(
        "browser",
        "Browser",
        json!({"version":"156","name":"chromium","browserName":"chromium"}),
    ))
    .await;
    let browser = created_event(&mut events, "Browser").await;
    for _ in 0..300 {
        peer.send(&json!({"guid":"browser","method":"close","params":{}}))
            .await;
    }
    let mut call = Box::pin(connection.call(&browser, "close", json!({}), deadline()));
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    let request = peer.request().await;
    peer.send(&json!({"id":request["id"],"result":{}})).await;
    assert!(call.await.is_ok());
    assert!(matches!(
        events.next().await,
        Some(Item::Gap { dropped: 44 })
    ));
    connection.close().await.unwrap();
}

#[tokio::test]
async fn incompatible_version_and_expired_calls_refuse_before_dispatch() {
    let (a, b) = tokio::io::duplex(4096);
    assert!(matches!(
        Connection::open(a, b, "wrong", deadline()).await,
        Err(Error::Version)
    ));
    let (connection, _events, mut peer) = fixture(4096).await;
    let browser_type = connection.bootstrap().unwrap().browser_types["chromium"].clone();
    assert!(matches!(
        connection
            .call(&browser_type, "launch", json!({}), Instant::now())
            .await,
        Err(Error::Timeout {
            dispatch: Dispatch::NotDispatched
        })
    ));
    assert!(matches!(
        connection
            .call(
                &browser_type,
                "launch",
                json!({}),
                Instant::now() + Duration::from_secs(901)
            )
            .await,
        Err(Error::Bounds(_))
    ));
    let mut valid = Box::pin(connection.call(&browser_type, "launch", json!({}), deadline()));
    assert!(futures_util::poll!(valid.as_mut()).is_pending());
    let request = peer.request().await;
    assert_eq!(request["id"], "2");
    peer.send(&method_error(2)).await;
    assert!(matches!(valid.await, Err(Error::Remote(_))));
    connection.close().await.unwrap();
}

#[tokio::test]
async fn shutdown_stops_the_writer_before_returning_dispatch_knowledge() {
    let (connection, _events, peer) = fixture(8).await;
    let browser_type = connection.bootstrap().unwrap().browser_types["chromium"].clone();
    let mut first = Box::pin(connection.call(&browser_type, "launch", json!({}), deadline()));
    assert!(futures_util::poll!(first.as_mut()).is_pending());
    // The tiny pipe holds the first frame in the writer while the second call
    // remains queued. Wait on admission rather than a scheduling delay.
    let mut second = Box::pin(connection.call(&browser_type, "launch", json!({}), deadline()));
    assert!(futures_util::poll!(second.as_mut()).is_pending());
    tokio::time::timeout(Duration::from_secs(5), async {
        while connection.status().pending != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut closing = Box::pin(connection.close());
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    assert!(matches!(first.await, Err(Error::Lost { .. })));
    assert!(peer.host_writer_dropped.load(Ordering::Acquire));
    assert!(matches!(
        second.await,
        Err(Error::Lost {
            dispatch: Dispatch::NotDispatched
        })
    ));
    closing.await.unwrap();
}

#[tokio::test]
async fn waiter_expiry_during_a_partial_frame_preserves_framing_and_late_reply_ownership() {
    let (connection, _events, mut peer) = fixture(8).await;
    let browser_type = connection.bootstrap().unwrap().browser_types["chromium"].clone();
    let expires = Instant::now() + Duration::from_millis(100);
    let mut call = Box::pin(connection.call(&browser_type, "launch", json!({}), expires));
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    tokio::time::sleep_until(expires + Duration::from_millis(20)).await;
    assert!(matches!(
        call.await,
        Err(Error::Timeout {
            dispatch: Dispatch::PossiblyDispatched
        })
    ));
    let request = peer.request().await;
    assert_eq!(request["method"], "launch");
    peer.send(&method_error(request["id"].clone())).await;
    let mut next = Box::pin(connection.call(&browser_type, "launch", json!({}), deadline()));
    assert!(futures_util::poll!(next.as_mut()).is_pending());
    let request = peer.request().await;
    peer.send(&method_error(request["id"].clone())).await;
    assert!(matches!(next.await, Err(Error::Remote(_))));
    assert!(connection.status().ready);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn interrupted_and_concurrent_close_requests_share_terminal_lane_shutdown() {
    let (connection, _events, _peer) = fixture(4096).await;
    let mut closing = Box::pin(connection.close());
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    drop(closing);
    let (first, second) = tokio::join!(connection.close(), connection.close());
    first.unwrap();
    second.unwrap();
    assert!(connection.0.shared.stopped.load(Ordering::Acquire));
    assert!(!connection.status().ready);
    assert!(connection.status().loss.is_some());
    connection.close().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn retained_workflow_receives_the_real_result_after_its_dispatch_deadline() {
    let (connection, _events, mut peer) = fixture(4096).await;
    let browser_type = connection.bootstrap().unwrap().browser_types["firefox"].clone();
    let mut call = Box::pin(connection.retained(
        &browser_type,
        "launch",
        json!({}),
        Instant::now() + Duration::from_secs(10),
        false,
    ));
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    let request = peer.request().await;
    tokio::time::advance(Duration::from_secs(11)).await;
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    peer.send(&created(
        "late-browser",
        "Browser",
        json!({"version":"157.0","name":"firefox","browserName":"firefox"}),
    ))
    .await;
    peer.send(&json!({"id":request["id"],"result":{"browser":{"guid":"late-browser"}}}))
        .await;
    assert_eq!(
        call.await.unwrap().unwrap()["browser"]["guid"],
        "late-browser"
    );
    connection.close().await.unwrap();
}
#[tokio::test]
async fn media_acknowledgements_do_not_wait_for_a_frame_consumer_or_fill_lifecycle_events() {
    let (connection, mut events, mut peer) = fixture(65536).await;
    peer.send(&created(
        "frame",
        "Frame",
        json!({"url":"about:blank","name":"","loadStates":[]}),
    ))
    .await;
    peer.send(&created(
        "page",
        "Page",
        json!({"mainFrame":{"guid":"frame"},"isClosed":false}),
    ))
    .await;
    loop {
        if let Some(Item::Event(Event::Created { kind, .. })) = events.next().await
            && kind == "Page"
        {
            break;
        }
    }
    let page = connection.resolve("page", "Page").await.unwrap();
    let lane = connection.frame_lane();
    lane.attach(page, uuid::Uuid::new_v4()).unwrap();
    let mut frames = lane.claim().unwrap();
    peer.send(&json!({"guid":"page","method":"screencastFrame","params":{"frameId":1,"data":"YQ==","timestamp":0,"viewportWidth":16,"viewportHeight":16}})).await;
    let acknowledgement = peer.request().await;
    assert_eq!(acknowledgement["method"], "screencastFrameAck");
    assert_eq!(acknowledgement["params"], json!({"frameId":1}));
    peer.send(&json!({"id":acknowledgement["id"],"result":{}}))
        .await;
    let raw = frames.next().await.unwrap();
    assert_eq!(raw.params["frameId"], 1);
    let barrier = tokio::spawn({
        let connection = connection.clone();
        async move { connection.event_barrier(deadline()).await }
    });
    match events.next().await {
        Some(Item::Barrier(reply)) => {
            let _ = reply.send(());
        }
        item => panic!("Frame/ACK leaked into lifecycle events: {item:?}"),
    }
    barrier.await.unwrap().unwrap();
    connection.close().await.unwrap();
    assert!(frames.next().await.is_none());
}

#[tokio::test]
async fn frame_acks_leave_responsive_capacity_and_defer_without_losing_the_media_lane() {
    let (connection, mut events, mut peer) = fixture(65536).await;
    for index in 0..6 {
        peer.send(&created(
            &format!("frame-{index}"),
            "Frame",
            json!({"url":"about:blank","name":"","loadStates":[]}),
        ))
        .await;
        peer.send(&created(
            &format!("page-{index}"),
            "Page",
            json!({"mainFrame":{"guid":format!("frame-{index}")},"isClosed":false}),
        ))
        .await;
    }
    let mut pages = 0;
    while pages < 6 {
        if let Some(Item::Event(Event::Created { kind, .. })) = events.next().await
            && kind == "Page"
        {
            pages += 1;
        }
    }
    for index in 0..6 {
        peer.send(&json!({"guid":format!("page-{index}"),"method":"screencastFrame","params":{"frameId":1,"data":"YQ==","timestamp":0,"viewportWidth":16,"viewportHeight":16}})).await;
    }
    let mut acknowledgements = Vec::new();
    for _ in 0..4 {
        let call = peer.request().await;
        assert_eq!(call["method"], "screencastFrameAck");
        acknowledgements.push(call);
    }
    let frame = connection.resolve("frame-0", "Frame").await.unwrap();
    let query = tokio::spawn({
        let connection = connection.clone();
        async move {
            connection
                .call(&frame, "queryCount", json!({"selector":"body"}), deadline())
                .await
        }
    });
    let normal = peer.request().await;
    assert_eq!(
        normal["method"], "queryCount",
        "ACKs consumed all responsive capacity"
    );
    peer.send(&json!({"id":normal["id"],"result":{"value":1}}))
        .await;
    assert!(query.await.unwrap().is_ok());
    for index in 0..2 {
        peer.send(&json!({"id":acknowledgements[index]["id"],"result":{}}))
            .await;
        let call = peer.request().await;
        assert_eq!(call["method"], "screencastFrameAck");
        acknowledgements.push(call);
    }
    for call in &acknowledgements[2..] {
        peer.send(&json!({"id":call["id"],"result":{}})).await;
    }
    assert!(connection.frame_lane().failure().is_none());
    connection.close().await.unwrap();
}
