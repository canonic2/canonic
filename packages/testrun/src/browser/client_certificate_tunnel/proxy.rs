//! Connect to the destination through the context's acknowledged upstream proxy.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};

pub(super) async fn connect(host: &str, port: u16, settings: &Settings) -> io::Result<Socket> {
    let Some(proxy) = &settings.proxy else {
        return Ok(Box::new(TcpStream::connect((host, port)).await?));
    };
    let destination = url::Url::parse(&origin(host, port)).map_err(|_| invalid())?;
    let hostname = &destination[url::Position::BeforeHost..url::Position::AfterHost];
    if super::super::http::proxy_bypasses(hostname, proxy.bypass.as_deref()) {
        return Ok(Box::new(TcpStream::connect((host, port)).await?));
    }
    let endpoint = url::Url::parse(&proxy.server).map_err(|_| invalid())?;
    let endpoint_host = endpoint
        .host_str()
        .ok_or_else(invalid)?
        .trim_matches(['[', ']']);
    let endpoint_port =
        endpoint
            .port()
            .filter(|port| *port != 0)
            .unwrap_or(match endpoint.scheme() {
                "https" => 443,
                "socks4" | "socks5" => 1080,
                _ => 80,
            });
    let socket = TcpStream::connect((endpoint_host, endpoint_port)).await?;
    let mut socket: Socket = if endpoint.scheme() == "https" {
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(settings.roots.clone())
            .with_no_client_auth();
        Box::new(
            tokio_rustls::TlsConnector::from(Arc::new(config))
                .connect(tls::name(endpoint_host)?, socket)
                .await?,
        )
    } else {
        Box::new(socket)
    };
    match endpoint.scheme() {
        "socks5" => socks5(&mut socket, host, port).await?,
        "socks4" => socks4(&mut socket, host, port).await?,
        "http" | "https" => {
            let authority = if host.contains(':') {
                format!("[{host}]:{port}")
            } else {
                format!("{host}:{port}")
            };
            let authorization = proxy
                .username
                .as_ref()
                .filter(|user| !user.is_empty())
                .map(|user| {
                    let token = STANDARD.encode(format!(
                        "{user}:{}",
                        proxy.password.as_deref().unwrap_or_default()
                    ));
                    format!("Proxy-Authorization: Basic {token}\r\n")
                })
                .unwrap_or_default();
            socket.write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n{authorization}Connection: close\r\n\r\n").as_bytes()).await?;
            // Read only the CONNECT head. TLS bytes already queued by the
            // upstream must remain in the stream for the destination handshake.
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") && head.len() < 16384 {
                head.push(socket.read_u8().await?);
            }
            if !head.ends_with(b"\r\n\r\n") {
                return Err(invalid());
            }
            // The pinned CONNECT helper projects bytes with Buffer's ASCII
            // codec, rather than requiring UTF-8 in otherwise valid headers.
            let head = head
                .iter()
                .map(|byte| char::from(byte & 0x7f))
                .collect::<String>();
            let mut lines = head.split("\r\n");
            if lines.next().and_then(|line| line.split(' ').nth(1)) != Some("200") {
                return Err(invalid());
            }
            if lines
                .take_while(|line| !line.is_empty())
                .any(|line| !line.contains(':'))
            {
                return Err(invalid());
            }
        }
        _ => return Err(invalid()),
    }
    Ok(socket)
}
async fn socks5(socket: &mut Socket, host: &str, port: u16) -> io::Result<()> {
    socket.write_all(&[5, 1, 0]).await?;
    let mut reply = [0; 2];
    socket.read_exact(&mut reply).await?;
    if reply != [5, 0] {
        return Err(invalid());
    }
    let mut request = vec![5, 1, 0];
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(address)) => {
            request.push(1);
            request.extend(address.octets());
        }
        Ok(std::net::IpAddr::V6(address)) => {
            request.push(4);
            request.extend(address.octets());
        }
        Err(_) => {
            request.extend([3, u8::try_from(host.len()).map_err(|_| invalid())?]);
            request.extend_from_slice(host.as_bytes());
        }
    }
    request.extend(port.to_be_bytes());
    socket.write_all(&request).await?;
    let mut prefix = [0; 4];
    socket.read_exact(&mut prefix).await?;
    if prefix[..3] != [5, 0, 0] {
        return Err(invalid());
    }
    let count = match prefix[3] {
        1 => 4,
        4 => 16,
        3 => socket.read_u8().await? as usize,
        _ => return Err(invalid()),
    };
    let mut rest = vec![0; count + 2];
    socket.read_exact(&mut rest).await?;
    Ok(())
}
async fn socks4(socket: &mut Socket, host: &str, port: u16) -> io::Result<()> {
    let mut request = vec![4, 1];
    request.extend(port.to_be_bytes());
    let domain = match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(address)) => {
            request.extend(address.octets());
            false
        }
        Ok(std::net::IpAddr::V6(_)) => return Err(invalid()),
        Err(_) => {
            request.extend([0, 0, 0, 1]);
            true
        }
    };
    request.push(0);
    if domain {
        request.extend_from_slice(host.as_bytes());
        request.push(0);
    }
    socket.write_all(&request).await?;
    let mut reply = [0; 8];
    socket.read_exact(&mut reply).await?;
    if reply[..2] != [0, 90] {
        return Err(invalid());
    }
    Ok(())
}
