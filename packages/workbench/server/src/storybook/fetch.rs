//! One GET with a deadline, at an `http`/`https` address or over a local
//! socket: how the server reads a Storybook's index. A server that isn't
//! running answers "isn't answering" in a moment instead of hanging.

use anyhow::{Context as _, Result, anyhow};
use http_body_util::{BodyExt as _, Empty};
use hyper::body::Bytes;
use hyper::{Request, Uri, header};
use hyper_util::rt::TokioIo;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

/// What a server answered: its status and its body as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Answer {
    pub(crate) status: u16,
    pub(crate) body: String,
}

/// GETs `address` within `timeout`, asking for `accept`.
pub(crate) async fn get(address: &str, accept: &str, timeout: Duration) -> Result<Answer> {
    let uri: Uri = address
        .parse()
        .with_context(|| format!("not an address: {address}"))?;
    let secure = match uri.scheme_str() {
        Some("http") => false,
        Some("https") => true,
        _ => return Err(anyhow!("not an http address: {address}")),
    };
    let host = uri
        .host()
        .ok_or_else(|| anyhow!("no host in {address}"))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let port = uri.port_u16().unwrap_or(if secure { 443 } else { 80 });
    let authority = uri
        .authority()
        .map_or_else(|| host.clone(), ToString::to_string);
    let target = uri
        .path_and_query()
        .map_or("/", |path| path.as_str())
        .to_owned();
    deadline(timeout, async move {
        let stream = TcpStream::connect((host.as_str(), port)).await?;
        if secure {
            let stream = crate::proxies::tls(&host, stream).await?;
            exchange(stream, &authority, &target, accept).await
        } else {
            exchange(stream, &authority, &target, accept).await
        }
    })
    .await
}

/// GETs `target` from the server on the local socket `socket` within `timeout`.
pub(crate) async fn get_socket(socket: &Path, target: &str, timeout: Duration) -> Result<Answer> {
    let socket = socket.to_owned();
    let target = target.to_owned();
    deadline(timeout, async move {
        let stream = local(&socket).await?;
        exchange(stream, "localhost", &target, "*/*").await
    })
    .await
}

#[cfg(unix)]
async fn local(socket: &Path) -> Result<tokio::net::UnixStream> {
    tokio::net::UnixStream::connect(socket)
        .await
        .with_context(|| format!("connect to {}", socket.display()))
}

#[cfg(windows)]
async fn local(socket: &Path) -> Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    tokio::net::windows::named_pipe::ClientOptions::new()
        .open(socket)
        .with_context(|| format!("connect to {}", socket.display()))
}

async fn deadline(timeout: Duration, work: impl Future<Output = Result<Answer>>) -> Result<Answer> {
    tokio::time::timeout(timeout, work)
        .await
        .map_err(|_| anyhow!("timed out after {}ms", timeout.as_millis()))?
}

async fn exchange(
    stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
    host: &str,
    target: &str,
    accept: &str,
) -> Result<Answer> {
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    tokio::spawn(async move {
        let _closed = connection.await;
    });
    let request = Request::get(target)
        .header(header::HOST, host)
        .header(header::ACCEPT, accept)
        .body(Empty::<Bytes>::new())?;
    let response = sender.send_request(request).await?;
    let status = response.status().as_u16();
    let body = response.into_body().collect().await?.to_bytes();
    Ok(Answer {
        status,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}
