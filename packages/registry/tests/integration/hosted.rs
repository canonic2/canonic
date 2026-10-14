//! HTTPS transport: TLS validation, redirects, response bounds and deadlines.
use canonic_registry::{
    acquisition::{AcquisitionError, Fetcher, MAX_CATALOG_BYTES},
    source::Source,
    testing::{Response, Server, SigningKey},
};
use std::time::Duration;

fn source(server: &Server) -> Source {
    Source::signed(server.url(), &SigningKey::generate().public_hex(), "acme")
        .expect("fixture source")
}

#[test]
fn https_validates_certificates_and_never_follows_redirects() {
    let server = Server::new();
    server.set("catalog.json", Response::ok(b"exact catalog".to_vec()));
    let trusted = Server::fetcher(Duration::from_secs(2));
    assert_eq!(
        trusted
            .read(&source(&server), "catalog.json", 1024)
            .unwrap(),
        b"exact catalog"
    );
    assert!(matches!(
        Fetcher::new(Duration::from_secs(2)).read(&source(&server), "catalog.json", 1024),
        Err(AcquisitionError::Transport)
    ));
    let by_address = Source::signed(
        &server.url().replace("localhost", "127.0.0.1"),
        &SigningKey::generate().public_hex(),
        "acme",
    )
    .unwrap();
    assert!(matches!(
        trusted.read(&by_address, "catalog.json", 1024),
        Err(AcquisitionError::Transport)
    ));
    server.set(
        "catalog.json",
        Response {
            status: 302,
            ..Response::ok(b"redirect".to_vec())
        },
    );
    assert!(matches!(
        trusted.read(&source(&server), "catalog.json", 1024),
        Err(AcquisitionError::Response)
    ));
    assert_eq!(server.requests(), vec!["/registry/catalog.json"; 2]);
}

#[test]
fn responses_are_bounded_unencoded_and_successful() {
    let server = Server::new();
    let trusted = Server::fetcher(Duration::from_secs(2));
    let limit = usize::try_from(MAX_CATALOG_BYTES).unwrap();
    server.set("catalog.json", Response::ok(vec![b'x'; limit]));
    assert_eq!(
        trusted
            .read(&source(&server), "catalog.json", MAX_CATALOG_BYTES)
            .unwrap()
            .len(),
        limit
    );
    let status = |status| Response {
        status,
        ..Response::ok(b"refused".to_vec())
    };
    for (response, refused_as_too_large) in [
        (Response::ok(vec![b'x'; limit + 1]), true),
        (
            Response {
                encoding: true,
                ..Response::ok(b"raw".to_vec())
            },
            false,
        ),
        (status(204), false),
        (status(404), false),
        (status(500), false),
    ] {
        server.set("catalog.json", response);
        let result = trusted.read(&source(&server), "catalog.json", MAX_CATALOG_BYTES);
        if refused_as_too_large {
            assert!(matches!(result, Err(AcquisitionError::TooLarge)));
        } else {
            assert!(
                matches!(result, Err(AcquisitionError::Response)),
                "{result:?}"
            );
        }
    }
    server.set("artifact", Response::ok(vec![0xff, 0x00]));
    assert_eq!(
        trusted.read(&source(&server), "artifact", 2).unwrap(),
        [0xff, 0x00],
        "artifacts are bytes, not text"
    );
}

#[test]
fn unsafe_paths_refuse_before_any_request() {
    let server = Server::new();
    let trusted = Server::fetcher(Duration::from_secs(2));
    for path in ["../catalog.json", "/catalog.json", "a?x", "a#x", "%2fx"] {
        assert!(matches!(
            trusted.read(&source(&server), path, 1024),
            Err(AcquisitionError::Path)
        ));
    }
    assert_eq!(server.requests(), Vec::<String>::new());
}

#[test]
fn a_body_cut_short_is_a_transport_failure_not_a_size_violation() {
    let server = Server::new();
    let trusted = Server::fetcher(Duration::from_secs(2));
    // The connection closes after 5 of the 1000 announced bytes.
    server.set(
        "catalog.json",
        Response {
            declared_length: Some(1000),
            ..Response::ok(b"short".to_vec())
        },
    );
    assert!(matches!(
        trusted.read(&source(&server), "catalog.json", MAX_CATALOG_BYTES),
        Err(AcquisitionError::Transport)
    ));
    // Announcing more than the limit is still a size violation once read past it.
    server.set(
        "catalog.json",
        Response {
            declared_length: Some(64),
            ..Response::ok(vec![b'x'; 64])
        },
    );
    assert!(matches!(
        trusted.read(&source(&server), "catalog.json", 16),
        Err(AcquisitionError::TooLarge)
    ));
}

#[test]
fn a_partial_tls_record_cannot_hold_a_connection_past_the_deadline() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };
    // A plain TCP host that never completes a TLS record: after the ClientHello
    // it announces a 4096-byte handshake record and drips one byte every 100 ms.
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let port = listener.local_addr().expect("address").port();
    let stop = Arc::new(AtomicBool::new(false));
    let host = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let Ok((mut socket, _)) = listener.accept() else {
                return;
            };
            let mut hello = [0; 4096];
            let _ = socket.read(&mut hello);
            let _ = socket.write_all(&[0x16, 0x03, 0x03, 0x10, 0x00]);
            while !stop.load(Ordering::Relaxed) && socket.write_all(&[0]).is_ok() {
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };
    let source = Source::signed(
        &format!("https://localhost:{port}/registry/"),
        &SigningKey::generate().public_hex(),
        "acme",
    )
    .expect("fixture source");
    let started = std::time::Instant::now();
    // The overall deadline is far away: the three-second setup deadline must end it.
    let result = Server::fetcher(Duration::from_secs(30)).read(&source, "catalog.json", 1024);
    let elapsed = started.elapsed();
    stop.store(true, Ordering::Relaxed);
    host.join().expect("host joined");
    assert!(
        matches!(result, Err(AcquisitionError::Transport)),
        "{result:?}"
    );
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
}

#[test]
fn an_established_connection_may_take_longer_than_connection_setup() {
    // Three seconds of trickled body, after a prompt handshake, within a longer deadline.
    let server = Server::new();
    server.set(
        "catalog.json",
        Response {
            trickle: Some(Duration::from_millis(100)),
            ..Response::ok(vec![b'x'; 40])
        },
    );
    assert_eq!(
        Server::fetcher(Duration::from_secs(20))
            .read(&source(&server), "catalog.json", 1024)
            .unwrap(),
        vec![b'x'; 40]
    );
}

#[test]
fn hosts_cannot_keep_a_body_read_alive_past_the_deadline() {
    let deadline = Duration::from_millis(500);
    let server = Server::new();
    // Gapless chunked trailers produce no body, so the body limit never triggers,
    // and no read waits long enough to time out on its own.
    server.set(
        "catalog.json",
        Response {
            endless_trailers: true,
            ..Response::ok(Vec::new())
        },
    );
    let started = std::time::Instant::now();
    assert!(matches!(
        Server::fetcher(deadline).read(&source(&server), "catalog.json", 1),
        Err(AcquisitionError::Transport)
    ));
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );

    // A body trickled one byte every 100 ms would take 20 s in full.
    server.set(
        "catalog.json",
        Response {
            trickle: Some(Duration::from_millis(100)),
            ..Response::ok(vec![b'x'; 200])
        },
    );
    let started = std::time::Instant::now();
    assert!(matches!(
        Server::fetcher(deadline).read(&source(&server), "catalog.json", MAX_CATALOG_BYTES),
        Err(AcquisitionError::Transport)
    ));
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn the_longest_deadline_reads_without_overflowing() {
    let server = Server::new();
    server.set("catalog.json", Response::ok(b"exact catalog".to_vec()));
    // The HTTP client adds the deadline to the current instant unchecked.
    assert_eq!(
        Server::fetcher(Duration::MAX)
            .read(&source(&server), "catalog.json", 1024)
            .unwrap(),
        b"exact catalog"
    );
}

#[test]
fn delayed_responses_expire_within_the_deadline() {
    let server = Server::new();
    server.set(
        "catalog.json",
        Response {
            delay: Duration::from_millis(300),
            ..Response::ok(b"late".to_vec())
        },
    );
    let started = std::time::Instant::now();
    assert!(matches!(
        Server::fetcher(Duration::from_millis(75)).read(&source(&server), "catalog.json", 1024),
        Err(AcquisitionError::Transport)
    ));
    assert!(started.elapsed() < Duration::from_secs(2));
}
