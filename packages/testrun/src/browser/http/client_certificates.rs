//! Origin-owned TLS identities. Private material never enters receipts.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::fmt;

const MAX_CERTIFICATES: usize = 16;
const MAX_BYTES: usize = 1048576;
mod paths;
#[cfg(test)]
pub(crate) use super::files::hold_readers as hold_file_readers;

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientCertificate {
    pub origin: String,
    /// Base64-encoded PEM certificate chain, as in the pinned wire protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert: Option<String>,
    /// Base64-encoded PEM private key, optionally encrypted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Base64-encoded PKCS#12 archive; mutually exclusive with PEM material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pfx: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pfx_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
}
impl ClientCertificate {
    fn cert(&self) -> &str {
        self.cert.as_deref().unwrap_or_default()
    }
    fn key(&self) -> &str {
        self.key.as_deref().unwrap_or_default()
    }
    fn selected_path(inline: &Option<String>, path: &Option<String>) -> bool {
        inline.is_none() && path.as_ref().is_some_and(|path| !path.is_empty())
    }
    fn has_paths(&self) -> bool {
        Self::selected_path(&self.cert, &self.cert_path)
            || Self::selected_path(&self.key, &self.key_path)
            || Self::selected_path(&self.pfx, &self.pfx_path)
    }
}
impl fmt::Debug for ClientCertificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClientCertificate { private: true }")
    }
}
#[derive(Clone)]
pub(crate) struct Prepared {
    pub origin: String,
    pub resolver: Arc<Resolver>,
}
pub(crate) struct Resolver {
    keys: Vec<Arc<rustls::sign::CertifiedKey>>,
}
impl fmt::Debug for Resolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PrivateOriginClientCertificates")
    }
}
impl Resolver {
    fn new(identities: Vec<ureq::tls::ClientCert>) -> Result<Self, &'static str> {
        let provider = rustls::crypto::ring::default_provider();
        let mut keys: Vec<Arc<rustls::sign::CertifiedKey>> = vec![];
        for identity in identities {
            let chain = identity
                .certs()
                .iter()
                .map(|cert| rustls::pki_types::CertificateDer::from(cert.der()).into_owned())
                .collect();
            let key =
                rustls::pki_types::PrivateKeyDer::try_from(identity.private_key().der().to_vec())
                    .map_err(|_| "HTTP_CLIENT_CERTIFICATE_KEY_UNSUPPORTED")?;
            let certified = rustls::sign::CertifiedKey::from_der(chain, key, &provider)
                .map_err(|_| "HTTP_CLIENT_CERTIFICATE_KEY_UNSUPPORTED")?;
            // OpenSSL retains the last configured certificate for each key type.
            keys.retain(|known| known.key.algorithm() != certified.key.algorithm());
            keys.push(Arc::new(certified));
        }
        Ok(Self { keys })
    }
}
impl rustls::client::ResolvesClientCert for Resolver {
    fn resolve(
        &self,
        _root_hint_subjects: &[&[u8]],
        sigschemes: &[rustls::SignatureScheme],
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        // The pinned explicitly configured identities ignore authority hints.
        // Select an active key supporting the server's signature algorithms.
        sigschemes.iter().find_map(|scheme| {
            self.keys
                .iter()
                .rev()
                .find(|key| key.key.choose_scheme(&[*scheme]).is_some())
                .cloned()
        })
    }
    fn has_certs(&self) -> bool {
        !self.keys.is_empty()
    }
}
#[derive(Clone)]
pub(crate) struct Cached {
    input: Vec<ClientCertificate>,
    prepared: Vec<Prepared>,
}
impl fmt::Debug for Cached {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PreparedClientCertificates { private: true }")
    }
}
pub(crate) async fn prepare_options(
    mut options: Options,
    deadline: Instant,
) -> Result<Options, Error> {
    let Some(mut input) = options.client_certificates.clone() else {
        return Ok(options);
    };
    if options
        .prepared_client_certificates
        .as_ref()
        .is_some_and(|cached| cached.input == input)
    {
        return Ok(options);
    }
    validate(&input)?;
    // Password KDF work must not hold an actor mutex or its async executor.
    // A timed-out job retains its permit until it actually exits.
    let permit = super::files::reader(deadline).await?;
    let cancel = paths::Cancellation(Arc::new(std::sync::atomic::AtomicBool::new(false)));
    let cancelled = cancel.0.clone();
    let work = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        paths::materialize(&mut input, &cancelled, deadline)?;
        let prepared = prepare(&input).map_err(Error::from)?;
        paths::checkpoint(&cancelled, deadline)?;
        options.client_certificates = Some(input.clone());
        options.prepared_client_certificates = Some(Cached { input, prepared });
        Ok(options)
    });
    tokio::time::timeout_at(deadline, work)
        .await
        .map_err(|_| Error::from("HTTP_DEADLINE"))?
        .map_err(|error| Error::caused("HTTP_CLIENT_CERTIFICATE_INVALID", error))?
}
pub(crate) fn take_prepared(options: &mut Options) -> Result<Vec<Prepared>, &'static str> {
    let input = options.client_certificates.as_deref().unwrap_or_default();
    match options.prepared_client_certificates.take() {
        Some(cached) if cached.input == input => Ok(cached.prepared),
        _ => prepare(input),
    }
}
fn normalized_origin(origin: &str) -> String {
    Url::parse(origin)
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_else(|_| origin.into())
}
pub(super) fn prepare(input: &[ClientCertificate]) -> Result<Vec<Prepared>, &'static str> {
    if input.len() > MAX_CERTIFICATES {
        return Err("HTTP_CLIENT_CERTIFICATE_BOUNDS");
    }
    let mut groups: Vec<(String, Vec<ureq::tls::ClientCert>)> = Vec::new();
    let mut bytes = 0usize;
    if input.iter().any(ClientCertificate::has_paths) {
        return Err("HTTP_CLIENT_CERTIFICATE_PREPARATION_REQUIRED");
    }
    // Node configures PEM arrays before PKCS#12 arrays, independently of their
    // original interleaving. Preserve order within each format.
    for item in input
        .iter()
        .filter(|item| item.pfx.is_none())
        .chain(input.iter().filter(|item| item.pfx.is_some()))
    {
        if item.origin.is_empty()
            || item.origin.len() > 4096
            || item.cert().len() > MAX_BYTES.div_ceil(3) * 4
            || item.key().len() > MAX_BYTES.div_ceil(3) * 4
            || item
                .pfx
                .as_ref()
                .is_some_and(|value| value.len() > MAX_BYTES.div_ceil(3) * 4)
            || item
                .passphrase
                .as_ref()
                .is_some_and(|value| value.len() > 4096 || value.contains('\0'))
        {
            return Err("HTTP_CLIENT_CERTIFICATE_BOUNDS");
        }
        let origin = normalized_origin(&item.origin);
        if item.pfx.is_some() && (item.cert.is_some() || item.key.is_some()) {
            return Err("HTTP_CLIENT_CERTIFICATE_MATERIAL_CONFLICT");
        }
        if item.pfx.is_none() && (item.cert.is_none() || item.key.is_none()) {
            return Err("HTTP_CLIENT_CERTIFICATE_MATERIAL_REQUIRED");
        }
        let mut cert = STANDARD
            .decode(item.cert())
            .map_err(|_| "HTTP_CLIENT_CERTIFICATE_ENCODING")?;
        let mut key = STANDARD
            .decode(item.key())
            .map_err(|_| "HTTP_CLIENT_CERTIFICATE_ENCODING")?;
        bytes = bytes.saturating_add(cert.len()).saturating_add(key.len());
        if let Some(pfx) = &item.pfx {
            let archive = STANDARD
                .decode(pfx)
                .map_err(|_| "HTTP_CLIENT_CERTIFICATE_ENCODING")?;
            bytes = bytes.saturating_add(archive.len());
            if bytes > MAX_BYTES {
                return Err("HTTP_CLIENT_CERTIFICATE_BOUNDS");
            }
            let parsed = openssl::pkcs12::Pkcs12::from_der(&archive)
                .and_then(|archive| archive.parse2(item.passphrase.as_deref().unwrap_or_default()))
                .map_err(|_| "HTTP_CLIENT_CERTIFICATE_PFX_INVALID")?;
            cert = parsed
                .cert
                .ok_or("HTTP_CLIENT_CERTIFICATE_INVALID")?
                .to_pem()
                .map_err(|_| "HTTP_CLIENT_CERTIFICATE_INVALID")?;
            if let Some(chain) = parsed.ca {
                for certificate in chain {
                    cert.extend(
                        certificate
                            .to_pem()
                            .map_err(|_| "HTTP_CLIENT_CERTIFICATE_INVALID")?,
                    );
                    if cert.len() > MAX_BYTES {
                        return Err("HTTP_CLIENT_CERTIFICATE_BOUNDS");
                    }
                }
            }
            key = parsed
                .pkey
                .ok_or("HTTP_CLIENT_CERTIFICATE_KEY_UNSUPPORTED")?
                .private_key_to_pem_pkcs8()
                .map_err(|_| "HTTP_CLIENT_CERTIFICATE_KEY_UNSUPPORTED")?;
        } else if let Some(passphrase) = &item.passphrase {
            key = openssl::pkey::PKey::private_key_from_pem_passphrase(&key, passphrase.as_bytes())
                .and_then(|key| key.private_key_to_pem_pkcs8())
                .map_err(|_| "HTTP_CLIENT_CERTIFICATE_KEY_UNSUPPORTED")?;
        }
        if bytes > MAX_BYTES || cert.len().saturating_add(key.len()) > MAX_BYTES {
            return Err("HTTP_CLIENT_CERTIFICATE_BOUNDS");
        }
        let identity = prepare_identity(&cert, &key)?;
        if let Some((_, identities)) = groups.iter_mut().find(|(known, _)| known == &origin) {
            identities.push(identity);
        } else {
            groups.push((origin, vec![identity]));
        }
    }
    groups
        .into_iter()
        .map(|(origin, identities)| {
            Ok(Prepared {
                origin,
                resolver: Arc::new(Resolver::new(identities)?),
            })
        })
        .collect()
}
fn prepare_identity(cert: &[u8], key: &[u8]) -> Result<ureq::tls::ClientCert, &'static str> {
    let leaf =
        openssl::x509::X509::from_pem(cert).map_err(|_| "HTTP_CLIENT_CERTIFICATE_INVALID")?;
    // Never invoke OpenSSL's terminal password callback for omitted secrets.
    let private = openssl::pkey::PKey::private_key_from_pem_passphrase(key, b"")
        .map_err(|_| "HTTP_CLIENT_CERTIFICATE_KEY_UNSUPPORTED")?;
    if !leaf
        .public_key()
        .map_err(|_| "HTTP_CLIENT_CERTIFICATE_INVALID")?
        .public_eq(&private)
    {
        return Err("HTTP_CLIENT_CERTIFICATE_KEY_MISMATCH");
    }
    let chain = ureq::tls::parse_pem(cert)
        .filter_map(|item| match item {
            Ok(ureq::tls::PemItem::Certificate(cert)) => Some(Ok(cert)),
            Ok(_) => None,
            Err(_) => Some(Err("HTTP_CLIENT_CERTIFICATE_INVALID")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if chain.is_empty() {
        return Err("HTTP_CLIENT_CERTIFICATE_INVALID");
    }
    let key = ureq::tls::PrivateKey::from_pem(key)
        .map_err(|_| "HTTP_CLIENT_CERTIFICATE_KEY_UNSUPPORTED")?;
    Ok(ureq::tls::ClientCert::new_with_certs(&chain, key))
}

pub(crate) fn validate(input: &[ClientCertificate]) -> Result<(), &'static str> {
    if input.len() > MAX_CERTIFICATES {
        return Err("HTTP_CLIENT_CERTIFICATE_BOUNDS");
    }
    let mut decoded = 0usize;
    for item in input {
        for (inline, path) in [
            (&item.cert, &item.cert_path),
            (&item.key, &item.key_path),
            (&item.pfx, &item.pfx_path),
        ] {
            if ClientCertificate::selected_path(inline, path)
                && path
                    .as_ref()
                    .is_some_and(|path| path.len() > 4096 || path.contains('\0'))
            {
                return Err("HTTP_CLIENT_CERTIFICATE_PATH_INVALID");
            }
        }
        if item.origin.is_empty()
            || item.origin.len() > 4096
            || [item.cert(), item.key()]
                .into_iter()
                .any(|value| value.len() > MAX_BYTES.div_ceil(3) * 4)
            || item
                .pfx
                .as_ref()
                .is_some_and(|value| value.len() > MAX_BYTES.div_ceil(3) * 4)
            || item
                .passphrase
                .as_ref()
                .is_some_and(|value| value.len() > 4096 || value.contains('\0'))
        {
            return Err("HTTP_CLIENT_CERTIFICATE_BOUNDS");
        }
        let pem_cert =
            item.cert.is_some() || ClientCertificate::selected_path(&item.cert, &item.cert_path);
        let pem_key =
            item.key.is_some() || ClientCertificate::selected_path(&item.key, &item.key_path);
        let archive =
            item.pfx.is_some() || ClientCertificate::selected_path(&item.pfx, &item.pfx_path);
        if archive && (pem_cert || pem_key) {
            return Err("HTTP_CLIENT_CERTIFICATE_MATERIAL_CONFLICT");
        }
        if !archive && (!pem_cert || !pem_key) {
            return Err("HTTP_CLIENT_CERTIFICATE_MATERIAL_REQUIRED");
        }
        let cert = STANDARD
            .decode(item.cert())
            .map_err(|_| "HTTP_CLIENT_CERTIFICATE_ENCODING")?;
        let key = STANDARD
            .decode(item.key())
            .map_err(|_| "HTTP_CLIENT_CERTIFICATE_ENCODING")?;
        let pfx = item
            .pfx
            .as_ref()
            .map(|value| STANDARD.decode(value))
            .transpose()
            .map_err(|_| "HTTP_CLIENT_CERTIFICATE_ENCODING")?;
        decoded = decoded
            .saturating_add(cert.len())
            .saturating_add(key.len())
            .saturating_add(pfx.as_ref().map_or(0, Vec::len));
        if decoded > MAX_BYTES {
            return Err("HTTP_CLIENT_CERTIFICATE_BOUNDS");
        }
        if !archive && !ClientCertificate::selected_path(&item.cert, &item.cert_path) {
            let mut certificates = 0;
            for part in ureq::tls::parse_pem(&cert) {
                if let ureq::tls::PemItem::Certificate(_) =
                    part.map_err(|_| "HTTP_CLIENT_CERTIFICATE_INVALID")?
                {
                    certificates += 1;
                }
            }
            if certificates == 0 {
                return Err("HTTP_CLIENT_CERTIFICATE_INVALID");
            }
            if item.passphrase.is_none()
                && !ClientCertificate::selected_path(&item.key, &item.key_path)
            {
                ureq::tls::PrivateKey::from_pem(&key)
                    .map_err(|_| "HTTP_CLIENT_CERTIFICATE_KEY_UNSUPPORTED")?;
            }
        }
    }
    Ok(())
}
pub(crate) fn schema() -> Value {
    json!({"type":["array","null"],"maxItems":MAX_CERTIFICATES,
        "description":"Origin-scoped TLS identities: base64 PEM cert/key or PKCS#12 pfx, or certPath/keyPath/pfxPath files (caller-relative, snapshotted before allocation; inline bytes, even empty, win). Omitted/null inherits associated context identities; [] disables them. Optional passphrase for encrypted keys. Repeated origins keep the last identity per key type, PFX after PEM. At most 16 entries; 1 MiB decoded input and per identity.",
        "items":{"type":"object","properties":{
            "origin":{"type":"string","minLength":1,"maxLength":4096},
            "cert":{"type":"string","maxLength":MAX_BYTES.div_ceil(3)*4},
            "key":{"type":"string","maxLength":MAX_BYTES.div_ceil(3)*4},
            "pfx":{"type":["string","null"],"maxLength":MAX_BYTES.div_ceil(3)*4},
            "certPath":{"type":["string","null"],"maxLength":4096},
            "keyPath":{"type":["string","null"],"maxLength":4096},
            "pfxPath":{"type":["string","null"],"maxLength":4096},
            "passphrase":{"type":["string","null"],"maxLength":4096}},
            "required":["origin"],"additionalProperties":false}})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identity(origin: &str) -> ClientCertificate {
        ClientCertificate {
            origin: origin.into(),
            cert: Some(STANDARD.encode(include_bytes!("fixtures/server.pem"))),
            key: Some(STANDARD.encode(include_bytes!("fixtures/server-key.pem"))),
            ..Default::default()
        }
    }
    #[test]
    fn client_certificate_same_origin_entries_validate_as_one_selection_group() {
        let first = identity("https://example.com/path");
        let second = identity("https://example.com");
        validate(&[first, second]).unwrap();
    }
    #[test]
    fn client_certificate_pinned_verify_cases_refuse_before_allocation_and_accept_any_origin_scheme()
     {
        // Pinned verifyClientCertificates: key without cert, cert without key
        // and pfx with cert/key refuse; origins are not scheme-checked.
        let mut key_only = identity("https://example.com");
        key_only.cert = None;
        let mut cert_only = identity("https://example.com");
        cert_only.key = None;
        let mut pfx_with_key = identity("https://example.com");
        pfx_with_key.cert = None;
        pfx_with_key.pfx = Some(STANDARD.encode(b"Acme archive"));
        for (item, code) in [
            (key_only, "HTTP_CLIENT_CERTIFICATE_MATERIAL_REQUIRED"),
            (cert_only, "HTTP_CLIENT_CERTIFICATE_MATERIAL_REQUIRED"),
            (pfx_with_key, "HTTP_CLIENT_CERTIFICATE_MATERIAL_CONFLICT"),
        ] {
            assert_eq!(validate(std::slice::from_ref(&item)), Err(code));
            let mut contexts = Contexts::default();
            let options = Options {
                client_certificates: Some(vec![item]),
                ..Default::default()
            };
            assert!(contexts.create(Uuid::new_v4(), options).is_err());
            assert!(contexts.items.is_empty());
        }
        let plain = identity("http://example.com:8080");
        validate(std::slice::from_ref(&plain)).unwrap();
        assert_eq!(
            prepare(&[plain]).unwrap()[0].origin,
            "http://example.com:8080"
        );
    }
    #[test]
    fn client_certificate_same_origin_entries_prepare_without_duplicate_refusal() {
        let first = identity("https://example.com/path");
        let second = identity("https://example.com");
        let prepared = prepare(&[first, second]).unwrap();
        assert_eq!(prepared.len(), 1);
        assert_eq!(prepared[0].origin, "https://example.com");
    }
    #[tokio::test]
    async fn client_certificate_preparation_deadline_and_cached_source_binding() {
        let deadline = Instant::now() + Duration::from_secs(2);
        let options = Options {
            client_certificates: Some(vec![identity("https://example.com")]),
            ..Default::default()
        };
        let mut prepared = prepare_options(options.clone(), deadline).await.unwrap();
        let readers = super::super::files::hold_readers().await;
        let failure = prepare_options(options, Instant::now() + Duration::from_millis(20))
            .await
            .unwrap_err();
        assert_eq!(failure.code, "HTTP_DEADLINE");
        // An unchanged prepared identity needs neither a new reader nor decryption.
        prepared = prepare_options(prepared, deadline).await.unwrap();
        drop(readers);
        prepared.client_certificates.as_mut().unwrap()[0].key =
            Some(STANDARD.encode(b"Acme changed key"));
        assert_eq!(
            take_prepared(&mut prepared).err(),
            Some("HTTP_CLIENT_CERTIFICATE_KEY_UNSUPPORTED")
        );
    }
    #[test]
    fn client_certificate_encrypted_pem_and_pfx_preserve_the_identity() {
        use openssl::{pkcs12::Pkcs12, pkey::PKey, symm::Cipher, x509::X509};
        let original = identity("https://example.com");
        let private =
            PKey::private_key_from_pem(include_bytes!("fixtures/server-key.pem")).unwrap();
        let leaf = X509::from_pem(include_bytes!("fixtures/server.pem")).unwrap();
        let expected = prepare(std::slice::from_ref(&original)).unwrap().remove(0);
        let mut encrypted = original.clone();
        encrypted.key = Some(
            STANDARD.encode(
                private
                    .private_key_to_pem_pkcs8_passphrase(Cipher::aes_256_cbc(), b"Acme test secret")
                    .unwrap(),
            ),
        );
        encrypted.passphrase = Some("Acme test secret".into());
        let mut builder = Pkcs12::builder();
        builder.name("Acme").pkey(&private).cert(&leaf);
        let archive = builder
            .build2("Acme test secret")
            .unwrap()
            .to_der()
            .unwrap();
        let pfx = ClientCertificate {
            origin: original.origin.clone(),
            pfx: Some(STANDARD.encode(archive)),
            passphrase: encrypted.passphrase.clone(),
            ..Default::default()
        };
        for item in [&encrypted, &pfx] {
            let prepared = prepare(std::slice::from_ref(item)).unwrap().remove(0);
            assert_eq!(
                prepared.resolver.keys[0].cert[0],
                expected.resolver.keys[0].cert[0]
            );
            let debug = format!("{item:?}");
            assert!(!debug.contains("Acme test secret"));
            let wire = serde_json::to_value(item).unwrap();
            assert_eq!(
                serde_json::from_value::<ClientCertificate>(wire).unwrap(),
                *item
            );
            let mut wrong = item.clone();
            wrong.passphrase = Some("incorrect".into());
            let mut contexts = Contexts::default();
            assert!(
                contexts
                    .create(
                        Uuid::new_v4(),
                        Options {
                            client_certificates: Some(vec![wrong]),
                            ..Default::default()
                        }
                    )
                    .is_err()
            );
            assert!(contexts.items.is_empty());
        }
        let mut conflict = pfx;
        conflict.cert = original.cert;
        assert_eq!(
            prepare(&[conflict]).err(),
            Some("HTTP_CLIENT_CERTIFICATE_MATERIAL_CONFLICT")
        );
        let mut missing = encrypted;
        missing.key = None;
        assert_eq!(
            prepare(&[missing]).err(),
            Some("HTTP_CLIENT_CERTIFICATE_MATERIAL_REQUIRED")
        );
    }
    #[test]
    fn client_certificate_private_material_bounds_and_mismatch_refuse_without_allocation() {
        let mut nul = identity("https://example.com");
        nul.passphrase = Some(String::from_utf8(vec![0]).unwrap());
        let mut excessive = identity("https://example.com");
        excessive.passphrase = Some("x".repeat(4097));
        let mut malformed = ClientCertificate {
            origin: "https://example.com".into(),
            pfx: Some(STANDARD.encode(b"Acme invalid PFX")),
            ..Default::default()
        };
        let mut mismatch = identity("https://example.com");
        let group =
            openssl::ec::EcGroup::from_curve_name(openssl::nid::Nid::X9_62_PRIME256V1).unwrap();
        let key = openssl::pkey::PKey::from_ec_key(openssl::ec::EcKey::generate(&group).unwrap())
            .unwrap();
        mismatch.key = Some(STANDARD.encode(key.private_key_to_pem_pkcs8().unwrap()));
        for (input, code) in [
            (nul, "HTTP_CLIENT_CERTIFICATE_BOUNDS"),
            (excessive, "HTTP_CLIENT_CERTIFICATE_BOUNDS"),
            (malformed.clone(), "HTTP_CLIENT_CERTIFICATE_PFX_INVALID"),
            (mismatch, "HTTP_CLIENT_CERTIFICATE_KEY_MISMATCH"),
        ] {
            let mut contexts = Contexts::default();
            assert_eq!(
                contexts
                    .create(
                        Uuid::new_v4(),
                        Options {
                            client_certificates: Some(vec![input]),
                            ..Default::default()
                        }
                    )
                    .unwrap_err()
                    .code,
                code
            );
            assert!(contexts.items.is_empty());
        }
        malformed.pfx = Some("Acme-invalid-base64!".into());
        assert_eq!(
            prepare(&[malformed]).err(),
            Some("HTTP_CLIENT_CERTIFICATE_ENCODING")
        );
    }
    #[test]
    fn client_certificate_normalizes_origins_and_preserves_pinned_pem_bytes() {
        let input = identity("HTTPS://EXAMPLE.COM:443/Acme?private=1");
        let prepared = prepare(std::slice::from_ref(&input)).unwrap();
        assert_eq!(prepared[0].origin, "https://example.com");
        assert_eq!(prepared[0].resolver.keys[0].cert.len(), 1);
        let parsed =
            ureq::tls::Certificate::from_pem(include_bytes!("fixtures/server.pem")).unwrap();
        assert_eq!(prepared[0].resolver.keys[0].cert[0].as_ref(), parsed.der());
        assert!(!format!("{input:?}").contains(input.key()));
        assert!(!format!("{input:?}").contains("Acme?private"));
        let mut duplicate = input.clone();
        duplicate.origin = "https://example.com".into();
        validate(&[input.clone(), duplicate.clone()]).unwrap();
        let browser = crate::browser::contract::ContextOptions {
            client_certificates: Some(vec![input.clone(), duplicate.clone()]),
            ..Default::default()
        };
        browser.validate().unwrap();
        let grouped = prepare(&[input, duplicate]).unwrap();
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].resolver.keys.len(), 1);
    }
    #[test]
    fn client_certificate_invalid_material_refuses_before_client_allocation() {
        let mut input = identity("https://example.com");
        input.cert = Some("Acme-invalid-base64!".into());
        assert_eq!(
            validate(&[input.clone()]),
            Err("HTTP_CLIENT_CERTIFICATE_ENCODING")
        );
        assert_eq!(
            prepare(&[input.clone()]).err(),
            Some("HTTP_CLIENT_CERTIFICATE_ENCODING")
        );
        input.cert = Some(STANDARD.encode(b"Acme private certificate"));
        assert_eq!(
            validate(&[input.clone()]),
            Err("HTTP_CLIENT_CERTIFICATE_INVALID")
        );
        assert_eq!(
            prepare(&[input.clone()]).err(),
            Some("HTTP_CLIENT_CERTIFICATE_INVALID")
        );
        input = identity("https://example.com");
        input.key = Some(STANDARD.encode(
            b"-----BEGIN ENCRYPTED PRIVATE KEY-----\nAA==\n-----END ENCRYPTED PRIVATE KEY-----\n",
        ));
        assert_eq!(
            validate(&[input.clone()]),
            Err("HTTP_CLIENT_CERTIFICATE_KEY_UNSUPPORTED")
        );
        assert_eq!(
            prepare(&[input.clone()]).err(),
            Some("HTTP_CLIENT_CERTIFICATE_KEY_UNSUPPORTED")
        );
        let mut contexts = Contexts::default();
        assert_eq!(
            contexts
                .create(
                    Uuid::new_v4(),
                    Options {
                        client_certificates: Some(vec![input]),
                        ..Default::default()
                    }
                )
                .unwrap_err()
                .code,
            "HTTP_CLIENT_CERTIFICATE_KEY_UNSUPPORTED"
        );
        assert!(contexts.items.is_empty());
    }
    #[test]
    fn client_certificate_inheritance_distinguishes_omission_empty_and_explicit() {
        let inherited = identity("https://example.com");
        let context = crate::browser::contract::ContextOptions {
            client_certificates: Some(vec![inherited.clone()]),
            ..Default::default()
        };
        let mut options = Options::default();
        options.inherit(&context);
        assert_eq!(options.client_certificates, Some(vec![inherited]));
        let mut empty = Options {
            client_certificates: Some(vec![]),
            ..Default::default()
        };
        empty.inherit(&context);
        assert_eq!(empty.client_certificates, Some(vec![]));
        let explicit = identity("https://other.example.com");
        let mut options = Options {
            client_certificates: Some(vec![explicit.clone()]),
            ..Default::default()
        };
        options.inherit(&context);
        assert_eq!(options.client_certificates, Some(vec![explicit]));
    }
    #[test]
    fn client_certificate_context_wire_and_native_creation_options_preserve_boundaries() {
        use crate::browser::contract::{ContextOptions, Engine};
        let input = identity("https://example.com");
        let options = ContextOptions {
            client_certificates: Some(vec![input.clone()]),
            ..Default::default()
        };
        assert!(options.validate_for(Engine::Firefox).is_ok());
        assert!(options.validate_for(Engine::Webkit).is_ok());
        assert!(options.validate_for(Engine::Chrome).is_ok());
        for value in [true, false] {
            let mut accepted = options.clone();
            accepted.ignore_https_errors = Some(value);
            assert!(accepted.validate_for(Engine::Chrome).is_ok());
        }
        let mut accepted = options.clone();
        accepted.user_agent = Some("Acme override".into());
        assert!(accepted.validate_for(Engine::Chrome).is_ok());
        assert_eq!(options.wire()["clientCertificates"], json!([input]));
        for engine in [Engine::Firefox, Engine::Webkit] {
            let capabilities = crate::browser::contract::capabilities(engine, true);
            assert!(
                capabilities["settings"]["creationOptions"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("clientCertificates"))
            );
            assert_eq!(
                capabilities["httpContexts"]["clientCertificates"]["browserInheritance"],
                true
            );
        }
        let native = crate::browser::contract::capabilities(Engine::Chrome, true);
        assert_eq!(
            native["httpContexts"]["clientCertificates"]["browserInheritance"],
            true
        );
        assert!(
            native["creationOptions"]
                .as_array()
                .unwrap()
                .contains(&json!("clientCertificates"))
        );
        assert_eq!(
            native["clientCertificateTunnels"]["upstreamTlsVerificationScope"],
            "configured-origins"
        );
        assert_eq!(native["clientCertificateTunnels"]["fullSdkParity"], false);
        assert_eq!(
            crate::browser::contract::schemas()["launch"]["properties"]["context"]["properties"]["clientCertificates"],
            schema()
        );
    }
}
