//! Chrome's inherited DevTools descriptors and NUL-delimited UTF-8 framing.

use super::{Connection, Events, MAX_MESSAGE_BYTES};
use std::io;
#[cfg(unix)]
use std::os::{
    fd::{AsRawFd, FromRawFd, OwnedFd},
    unix::net::UnixStream as StdUnixStream,
};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader,
};
#[cfg(unix)]
use tokio::{net::UnixStream, process::Command};
use tokio_tungstenite::tungstenite::{Error, Message, error::CapacityError};

#[cfg(unix)]
pub(in crate::browser::cdp) fn configure(command: &mut Command) -> io::Result<UnixStream> {
    const CHROME_READ: libc::c_int = 3;
    const CHROME_WRITE: libc::c_int = 4;
    let (parent, child) = StdUnixStream::pair()?;
    parent.set_nonblocking(true)?;
    let parent = UnixStream::from_std(parent)?;
    // Reserve a source that cannot collide with either Chrome descriptor.
    // SAFETY: the socket is live and fcntl returns a new descriptor on success.
    let source = unsafe { libc::fcntl(child.as_raw_fd(), libc::F_DUPFD_CLOEXEC, CHROME_WRITE + 1) };
    if source < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fcntl transferred unique ownership of this descriptor.
    let source = unsafe { OwnedFd::from_raw_fd(source) };
    command.arg("--remote-debugging-pipe");
    // SAFETY: the child hook uses only async-signal-safe dup2 and errno access.
    // The source and the parent's socket retain CLOEXEC. Only 3 and 4 survive.
    unsafe {
        command.pre_exec(move || {
            // Keep both original socket descriptors reserved through spawn.
            let _ = child.as_raw_fd();
            let source = source.as_raw_fd();
            if libc::dup2(source, CHROME_READ) < 0 || libc::dup2(source, CHROME_WRITE) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(parent)
}

pub(in crate::browser::cdp) fn open(
    reader: impl AsyncRead + Unpin + Send + 'static,
    writer: impl AsyncWrite + Unpin + Send + 'static,
) -> (Connection, Events) {
    let stream = futures_util::stream::unfold(BufReader::new(reader), |mut reader| async {
        let message = read_message(&mut reader).await?;
        Some((message, reader))
    });
    let sink = futures_util::sink::unfold(writer, |mut writer, message| async {
        let Message::Text(text) = message else {
            return Err(Error::Io(io::ErrorKind::InvalidInput.into()));
        };
        writer.write_all(text.as_bytes()).await?;
        writer.write_all(&[0]).await?;
        writer.flush().await?;
        Ok(writer)
    });
    Connection::start_transport(Box::pin(sink), Box::pin(stream), false)
}

async fn read_message(reader: &mut (impl AsyncBufRead + Unpin)) -> Option<Result<Message, Error>> {
    let mut message = Vec::new();
    loop {
        let available = match reader.fill_buf().await {
            Ok(available) => available,
            Err(error) => return Some(Err(error.into())),
        };
        if available.is_empty() {
            return if message.is_empty() {
                None
            } else {
                Some(Err(Error::Io(io::ErrorKind::UnexpectedEof.into())))
            };
        }
        let delimiter = available.iter().position(|&byte| byte == 0);
        let length = delimiter.unwrap_or(available.len());
        if message.len() + length > MAX_MESSAGE_BYTES {
            return Some(Err(Error::Capacity(CapacityError::MessageTooLong {
                size: message.len() + length,
                max_size: MAX_MESSAGE_BYTES,
            })));
        }
        message.extend_from_slice(&available[..length]);
        reader.consume(length + usize::from(delimiter.is_some()));
        if delimiter.is_some() {
            return Some(
                String::from_utf8(message)
                    .map(|text| Message::Text(text.into()))
                    .map_err(|_| Error::Utf8("Chrome pipe emitted invalid UTF-8".into())),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use serde_json::{Value, json};
    #[cfg(unix)]
    use std::time::Duration;
    #[cfg(unix)]
    use tokio::time::Instant;

    #[tokio::test]
    async fn native_cdp_pipe_rejects_invalid_truncated_and_oversized_frames() {
        let mut invalid = BufReader::new(&b"\xff\0"[..]);
        assert!(matches!(
            read_message(&mut invalid).await,
            Some(Err(Error::Utf8(_)))
        ));
        let mut truncated = BufReader::new(&b"Acme"[..]);
        assert!(
            matches!(read_message(&mut truncated).await, Some(Err(Error::Io(error))) if error.kind() == io::ErrorKind::UnexpectedEof)
        );
        let bytes = vec![b'A'; MAX_MESSAGE_BYTES + 1];
        let mut oversized = BufReader::new(bytes.as_slice());
        assert!(matches!(
            read_message(&mut oversized).await,
            Some(Err(Error::Capacity(CapacityError::MessageTooLong { .. })))
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_cdp_pipe_transfers_both_child_descriptors() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "IFS= read -r value <&3 && printf '%s\\000%s\\000' \"$value\" Done >&4",
        ]);
        command.kill_on_drop(true);
        let mut parent = BufReader::new(configure(&mut command).unwrap());
        let mut child = command.spawn().unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            parent.get_mut().write_all(b"Acme\n").await.unwrap();
            assert_eq!(
                read_message(&mut parent).await.unwrap().unwrap(),
                Message::Text("Acme".into())
            );
            assert_eq!(
                read_message(&mut parent).await.unwrap().unwrap(),
                Message::Text("Done".into())
            );
            assert!(child.wait().await.unwrap().success());
        })
        .await
        .unwrap();

        let mut together = BufReader::new(&b"one\0two\0"[..]);
        for value in ["one", "two"] {
            assert_eq!(
                read_message(&mut together).await.unwrap().unwrap(),
                Message::Text(value.into())
            );
        }
        assert!(read_message(&mut together).await.is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_cdp_pipe_frames_and_uses_the_shared_call_lane() {
        let (local, peer) = UnixStream::pair().unwrap();
        let (reader, writer) = tokio::io::split(local);
        let (connection, _events) = open(reader, writer);
        let mut peer = BufReader::new(peer);
        let deadline = Instant::now() + Duration::from_secs(2);
        let probing = tokio::spawn(async move {
            connection
                .call(
                    None,
                    "Target.getTargets",
                    json!({}),
                    super::super::super::Lane::Ordinary,
                    deadline,
                )
                .await
        });
        let mut request = Vec::new();
        tokio::time::timeout_at(deadline, peer.read_until(0, &mut request))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.pop(), Some(0));
        let request: Value = serde_json::from_slice(&request).unwrap();
        assert_eq!(request["method"], "Target.getTargets");
        let reply = json!({"id":request["id"],"result":{"targetInfos":[]}}).to_string();
        peer.get_mut()
            .write_all(&reply.as_bytes()[..4])
            .await
            .unwrap();
        tokio::task::yield_now().await;
        peer.get_mut()
            .write_all(&reply.as_bytes()[4..])
            .await
            .unwrap();
        peer.get_mut().write_all(&[0]).await.unwrap();
        assert_eq!(probing.await.unwrap().unwrap(), json!({"targetInfos":[]}));
    }
}
