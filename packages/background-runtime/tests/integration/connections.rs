//! Product-owned duplex streams, bounded handshakes and drain cleanup.

use super::fixtures::*;
use canonic_background_runtime::{
    Error, client,
    connection::{Connection, Handoff},
    core::{Admission, Explicit},
    endpoint::Lease,
    protocol::{Code, ProductError, Request},
    server::{self, Context, Handler},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Default)]
struct Sessions {
    state: Arc<Mutex<(bool, Vec<Connection>)>>,
    opened: AtomicUsize,
    resume: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}
impl Handler for Sessions {
    fn call(&self, request: &Request, _: &Context<'_>) -> Result<Value, ProductError> {
        Ok(request.payload.clone())
    }
    fn open(&self, request: &Request, _: &Context<'_>) -> Result<Handoff, ProductError> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        if request.payload == "pending" {
            self.resume.lock().unwrap().take().unwrap().recv().unwrap();
        }
        if request.payload == "refuse" {
            return Err(ProductError(json!("acme-refusal")));
        }
        let state = self.state.clone();
        let reply = if request.payload == "large" {
            json!("x".repeat(4096))
        } else {
            json!("connected")
        };
        Ok(Handoff::new(reply, move |connection| {
            let mut state = state.lock().unwrap();
            if !state.0 {
                state.1.push(connection);
            }
        }))
    }
    fn drain(&self) {
        let connections = {
            let mut state = self.state.lock().unwrap();
            state.0 = true;
            std::mem::take(&mut state.1)
        };
        drop(connections);
    }
}

fn start(
    root: &canonic_background_runtime::endpoint::Root,
    gate: &Admission,
    handler: &Arc<Sessions>,
    capacity: usize,
) -> std::thread::JoinHandle<canonic_background_runtime::Result<()>> {
    let lease = Lease::acquire(root, Duration::ZERO).unwrap();
    let gate = gate.clone();
    let handler = handler.clone();
    std::thread::spawn(move || {
        server::serve(
            lease,
            identity(),
            gate,
            handler,
            &Explicit,
            server::Options {
                workers: 1,
                capacity: 2,
                connection_capacity: capacity,
                frame_limit: 2048,
                ..Default::default()
            },
        )
    })
}

#[test]
fn duplex_handoff_frees_workers_bounds_sessions_and_drain_closes_them() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let handler = Arc::new(Sessions::default());
    let server = start(&root, &gate, &handler, 3);
    ready(&root);
    let mut clients = Vec::new();
    for _ in 0..3 {
        let (reply, stream) = client::open(&root, &request(Value::Null), &options()).unwrap();
        assert_eq!(reply.data, "connected");
        assert_eq!(stream.read_timeout().unwrap(), None);
        assert_eq!(stream.write_timeout().unwrap(), None);
        clients.push(stream);
    }
    wait(|| {
        (handler.state.lock().unwrap().1.len() == 3 && gate.snapshot().work == 0).then_some(())
    });
    assert_eq!(gate.snapshot().held, 3);
    let retirement =
        client::control(&root, &control(&root, "retire_if_empty"), &options()).unwrap();
    assert_eq!(retirement.data["retired"], false);
    assert!(matches!(
        client::open(&root, &request(Value::Null), &options()),
        Err(Error::Capacity)
    ));
    assert_eq!(handler.opened.load(Ordering::SeqCst), 3);
    assert_eq!(
        client::call(&root, &request(json!("ordinary")), &options())
            .unwrap()
            .data,
        "ordinary"
    );
    let status = client::control(&root, &control(&root, "status"), &options()).unwrap();
    assert_eq!(status.data["connections"]["transferred"], 3);
    {
        let state = handler.state.lock().unwrap();
        let mut server_stream = state.1[0].stream();
        assert_eq!(server_stream.read_timeout().unwrap(), None);
        assert_eq!(server_stream.write_timeout().unwrap(), None);
        server_stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        clients[0]
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        // Product bytes immediately after the handshake contain no extra library reply.
        server_stream.write_all(b"server-first").unwrap();
        let mut bytes = [0; 12];
        clients[0].read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"server-first");
        clients[0].write_all(b"client-reply").unwrap();
        server_stream.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"client-reply");
    }
    stop(&root, "drain");
    server.join().unwrap().unwrap();
    for mut client in clients {
        assert_eq!(client.read(&mut [0]).unwrap(), 0);
    }
    assert_eq!(gate.snapshot().held, 0);
    assert!(!root.leased().unwrap());
}

#[test]
fn handoff_refusal_and_oversize_release_slots_without_delivering_a_connection() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let handler = Arc::new(Sessions::default());
    let server = start(&root, &gate, &handler, 1);
    ready(&root);
    for (payload, expected) in [("refuse", Code::Product), ("large", Code::FrameTooLarge)] {
        let error = client::open(&root, &request(json!(payload)), &options()).unwrap_err();
        assert!(matches!(error, Error::HandoffRefused(reply) if reply.code == Some(expected)));
        wait(|| (gate.snapshot().held == 0 && gate.snapshot().work == 0).then_some(()));
        assert!(handler.state.lock().unwrap().1.is_empty());
    }
    let (_, client) = client::open(&root, &request(Value::Null), &options()).unwrap();
    wait(|| (handler.state.lock().unwrap().1.len() == 1).then_some(()));
    let owned = handler.state.lock().unwrap().1.pop().unwrap();
    let (mut stream, guard) = owned.into_parts();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    drop(client);
    assert_eq!(stream.read(&mut [0]).unwrap(), 0);
    drop(stream);
    assert_eq!(gate.snapshot().held, 1);
    assert!(matches!(
        client::open(&root, &request(Value::Null), &options()),
        Err(Error::Capacity)
    ));
    drop(guard);
    let (_, client) = client::open(&root, &request(Value::Null), &options()).unwrap();
    stop(&root, "drain");
    drop(client);
    server.join().unwrap().unwrap();
}

#[test]
fn disabled_or_missing_capability_never_dispatches_open() {
    let (_temp, root) = root();
    let gate = Admission::new();
    let handler = Arc::new(Sessions::default());
    let server = start(&root, &gate, &handler, 0);
    ready(&root);
    assert!(matches!(
        client::open(&root, &request(Value::Null), &options()),
        Err(Error::Incompatible)
    ));
    assert_eq!(handler.opened.load(Ordering::SeqCst), 0);
    stop(&root, "drain");
    server.join().unwrap().unwrap();
}

#[test]
fn failed_final_reply_never_delivers_and_a_handoff_racing_drain_is_closed() {
    use canonic_background_runtime::{endpoint, frame};
    use std::net::Shutdown;
    for draining in [false, true] {
        let (_temp, root) = root();
        let gate = Admission::new();
        let handler = Arc::new(Sessions::default());
        let (release, resume) = std::sync::mpsc::channel();
        *handler.resume.lock().unwrap() = Some(resume);
        let server = start(&root, &gate, &handler, 1);
        ready(&root);
        let mut peer = endpoint::connect(&root, false, options().io).unwrap();
        peer.set_read_timeout(Some(options().io)).unwrap();
        frame::write_json(
            &mut peer,
            &json!({"connection": request(json!("pending"))}),
            2048,
        )
        .unwrap();
        let ack: canonic_background_runtime::protocol::Reply =
            frame::read_json(&mut peer, 2048).unwrap();
        assert_eq!(ack.accepted, Some(true));
        wait(|| (handler.opened.load(Ordering::SeqCst) == 1).then_some(()));
        if draining {
            stop(&root, "drain");
            assert!(root.leased().unwrap());
            release.send(()).unwrap();
            let reply: canonic_background_runtime::protocol::Reply =
                frame::read_json(&mut peer, 2048).unwrap();
            assert_eq!(reply.code, None);
            assert_eq!(peer.read(&mut [0]).unwrap(), 0);
        } else {
            peer.shutdown(Shutdown::Both).unwrap();
            drop(peer);
            release.send(()).unwrap();
            wait(|| (gate.snapshot().held == 0 && gate.snapshot().work == 0).then_some(()));
            assert!(handler.state.lock().unwrap().1.is_empty());
            stop(&root, "drain");
        }
        server.join().unwrap().unwrap();
        assert_eq!(gate.snapshot().held, 0);
        assert!(handler.state.lock().unwrap().1.is_empty());
    }
}
