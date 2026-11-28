//! Owned USB CoreDevice media transport, capability checks and session lifecycle.
use crate::error::{Error, Result};
#[cfg(unix)]
use idevice::{
    IdeviceService,
    core_device_proxy::CoreDeviceProxy,
    rsd::RsdHandshake,
    usbmuxd::{Connection, UsbmuxdAddr},
};
use idevice::{ReadWrite, RemoteXpcClient, tcp::handle::AdapterHandle};
use serde_json::{Value, json};
use std::time::Duration;
#[cfg(target_os = "macos")]
pub(crate) mod apple;
#[cfg(target_os = "macos")]
pub(crate) mod audio;
mod core_error;
#[cfg(target_os = "macos")]
mod hevc;
#[cfg(target_os = "macos")]
mod lifecycle;
pub(crate) mod lock;
#[cfg(target_os = "macos")]
mod negotiation;
#[cfg(target_os = "macos")]
mod observation;
#[cfg(target_os = "macos")]
pub(crate) mod record;
#[cfg(target_os = "macos")]
mod registry;
#[cfg(target_os = "macos")]
mod source;
#[cfg(target_os = "macos")]
mod startup;
#[cfg(target_os = "macos")]
mod stream;
#[cfg(unix)]
mod tunnel;
#[cfg(target_os = "macos")]
pub use registry::Sources;
#[cfg(target_os = "macos")]
pub use source::{Source, VideoFrame};

type Service = RemoteXpcClient<Box<dyn ReadWrite>>;

#[cfg(target_os = "macos")]
fn owner_id(session: &Value) -> Option<&str> {
    session["connection"]["options"]["avcMediaStreamOptionClientSessionID"]["uuid"].as_str()
}

#[cfg(not(unix))]
fn unsupported_usb_host() -> Error {
    Error::new(
        "UNSUPPORTED_HOST",
        "CoreDevice USB media requires a Unix host",
    )
}

pub struct ConnectionOwner {
    pub(crate) tunnel: AdapterHandle,
    pub(crate) service: Service,
    pub(crate) version: String,
    owned_tunnel: Option<tokio::sync::oneshot::Receiver<()>>,
    #[cfg(target_os = "macos")]
    display_port: u16,
    #[cfg(target_os = "macos")]
    hid_port: Option<u16>,
    #[cfg(target_os = "macos")]
    indigo_port: Option<u16>,
    #[cfg(target_os = "macos")]
    screenshot_port: Option<u16>,
    #[cfg(target_os = "macos")]
    device_info_port: Option<u16>,
}
pub(super) fn platform(error: idevice::IdeviceError) -> Error {
    platform_at(error, "coredevice_transport")
}
fn platform_at(error: idevice::IdeviceError, phase: &str) -> Error {
    // Keep dependency diagnostics bounded; pairing material never enters errors.
    let (code, state, device, retryable) = match &error {
        idevice::IdeviceError::Socket(io) if io.kind() == std::io::ErrorKind::PermissionDenied => (
            "HOST_ACCESS_REQUIRED",
            "host_access_denied",
            "unknown",
            false,
        ),
        idevice::IdeviceError::Socket(_) | idevice::IdeviceError::Timeout => (
            "DEVICE_MEDIA_UNAVAILABLE",
            "socket_unavailable",
            "unknown",
            true,
        ),
        idevice::IdeviceError::DeviceLocked => ("DEVICE_LOCKED", "device_locked", "locked", false),
        idevice::IdeviceError::DeviceNotFound => (
            "DEVICE_DISCONNECTED",
            "device_disconnected",
            "disconnected",
            false,
        ),
        _ => (
            "DEVICE_MEDIA_UNAVAILABLE",
            "transport_unavailable",
            "unknown",
            false,
        ),
    };
    // Device error envelopes arrive as XPC debug text with archived bytes.
    let device_error = match &error {
        idevice::IdeviceError::CoreDevice(idevice::core_device::CoreDeviceError::DeviceError(
            text,
        )) => Some(core_error::summarize(text)),
        _ => None,
    };
    // An error with nothing readable keeps the device's own text.
    let reason = device_error
        .as_ref()
        .and_then(core_error::Summary::message)
        .unwrap_or_else(|| error.to_string());
    let mut failure = Error::new(
        code,
        format!(
            "CoreDevice transport: {}",
            reason.chars().take(256).collect::<String>()
        ),
    );
    failure.retryable = retryable;
    let mut details = json!({"blockingState":state,"phase":phase,"deviceState":device,"requiresHostAccess":code == "HOST_ACCESS_REQUIRED"});
    if let Some(summary) = device_error {
        details["deviceError"] = json!({"domain":summary.domain,"code":summary.code});
    }
    if let idevice::IdeviceError::Socket(io) = error {
        details["osError"] = json!(io.raw_os_error());
        details["ioKind"] = json!(format!("{:?}", io.kind()));
    }
    failure.details(details)
}
impl Drop for ConnectionOwner {
    fn drop(&mut self) {
        if self.owned_tunnel.is_none() {
            return;
        }
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let mut tunnel = self.tunnel.clone();
            runtime.spawn(async move {
                let _ = tunnel.close().await;
            });
        }
    }
}
impl ConnectionOwner {
    pub async fn connect(serial: &str) -> Result<Self> {
        tokio::time::timeout(Duration::from_secs(20), Self::connect_inner(serial))
            .await
            .map_err(|_| {
                let mut error =
                    platform_at(idevice::IdeviceError::Timeout, "transport_acquisition");
                error.message = "CoreDevice connection exceeded twenty seconds".into();
                error
            })?
    }
    #[cfg(unix)]
    async fn usb_proxy(serial: &str) -> Result<CoreDeviceProxy> {
        let address = UsbmuxdAddr::UnixSocket("/var/run/usbmuxd".into());
        let mut mux = address
            .connect(1)
            .await
            .map_err(|error| platform_at(error, "usb_daemon_connect"))?;
        let device = mux
            .get_devices()
            .await
            .map_err(|error| platform_at(error,"usb_device_discovery"))?
            .into_iter()
            .find(|d| d.udid == serial && d.connection_type == Connection::Usb)
            .ok_or_else(|| {
                Error::new("USB_UNAVAILABLE", "Selected device has no USB connection")
                    .details(json!({"blockingState":"device_disconnected","deviceState":"usb_disconnected","phase":"usb_device_discovery"}))
            })?;
        let provider = device.to_provider(address, "Testrun");
        CoreDeviceProxy::connect(&provider)
            .await
            .map_err(|error| platform_at(error, "coredevice_proxy_connect"))
    }
    #[cfg(unix)]
    pub(crate) async fn usb_tunnel(serial: &str) -> Result<(AdapterHandle, u16)> {
        let proxy = Self::usb_proxy(serial).await?;
        let port = proxy.tunnel_info().server_rsd_port;
        let tunnel = proxy
            .create_software_tunnel()
            .map_err(platform)?
            .to_async_handle();
        Ok((tunnel, port))
    }
    #[cfg(not(unix))]
    pub(crate) async fn usb_tunnel(_serial: &str) -> Result<(AdapterHandle, u16)> {
        Err(unsupported_usb_host())
    }
    #[cfg(unix)]
    async fn connect_inner(serial: &str) -> Result<Self> {
        let proxy = Self::usb_proxy(serial).await?;
        let (mut tunnel, port, mut released) = tunnel::from_proxy(proxy)?;
        let result = async {
            let rsd = RsdHandshake::new(
                tunnel
                    .connect(port)
                    .await
                    .map_err(|error| platform_at(error.into(), "device_discovery_connect"))?,
            )
            .await
            .map_err(platform)?;
            if rsd
                .properties
                .get("UniqueDeviceID")
                .and_then(plist::Value::as_string)
                != Some(serial)
            {
                return Err(Error::new(
                    "STALE_TARGET",
                    "CoreDevice identity does not match selected device",
                ));
            }
            let version = rsd
                .properties
                .get("OSVersion")
                .and_then(plist::Value::as_string)
                .unwrap_or("")
                .to_owned();
            require_version(&version)?;
            let port = rsd
                .services
                .get("com.apple.coredevice.displayservice")
                .ok_or_else(|| {
                    Error::new(
                        "DEVELOPER_SUPPORT_REQUIRED",
                        "Compatible developer support must expose the CoreDevice display service",
                    ).details(json!({"osVersion":version,"displayService":"absent","nextAction":"doctor","requiredService":"com.apple.coredevice.displayservice"}))
                })?
                .port;
            let stream = tunnel
                .connect(port)
                .await
                .map_err(|error| platform_at(error.into(), "display_service_connect"))?;
            let mut service = RemoteXpcClient::new(Box::new(stream) as Box<dyn ReadWrite>)
                .await
                .map_err(platform)?;
            service.do_handshake().await.map_err(platform)?;
            let port_of = |name: &str| rsd.services.get(name).map(|s| s.port);
            let hid_port = port_of("com.apple.coredevice.hid.universalhidservice");
            let screenshot_port = port_of("com.apple.coredevice.screencaptureservice");
            let indigo_port = port_of("com.apple.coredevice.hid.indigo");
            let device_info_port = port_of("com.apple.coredevice.deviceinfo");
            Ok((version, service, hid_port, screenshot_port, indigo_port, port, device_info_port))
        }
        .await;
        match result {
            Ok((
                version,
                service,
                hid_port,
                screenshot_port,
                indigo_port,
                display_port,
                device_info_port,
            )) => {
                #[cfg(not(target_os = "macos"))]
                let _ = (
                    hid_port,
                    screenshot_port,
                    indigo_port,
                    display_port,
                    device_info_port,
                );
                Ok(Self {
                    tunnel,
                    service,
                    version,
                    owned_tunnel: Some(released),
                    #[cfg(target_os = "macos")]
                    display_port,
                    #[cfg(target_os = "macos")]
                    hid_port,
                    #[cfg(target_os = "macos")]
                    indigo_port,
                    #[cfg(target_os = "macos")]
                    screenshot_port,
                    #[cfg(target_os = "macos")]
                    device_info_port,
                })
            }
            Err(error) => {
                tunnel::shutdown(&mut tunnel, &mut released).await;
                Err(error)
            }
        }
    }
    #[cfg(not(unix))]
    async fn connect_inner(_serial: &str) -> Result<Self> {
        Err(unsupported_usb_host())
    }
    #[cfg(target_os = "macos")]
    async fn close(&mut self) {
        if let Some(released) = &mut self.owned_tunnel {
            tunnel::shutdown(&mut self.tunnel, released).await;
            self.owned_tunnel = None;
        }
    }
    pub async fn status(&mut self) -> Result<Value> {
        self.invoke(
            "com.apple.coredevice.feature.getmediastreamserverstatus",
            "com.apple.coredevice.action.mediastreamstatus",
            idevice::xpc::Dictionary::new(),
        )
        .await
    }
    #[cfg(target_os = "macos")]
    fn audio_context(&self, session: uuid::Uuid) -> audio::Context {
        audio::Context {
            tunnel: self.tunnel.clone(),
            port: self.display_port,
            version: self.version.clone(),
            session,
        }
    }
    #[cfg(target_os = "macos")]
    async fn shared(context: &audio::Context) -> Result<Self> {
        let mut tunnel = context.tunnel.clone();
        let stream = tunnel
            .connect(context.port)
            .await
            .map_err(|e| platform(e.into()))?;
        let mut service = RemoteXpcClient::new(Box::new(stream) as Box<dyn ReadWrite>)
            .await
            .map_err(platform)?;
        service.do_handshake().await.map_err(platform)?;
        Ok(Self {
            tunnel,
            service,
            version: context.version.clone(),
            owned_tunnel: None,
            display_port: context.port,
            hid_port: None,
            indigo_port: None,
            screenshot_port: None,
            device_info_port: None,
        })
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn hid_transport(&self) -> Result<(AdapterHandle, u16)> {
        let port = self.hid_port.ok_or_else(|| {
            Error::new(
                "DEVELOPER_SUPPORT_REQUIRED",
                "CoreDevice HID service is absent",
            )
        })?;
        Ok((self.tunnel.clone(), port))
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn consumer_transport(&self) -> Result<(AdapterHandle, u16)> {
        self.indigo_port
            .map(|port| (self.tunnel.clone(), port))
            .ok_or_else(|| {
                Error::new(
                    "DEVELOPER_SUPPORT_REQUIRED",
                    "CoreDevice Indigo service is absent",
                )
            })
    }
    #[cfg(target_os = "macos")]
    pub(crate) async fn open_hid(
        mut tunnel: AdapterHandle,
        port: u16,
    ) -> Result<std::sync::Arc<crate::hid::Hid>> {
        let stream = tunnel
            .connect(port)
            .await
            .map_err(|error| platform_at(error.into(), "hid_channel_connect"))?;
        let mut service = RemoteXpcClient::new(Box::new(stream) as Box<dyn ReadWrite>)
            .await
            .map_err(platform)?;
        service.do_handshake().await.map_err(platform)?;
        crate::hid::Hid::open(service).await
    }
    #[cfg(target_os = "macos")]
    pub(crate) async fn display_geometry(&mut self) -> Result<(u32, u32)> {
        use idevice::xpc::{Dictionary, XPCObject as X};
        let port = self.screenshot_port.ok_or_else(|| {
            Error::new(
                "CAPABILITY_UNAVAILABLE",
                "CoreDevice screenshot service is required to calibrate HID video geometry",
            )
        })?;
        let stream = self
            .tunnel
            .connect(port)
            .await
            .map_err(|error| platform_at(error.into(), "display_calibration_connect"))?;
        let mut service = RemoteXpcClient::new(Box::new(stream) as Box<dyn ReadWrite>)
            .await
            .map_err(platform)?;
        service.do_handshake().await.map_err(platform)?;
        let previous = std::mem::replace(&mut self.service, service);
        let mut args = Dictionary::new();
        args.insert("requestedFormat".into(), X::String("png".into()));
        args.insert("displayUniqueID".into(), X::Null);
        let result = self
            .invoke(
                "com.apple.coredevice.feature.capturescreenshot",
                "com.apple.coredevice.action.capturescreenshot",
                args,
            )
            .await;
        self.service = previous;
        let result = result?;
        let bytes: Vec<u8> = serde_json::from_value(result["image"].clone())
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Display calibration omitted PNG data"))?;
        crate::ui::png_dimensions(&bytes)
    }
    pub(crate) async fn invoke(
        &mut self,
        feature: &str,
        action: &str,
        input: idevice::xpc::Dictionary,
    ) -> Result<Value> {
        use idevice::xpc::{Dictionary, XPCObject as X};
        let mut version = Dictionary::new();
        version.insert(
            "components".into(),
            X::Array(vec![X::UInt64(629), X::UInt64(3)]),
        );
        version.insert("originalComponentsCount".into(), X::Int64(2));
        version.insert("stringValue".into(), X::String("629.3".into()));
        let mut request = Dictionary::new();
        request.insert(
            "CoreDevice.CoreDeviceDDIProtocolVersion".into(),
            X::Int64(2),
        );
        request.insert(
            "CoreDevice.coreDeviceVersion".into(),
            X::Dictionary(version),
        );
        for (key, value) in [
            (
                "CoreDevice.deviceIdentifier",
                uuid::Uuid::new_v4().to_string(),
            ),
            (
                "CoreDevice.invocationIdentifier",
                uuid::Uuid::new_v4().to_string(),
            ),
            ("CoreDevice.featureIdentifier", feature.into()),
            ("CoreDevice.actionIdentifier", action.into()),
        ] {
            request.insert(key.into(), X::String(value));
        }
        request.insert("CoreDevice.action".into(), X::Dictionary(Dictionary::new()));
        request.insert("CoreDevice.input".into(), X::Dictionary(input));
        tokio::time::timeout(Duration::from_secs(20),async{
            self.service.send_object(request,true).await.map_err(platform)?;
            let reply=self.service.recv().await.map_err(platform)?;
            let output=reply.as_dictionary().and_then(|v|v.get("CoreDevice.output"));
            match output{
                Some(value)=>serde_json::to_value(value).map_err(|_|Error::new("PLATFORM_PROTOCOL","Invalid media reply")),
                None=>{let code=reply.as_dictionary().and_then(|v|v.get("CoreDevice.error")).and_then(plist::Value::as_dictionary).and_then(|v|v.get("code")).and_then(plist::Value::as_signed_integer);
                    Err(Error::new(match code{Some(9021)=>"CAPABILITY_UNAVAILABLE",Some(9022)=>"SENSOR_IN_USE",_=>"DEVICE_MEDIA_UNAVAILABLE"},"Device rejected the media request; verify unlock, developer support and sensor availability").details(json!({"nativeCode":code})))},
            }
        }).await.map_err(|_|Error::new("TIMEOUT","CoreDevice media request timed out"))?
    }
}
pub fn require_version(version: &str) -> Result<()> {
    if version
        .split('.')
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .is_none_or(|v| v < 27)
    {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Physical streaming and recording require iOS 27+",
        ));
    }
    Ok(())
}
pub async fn inspect(serial: &str) -> Result<Value> {
    let mut connection = ConnectionOwner::connect(serial).await?;
    let status = connection.status().await?;
    Ok(json!({"osVersion":connection.version,"status":status}))
}

#[cfg(test)]
mod tests;
