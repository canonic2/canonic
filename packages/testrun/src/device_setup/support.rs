//! Rootless developer-image inventory, exclusive replacement and service verification.
use super::*;
use idevice::{
    RsdService,
    cryptexd::{CryptexdClient, InstalledCryptex, assets::Cryptex1Assets},
    rsd::RsdHandshake,
};

mod assets;

pub(super) fn asset_build() -> &'static str {
    assets::BUILD
}

pub(super) fn image_summary(image: &plist::Value) -> Value {
    let get = |key| image_field(image, key);
    json!({"type":get("DiskImageType"),"mountPath":get("MountPath"),"build":get("BuildVersion").or_else(||get("BuildUpdate"))})
}

fn image_field<'a>(image: &'a plist::Value, key: &str) -> Option<&'a str> {
    image.as_dictionary()?.get(key)?.as_string()
}

pub(super) async fn competing_tools() -> Result<Vec<String>> {
    let output = crate::process::run(
        "/bin/ps",
        &["-axo", "comm="],
        Duration::from_secs(5),
        1024 * 1024,
    )
    .await?;
    Ok(String::from_utf8_lossy(&output.bytes)
        .lines()
        .filter_map(|line| {
            let name = std::path::Path::new(line.trim()).file_name()?.to_str()?;
            matches!(name, "Xcode" | "xcodebuild").then(|| name.to_owned())
        })
        .collect())
}
pub(super) async fn services(serial: &str) -> Result<Value> {
    let (handle, port) = crate::device_media::ConnectionOwner::usb_tunnel(serial).await?;
    let mut tunnel = OwnedTunnel {
        handle,
        closed: false,
    };
    let result = async {
        let stream = tunnel
            .handle
            .connect(port)
            .await
            .map_err(|e| platform(e.into()))?;
        let mut rsd = RsdHandshake::new(stream).await.map_err(platform)?;
        if rsd
            .properties
            .get("UniqueDeviceID")
            .and_then(plist::Value::as_string)
            != Some(serial)
        {
            return Err(Error::new(
                "STALE_TARGET",
                "Developer transport identity does not match the selected device",
            ));
        }
        let display = rsd
            .services
            .contains_key("com.apple.coredevice.displayservice");
        let hid = rsd
            .services
            .contains_key("com.apple.coredevice.hid.universalhidservice");
        let installed = idevice::cryptexd::installed_ddi(&mut tunnel.handle, &mut rsd)
            .await
            .ok()
            .flatten();
        let lock = crate::device_media::lock::bounded(lock_state(&mut tunnel.handle, &rsd)).await;
        Ok(service_report(display, hid, installed, lock))
    }
    .await;
    let cleanup = tunnel.close().await;
    match (result, cleanup) {
        (Ok(v), Ok(())) => Ok(v),
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

fn service_report(
    display: bool,
    hid: bool,
    installed: Option<InstalledCryptex>,
    mut lock: Value,
) -> Value {
    let compatible = display && hid;
    let mut support = if compatible {
        json!({"state":"compatible","nextAction":"connect"})
    } else {
        json!({"state":"required","nextAction":"developer_support_install"})
    };
    if let Some(installed) = installed {
        if installed.version == "27.1.5228.8" {
            support["build"] = json!(assets::BUILD);
        }
        support["identifier"] = json!(installed.identifier);
        support["version"] = json!(installed.version);
    }
    if compatible && lock["state"] == "unknown" {
        lock["nextAction"] = json!("connect");
    }
    json!({"displayService":{"state":if display{"available"}else{"absent"}},"hidService":{"state":if hid{"available"}else{"absent"}},"developerSupport":support,"lock":lock})
}
async fn lock_state(
    tunnel: &mut idevice::tcp::handle::AdapterHandle,
    rsd: &RsdHandshake,
) -> Result<Value> {
    let port = rsd
        .services
        .get("com.apple.coredevice.deviceinfo")
        .ok_or_else(|| {
            Error::new(
                "CAPABILITY_UNAVAILABLE",
                "The device-info service is not advertised by this device",
            )
        })?
        .port;
    crate::device_media::lock::probe(tunnel, port).await
}
pub(super) async fn prepare(target: &str) -> Result<Value> {
    let serial = super::serial(target)?;
    let _target = setup_lease(target)?;
    let _media = setup_lease(&format!("device-media:{serial}"))?;
    tokio::time::timeout(Duration::from_secs(600), prepare_inner(target, serial))
        .await
        .map_err(|_| {
            Error::new(
                "PREPARATION_OUTCOME_UNKNOWN",
                "Developer support preparation timed out; inspect doctor before trying again",
            )
            .details(json!({"nextAction":"doctor"}))
        })?
}
async fn prepare_inner(target: &str, serial: &str) -> Result<Value> {
    let before = preparation_doctor(target).await?;
    if before["checks"]["developerSupport"]["state"] == "compatible" {
        return Ok(json!({"targetId":target,"changed":false,"verified":before}));
    }
    if before["checks"]["developerMode"]["state"] != "enabled" {
        return Err(Error::new(
            "DEVELOPER_MODE_REQUIRED",
            "Enable Developer Mode on the selected phone before preparing support",
        )
        .details(json!({"nextAction":"developer_mode_reveal","readiness":before})));
    }
    if before["checks"]["lock"]["state"] == "locked" {
        return Err(Error::new(
            "DEVICE_LOCKED",
            "Unlock the selected device before preparing developer support",
        )
        .details(json!({"nextAction":"doctor"})));
    }
    let identity = identity(serial).await?;
    let version = identity
        .get("osVersion")
        .and_then(Value::as_str)
        .unwrap_or("");
    if version
        .split('.')
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .is_none_or(|v| v < 27)
    {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Cryptex developer support preparation requires iOS 27 or later",
        ));
    }
    let assets = assets::load().await?;
    if let Err(error) = install(serial, assets).await {
        if matches!(
            error.code.as_str(),
            "DEVELOPER_SUPPORT_IN_USE" | "DEVELOPER_SUPPORT_CHECK_FAILED"
        ) {
            return Err(error);
        }
        let observed = preparation_doctor(target).await.ok();
        return Err(Error::new("DEVELOPER_SUPPORT_PREPARATION_FAILED",error.message)
            .details(json!({"cause":error.code,"nextAction":"doctor","readiness":observed,"replacementMayHaveStarted":true})));
    }
    let after = preparation_doctor(target).await?;
    if after["checks"]["developerSupport"]["state"] != "compatible" {
        return Err(Error::new("DEVELOPER_SUPPORT_REQUIRED","Support installation completed but required display/HID services are still unavailable").details(json!({"nextAction":"doctor","readiness":after})));
    }
    Ok(json!({"targetId":target,"changed":true,"assetBuild":assets::BUILD,"verified":after}))
}
async fn install(serial: &str, assets: Cryptex1Assets) -> Result<()> {
    let (handle, port) = crate::device_media::ConnectionOwner::usb_tunnel(serial).await?;
    let mut tunnel = OwnedTunnel {
        handle,
        closed: false,
    };
    let result=async {
        let stream=tunnel.handle.connect(port).await.map_err(|error|platform(error.into()))?;
        let mut rsd=RsdHandshake::new(stream).await.map_err(platform)?;
        if rsd.properties.get("UniqueDeviceID").and_then(plist::Value::as_string)!=Some(serial) {
            return Err(Error::new("STALE_TARGET","Developer transport identity changed"));
        }
        // Personalization must succeed before existing support is touched.
        let request=personalize(&mut tunnel.handle,&mut rsd,assets).await?;
        if rsd.services.contains_key("com.apple.coredevice.displayservice") {
            let mut display=crate::device_media::ConnectionOwner::connect(serial).await?;
            let status=display.status().await?;
            if status["running"]!=false || status["sessions"].as_array().is_none_or(|sessions|!sessions.is_empty()) {
                return Err(Error::new(
                    "DEVELOPER_SUPPORT_IN_USE",
                    "An existing device media client must close its session before support replacement",
                ).details(json!({"nextAction":"developer_support_install"})));
            }
        }
        let provider=provider(serial).await?;
        let mut mounter=idevice::mobile_image_mounter::ImageMounter::connect(&provider).await.map_err(platform)?;
        let images=mounter.copy_devices().await.map_err(platform)?;
        let path=replacement_path(&images)?;
        let old=idevice::cryptexd::installed_ddi(&mut tunnel.handle,&mut rsd).await.map_err(platform)?;
        // Check the selected device after personalization. With no image to
        // remove, an unrelated host process cannot block a fresh install.
        guard_replacement(path, old.as_ref(), competing_tools()).await?;
        if let Some(path) = path {
            mounter.unmount_image(path).await.map_err(platform)?;
        }
        if let Some(old)=old {
            let client:CryptexdClient<Box<dyn idevice::ReadWrite>>=CryptexdClient::connect_rsd(&mut tunnel.handle,&mut rsd).await.map_err(platform)?;
            client.uninstall(&old.identifier,Some(&old.version)).await.map_err(platform)?;
        }
        let client:CryptexdClient<Box<dyn idevice::ReadWrite>>=CryptexdClient::connect_rsd(&mut tunnel.handle,&mut rsd).await.map_err(platform)?;
        client.install(request).await.map_err(platform)?;
        Ok(())
    }.await;
    let cleanup = tunnel.close().await;
    result.and(cleanup)
}

async fn guard_replacement(
    path: Option<&str>,
    installed: Option<&InstalledCryptex>,
    tools: impl Future<Output = Result<Vec<String>>>,
) -> Result<()> {
    if path.is_none() && installed.is_none() {
        return Ok(());
    }
    let tools = tools.await.map_err(|cause| {
        Error::new(
            "DEVELOPER_SUPPORT_CHECK_FAILED",
            "Could not check for Xcode before replacing developer support. Existing support was preserved.",
        )
        .details(json!({"nextAction":"doctor","cause":cause,"replacementMayHaveStarted":false}))
    })?;
    if tools.is_empty() {
        return Ok(());
    }
    Err(Error::new(
        "DEVELOPER_SUPPORT_IN_USE",
        "Quit Xcode and xcodebuild before replacing the selected iPhone's developer support; Testrun cannot prove which device they are using",
    )
    .details(json!({"nextAction":"developer_support_install","tools":tools})))
}

async fn personalize(
    tunnel: &mut idevice::tcp::handle::AdapterHandle,
    rsd: &mut RsdHandshake,
    assets: Cryptex1Assets,
) -> Result<idevice::cryptexd::CryptexInstallRequest> {
    let client: CryptexdClient<Box<dyn idevice::ReadWrite>> =
        CryptexdClient::connect_rsd(tunnel, rsd)
            .await
            .map_err(platform)?;
    let identifiers = client
        .read_personalization_identifiers()
        .await
        .map_err(platform)?;
    let client: CryptexdClient<Box<dyn idevice::ReadWrite>> =
        CryptexdClient::connect_rsd(tunnel, rsd)
            .await
            .map_err(platform)?;
    let nonce = client
        .cryptex_nonce(assets.nonce_domain().map_err(platform)?)
        .await
        .map_err(platform)?;
    let mut request = idevice::tss::TSSRequest::new();
    request
        .add_cryptex1_tags(&assets.build_identity, &identifiers, &nonce)
        .map_err(platform)?;
    let response=request.send().await.map_err(|_|Error::new("PERSONALIZATION_FAILED","Apple did not personalize the selected device's developer image; existing support was preserved"))?;
    let ticket=response.as_dictionary().and_then(|d|d.get("Cryptex1,Ticket").or_else(||d.get("ApImg4Ticket"))).and_then(plist::Value::as_data).ok_or_else(||Error::new("PERSONALIZATION_FAILED","Apple personalization response omitted the developer ticket; existing support was preserved"))?;
    let properties = assets.cryptex1_properties().map_err(platform)?;
    Ok(idevice::cryptexd::CryptexInstallRequest::new(
        assets.image,
        assets.trustcache,
        ticket.to_vec(),
        assets.info,
        assets.volumehash,
        properties,
    ))
}
struct OwnedTunnel {
    handle: idevice::tcp::handle::AdapterHandle,
    closed: bool,
}
impl OwnedTunnel {
    async fn close(&mut self) -> Result<()> {
        self.handle
            .close()
            .await
            .map_err(|error| platform(error.into()))?;
        self.closed = true;
        Ok(())
    }
}
impl Drop for OwnedTunnel {
    fn drop(&mut self) {
        if !self.closed
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let mut handle = self.handle.clone();
            runtime.spawn(async move {
                let _ = handle.close().await;
            });
        }
    }
}

fn replacement_path(images: &[plist::Value]) -> Result<Option<&'static str>> {
    const DEVELOPER_MOUNT: &str = "/System/Developer";
    let mut path = None;
    for image in images {
        if image_field(image, "DiskImageType") != Some("Personalized") {
            continue;
        }
        if image_field(image, "MountPath") != Some(DEVELOPER_MOUNT) {
            return Err(Error::new(
                "DEVELOPER_SUPPORT_IN_USE",
                "Unexpected developer-image mount path; existing support was preserved",
            )
            .details(json!({"nextAction":"doctor"})));
        }
        path = Some(DEVELOPER_MOUNT);
    }
    Ok(path)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fresh_install_does_not_wait_for_other_xcode_builds() {
        let path = replacement_path(&[]).unwrap();
        assert_eq!(path, None);
        guard_replacement(path, None, async {
            panic!("A fresh install must not inspect host-wide Xcode activity")
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn replacement_checks_xcode_for_either_kind_of_existing_support() {
        let cryptex = InstalledCryptex {
            identifier: "com.apple.MobileAsset.DDI".into(),
            version: "27.1.5228.8".into(),
        };
        for (path, installed) in [(Some("/System/Developer"), None), (None, Some(&cryptex))] {
            let error = guard_replacement(path, installed, async { Ok(vec!["xcodebuild".into()]) })
                .await
                .unwrap_err();
            assert_eq!(error.code, "DEVELOPER_SUPPORT_IN_USE");
            assert_eq!(
                error.details.unwrap(),
                json!({"nextAction":"developer_support_install","tools":["xcodebuild"]})
            );
            guard_replacement(path, installed, async { Ok(vec![]) })
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn failed_xcode_check_preserves_existing_support() {
        let error = guard_replacement(Some("/System/Developer"), None, async {
            Err(Error::new("HOST_IO", "Could not inspect host processes"))
        })
        .await
        .unwrap_err();
        assert_eq!(error.code, "DEVELOPER_SUPPORT_CHECK_FAILED");
        let details = error.details.unwrap();
        assert_eq!(details["nextAction"], "doctor");
        assert_eq!(details["cause"]["code"], "HOST_IO");
        assert_eq!(details["replacementMayHaveStarted"], false);
    }

    #[test]
    fn unknown_lock_routes_to_connect_only_with_both_control_services() {
        for display in [false, true] {
            for hid in [false, true] {
                let next = if display && hid {
                    "connect"
                } else {
                    "developer_support_install"
                };
                let lock = crate::device_media::lock::unknown(&Error::new(
                    "CAPABILITY_UNAVAILABLE",
                    "No device-info service",
                ));
                let report = service_report(display, hid, None, lock);
                assert_eq!(report["developerSupport"]["nextAction"], next);
                assert_eq!(report["lock"]["state"], "unknown");
                assert_eq!(report["lock"]["error"]["code"], "CAPABILITY_UNAVAILABLE");
                if display && hid {
                    assert_eq!(report["lock"]["nextAction"], "connect");
                } else {
                    assert!(report["lock"].get("nextAction").is_none());
                }
            }
        }
        for state in ["locked", "unlocked"] {
            let report = service_report(true, true, None, json!({"state":state}));
            assert_eq!(report["lock"]["state"], state);
            assert!(report["lock"].get("nextAction").is_none());
        }
    }

    #[test]
    fn validates_every_mount_before_returning_a_replacement_plan() {
        let mounted = |path: &str| {
            plist::Value::Dictionary(plist::Dictionary::from_iter([
                ("DiskImageType", plist::Value::String("Personalized".into())),
                ("MountPath", plist::Value::String(path.into())),
            ]))
        };
        assert_eq!(
            replacement_path(&[mounted("/System/Developer"), mounted("/System/Developer")])
                .unwrap(),
            Some("/System/Developer")
        );
        assert!(replacement_path(&[mounted("/System/Developer"), mounted("/foreign")]).is_err());
    }
}
