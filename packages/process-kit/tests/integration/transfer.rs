//! Real cross-process copies; originals close before the child uses its standby inventory.

use super::fixtures;
use canonic_process_kit::{
    core::{Admission, ControlRole, Explicit},
    endpoint::Lease,
    net::{Listener, Stream},
    protocol::{Control, ProductError, Request},
    server::{self, Context, Handler},
    transfer::{Binding, Receiver, Resource, Sender},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

fn binding() -> Binding {
    Binding {
        transfer_id: "acme-transfer".into(),
        predecessor: "acme-old".into(),
        successor: "acme-new".into(),
    }
}

struct Reap(Child);
impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn echo(mut stream: impl Read + Write) {
    let mut bytes = [0; 4];
    stream.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"acme");
    stream.write_all(b"next").unwrap();
}

fn exchange(mut stream: impl Read + Write) {
    stream.write_all(b"acme").unwrap();
    let mut bytes = [0; 4];
    stream.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"next");
}

#[test]
#[ignore = "child of the cross-process resource-transfer fixture"]
fn transfer_child() {
    let path = std::env::var_os("ACME_TRANSFER_SOCKET").unwrap();
    let mut receiver = Receiver::new(
        Stream::connect(path).unwrap(),
        binding(),
        Duration::from_secs(5),
    )
    .unwrap();
    let mut inventory = Vec::new();
    for index in 0..6 {
        let (resource, metadata) = receiver.receive().unwrap();
        assert_eq!(metadata, json!({"index":index}));
        inventory.push(resource);
    }
    // Parent closes every original before allowing standby resources to run.
    let mut go = [0];
    std::io::stdin().read_exact(&mut go).unwrap();
    assert_eq!(go, [1]);
    for (index, resource) in inventory.into_iter().enumerate() {
        match resource {
            Resource::File(mut file) => {
                if index == 5 {
                    file.write_all(b"pipe").unwrap();
                    continue;
                }
                file.seek(SeekFrom::Start(0)).unwrap();
                let mut bytes = [0; 4];
                file.read_exact(&mut bytes).unwrap();
                assert_eq!(&bytes, b"acme");
                file.write_all(b"next").unwrap();
            }
            Resource::TcpListener(listener) => echo(listener.accept().unwrap().0),
            Resource::TcpStream(stream) => echo(stream),
            Resource::LocalListener(listener) => echo(listener.accept().unwrap().0),
            Resource::LocalStream(stream) => echo(stream),
        }
    }
}

#[test]
fn transfer_copies_live_resources_to_another_process_after_originals_close() {
    let (temporary, root) = fixtures::root();
    root.prepare().unwrap();
    let path = root.path().join("transfer.sock");
    let listener = Listener::bind(&path).unwrap();
    let mut child = Reap(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "transfer::transfer_child",
                "--ignored",
                "--nocapture",
            ])
            .env("ACME_TRANSFER_SOCKET", &path)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stream = listener.accept().unwrap().0;
    let mut sender = Sender::new(stream, binding(), Duration::from_secs(5)).unwrap();
    let file_path = temporary.path().join("file");
    let mut file = std::fs::File::options()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&file_path)
        .unwrap();
    file.write_all(b"acme").unwrap();
    let tcp_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = tcp_listener.local_addr().unwrap();
    let pair = TcpListener::bind("127.0.0.1:0").unwrap();
    let tcp_peer = TcpStream::connect(pair.local_addr().unwrap()).unwrap();
    tcp_peer
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let tcp_stream = pair.accept().unwrap().0;
    let local_path = root.path().join("resource.sock");
    let local_listener = Listener::bind(&local_path).unwrap();
    let (local_stream, local_peer) = Stream::pair().unwrap();
    local_peer
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let (mut pipe_reader, pipe_writer) = std::io::pipe().unwrap();
    #[cfg(unix)]
    let pipe = Resource::File(std::os::fd::OwnedFd::from(pipe_writer).into());
    #[cfg(windows)]
    let pipe = Resource::File(std::os::windows::io::OwnedHandle::from(pipe_writer).into());
    let resources = [
        Resource::File(file),
        Resource::TcpListener(tcp_listener),
        Resource::TcpStream(tcp_stream),
        Resource::LocalListener(local_listener),
        Resource::LocalStream(local_stream),
        pipe,
    ];
    for (index, resource) in resources.iter().enumerate() {
        sender.send(resource, json!({"index":index})).unwrap();
    }
    drop(resources);
    child.0.stdin.take().unwrap().write_all(&[1]).unwrap();
    let tcp = TcpStream::connect_timeout(&address, Duration::from_secs(5)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    exchange(tcp);
    exchange(tcp_peer);
    let local = Stream::connect(local_path).unwrap();
    local
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    exchange(local);
    exchange(local_peer);
    let mut pipe_bytes = [0; 4];
    pipe_reader.read_exact(&mut pipe_bytes).unwrap();
    assert_eq!(&pipe_bytes, b"pipe");
    assert!(child.0.wait().unwrap().success());
    assert_eq!(std::fs::read(file_path).unwrap(), b"acmenext");
}

#[test]
fn transfer_refuses_wrong_binding_and_bounds_metadata_before_sending() {
    let (left, right) = Stream::pair().unwrap();
    let rejected = std::thread::spawn(move || {
        let mut other = binding();
        other.successor = "acme-wrong".into();
        assert!(Receiver::new(right, other, Duration::from_secs(2)).is_err());
    });
    assert!(Sender::new(left, binding(), Duration::from_secs(2)).is_err());
    rejected.join().unwrap();
    let (left, right) = Stream::pair().unwrap();
    let receiving = std::thread::spawn(move || {
        let mut receiver = Receiver::new(right, binding(), Duration::from_secs(2)).unwrap();
        let (_, metadata) = receiver.receive().unwrap();
        assert_eq!(metadata, "valid");
    });
    let mut sender = Sender::new(left, binding(), Duration::from_secs(2)).unwrap();
    let file = Resource::File(tempfile::tempfile().unwrap());
    assert!(matches!(
        sender.send(&file, json!("x".repeat(65536))),
        Err(canonic_process_kit::Error::FrameTooLarge)
    ));
    sender.send(&file, json!("valid")).unwrap();
    receiving.join().unwrap();
}

#[test]
fn transfer_peer_loss_is_unknown_and_never_retried() {
    let (left, right) = Stream::pair().unwrap();
    let peer = std::thread::spawn(move || {
        let receiver = Receiver::new(right, binding(), Duration::from_secs(2)).unwrap();
        drop(receiver);
    });
    let mut sender = Sender::new(left, binding(), Duration::from_secs(2)).unwrap();
    peer.join().unwrap();
    let file = Resource::File(tempfile::tempfile().unwrap());
    assert!(matches!(
        sender.send(&file, Value::Null),
        Err(canonic_process_kit::Error::OutcomeUnknown(_))
    ));
    assert!(matches!(
        sender.send(&file, Value::Null),
        Err(canonic_process_kit::Error::HandoverState)
    ));
}

struct Coordinator {
    gate: Admission,
    fence: Mutex<Option<canonic_process_kit::core::Quiescence>>,
    drains: AtomicUsize,
}

impl Handler for Coordinator {
    fn call(&self, _: &Request, _: &Context<'_>) -> Result<Value, ProductError> {
        Ok(json!("work"))
    }
    fn extensions(&self) -> &[&str] {
        &["prepare", "abort", "commit"]
    }
    fn control_role(&self, _: &Control) -> ControlRole {
        ControlRole::Inspection
    }
    fn control(&self, r: &Control, _: &Context<'_>) -> Result<Value, ProductError> {
        let mut fence = self.fence.lock().unwrap();
        match r.action.as_str() {
            "prepare" => *fence = Some(self.gate.quiesce().unwrap()),
            "abort" => {
                fence.take().unwrap().abort().unwrap();
            }
            "commit" => fence.as_mut().unwrap().commit().unwrap(),
            _ => unreachable!(),
        }
        Ok(Value::Null)
    }
    fn drain(&self) {
        self.drains.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn transfer_quiescence_resumes_or_retires_the_real_driver_without_drain_cleanup() {
    let (_temporary, root) = fixtures::root();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    let gate = Admission::new();
    let handler = Arc::new(Coordinator {
        gate: gate.clone(),
        fence: Mutex::new(None),
        drains: AtomicUsize::new(0),
    });
    let serving = Arc::clone(&handler);
    let thread = std::thread::spawn(move || {
        server::serve(
            lease,
            fixtures::identity(),
            gate,
            serving,
            &Explicit,
            server::Options::default(),
        )
    });
    fixtures::ready(&root);
    for action in ["prepare", "abort", "prepare", "commit"] {
        let reply = canonic_process_kit::client::control(
            &root,
            &fixtures::control(&root, action),
            &fixtures::options(),
        )
        .unwrap();
        assert!(reply.code.is_none());
        if action == "prepare" {
            let result = canonic_process_kit::client::call(
                &root,
                &fixtures::request(Value::Null),
                &fixtures::options(),
            );
            assert!(matches!(result, Err(canonic_process_kit::Error::Draining)));
        } else if action == "abort" {
            assert!(
                canonic_process_kit::client::call(
                    &root,
                    &fixtures::request(Value::Null),
                    &fixtures::options()
                )
                .is_ok()
            );
        }
    }
    thread.join().unwrap().unwrap();
    assert_eq!(handler.drains.load(Ordering::SeqCst), 0);
    assert!(!root.leased().unwrap());
    let _successor = Lease::acquire(&root, Duration::ZERO).unwrap();
}
