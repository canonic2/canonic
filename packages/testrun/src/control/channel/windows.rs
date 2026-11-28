//! Nonblocking Windows AF_UNIX I/O for the shared viewer protocol.
use crate::windows_ipc::IdleBackoff;
use canonic_process_kit::{endpoint, net::Stream};
use std::{
    future::Future,
    io::{self, Read, Write},
    pin::Pin,
    task::{Context, Poll, ready},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    time::{Sleep, sleep},
};

pub(super) async fn lost(listener: &endpoint::Endpoint<'_>) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        if listener.verify().is_err() {
            return;
        }
    }
}

pub(super) async fn accept(listener: &endpoint::Endpoint<'_>) -> io::Result<Option<Socket>> {
    let mut backoff = IdleBackoff::default();
    loop {
        match listener.listener().accept() {
            Ok((stream, _)) => {
                if endpoint::check_peer(&stream).is_err() {
                    return Ok(None);
                }
                return Ok(Socket::new(stream).ok());
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                sleep(backoff.next_delay()).await;
            }
            Err(error) => return Err(error),
        }
    }
}

#[derive(Default)]
struct Retry {
    sleep: Option<Pin<Box<Sleep>>>,
    backoff: IdleBackoff,
}

impl Retry {
    fn poll<T>(
        &mut self,
        cx: &mut Context<'_>,
        mut io: impl FnMut() -> io::Result<T>,
    ) -> Poll<io::Result<T>> {
        loop {
            if let Some(sleep) = &mut self.sleep {
                ready!(sleep.as_mut().poll(cx));
            }
            match io() {
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    let deadline = tokio::time::Instant::now() + self.backoff.next_delay();
                    self.sleep
                        .get_or_insert_with(|| Box::pin(tokio::time::sleep_until(deadline)))
                        .as_mut()
                        .reset(deadline);
                }
                result => {
                    self.sleep = None;
                    self.backoff.reset();
                    return Poll::Ready(result);
                }
            }
        }
    }
}

pub(super) struct Socket {
    stream: Stream,
    read: Retry,
    write: Retry,
}

impl Socket {
    pub(super) fn new(stream: Stream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            read: Retry::default(),
            write: Retry::default(),
        })
    }
}

impl AsyncRead for Socket {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let socket = self.get_mut();
        let count = ready!(
            socket
                .read
                .poll(cx, || socket.stream.read(output.initialize_unfilled()))
        )?;
        output.advance(count);
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Socket {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let socket = self.get_mut();
        socket.write.poll(cx, || socket.stream.write(bytes))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(self.stream.shutdown(std::net::Shutdown::Write))
    }
}
