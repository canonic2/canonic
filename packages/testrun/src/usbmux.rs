//! Native usbmuxd framing and USB-only tunnels; no separately installed proxy.
use crate::error::{Error, Result};
#[cfg(unix)]
use plist::{Dictionary, Value};
use std::path::Path;
#[cfg(unix)]
use std::{io::Cursor, time::Duration};
#[cfg(unix)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone)]
pub struct Device {
    pub id: u32,
    pub serial: String,
}
const MAX_PACKET: usize = 1024 * 1024;
pub fn header(length: usize, tag: u32) -> Result<[u8; 16]> {
    if !(16..=MAX_PACKET).contains(&length) {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "USB packet exceeds its framing bounds",
        ));
    }
    let mut bytes = [0; 16];
    for (index, value) in [length as u32, 1, 8, tag].into_iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    Ok(bytes)
}
pub fn payload_length(bytes: &[u8; 16], tag: u32) -> Result<usize> {
    let words: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    if !(16..=MAX_PACKET as u32).contains(&words[0])
        || words[1] != 1
        || words[2] != 8
        || words[3] != tag
    {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "Invalid USB response header",
        ));
    }
    Ok(words[0] as usize - 16)
}
#[cfg(unix)]
fn request(kind: &str) -> Dictionary {
    let mut values = Dictionary::new();
    for (key, value) in [
        ("MessageType", kind),
        ("ClientVersionString", env!("CARGO_PKG_VERSION")),
        ("ProgName", "Testrun"),
    ] {
        values.insert(key.into(), Value::String(value.into()));
    }
    values.insert("kLibUSBMuxVersion".into(), Value::Integer(3.into()));
    values
}
#[cfg(unix)]
async fn exchange(socket: &mut tokio::net::UnixStream, request: Dictionary) -> Result<Value> {
    let mut bytes = Vec::new();
    Value::Dictionary(request)
        .to_writer_xml(&mut bytes)
        .map_err(|_| Error::new("INTERNAL", "Cannot encode USB request"))?;
    socket
        .write_all(&header(bytes.len() + 16, 1)?)
        .await
        .map_err(|_| Error::new("USB_UNAVAILABLE", "Cannot write USB header"))?;
    socket
        .write_all(&bytes)
        .await
        .map_err(|_| Error::new("USB_UNAVAILABLE", "Cannot write USB request"))?;
    let mut prefix = [0; 16];
    socket
        .read_exact(&mut prefix)
        .await
        .map_err(|_| Error::new("USB_UNAVAILABLE", "USB service disconnected"))?;
    let mut bytes = vec![0; payload_length(&prefix, 1)?];
    socket
        .read_exact(&mut bytes)
        .await
        .map_err(|_| Error::new("USB_UNAVAILABLE", "USB response was interrupted"))?;
    Value::from_reader(Cursor::new(bytes))
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid USB property list"))
}
#[cfg(unix)]
pub async fn list(path: &Path) -> Result<Vec<Device>> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut socket = tokio::net::UnixStream::connect(path).await.map_err(|_| {
            Error::new(
                "USB_UNAVAILABLE",
                "Host USB multiplexing service is unavailable",
            )
        })?;
        let response = exchange(&mut socket, request("ListDevices")).await?;
        let items = response
            .as_dictionary()
            .and_then(|d| d.get("DeviceList"))
            .and_then(Value::as_array)
            .ok_or_else(|| {
                Error::new("PLATFORM_PROTOCOL", "USB service omitted its device list")
            })?;
        let mut devices = Vec::new();
        for item in items {
            let Some(item) = item.as_dictionary() else {
                continue;
            };
            let Some(properties) = item.get("Properties").and_then(Value::as_dictionary) else {
                continue;
            };
            if properties.get("ConnectionType").and_then(Value::as_string) != Some("USB") {
                continue;
            }
            if let (Some(id), Some(serial)) = (
                item.get("DeviceID").and_then(Value::as_unsigned_integer),
                properties.get("SerialNumber").and_then(Value::as_string),
            ) && let Ok(id) = u32::try_from(id)
            {
                devices.push(Device {
                    id,
                    serial: serial.to_owned(),
                });
            }
        }
        Ok(devices)
    })
    .await
    .map_err(|_| Error::new("TIMEOUT", "USB discovery exceeded its deadline"))?
}
#[cfg(unix)]
pub async fn connect(path: &Path, device: u32, port: u16) -> Result<tokio::net::UnixStream> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut socket = tokio::net::UnixStream::connect(path)
            .await
            .map_err(|_| Error::new("USB_UNAVAILABLE", "Cannot connect to host USB service"))?;
        let mut values = request("Connect");
        values.insert("DeviceID".into(), Value::Integer(u64::from(device).into()));
        values.insert(
            "PortNumber".into(),
            Value::Integer(u64::from(port.to_be()).into()),
        );
        let response = exchange(&mut socket, values).await?;
        if response
            .as_dictionary()
            .and_then(|d| d.get("Number"))
            .and_then(Value::as_unsigned_integer)
            != Some(0)
        {
            return Err(Error::new(
                "RUNNER_UNREACHABLE",
                "Owned device runner is not listening; verify setup, unlock and Developer Mode",
            ));
        }
        Ok(socket)
    })
    .await
    .map_err(|_| Error::new("TIMEOUT", "USB connection exceeded its deadline"))?
}
#[cfg(not(unix))]
pub async fn list(_path: &Path) -> Result<Vec<Device>> {
    Err(Error::new(
        "UNSUPPORTED_HOST",
        "USB device adapter currently requires a Unix host",
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn discovers_only_usb_and_connects_with_network_order_port() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mux.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let host = tokio::spawn(async move {
            for number in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut prefix = [0; 16];
                socket.read_exact(&mut prefix).await.unwrap();
                let mut bytes = vec![0; payload_length(&prefix, 1).unwrap()];
                socket.read_exact(&mut bytes).await.unwrap();
                let request = Value::from_reader(Cursor::new(bytes)).unwrap();
                let response = if number == 0 {
                    assert_eq!(
                        request.as_dictionary().unwrap()["MessageType"].as_string(),
                        Some("ListDevices")
                    );
                    plist::from_bytes::<Value>(br#"<plist version="1.0"><dict><key>DeviceList</key><array><dict><key>DeviceID</key><integer>7</integer><key>Properties</key><dict><key>ConnectionType</key><string>USB</string><key>SerialNumber</key><string>Acme</string></dict></dict><dict><key>DeviceID</key><integer>8</integer><key>Properties</key><dict><key>ConnectionType</key><string>Network</string><key>SerialNumber</key><string>Ignore</string></dict></dict></array></dict></plist>"#).unwrap()
                } else {
                    let request = request.as_dictionary().unwrap();
                    assert_eq!(request["DeviceID"].as_unsigned_integer(), Some(7));
                    assert_eq!(
                        request["PortNumber"].as_unsigned_integer(),
                        Some(u64::from(8101u16.to_be()))
                    );
                    let mut reply = Dictionary::new();
                    reply.insert("Number".into(), Value::Integer(0.into()));
                    Value::Dictionary(reply)
                };
                let mut bytes = Vec::new();
                response.to_writer_xml(&mut bytes).unwrap();
                socket
                    .write_all(&header(bytes.len() + 16, 1).unwrap())
                    .await
                    .unwrap();
                socket.write_all(&bytes).await.unwrap();
            }
        });
        let devices = list(&path).await.unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].serial, "Acme");
        connect(&path, devices[0].id, 8101).await.unwrap();
        host.await.unwrap();
    }
    #[test]
    fn validates_usb_headers_before_allocating_payloads() {
        let packet = header(100, 42).unwrap();
        assert_eq!(payload_length(&packet, 42).unwrap(), 84);
        assert!(payload_length(&packet, 41).is_err());
        let mut invalid = packet;
        invalid[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(payload_length(&invalid, 42).is_err());
        assert!(header(15, 1).is_err());
    }
}
