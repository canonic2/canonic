//! Selected physical-device identity, readiness and explicit developer preparation.
use crate::error::{Error, Result};
#[cfg(unix)]
use idevice::usbmuxd::{Connection, UsbmuxdAddr};
use idevice::{IdeviceService, provider::UsbmuxdProvider};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};

mod discovery;
mod support;
pub(crate) use discovery::{inventory, is_product_type, product_names};

pub(crate) fn serial(target: &str) -> Result<&str> {
    target
        .strip_prefix("device:")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Error::invalid(
                "Choose an explicit physical iPhone: a device: targetId from target_list",
            )
        })
}
fn platform(error: idevice::IdeviceError) -> Error {
    crate::device_media::platform(error)
}
fn setup_lease(authority: &str) -> Result<crate::lease::ControlLease> {
    crate::lease::ControlLease::acquire(authority).map_err(|error| {
        if error.code=="TARGET_BUSY" {
            Error::new("DEVICE_IN_USE","Another Testrun process is using this device; existing resources were preserved")
                .details(json!({"nextAction":"doctor","manualAction":"Disconnect the device, or let the process using it finish, before preparing it; do not stop that process"}))
        } else {error}
    })
}
#[cfg(unix)]
pub(crate) async fn provider(serial: &str) -> Result<UsbmuxdProvider> {
    let address = UsbmuxdAddr::UnixSocket("/var/run/usbmuxd".into());
    let mut mux = address.connect(1).await.map_err(platform)?;
    let device = mux
        .get_devices()
        .await
        .map_err(platform)?
        .into_iter()
        .find(|d| d.udid == serial && d.connection_type == Connection::Usb)
        .ok_or_else(|| {
            Error::new(
                "USB_UNAVAILABLE",
                "Connect the selected device over USB; network control is not supported",
            )
        })?;
    Ok(device.to_provider(address, "Testrun"))
}
#[cfg(not(unix))]
pub(crate) async fn provider(_serial: &str) -> Result<UsbmuxdProvider> {
    Err(Error::new(
        "UNSUPPORTED_HOST",
        "iOS USB device setup requires a Unix host",
    ))
}
pub(crate) async fn identity(serial: &str) -> Result<BTreeMap<String, Value>> {
    let provider = provider(serial).await?;
    let mut lockdown = idevice::lockdown::LockdownClient::connect(&provider)
        .await
        .map_err(platform)?;
    let values = lockdown.get_value(None, None).await.map_err(platform)?;
    let get = |key| {
        values
            .as_dictionary()
            .and_then(|d| d.get(key))
            .and_then(plist::Value::as_string)
            .unwrap_or("unknown")
    };
    Ok(BTreeMap::from([
        ("name".into(), json!(get("DeviceName"))),
        ("model".into(), json!(get("ProductType"))),
        ("osVersion".into(), json!(get("ProductVersion"))),
        ("transport".into(), json!("usb")),
        ("pairingState".into(), json!("unknown")),
    ]))
}
/// Developer Mode as the phone's image mounter reports it, which `doctor` uses
/// too; `devicectl` inventory can report it as unknown for the same phone.
pub(crate) async fn developer_mode(serial: &str) -> Result<&'static str> {
    let provider = provider(serial).await?;
    let mut mounter = idevice::mobile_image_mounter::ImageMounter::connect(&provider)
        .await
        .map_err(platform)?;
    let enabled = mounter
        .query_developer_mode_status()
        .await
        .map_err(platform)?;
    Ok(if enabled { "enabled" } else { "disabled" })
}
pub(crate) async fn doctor(target: &str) -> Result<Value> {
    let serial = serial(target)?;
    tokio::time::timeout(Duration::from_secs(30), doctor_inner(serial, false))
        .await
        .map_err(|_| Error::new("TIMEOUT", "Device readiness exceeded thirty seconds"))?
}
/// Readiness observed by a preparation, which holds the device's lease itself
/// and so must not report its own hold as another owner.
async fn preparation_doctor(target: &str) -> Result<Value> {
    let serial = serial(target)?;
    tokio::time::timeout(Duration::from_secs(30), doctor_inner(serial, true))
        .await
        .map_err(|_| Error::new("TIMEOUT", "Device readiness exceeded thirty seconds"))?
}
async fn doctor_inner(serial: &str, preparing: bool) -> Result<Value> {
    let mut checks = json!({
        "pairing":{"state":"unknown","manualAction":"Unlock the selected device and accept Trust This Computer if prompted"},
        "developerMode":{"state":"unknown","nextAction":"developer_mode_reveal"},
        "lock":{"state":"unknown","reason":"Lock state has not been verified"},
        "developerSupport":{"state":"unknown","nextAction":"developer_support_install"},
        "displayService":{"state":"unknown"},
        "hidService":{"state":"unknown"},
        "competingTools":{"state":"unknown","reason":"Other clients can change device support after this observation"}
    });
    checks["testrunOwner"] = match crate::lease::ControlLease::inspect(&format!("device:{serial}"))
    {
        Ok(Some((true, _))) if preparing => {
            json!({"state":"held_by_preparation","reason":"This preparation holds the device while it works and releases it when it returns"})
        }
        Ok(Some((true, _))) => {
            json!({"state":"held","manualAction":"A Testrun device actor holds this device; disconnect it before preparation and preserve its process"})
        }
        Ok(_) => json!({"state":"not_observed"}),
        Err(_) => json!({"state":"unknown","reason":"Testrun ownership could not be checked"}),
    };
    checks["developerSupport"]["availableAssetBuild"] = json!(support::asset_build());
    let provider = match provider(serial).await {
        Ok(provider) => provider,
        Err(error) => {
            return Ok(
                json!({"targetId":format!("device:{serial}"),"ready":false,"checks":checks,"error":error}),
            );
        }
    };
    match idevice::mobile_image_mounter::ImageMounter::connect(&provider).await {
        Ok(mut mounter) => {
            checks["pairing"] = json!({"state":"paired"});
            if let Ok(enabled) = mounter.query_developer_mode_status().await {
                checks["developerMode"] = if enabled {
                    json!({"state":"enabled"})
                } else {
                    json!({"state":"disabled","nextAction":"developer_mode_reveal","manualAction":"On the selected phone, open Settings > Privacy & Security > Developer Mode, enable it and confirm after restart"})
                };
            }
            if let Ok(images) = mounter.copy_devices().await {
                checks["developerSupport"]["mountedImages"] = json!(
                    images
                        .iter()
                        .map(support::image_summary)
                        .collect::<Vec<_>>()
                );
            }
        }
        Err(error) => {
            if matches!(error, idevice::IdeviceError::DeviceLocked) {
                checks["lock"]["state"] = json!("locked");
            }
            checks["pairing"]["error"] = json!(platform(error));
        }
    }
    if let Ok(tools) = support::competing_tools().await {
        checks["competingTools"] = json!({"state":if tools.is_empty(){"not_observed"}else{"potential_conflict"},"tools":tools,"reason":"Process presence does not establish which device a client is using"});
    }
    match support::services(serial).await {
        Ok(services) => {
            for (key, value) in services.as_object().unwrap() {
                if key == "developerSupport" {
                    for (field, value) in value.as_object().unwrap() {
                        checks[key][field] = value.clone();
                    }
                } else {
                    checks[key] = value.clone();
                }
            }
        }
        Err(error) => {
            checks["displayService"]["error"] = json!(error);
        }
    }
    let ready = checks["developerMode"]["state"] == "enabled"
        && checks["lock"]["state"] == "unlocked"
        && checks["displayService"]["state"] == "available"
        && checks["hidService"]["state"] == "available";
    Ok(json!({"targetId":format!("device:{serial}"),"ready":ready,"checks":checks}))
}
pub(crate) async fn reveal(target: &str) -> Result<Value> {
    let serial = serial(target)?;
    let _lease = setup_lease(target)?;
    let operation = async {
        let provider = provider(serial).await?;
        let mut amfi = idevice::amfi::AmfiClient::connect(&provider)
            .await
            .map_err(platform)?;
        reveal_setting(&mut amfi.idevice).await?;
        Ok(
            json!({"targetId":target,"toggleRevealed":true,"developerModeEnabledByTestrun":false,"manualAction":"On the selected phone, open Settings > Privacy & Security > Developer Mode, enable it and confirm after restart","nextAction":"doctor"}),
        )
    };
    tokio::time::timeout(Duration::from_secs(20), operation)
        .await
        .map_err(|_| {
            Error::new(
                "TIMEOUT",
                "Developer Mode preparation exceeded twenty seconds",
            )
        })?
}
async fn reveal_setting(device: &mut idevice::Idevice) -> Result<()> {
    // The pinned AMFI helper accepts any present `success` key, including false.
    // Keep this one request explicit and bound its reply before decoding it.
    let request = plist::Value::Dictionary(plist::Dictionary::from_iter([(
        "action",
        plist::Value::Integer(0.into()),
    )]));
    let mut bytes = Vec::new();
    request
        .to_writer_xml(&mut bytes)
        .map_err(|_| Error::new("INTERNAL", "Cannot encode Developer Mode request"))?;
    device
        .send_raw(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(platform)?;
    device.send_raw(&bytes).await.map_err(platform)?;
    let header = device.read_raw(4).await.map_err(platform)?;
    let size = u32::from_be_bytes(
        header
            .try_into()
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid AMFI reply header"))?,
    );
    if size > 65536 {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "AMFI reply exceeds its size limit",
        ));
    }
    let bytes = device.read_raw(size as usize).await.map_err(platform)?;
    let response = plist::Value::from_reader(std::io::Cursor::new(bytes))
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid AMFI reply"))?;
    let response = response
        .as_dictionary()
        .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "AMFI reply is not a dictionary"))?;
    if response.get("success").and_then(plist::Value::as_boolean) != Some(true)
        || response.contains_key("Error")
    {
        return Err(Error::new(
            "DEVELOPER_MODE_PREPARATION_FAILED",
            "The device did not confirm revealing the Developer Mode setting",
        )
        .details(json!({"nextAction":"doctor"})));
    }
    Ok(())
}
pub(crate) async fn prepare(target: &str) -> Result<Value> {
    support::prepare(target).await
}

/// Run `developer_mode_reveal` (reveal Developer Mode) or
/// `developer_support_install` on `target`.
pub(crate) async fn preparation(name: &str, target: &str) -> Result<Value> {
    if name == "developer_mode_reveal" {
        reveal(target).await
    } else {
        prepare(target).await
    }
}

pub(crate) async fn managed_call(
    client: &mut crate::managed::Client,
    name: &str,
    args: Value,
) -> Result<Value> {
    let result = client.call(name, args.clone()).await;
    if name == "connect"
        && args["prepareSupport"] == true
        && result
            .as_ref()
            .is_err_and(|error| error.code == "DEVELOPER_SUPPORT_REQUIRED")
    {
        let target = args["targetId"]
            .as_str()
            .ok_or_else(|| Error::invalid("targetId is required"))?;
        // Preparation runs in the manager, in the device's queue.
        client
            .call(
                "developer_support_install",
                serde_json::json!({"targetId":target}),
            )
            .await?;
        client.call(name, args).await
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn reveal_requires_explicit_amfi_success_and_sends_only_the_reveal_action() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let reply =
            |key, value| plist::Value::Dictionary(plist::Dictionary::from_iter([(key, value)]));
        for (response, accepted) in [
            (reply("success", plist::Value::Boolean(true)), true),
            (reply("success", plist::Value::Boolean(false)), false),
            (reply("success", plist::Value::String("true".into())), false),
            (reply("Error", plist::Value::String("denied".into())), false),
        ] {
            let (client, mut peer) = tokio::io::duplex(4096);
            let mut client = idevice::Idevice::new(Box::new(client), "Acme fixture");
            let fixture = async {
                let size = peer.read_u32().await.unwrap();
                let mut bytes = vec![0; size as usize];
                peer.read_exact(&mut bytes).await.unwrap();
                let request = plist::Value::from_reader(std::io::Cursor::new(bytes)).unwrap();
                let request = request.as_dictionary().unwrap();
                assert_eq!(request.len(), 1);
                assert_eq!(request["action"].as_unsigned_integer(), Some(0));
                let mut bytes = Vec::new();
                response.to_writer_xml(&mut bytes).unwrap();
                peer.write_u32(bytes.len() as u32).await.unwrap();
                peer.write_all(&bytes).await.unwrap();
            };
            let (result, ()) = tokio::join!(reveal_setting(&mut client), fixture);
            assert_eq!(result.is_ok(), accepted);
            if !accepted {
                let error = result.unwrap_err();
                assert_eq!(error.code, "DEVELOPER_MODE_PREPARATION_FAILED");
                assert_eq!(error.details.unwrap()["nextAction"], "doctor");
            }
        }
    }
    #[tokio::test]
    async fn preparation_preserves_a_busy_owner_without_contacting_hardware_or_exposing_locks() {
        let target = format!("device:Acme-{}", uuid::Uuid::new_v4());
        let _owner = crate::lease::ControlLease::acquire(&target).unwrap();
        for error in [
            reveal(&target).await.unwrap_err(),
            prepare(&target).await.unwrap_err(),
        ] {
            assert_eq!(error.code, "DEVICE_IN_USE");
            assert_eq!(error.details.as_ref().unwrap()["nextAction"], "doctor");
            assert!(!serde_json::to_string(&error).unwrap().contains("lockPath"));
        }
        assert!(
            crate::lease::ControlLease::inspect(&target)
                .unwrap()
                .unwrap()
                .0
        );
        assert!(serial("simulator:Acme").is_err());
        assert!(serial("device:").is_err());
    }
}
