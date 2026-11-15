//! `SCM_RIGHTS` with owned descriptors, bounded ancillary storage and close-on-exec.

use super::{Kind, Packet, Resource, read, remaining, write};
use crate::{Error, Result, net::Stream};
use rustix::{
    io::{FdFlags, fcntl_setfd},
    net::{
        RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags, SendAncillaryBuffer,
        SendAncillaryMessage, SendFlags, recvmsg, sendmsg,
    },
};
use std::{
    io::{IoSlice, IoSliceMut},
    mem::MaybeUninit,
    os::fd::{AsFd, BorrowedFd, OwnedFd},
    time::Instant,
};

impl Resource {
    fn descriptor(&self) -> BorrowedFd<'_> {
        match self {
            Self::File(v) => v.as_fd(),
            Self::TcpListener(v) => v.as_fd(),
            Self::TcpStream(v) => v.as_fd(),
            Self::LocalListener(v) => v.as_fd(),
            Self::LocalStream(v) => v.as_fd(),
        }
    }
}

pub(super) fn send(
    stream: &Stream,
    resource: &Resource,
    packet: &mut Packet,
    until: Instant,
) -> Result<()> {
    #[cfg(target_os = "macos")]
    rustix::net::sockopt::set_socket_nosigpipe(stream, true).map_err(std::io::Error::from)?;
    #[cfg(target_os = "linux")]
    let flags = SendFlags::NOSIGNAL;
    #[cfg(not(target_os = "linux"))]
    let flags = SendFlags::empty();
    let descriptors = [resource.descriptor()];
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut ancillary = SendAncillaryBuffer::new(&mut space);
    if !ancillary.push(SendAncillaryMessage::ScmRights(&descriptors)) {
        return Err(Error::Envelope);
    }
    loop {
        stream.set_write_timeout(Some(remaining(until)?))?;
        match sendmsg(stream, &[IoSlice::new(&[1])], &mut ancillary, flags) {
            Err(rustix::io::Errno::INTR) => {}
            Ok(1) => break,
            Ok(_) => return Err(Error::Envelope),
            Err(error) => return Err(std::io::Error::from(error).into()),
        }
    }
    write(stream, packet, until)
}

pub(super) fn receive(stream: &Stream, until: Instant) -> Result<(Resource, Packet)> {
    // XNU externalizes all rights before truncating control copyout, leaking
    // descriptors whose numbers do not fit. Cover its UIPC_MAX_CMSG_FD (512)
    // rather than receiving only the one descriptor our protocol accepts.
    // Linux's per-message SCM_MAX_FD (253) also fits. Reject/close every excess.
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(512))];
    let mut ancillary = RecvAncillaryBuffer::new(&mut space);
    let mut marker = [0];
    // Linux can set CLOEXEC atomically during receipt. Darwin requires fcntl;
    // products spawning concurrently must account for that platform window.
    #[cfg(target_os = "linux")]
    let flags = RecvFlags::CMSG_CLOEXEC;
    #[cfg(not(target_os = "linux"))]
    let flags = RecvFlags::empty();
    let received = loop {
        stream.set_read_timeout(Some(remaining(until)?))?;
        match recvmsg(
            stream,
            &mut [IoSliceMut::new(&mut marker)],
            &mut ancillary,
            flags,
        ) {
            Err(rustix::io::Errno::INTR) => {}
            result => break result.map_err(std::io::Error::from)?,
        }
    };
    let mut descriptors: Vec<OwnedFd> = Vec::new();
    for message in ancillary.drain() {
        if let RecvAncillaryMessage::ScmRights(fds) = message {
            descriptors.extend(fds);
        }
    }
    if received.bytes != 1
        || marker != [1]
        || received.flags.contains(ReturnFlags::CTRUNC)
        || descriptors.len() != 1
    {
        return Err(Error::Envelope);
    }
    let descriptor = descriptors.pop().ok_or(Error::Envelope)?;
    fcntl_setfd(&descriptor, FdFlags::CLOEXEC).map_err(std::io::Error::from)?;
    let packet: Packet = read(stream, until)?;
    if !packet.native.is_null() {
        return Err(Error::Envelope);
    }
    if !matches!(packet.kind, Kind::File) {
        use rustix::net::{AddressFamily, SocketType, getsockname, sockopt::socket_type};
        let family = getsockname(&descriptor)
            .map_err(std::io::Error::from)?
            .address_family();
        let local = matches!(packet.kind, Kind::LocalListener | Kind::LocalStream);
        if socket_type(&descriptor).map_err(std::io::Error::from)? != SocketType::STREAM
            || (local && family != AddressFamily::UNIX)
            || (!local && family != AddressFamily::INET && family != AddressFamily::INET6)
        {
            return Err(Error::Envelope);
        }
    }
    let resource = match packet.kind {
        Kind::File => Resource::File(descriptor.into()),
        Kind::TcpListener => Resource::TcpListener(descriptor.into()),
        Kind::TcpStream => Resource::TcpStream(descriptor.into()),
        Kind::LocalListener => Resource::LocalListener(descriptor.into()),
        Kind::LocalStream => Resource::LocalStream(descriptor.into()),
    };
    Ok((resource, packet))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        time::Duration,
    };

    #[test]
    fn transfer_missing_or_excess_ancillary_resources_are_closed_on_refusal() {
        let (mut left, right) = Stream::pair().unwrap();
        left.write_all(&[1]).unwrap();
        assert!(matches!(
            receive(&right, Instant::now() + Duration::from_secs(1)),
            Err(Error::Envelope)
        ));
        for count in [2, 3, 64] {
            let (left, right) = Stream::pair().unwrap();
            let mut originals = Vec::new();
            let mut peers = Vec::new();
            for _ in 0..count {
                let (original, peer) = Stream::pair().unwrap();
                peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                originals.push(original);
                peers.push(peer);
            }
            let descriptors: Vec<_> = originals.iter().map(AsFd::as_fd).collect();
            let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(64))];
            let mut ancillary = SendAncillaryBuffer::new(&mut space);
            assert!(ancillary.push(SendAncillaryMessage::ScmRights(&descriptors)));
            assert_eq!(
                sendmsg(
                    &left,
                    &[IoSlice::new(&[1])],
                    &mut ancillary,
                    SendFlags::empty()
                )
                .unwrap(),
                1
            );
            drop(originals);
            assert!(matches!(
                receive(&right, Instant::now() + Duration::from_secs(1)),
                Err(Error::Envelope)
            ));
            // A leaked received duplicate would keep one of these streams alive.
            for mut peer in peers {
                assert_eq!(peer.read(&mut [0]).unwrap(), 0);
            }
        }
    }
}
