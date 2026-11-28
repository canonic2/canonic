//! Explicit WebSocket attachment shared by CDP and the Playwright binding.

use super::contract::Engine;
use rustls::pki_types::{CertificateDer, ServerName, pem::PemObject};
use serde::Deserialize;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    time::{Instant, timeout_at},
};
use tokio_tungstenite::{
    WebSocketStream, client_async_with_config,
    tungstenite::{
        client::IntoClientRequest,
        http::{
            HeaderValue, Request,
            header::{AUTHORIZATION, USER_AGENT},
        },
        protocol::WebSocketConfig,
    },
};
use url::{Host, Url};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Attachment {
    pub endpoint: String,
    pub credential_path: Option<PathBuf>,
    pub ca_path: Option<PathBuf>,
    pub context_id: Option<String>,
}

impl std::fmt::Debug for Attachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attachment").finish_non_exhaustive()
    }
}

pub(super) trait SocketIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> SocketIo for T {}
pub(super) type Socket = WebSocketStream<Box<dyn SocketIo>>;

impl Attachment {
    pub(super) fn validate(&self, engine: Engine) -> Result<(), &'static str> {
        let url = endpoint(&self.endpoint)?;
        if [&self.credential_path, &self.ca_path]
            .into_iter()
            .flatten()
            .any(|path| !private_path(path))
        {
            return Err("ATTACHMENT_FILE_PATH_INVALID");
        }
        if self.ca_path.is_some() && url.scheme() != "wss" {
            return Err("ATTACHMENT_CA_REQUIRES_TLS");
        }
        if self.context_id.is_some() && engine != Engine::Chrome {
            return Err("ATTACHMENT_ADOPTION_UNSUPPORTED");
        }
        if self.context_id.as_deref().is_some_and(|id| {
            id.is_empty()
                || id.len() > 128
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        }) {
            return Err("ATTACHMENT_CONTEXT_INVALID");
        }
        Ok(())
    }

    pub(super) async fn connect(
        &self,
        user_agent: Option<HeaderValue>,
        limit: usize,
        deadline: Instant,
    ) -> Result<Socket, &'static str> {
        let url = endpoint(&self.endpoint)?;
        let host = url.host().ok_or("ATTACHMENT_ENDPOINT_INVALID")?;
        let port = url
            .port_or_known_default()
            .ok_or("ATTACHMENT_ENDPOINT_INVALID")?;
        let root = if self.credential_path.is_some() || self.ca_path.is_some() {
            super::preparation::root()
                .map_err(|_| "ATTACHMENT_FILE_NOT_APPROVED")?
                .join("attachments")
        } else {
            PathBuf::new()
        };
        let request = self.request(&url, &root, user_agent)?;
        let tls = (url.scheme() == "wss")
            .then(|| Self::tls_config(&root, self.ca_path.as_deref()))
            .transpose()?;
        let config = WebSocketConfig::default()
            .read_buffer_size(16 * 1024)
            .write_buffer_size(0)
            .max_write_buffer_size(limit + 4096)
            .max_message_size(Some(limit))
            .max_frame_size(Some(limit));
        timeout_at(deadline, async {
            let stream = match host {
                Host::Ipv4(ip) => TcpStream::connect(SocketAddr::from((ip, port))).await,
                Host::Ipv6(ip) => TcpStream::connect(SocketAddr::from((ip, port))).await,
                Host::Domain("localhost") => {
                    TcpStream::connect(
                        &[
                            SocketAddr::from(([127, 0, 0, 1], port)),
                            SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port)),
                        ][..],
                    )
                    .await
                }
                Host::Domain(host) => TcpStream::connect((host, port)).await,
            }
            .map_err(|_| "ATTACHMENT_ENDPOINT_UNAVAILABLE")?;
            let stream: Box<dyn SocketIo> = if let Some(config) = tls {
                let name =
                    ServerName::try_from(host.to_string().trim_matches(['[', ']']).to_owned())
                        .map_err(|_| "ATTACHMENT_ENDPOINT_INVALID")?;
                let socket = tokio_rustls::TlsConnector::from(Arc::new(config))
                    .connect(name, stream)
                    .await
                    .map_err(|_| "ATTACHMENT_TLS_FAILED")?;
                Box::new(socket)
            } else {
                Box::new(stream)
            };
            client_async_with_config(request, stream, Some(config))
                .await
                .map(|(socket, _)| socket)
                .map_err(|error| match error {
                    tokio_tungstenite::tungstenite::Error::Http(response)
                        if matches!(response.status().as_u16(), 401 | 403) =>
                    {
                        "ATTACHMENT_AUTHORIZATION_FAILED"
                    }
                    tokio_tungstenite::tungstenite::Error::Http(response)
                        if response.status().as_u16() == 404 =>
                    {
                        "ATTACHMENT_ENDPOINT_NOT_FOUND"
                    }
                    _ => "ATTACHMENT_HANDSHAKE_FAILED",
                })
        })
        .await
        .map_err(|_| "ATTACHMENT_TIMEOUT")?
    }

    fn request(
        &self,
        url: &Url,
        root: &Path,
        user_agent: Option<HeaderValue>,
    ) -> Result<Request<()>, &'static str> {
        let mut request = self
            .endpoint
            .as_str()
            .into_client_request()
            .map_err(|_| "ATTACHMENT_ENDPOINT_INVALID")?;
        if let Some(user_agent) = user_agent {
            request.headers_mut().insert(USER_AGENT, user_agent);
        }
        if let Some(path) = &self.credential_path {
            let bytes = read_approved_file(root, path, 4096, "ATTACHMENT_CREDENTIAL_UNAVAILABLE")?;
            let credential: Credential =
                serde_json::from_slice(&bytes).map_err(|_| "ATTACHMENT_CREDENTIAL_INVALID")?;
            if credential.origin != url.origin().ascii_serialization() {
                return Err("ATTACHMENT_CREDENTIAL_ORIGIN_MISMATCH");
            }
            let token = credential.token;
            if token.is_empty()
                || !token
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-._~+/=".contains(&byte))
            {
                return Err("ATTACHMENT_CREDENTIAL_INVALID");
            }
            let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|_| "ATTACHMENT_CREDENTIAL_INVALID")?;
            value.set_sensitive(true);
            request.headers_mut().insert(AUTHORIZATION, value);
        }
        Ok(request)
    }

    fn tls_config(
        root: &Path,
        ca_path: Option<&Path>,
    ) -> Result<rustls::ClientConfig, &'static str> {
        let roots = if let Some(path) = ca_path {
            let bytes = read_approved_file(root, path, 64 * 1024, "ATTACHMENT_CA_INVALID")?;
            let mut roots = rustls::RootCertStore::empty();
            roots
                .add(CertificateDer::from_pem_slice(&bytes).map_err(|_| "ATTACHMENT_CA_INVALID")?)
                .map_err(|_| "ATTACHMENT_CA_INVALID")?;
            roots
        } else {
            rustls::RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            }
        };
        Ok(rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| "ATTACHMENT_TLS_FAILED")?
        .with_root_certificates(roots)
        .with_no_client_auth())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    origin: String,
    token: String,
}

fn approved_file(root: &Path, path: &Path) -> Result<PathBuf, &'static str> {
    let error = "ATTACHMENT_FILE_NOT_APPROVED";
    if !private_path(path) || path.parent() != Some(root) {
        return Err(error);
    }
    #[cfg(windows)]
    canonic_process_kit::endpoint::check_private_directory(root).map_err(|_| error)?;
    #[cfg(not(windows))]
    crate::artifacts::check_private_directory(root).map_err(|_| error)?;
    Ok(root.join(path.file_name().ok_or(error)?))
}

fn read_approved_file(
    root: &Path,
    path: &Path,
    max: usize,
    unavailable: &'static str,
) -> Result<Vec<u8>, &'static str> {
    let path = approved_file(root, path)?;
    #[cfg(not(windows))]
    let bytes = crate::artifacts::read_private_file(&path, max);
    #[cfg(windows)]
    let bytes = crate::artifacts::read_private_file_with(&path, max, |file| {
        canonic_process_kit::endpoint::check_private_child_file(file).map_err(|_| {
            crate::error::Error::invalid("Attachment file must be private to this user")
        })
    });
    bytes.map_err(|_| unavailable)
}

fn private_path(path: &Path) -> bool {
    path.is_absolute()
        && path
            .to_str()
            .is_some_and(|path| path.len() <= 4096 && !path.contains('\0'))
}

fn endpoint(value: &str) -> Result<Url, &'static str> {
    let invalid = "ATTACHMENT_ENDPOINT_INVALID";
    if value.len() > 2048
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'\\')
    {
        return Err(invalid);
    }
    let url = Url::parse(value).map_err(|_| invalid)?;
    let host = url.host().ok_or(invalid)?;
    let loopback = match host {
        Host::Ipv4(ip) => ip.is_loopback(),
        Host::Ipv6(ip) => ip.is_loopback(),
        Host::Domain(name) => name == "localhost",
    };
    if url.password().is_some()
        || !url.username().is_empty()
        || value.split_once("://").is_some_and(|(_, tail)| {
            tail.split('/')
                .next()
                .is_some_and(|authority| authority.contains('@'))
        })
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port() == Some(0)
        || !(url.scheme() == "wss" || (url.scheme() == "ws" && loopback))
    {
        return Err(invalid);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_refuse_cleartext_remote_and_url_credentials() {
        for invalid in [
            "ws://example.com/cdp",
            "wss://user@example.com/cdp",
            "wss://example.com/?key=secret",
            "ws://localhost:0/",
            "wss://example.com/#secret",
        ] {
            assert!(endpoint(invalid).is_err(), "{invalid}");
        }
        for valid in [
            "ws://localhost:1234/acme",
            "ws://[::1]:1234/acme",
            "wss://example.com/acme",
        ] {
            assert!(endpoint(valid).is_ok(), "{valid}");
        }
    }

    #[test]
    fn credentials_are_private_single_bearer_values() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("attachments");
        #[cfg(windows)]
        canonic_process_kit::endpoint::prepare_directory(&root).unwrap();
        #[cfg(not(windows))]
        crate::artifacts::directory(&root).unwrap();
        let path = root.join("token");
        crate::artifacts::write(
            &path,
            br#"{"origin":"wss://example.com","token":"Acme-token"}"#,
        )
        .unwrap();
        let mut attachment = Attachment {
            endpoint: "wss://example.com/acme".into(),
            credential_path: Some(path),
            ca_path: None,
            context_id: None,
        };
        let url = endpoint(&attachment.endpoint).unwrap();
        let request = attachment.request(&url, &root, None).unwrap();
        let auth = &request.headers()[AUTHORIZATION];
        assert_eq!(auth, "Bearer Acme-token");
        assert!(auth.is_sensitive());
        assert!(!format!("{request:?} {attachment:?}").contains("Acme-token"));

        attachment.endpoint = "wss://example.com:8443/acme".into();
        let foreign = endpoint(&attachment.endpoint).unwrap();
        assert_eq!(
            attachment.request(&foreign, &root, None).unwrap_err(),
            "ATTACHMENT_CREDENTIAL_ORIGIN_MISMATCH"
        );
        attachment.credential_path = Some(temporary.path().join("outside"));
        assert_eq!(
            attachment.request(&foreign, &root, None).unwrap_err(),
            "ATTACHMENT_FILE_NOT_APPROVED"
        );
        assert_eq!(
            Attachment::tls_config(&root, attachment.credential_path.as_deref()).unwrap_err(),
            "ATTACHMENT_FILE_NOT_APPROVED"
        );
        let invalid = root.join("invalid");
        crate::artifacts::write(
            &invalid,
            br#"{"origin":"wss://example.com","token":"Acme: invalid"}"#,
        )
        .unwrap();
        attachment.credential_path = Some(invalid);
        attachment.endpoint = url.to_string();
        assert_eq!(
            attachment.request(&url, &root, None).unwrap_err(),
            "ATTACHMENT_CREDENTIAL_INVALID"
        );
    }
}
