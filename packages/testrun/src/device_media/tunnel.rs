//! Witness release of the actual CoreDevice USB stream behind the TCP worker.
use idevice::{
    ReadWrite,
    tcp::{adapter::Adapter, handle::AdapterHandle},
};
use std::{
    io,
    net::IpAddr,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::oneshot,
};

#[derive(Debug)]
struct Transport {
    stream: Box<dyn ReadWrite>,
    // Fields drop in order: release the socket before notifying the owner.
    _lifetime: oneshot::Sender<()>,
}

impl AsyncRead for Transport {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for Transport {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

pub(super) fn from_proxy(
    proxy: idevice::core_device_proxy::CoreDeviceProxy,
) -> crate::error::Result<(AdapterHandle, u16, oneshot::Receiver<()>)> {
    let info = proxy.tunnel_info();
    let port = info.server_rsd_port;
    let host = info
        .client_address
        .parse()
        .map_err(|error: std::net::AddrParseError| super::platform(error.into()))?;
    let peer = info
        .server_address
        .parse()
        .map_err(|error: std::net::AddrParseError| super::platform(error.into()))?;
    let mss = (info.mtu as usize).saturating_sub(60);
    let (mut adapter, released) = track(proxy.into_tunnel().into_inner(), host, peer);
    adapter.set_mss(mss);
    Ok((adapter.to_async_handle(), port, released))
}

pub(super) fn track(
    stream: Box<dyn ReadWrite>,
    host: IpAddr,
    peer: IpAddr,
) -> (Adapter, oneshot::Receiver<()>) {
    let (lifetime, released) = oneshot::channel();
    let transport = Transport {
        stream,
        _lifetime: lifetime,
    };
    (Adapter::new(Box::new(transport), host, peer), released)
}

pub(super) async fn shutdown(handle: &mut AdapterHandle, released: &mut oneshot::Receiver<()>) {
    // jktcp close only queues a stop. The receiver settles when the USB stream
    // is dropped, including when the worker has already exited independently.
    let _ = handle.close().await;
    let _ = released.await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::{Future, poll_fn};
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn tunnel_shutdown_waits_for_the_transport_to_drop() {
        let (stream, mut peer) = tokio::io::duplex(1024);
        let (adapter, mut released) = track(
            Box::new(stream),
            "fd00::1".parse().unwrap(),
            "fd00::2".parse().unwrap(),
        );
        let mut handle = adapter.to_async_handle();
        let mut closing = std::pin::pin!(shutdown(&mut handle, &mut released));
        // On the current-thread runtime the queued close cannot run until we yield.
        assert!(poll_fn(|cx| Poll::Ready(closing.as_mut().poll(cx).is_pending())).await);
        tokio::time::timeout(std::time::Duration::from_secs(1), closing)
            .await
            .unwrap();
        let eof = poll_fn(|cx| {
            let mut byte = [0; 1];
            let mut read = std::pin::pin!(peer.read(&mut byte));
            Poll::Ready(read.as_mut().poll(cx))
        })
        .await;
        assert!(matches!(eof, Poll::Ready(Ok(0))));
    }
}
