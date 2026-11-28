//! Endpoint identities must never survive a cached HTTPS-proxy configuration.
use super::*;
use crate::browser::http::{ClientCertificate, client_certificates};
use base64::Engine as _;

#[test]
fn client_certificate_tls_configuration_keeps_proxy_scope_separate_in_both_cache_orders() {
    let input = ClientCertificate {
        origin: "https://example.com".into(),
        cert: Some(
            base64::engine::general_purpose::STANDARD
                .encode(include_bytes!("../../fixtures/server.pem")),
        ),
        key: Some(
            base64::engine::general_purpose::STANDARD
                .encode(include_bytes!("../../fixtures/server-key.pem")),
        ),
        ..Default::default()
    };
    let identities = client_certificates::prepare(&[input]).unwrap();
    let config = ureq::tls::TlsConfig::builder().build();
    for first_scope in [true, false] {
        let tls = Tls::new(Trace::default())
            .with_client_certificates(Some(identities[0].resolver.clone()));
        let first = tls.configuration(&config, first_scope).unwrap();
        let second = tls.configuration(&config, !first_scope).unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
        for (selected, endpoint) in [(&first, first_scope), (&second, !first_scope)] {
            assert_eq!(selected.client_auth_cert_resolver.has_certs(), endpoint);
            let certificate = selected
                .client_auth_cert_resolver
                .resolve(&[], &[rustls::SignatureScheme::RSA_PSS_SHA256]);
            assert_eq!(
                certificate.is_some(),
                endpoint,
                "A proxy must not receive the endpoint's client identity"
            );
            assert!(Arc::ptr_eq(
                selected,
                &tls.configuration(&config, endpoint).unwrap()
            ));
        }
    }
}
