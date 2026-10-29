//! HTTPS acquisition as Shield configures it: TLS validation, no redirects,
//! the artifact size bound, UTF-8 text and the request deadline.
use super::*;
use canonic_registry::{
    digest,
    testing::{Response, Server},
};

const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The release `path` of a signed source at `base`, expected to hold `body`.
fn release(base: &str, path: &str, body: &[u8]) -> (Source, Entry) {
    let source = Source::signed(base, KEY, "acme").expect("source");
    let entry = Entry {
        id: "acme/test".parse().expect("id"),
        version: "1.0.0".parse().expect("version"),
        path: path.into(),
        sha256: digest::sha256_hex(body),
    };
    (source, entry)
}

fn read(fetcher: &Fetcher, base: &str, path: &str, body: &[u8]) -> Result<String, Error> {
    let (source, entry) = release(base, path, body);
    artifact(fetcher, &source, &entry)
}

#[test]
fn unsupported_urls_and_artifact_paths_refuse_before_acquisition() {
    for url in [
        "http://example.com/",
        "https://user@example.com/",
        "https://example.com/a/../",
        "https://example.com/%2e%2e/",
        "https://example.com/?secret=x",
        "https://example.com/#x",
        "https://example.com//a/",
        "https://example.com/a\\b/",
        "https:///a/",
        "https://example.com:65536/",
        "https://example.com:/",
        "https://example.com:0/",
        "https://example.com:invalid/",
    ] {
        assert!(enrollment(&fetcher(), url).is_err(), "{url}");
    }
    for path in [
        "../pack.yml",
        "/pack.yml",
        "https://other.example/pack",
        "%2fpack",
        "a?x",
        "a#x",
        "a\\b",
    ] {
        assert!(matches!(
            read(
                &Server::fetcher(Duration::from_millis(1)),
                "https://example.com/",
                path,
                b""
            ),
            Err(Error::Invalid("invalid registry artifact path"))
        ));
    }
}

#[test]
fn https_validates_certificates_and_never_follows_redirects() {
    let server = Server::new();
    server.set("catalog.json", Response::ok(b"exact catalog".to_vec()));
    let trusted = Server::fetcher(Duration::from_secs(2));
    assert_eq!(
        read(&trusted, server.url(), "catalog.json", b"exact catalog").expect("trusted TLS"),
        "exact catalog"
    );
    assert!(read(&fetcher(), server.url(), "catalog.json", b"exact catalog").is_err());
    assert!(
        read(
            &trusted,
            &server.url().replace("localhost", "127.0.0.1"),
            "catalog.json",
            b"exact catalog"
        )
        .is_err()
    );
    server.set(
        "catalog.json",
        Response {
            status: 302,
            ..Response::ok(b"redirect".to_vec())
        },
    );
    assert!(read(&trusted, server.url(), "catalog.json", b"redirect").is_err());
    assert_eq!(server.requests(), vec!["/registry/catalog.json"; 2]);
}

#[test]
fn responses_are_bounded_unencoded_utf8_and_successful() {
    let server = Server::new();
    let trusted = Server::fetcher(Duration::from_secs(2));
    let limit = usize::try_from(MAX_HOSTED_ARTIFACT).expect("fixture byte limit");
    let largest = vec![b'x'; limit];
    server.set("pack.yml", Response::ok(largest.clone()));
    assert_eq!(
        read(&trusted, server.url(), "pack.yml", &largest)
            .expect("maximum-sized response")
            .len(),
        limit
    );
    for response in [
        Response::ok(vec![b'x'; limit + 1]),
        Response::ok(vec![0xff]),
        Response {
            encoding: true,
            ..Response::ok(b"raw".to_vec())
        },
        Response {
            status: 404,
            ..Response::ok(b"missing".to_vec())
        },
    ] {
        let body = response.body.clone();
        server.set("pack.yml", response);
        assert!(read(&trusted, server.url(), "pack.yml", &body).is_err());
    }
}

#[test]
fn delayed_response_expires_and_owned_fixture_joins() {
    let server = Server::new();
    server.set(
        "catalog.json",
        Response {
            delay: Duration::from_millis(300),
            ..Response::ok(b"late".to_vec())
        },
    );
    let started = std::time::Instant::now();
    assert!(
        read(
            &Server::fetcher(Duration::from_millis(75)),
            server.url(),
            "catalog.json",
            b"late"
        )
        .is_err()
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}
