//! Native resource copies. File handles are pulled into this process, so a
//! failed send cannot leak an unowned remote file duplicate. Sockets use Winsock.

use super::{check, is_user, open_token, token_user, wsa_error};
use crate::{
    Error, Result,
    net::Stream,
    transfer::{Kind, Packet, Resource, read, write},
};
use serde::{Deserialize, Serialize};
use std::{
    os::windows::io::{
        AsRawHandle, AsRawSocket, FromRawHandle, FromRawSocket, OwnedHandle, OwnedSocket,
        RawHandle, RawSocket,
    },
    ptr,
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle},
    Networking::WinSock::{
        AF_INET, AF_INET6, AF_UNIX, FROM_PROTOCOL_INFO, INVALID_SOCKET, SIO_AF_UNIX_GETPEERPID,
        SOCK_STREAM, WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED, WSADuplicateSocketW,
        WSAIoctl, WSAPROTOCOL_INFOW, WSAPROTOCOLCHAIN, WSASocketW,
    },
    Storage::FileSystem::{FILE_TYPE_CHAR, FILE_TYPE_DISK, FILE_TYPE_PIPE, GetFileType},
    System::Threading::{
        GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE, PROCESS_QUERY_LIMITED_INFORMATION,
    },
};

/// Serialize fields, never a struct's padding or an arbitrary memory image.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SocketInfo {
    flags: [u32; 5],
    provider: (u32, u16, u16, [u8; 8]),
    catalog: u32,
    chain: (i32, [u32; 7]),
    protocol: [i32; 9],
    sizes: [u32; 2],
    name: Vec<u16>,
}

impl From<WSAPROTOCOL_INFOW> for SocketInfo {
    fn from(v: WSAPROTOCOL_INFOW) -> Self {
        Self {
            flags: [
                v.dwServiceFlags1,
                v.dwServiceFlags2,
                v.dwServiceFlags3,
                v.dwServiceFlags4,
                v.dwProviderFlags,
            ],
            provider: (
                v.ProviderId.data1,
                v.ProviderId.data2,
                v.ProviderId.data3,
                v.ProviderId.data4,
            ),
            catalog: v.dwCatalogEntryId,
            chain: (v.ProtocolChain.ChainLen, v.ProtocolChain.ChainEntries),
            protocol: [
                v.iVersion,
                v.iAddressFamily,
                v.iMaxSockAddr,
                v.iMinSockAddr,
                v.iSocketType,
                v.iProtocol,
                v.iProtocolMaxOffset,
                v.iNetworkByteOrder,
                v.iSecurityScheme,
            ],
            sizes: [v.dwMessageSize, v.dwProviderReserved],
            name: v.szProtocol.to_vec(),
        }
    }
}

impl SocketInfo {
    fn native(self, kind: Kind) -> Result<WSAPROTOCOL_INFOW> {
        let local = matches!(kind, Kind::LocalListener | Kind::LocalStream);
        let family = self.protocol[1];
        if self.protocol[4] != SOCK_STREAM
            || !(0..=7).contains(&self.chain.0)
            || self.name.len() != 256
            || !self.name.contains(&0)
            || (local && family != i32::from(AF_UNIX))
            || (!local && family != i32::from(AF_INET) && family != i32::from(AF_INET6))
        {
            return Err(Error::Envelope);
        }
        let v = self;
        Ok(WSAPROTOCOL_INFOW {
            dwServiceFlags1: v.flags[0],
            dwServiceFlags2: v.flags[1],
            dwServiceFlags3: v.flags[2],
            dwServiceFlags4: v.flags[3],
            dwProviderFlags: v.flags[4],
            ProviderId: windows_sys::core::GUID {
                data1: v.provider.0,
                data2: v.provider.1,
                data3: v.provider.2,
                data4: v.provider.3,
            },
            dwCatalogEntryId: v.catalog,
            ProtocolChain: WSAPROTOCOLCHAIN {
                ChainLen: v.chain.0,
                ChainEntries: v.chain.1,
            },
            iVersion: v.protocol[0],
            iAddressFamily: v.protocol[1],
            iMaxSockAddr: v.protocol[2],
            iMinSockAddr: v.protocol[3],
            iSocketType: v.protocol[4],
            iProtocol: v.protocol[5],
            iProtocolMaxOffset: v.protocol[6],
            iNetworkByteOrder: v.protocol[7],
            iSecurityScheme: v.protocol[8],
            dwMessageSize: v.sizes[0],
            dwProviderReserved: v.sizes[1],
            szProtocol: v.name.try_into().map_err(|_| Error::Envelope)?,
        })
    }
}

fn peer(stream: &Stream, duplicate: bool) -> Result<(u32, OwnedHandle)> {
    let (mut pid, mut returned) = (0u32, 0u32);
    let socket = usize::try_from(stream.as_raw_socket()).map_err(|_| Error::Envelope)?;
    // SAFETY: the socket and correctly sized output remain live; synchronous ioctl.
    if unsafe {
        WSAIoctl(
            socket,
            SIO_AF_UNIX_GETPEERPID,
            ptr::null(),
            0,
            (&raw mut pid).cast(),
            4,
            &raw mut returned,
            ptr::null_mut(),
            None,
        )
    } != 0
    {
        return Err(wsa_error().into());
    }
    // The Windows AF_UNIX provider can populate the PID successfully while
    // leaving the returned-byte count at zero. Require the initialized output,
    // then verify its process token; the byte count is not peer identity.
    if pid == 0 {
        return Err(Error::Peer);
    }
    let access = PROCESS_QUERY_LIMITED_INFORMATION | if duplicate { PROCESS_DUP_HANDLE } else { 0 };
    // SAFETY: process lookup has no memory-safety preconditions.
    let process = unsafe { OpenProcess(access, 0, pid) };
    if process.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: successful OpenProcess returned one owned process handle.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    if !is_user(&token_user(&open_token(process.as_raw_handle())?)?) {
        return Err(Error::Peer);
    }
    Ok((pid, process))
}

pub(crate) fn send(
    stream: &Stream,
    resource: &Resource,
    packet: &mut Packet,
    until: Instant,
) -> Result<()> {
    let (pid, _process) = peer(stream, false)?;
    packet.native = if let Resource::File(file) = resource {
        serde_json::json!(file.as_raw_handle() as usize as u64)
    } else {
        let socket = match resource {
            Resource::TcpListener(v) => v.as_raw_socket(),
            Resource::TcpStream(v) => v.as_raw_socket(),
            Resource::LocalListener(v) => v.as_raw_socket(),
            Resource::LocalStream(v) => v.as_raw_socket(),
            Resource::File(_) => unreachable!(),
        };
        let socket = usize::try_from(socket).map_err(|_| Error::Envelope)?;
        let mut info = WSAPROTOCOL_INFOW::default();
        // SAFETY: source stays borrowed through acknowledgement; info is a
        // live output. Winsock binds the import information to the peer PID.
        if unsafe { WSADuplicateSocketW(socket, pid, &raw mut info) } != 0 {
            return Err(wsa_error().into());
        }
        serde_json::to_value(SocketInfo::from(info)).map_err(|_| Error::Envelope)?
    };
    write(stream, packet, until)
}

pub(crate) fn receive(stream: &Stream, until: Instant) -> Result<(Resource, Packet)> {
    let packet: Packet = read(stream, until)?;
    let resource = if matches!(packet.kind, Kind::File) {
        let (_pid, process) = peer(stream, true)?;
        let raw = usize::try_from(packet.native.as_u64().ok_or(Error::Envelope)?)
            .map_err(|_| Error::Envelope)?;
        // Pseudo handles are not transferable resources.
        if raw == 0 || raw >= usize::MAX - 16 {
            return Err(Error::Envelope);
        }
        let mut duplicate = ptr::null_mut();
        // SAFETY: source is only interpreted by DuplicateHandle in the verified
        // peer process. Success creates an owned, non-inheritable local handle.
        check(unsafe {
            DuplicateHandle(
                process.as_raw_handle(),
                raw as RawHandle,
                GetCurrentProcess(),
                &raw mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        })?;
        // SAFETY: successful duplication transferred ownership of this handle.
        let owned = unsafe { OwnedHandle::from_raw_handle(duplicate) };
        // SAFETY: GetFileType only queries the live, newly duplicated handle.
        if !matches!(
            unsafe { GetFileType(owned.as_raw_handle()) },
            FILE_TYPE_DISK | FILE_TYPE_PIPE | FILE_TYPE_CHAR
        ) {
            return Err(Error::Envelope);
        }
        Resource::File(owned.into())
    } else {
        let info: SocketInfo =
            serde_json::from_value(packet.native.clone()).map_err(|_| Error::Envelope)?;
        let info = info.native(packet.kind)?;
        // SAFETY: info is a validated, initialized structure, consumed by Winsock.
        // Success returns an owned socket, explicitly not inheritable.
        let socket = unsafe {
            WSASocketW(
                FROM_PROTOCOL_INFO,
                FROM_PROTOCOL_INFO,
                FROM_PROTOCOL_INFO,
                &raw const info,
                0,
                WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
            )
        };
        if socket == INVALID_SOCKET {
            return Err(wsa_error().into());
        }
        // SAFETY: WSASocketW returned an owned, open socket.
        let socket = unsafe { OwnedSocket::from_raw_socket(socket as RawSocket) };
        match packet.kind {
            Kind::TcpListener => Resource::TcpListener(socket.into()),
            Kind::TcpStream => Resource::TcpStream(socket.into()),
            Kind::LocalListener => {
                use std::os::windows::io::IntoRawSocket;
                // SAFETY: move the one owned socket into uds_windows's owner.
                Resource::LocalListener(unsafe {
                    crate::net::Listener::from_raw_socket(socket.into_raw_socket())
                })
            }
            Kind::LocalStream => {
                use std::os::windows::io::IntoRawSocket;
                // SAFETY: move the one owned socket into uds_windows's owner.
                Resource::LocalStream(unsafe { Stream::from_raw_socket(socket.into_raw_socket()) })
            }
            Kind::File => unreachable!(),
        }
    };
    Ok((resource, packet))
}
