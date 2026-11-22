//! Bounded, read-only Docker Engine API over an explicitly selected Unix socket.

use super::{Container, Error};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use serde::Deserialize;
use std::{
    fs,
    io::{ErrorKind, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MAX_RESPONSE: usize = 1024 * 1024;
const MAX_HEADER: usize = 16 * 1024;

pub(super) struct Engine {
    path: PathBuf,
    identity: (u64, u64),
    deadline: Instant,
}

impl Engine {
    pub(super) fn open(socket: &Path) -> Result<Self, Error> {
        if !socket.is_absolute() {
            return Err(Error::Observation("select an absolute Unix socket path"));
        }
        let path = socket
            .canonicalize()
            .map_err(|_| Error::Observation("socket unavailable"))?;
        Ok(Self {
            identity: socket_identity(&path)?,
            path,
            deadline: Instant::now() + Duration::from_secs(3),
        })
    }

    pub(super) fn revalidate(&self) -> Result<(), Error> {
        if socket_identity(&self.path)? != self.identity {
            return Err(Error::Observation("socket identity changed"));
        }
        Ok(())
    }

    pub(super) fn identity(&self) -> Result<String, Error> {
        #[derive(Deserialize)]
        struct Info {
            #[serde(rename = "ID")]
            id: String,
            #[serde(rename = "OSType")]
            os: String,
        }
        let info: Info = self.get("/info")?;
        if info.id.is_empty()
            || info.id.len() > 128
            || info.id.chars().any(char::is_control)
            || info.os != "linux"
        {
            return Err(Error::Observation("unexpected engine identity or platform"));
        }
        Ok(info.id)
    }

    pub(super) fn container(&self, id: &str) -> Result<Container, Error> {
        self.get(&format!("/containers/{id}/json"))
    }

    fn remaining(&self) -> Result<Duration, Error> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or(Error::Observation("inspection deadline exceeded"))
    }

    fn ready(&self, stream: &UnixStream, interest: PollFlags) -> Result<(), Error> {
        loop {
            let timeout = Timespec::try_from(self.remaining()?)
                .map_err(|_| Error::Observation("invalid inspection deadline"))?;
            let mut fds = [PollFd::new(stream, interest)];
            match poll(&mut fds, Some(&timeout)) {
                Ok(0) => return Err(Error::Observation("inspection deadline exceeded")),
                Ok(_) => return Ok(()),
                Err(rustix::io::Errno::INTR) => (),
                Err(_) => return Err(Error::Observation("could not poll engine connection")),
            }
        }
    }

    fn get<T: serde::de::DeserializeOwned>(&self, route: &str) -> Result<T, Error> {
        self.revalidate()?;
        let mut stream = connect(&self.path)?;
        self.revalidate()?;
        let request = format!(
            "GET {route} HTTP/1.0\r\nHost: localhost\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
        );
        let mut unsent = request.as_bytes();
        while !unsent.is_empty() {
            self.ready(&stream, PollFlags::OUT)?;
            match stream.write(unsent) {
                Ok(0) => {
                    return Err(Error::Observation(
                        "engine closed before receiving inspection request",
                    ));
                }
                Ok(count) => unsent = &unsent[count..],
                Err(error) if deferred(&error) => (),
                Err(_) => return Err(Error::Observation("could not send inspection request")),
            }
        }
        let mut response = Vec::new();
        let mut chunk = [0; 8192];
        loop {
            self.ready(&stream, PollFlags::IN)?;
            let count = match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if deferred(&error) => continue,
                Err(_) => return Err(Error::Observation("could not read inspection response")),
            };
            if count > MAX_RESPONSE - response.len() {
                return Err(Error::Observation("inspection response exceeds its limit"));
            }
            response.extend_from_slice(&chunk[..count]);
        }
        self.revalidate()?;
        decode(&response)
    }
}

fn deferred(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted)
}

fn connect(path: &Path) -> Result<UnixStream, Error> {
    use rustix::net::{AddressFamily, SocketAddrUnix, SocketType, connect, socket};
    let failure = |_| Error::Observation("could not connect to engine");
    let address = SocketAddrUnix::new(path).map_err(failure)?;
    let socket = socket(AddressFamily::UNIX, SocketType::STREAM, None).map_err(failure)?;
    rustix::io::fcntl_setfd(&socket, rustix::io::FdFlags::CLOEXEC).map_err(failure)?;
    rustix::fs::fcntl_setfl(&socket, rustix::fs::OFlags::NONBLOCK).map_err(failure)?;
    connect(&socket, &address).map_err(failure)?;
    Ok(UnixStream::from(socket))
}

fn socket_identity(path: &Path) -> Result<(u64, u64), Error> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| Error::Observation("socket unavailable"))?;
    if !metadata.file_type().is_socket() {
        return Err(Error::Observation("selected path is not a Unix socket"));
    }
    Ok((metadata.dev(), metadata.ino()))
}

fn decode<T: serde::de::DeserializeOwned>(response: &[u8]) -> Result<T, Error> {
    let invalid = || Error::Observation("invalid inspection response");
    let separator = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .filter(|end| *end <= MAX_HEADER)
        .ok_or_else(invalid)?;
    let header = std::str::from_utf8(&response[..separator]).map_err(|_| invalid())?;
    let mut lines = header.split("\r\n");
    let mut status = lines.next().ok_or_else(invalid)?.split_ascii_whitespace();
    if !matches!(status.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
        return Err(invalid());
    }
    if status.next() != Some("200") {
        return Err(Error::Observation("engine rejected inspection request"));
    }
    let body = &response[separator + 4..];
    let mut length = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(invalid)?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(invalid());
        }
        if name.eq_ignore_ascii_case("content-length") {
            let value = value.trim().parse::<usize>().map_err(|_| invalid())?;
            if length.replace(value).is_some() || value != body.len() {
                return Err(invalid());
            }
        }
    }
    serde_json::from_slice(body).map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_truncated_or_ambiguous_response_framing() {
        let decode = |bytes: &[u8]| decode::<serde_json::Value>(bytes);
        assert!(decode(b"HTTP/1.0 200 OK\r\n\r\n{}").is_ok());
        assert!(decode(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\n{}").is_err());
        assert!(decode(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{}").is_err());
    }

    #[test]
    fn inspection_socket_readiness_has_an_absolute_deadline() {
        let (stream, _peer) = UnixStream::pair().unwrap();
        let engine = Engine {
            path: PathBuf::new(),
            identity: (0, 0),
            deadline: Instant::now() + Duration::from_millis(20),
        };
        assert!(matches!(
            engine.ready(&stream, PollFlags::IN),
            Err(Error::Observation("inspection deadline exceeded"))
        ));
    }
}
