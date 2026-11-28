use super::*;

#[test]
fn http_proxy_refusal_capture_requires_complete_valid_non200_bounded_evidence() {
    let valid = b"HTTP/1.1 407 Acme refusal\r\nContent-Length: 4\r\n\r\nAcme";
    let mut oversized_body = valid.to_vec();
    oversized_body.resize(32769, b'a');
    let oversized_head =
        format!("HTTP/1.1 407 Acme\r\nX-Acme: {}\r\n\r\n", "a".repeat(16384)).into_bytes();
    for bytes in [
        b"HTTP/1.1 407 Acme\r\n".as_slice(),
        b"HTTP/1.1 200 Acme\r\n\r\n",
        b"HTTP/1.1 xxx Acme\r\n\r\n",
        b"HTTP/1.1 407 Acme\r\nInvalid Header: value\r\n\r\n",
        &oversized_body,
        &oversized_head,
    ] {
        let capture = Capture::default();
        capture.observe(bytes);
        assert!(
            capture.take().is_none(),
            "Incomplete, invalid, successful or over-bound CONNECT must not become response-only evidence"
        );
    }
    let capture = Capture::default();
    capture.observe(valid);
    capture.observe(b"HTTP/1.1 502 changed\r\n\r\n");
    let refusal = capture.take().unwrap();
    assert_eq!(refusal.response.get_ref(), valid);
    assert!(capture.take().is_none());
    capture.observe(valid);
    capture.clear();
    assert!(capture.take().is_none());
}

#[test]
fn http_proxy_refusal_debug_never_projects_captured_response_bytes() {
    let capture = Capture::default();
    capture.observe(
        b"HTTP/1.1 407 Acme private phrase\r\nSet-Cookie: Acme=private\r\n\r\nprivate body",
    );
    let refusal = capture.take().unwrap();
    assert_eq!(format!("{capture:?}"), "PrivateProxyRefusalCapture");
    assert_eq!(format!("{refusal:?}"), "PrivateProxyRefusal");
    assert!(
        !refusal.is_tls(),
        "A proxy refusal never certifies destination TLS"
    );
}

#[test]
fn http_proxy_refusal_memory_parse_keeps_the_original_deadline() {
    let capture = Capture::default();
    capture.observe(b"HTTP/1.1 502 Acme\r\nContent-Length: 0\r\n\r\n");
    let result = capture
        .take()
        .unwrap()
        .response(&ureq::http::Method::POST, std::time::Instant::now());
    assert!(matches!(
        result,
        Err(ureq::Error::Timeout(ureq::Timeout::Global))
    ));
}
