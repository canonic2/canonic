//! Context-owned SOCKS/TLS relay for native browser client certificates.
//! Identity material never enters proxy settings, observations or Debug output.
use super::{contract::ContextProxy, http::client_certificates::Prepared};
use std::{collections::BTreeMap, io, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::{JoinHandle, JoinSet},
    time::Instant,
};
mod proxy;
#[cfg(test)]
mod tests;
mod tls;

trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
type Socket = Box<dyn Stream>;
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid private certificate tunnel",
    )
}

pub(crate) struct Tunnel {
    port: u16,
    #[cfg(test)]
    browser_certificate: rustls::pki_types::CertificateDer<'static>,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
    settled: Option<bool>,
}
impl std::fmt::Debug for Tunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrivateClientCertificateTunnel")
    }
}
impl Drop for Tunnel {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
#[derive(Clone)]
struct Settings {
    identities: Arc<BTreeMap<String, Arc<super::http::client_certificates::Resolver>>>,
    proxy: Option<ContextProxy>,
    roots: Arc<rustls::RootCertStore>,
    server: Arc<rustls::ServerConfig>,
}
impl Tunnel {
    pub(crate) async fn start(
        identities: Vec<Prepared>,
        proxy: Option<ContextProxy>,
        deadline: Instant,
    ) -> Result<Self, super::session::Error> {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        Self::with_roots(identities, proxy, roots, deadline).await
    }
    async fn with_roots(
        identities: Vec<Prepared>,
        proxy: Option<ContextProxy>,
        roots: rustls::RootCertStore,
        deadline: Instant,
    ) -> Result<Self, super::session::Error> {
        let (server, _browser_certificate) =
            tokio::time::timeout_at(deadline, tokio::task::spawn_blocking(tls::server))
                .await
                .map_err(|_| super::session::Error::from("HTTP_DEADLINE"))?
                .map_err(|_| super::session::Error::from("HTTP_CLIENT_CERTIFICATE_INVALID"))?
                .map_err(|_| super::session::Error::from("HTTP_CLIENT_CERTIFICATE_INVALID"))?;
        let settings = Settings {
            identities: Arc::new(
                identities
                    .into_iter()
                    .map(|value| (value.origin, value.resolver))
                    .collect(),
            ),
            proxy,
            roots: Arc::new(roots),
            server: Arc::new(server),
        };
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| super::session::Error::from("HOST_IO"))?;
        let port = listener
            .local_addr()
            .map_err(|_| super::session::Error::from("HOST_IO"))?
            .port();
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut sockets = JoinSet::new();
            loop {
                tokio::select! {
                    _ = &mut stopped => break,
                    accepted = listener.accept() => {
                        let Ok((stream, peer)) = accepted else { break };
                        if !peer.ip().is_loopback() { continue; }
                        let settings = settings.clone();
                        sockets.spawn(async move {
                            let _ = connection(stream, settings).await;
                        });
                    }
                    _ = sockets.join_next(), if !sockets.is_empty() => {}
                }
            }
            drop(listener);
            // JoinSet::shutdown aborts and then joins every owned socket future.
            // The close receipt cannot race detached relays still holding keys.
            sockets.shutdown().await;
        });
        Ok(Self {
            port,
            stop: Some(stop),
            task,
            settled: None,
            #[cfg(test)]
            browser_certificate: _browser_certificate,
        })
    }
    pub(crate) fn settings(&self) -> ContextProxy {
        ContextProxy {
            server: format!("socks5://127.0.0.1:{}", self.port),
            bypass: None,
            username: None,
            password: None,
        }
    }
    pub(crate) async fn close(&mut self, deadline: Instant) -> Result<(), super::session::Error> {
        if let Some(joined) = self.settled {
            return if joined {
                Ok(())
            } else {
                Err("CLEANUP_PENDING".into())
            };
        }
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let result = tokio::time::timeout_at(deadline, &mut self.task)
            .await
            .map_err(|_| super::session::Error::from("CLEANUP_PENDING"))?;
        self.settled = Some(result.is_ok());
        result.map_err(|_| super::session::Error::from("CLEANUP_PENDING"))
    }
}

async fn destination(browser: &mut TcpStream) -> io::Result<(String, u16)> {
    if browser.read_u8().await? != 5 {
        return Err(invalid());
    }
    let count = browser.read_u8().await? as usize;
    let mut methods = vec![0; count];
    browser.read_exact(&mut methods).await?;
    if !methods.contains(&0) {
        browser.write_all(&[5, 255]).await?;
        return Err(invalid());
    }
    browser.write_all(&[5, 0]).await?;
    if browser.read_u8().await? != 5
        || browser.read_u8().await? != 1
        || browser.read_u8().await? != 0
    {
        return Err(invalid());
    }
    let host = match browser.read_u8().await? {
        1 => {
            let mut bytes = [0; 4];
            browser.read_exact(&mut bytes).await?;
            std::net::Ipv4Addr::from(bytes).to_string()
        }
        4 => {
            let mut bytes = [0; 16];
            browser.read_exact(&mut bytes).await?;
            std::net::Ipv6Addr::from(bytes).to_string()
        }
        3 => {
            let count = browser.read_u8().await? as usize;
            let mut bytes = vec![0; count];
            browser.read_exact(&mut bytes).await?;
            String::from_utf8(bytes).map_err(|_| invalid())?
        }
        _ => return Err(invalid()),
    };
    let port = browser.read_u16().await?;
    if host.is_empty() || host.contains('\0') || port == 0 {
        return Err(invalid());
    }
    Ok((host, port))
}
fn origin(host: &str, port: u16) -> String {
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.into()
    };
    url::Url::parse(&format!("https://{host}:{port}"))
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_default()
}
async fn connection(mut browser: TcpStream, settings: Settings) -> io::Result<()> {
    let (host, port) = destination(&mut browser).await?;
    let upstream = proxy::connect(&host, port, &settings).await;
    let mut upstream = match upstream {
        Ok(stream) => stream,
        _ => {
            browser.write_all(&[5, 1, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
            return Err(invalid());
        }
    };
    browser.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]).await?;
    let identity = settings.identities.get(&origin(&host, port)).cloned();
    if let Some(identity) = identity {
        let mut first = [0];
        let count = browser.peek(&mut first).await?;
        if count != 0 && first[0] == 22 {
            return tls::relay(browser, upstream, &host, identity, settings).await;
        }
    }
    tokio::io::copy_bidirectional(&mut browser, &mut upstream).await?;
    Ok(())
}
