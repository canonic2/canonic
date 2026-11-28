//! TLS peers witness identity selection, redirect boundaries and explicit absence.
use super::*;
use crate::http_fixtures::certificates as fixture;

async fn identity(contexts: &mut Contexts, context: Reference, url: &str) -> String {
    let id = contexts
        .start(context, fetch(url), Instant::now() + Duration::from_secs(5))
        .unwrap();
    let reply = match ready(contexts, id).await.unwrap() {
        Ready::Reply(reply) => reply,
        Ready::Complete(value) => panic!("Expected actual TLS response: {value}"),
    };
    let body = String::from_utf8(reply.body.clone().unwrap()).unwrap();
    contexts.restore(id, reply);
    contexts.discard(id).unwrap();
    body
}
fn create(contexts: &mut Contexts, connection: Uuid, certificates: Value) -> Reference {
    let options: Options =
        serde_json::from_value(json!({"ignoreHttpsErrors":true,"clientCertificates":certificates}))
            .unwrap();
    serde_json::from_value(contexts.create(connection, options).unwrap()["httpContext"].clone())
        .unwrap()
}
#[tokio::test]
async fn client_certificate_same_origin_wire_format_priority_matches_the_pinned_peer() {
    let keys = fixture::Identities::new().await;
    let site = fixture::Site::start(&keys).await;
    let a = keys.input(&site.url, 0);
    let b = keys.input(&site.url, 1);
    let mut contexts = Contexts::default();
    let connection = Uuid::new_v4();
    for (input, expected) in [
        (json!([a, b]), "B"),
        (json!([b, a]), "A"),
        (json!([a, b, a]), "A"),
        (json!([keys.encrypted_input(&site.url, 0, true), b]), "A"),
    ] {
        let client = create(&mut contexts, connection, input);
        assert_eq!(identity(&mut contexts, client, &site.url).await, expected);
        contexts.remove(client).unwrap();
    }
    assert_eq!(site.requests.lock().unwrap().len(), 4);
    contexts.close().unwrap();
}
#[tokio::test]
async fn client_certificate_same_origin_wire_key_types_ignore_authority_hints_as_pinned() {
    let keys = fixture::Identities::different_algorithms().await;
    let mut contexts = Contexts::default();
    let connection = Uuid::new_v4();
    for (site, expected) in [
        (fixture::Site::start(&keys).await, "B"),
        (fixture::Site::start_with_hint(&keys, Some(0)).await, "B"),
        (fixture::Site::start_with_hint(&keys, Some(1)).await, "B"),
        (fixture::Site::start_rsa_only(&keys).await, "A"),
        (fixture::Site::start_signature_order(&keys, true).await, "A"),
        (
            fixture::Site::start_signature_order(&keys, false).await,
            "B",
        ),
    ] {
        let a = keys.input(&site.url, 0);
        let b = keys.input(&site.url, 1);
        for input in [
            json!([a, b]),
            json!([b, a]),
            json!([keys.encrypted_input(&site.url, 0, true), b]),
        ] {
            let client = create(&mut contexts, connection, input);
            assert_eq!(identity(&mut contexts, client, &site.url).await, expected);
            contexts.remove(client).unwrap();
        }
        assert_eq!(site.requests.lock().unwrap().len(), 3);
    }
    contexts.close().unwrap();
}
#[tokio::test]
async fn client_certificate_same_origin_resolver_selects_only_a_supported_signature() {
    use rustls::client::ResolvesClientCert as _;
    let keys = fixture::Identities::different_algorithms().await;
    let input: Vec<ClientCertificate> = serde_json::from_value(json!([
        keys.input("https://example.com", 0),
        keys.input("https://example.com", 1)
    ]))
    .unwrap();
    let prepared = crate::browser::http::client_certificates::prepare(&input).unwrap();
    let resolver = &prepared[0].resolver;
    for (scheme, index) in [
        (rustls::SignatureScheme::RSA_PSS_SHA256, 0),
        (rustls::SignatureScheme::ECDSA_NISTP256_SHA256, 1),
    ] {
        let selected = resolver
            .resolve(&[b"Acme authority hint"], &[scheme])
            .unwrap();
        let expected = ureq::tls::Certificate::from_pem(keys.certs[index].as_bytes()).unwrap();
        assert_eq!(selected.cert[0].as_ref(), expected.der());
        assert!(selected.key.choose_scheme(&[scheme]).is_some());
    }
    assert!(resolver.resolve(&[], &[]).is_none());
}
#[tokio::test]
async fn client_certificate_tls_reselects_exact_origin_on_every_redirect_without_identity_leak() {
    let keys = fixture::Identities::new().await;
    let first = fixture::Site::start(&keys).await;
    let second = fixture::Site::start(&keys).await;
    first.redirect_to(&format!("{}/destination", second.url));
    let connection = Uuid::new_v4();
    let mut contexts = Contexts::default();
    let context = create(
        &mut contexts,
        connection,
        json!([
            keys.input(&format!("{}/Acme-normalized-path", first.url), 0),
            keys.input(&second.url, 1)
        ]),
    );
    assert_eq!(identity(&mut contexts, context, &first.url).await, "A");
    assert_eq!(identity(&mut contexts, context, &second.url).await, "B");
    assert_eq!(
        identity(&mut contexts, context, &format!("{}/redirect", first.url)).await,
        "B"
    );
    let only_first = create(
        &mut contexts,
        connection,
        json!([keys.input(&first.url, 0)]),
    );
    assert_eq!(
        identity(
            &mut contexts,
            only_first,
            &format!("{}/redirect", first.url)
        )
        .await,
        "none"
    );
    assert!(
        first
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|request| request["identity"] == "A")
    );
    assert_eq!(
        second
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request["identity"].clone())
            .collect::<Vec<_>>(),
        vec![json!("B"), json!("B"), json!("none")]
    );
    let listed = contexts.list().to_string();
    assert!(!listed.contains("BEGIN"));
    assert!(!listed.contains(&keys.key));
    contexts.close().unwrap();
}
#[tokio::test]
async fn client_certificate_tls_inherited_identity_and_empty_override_keep_separate_connections() {
    let keys = fixture::Identities::new().await;
    let site = fixture::Site::start(&keys).await;
    let client = serde_json::from_value(keys.input(&site.url, 0)).unwrap();
    let browser = crate::browser::contract::ContextOptions {
        client_certificates: Some(vec![client]),
        ..Default::default()
    };
    let mut contexts = Contexts::default();
    let connection = Uuid::new_v4();
    for explicit in [None, Some(vec![])] {
        let expected = if explicit.is_none() { "A" } else { "none" };
        let mut options = Options {
            ignore_https_errors: Some(true),
            client_certificates: explicit,
            ..Default::default()
        };
        options.inherit(&browser);
        let context = serde_json::from_value(
            contexts.create(connection, options).unwrap()["httpContext"].clone(),
        )
        .unwrap();
        assert_eq!(identity(&mut contexts, context, &site.url).await, expected);
        assert_eq!(identity(&mut contexts, context, &site.url).await, expected);
        let before = site.requests.lock().unwrap().len();
        let id = contexts
            .start(
                context,
                Fetch {
                    ignore_https_errors: Some(false),
                    ..fetch(&site.url)
                },
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let error = ready(&mut contexts, id)
            .await
            .err()
            .expect("A client identity cannot disable server verification");
        assert_eq!(error.partial.as_ref().unwrap()["failureKind"], "tls");
        assert_eq!(site.requests.lock().unwrap().len(), before);
        assert_eq!(identity(&mut contexts, context, &site.url).await, expected);
    }
    contexts.close().unwrap();
}

#[tokio::test]
async fn client_certificate_tls_encrypted_formats_reselect_on_redirect_and_keep_verification() {
    let keys = fixture::Identities::new().await;
    let first = fixture::Site::start(&keys).await;
    let second = fixture::Site::start(&keys).await;
    first.redirect_to(&second.url);
    let mut contexts = Contexts::default();
    let connection = Uuid::new_v4();
    for pfx in [false, true] {
        let context = create(
            &mut contexts,
            connection,
            json!([
                keys.encrypted_input(&first.url, 0, pfx),
                keys.encrypted_input(&second.url, 1, !pfx)
            ]),
        );
        assert_eq!(identity(&mut contexts, context, &first.url).await, "A");
        assert_eq!(
            identity(&mut contexts, context, &format!("{}/redirect", first.url)).await,
            "B"
        );
        let before = first.requests.lock().unwrap().len();
        let id = contexts
            .start(
                context,
                Fetch {
                    ignore_https_errors: Some(false),
                    ..fetch(&first.url)
                },
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let failure = ready(&mut contexts, id).await.err().unwrap();
        assert_eq!(failure.partial.as_ref().unwrap()["failureKind"], "tls");
        assert_eq!(first.requests.lock().unwrap().len(), before);
        assert_eq!(identity(&mut contexts, context, &first.url).await, "A");
    }
    contexts.close().unwrap();
}
