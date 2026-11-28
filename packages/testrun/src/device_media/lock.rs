//! Read-only CoreDevice lock observations; transport availability never implies unlock.
use super::platform;
use crate::error::{Error, Result};
use idevice::tcp::handle::AdapterHandle;
use serde_json::{Value, json};

pub(crate) async fn probe(tunnel: &mut AdapterHandle, port: u16) -> Result<Value> {
    let stream = tunnel
        .connect(port)
        .await
        .map_err(|error| platform(error.into()))?;
    let mut client = idevice::core_device::CoreDeviceServiceClient::new(stream)
        .await
        .map_err(platform)?;
    let state = client
        .invoke("com.apple.coredevice.feature.getlockstate", None)
        .await
        .map_err(platform)?;
    let properties = serde_json::to_value(state)
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Cannot decode device lock state"))?;
    Ok(observation(&properties))
}

fn observation(properties: &Value) -> Value {
    let fields = [
        "isPasscodeRequired",
        "passcodeRequired",
        "locked",
        "isLocked",
    ];
    let values: Vec<bool> = fields
        .iter()
        .filter_map(|key| properties[*key].as_bool())
        .collect();
    let state = match values.first() {
        Some(first) if values.iter().all(|value| value == first) => {
            if *first {
                "locked"
            } else {
                "unlocked"
            }
        }
        _ => "unknown",
    };
    json!({"state":state,"reason":if state == "unknown" {Some("The device returned missing or conflicting lock-state fields")}else{None}})
}

pub(crate) fn unknown(error: &Error) -> Value {
    json!({"state":"unknown","reason":"The lock-state probe failed; this does not establish that the device is locked or its display is unavailable","code":error.code,"error":error})
}

pub(crate) async fn bounded(probe: impl std::future::Future<Output = Result<Value>>) -> Value {
    match tokio::time::timeout(std::time::Duration::from_millis(750), probe).await {
        Ok(Ok(state)) => state,
        Ok(Err(error)) => unknown(&error),
        Err(_) => unknown(&Error::new("TIMEOUT", "Device lock probe exceeded 750 ms")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_requires_an_explicit_consistent_boolean() {
        for key in [
            "isPasscodeRequired",
            "passcodeRequired",
            "locked",
            "isLocked",
        ] {
            assert_eq!(observation(&json!({key:true}))["state"], "locked");
            assert_eq!(observation(&json!({key:false}))["state"], "unlocked");
        }
        for properties in [
            json!({}),
            json!({"locked":"false"}),
            json!({"locked":false,"isLocked":true}),
        ] {
            assert_eq!(observation(&properties)["state"], "unknown");
        }
        assert_eq!(
            unknown(&Error::new("TIMEOUT", "Lock probe timed out"))["state"],
            "unknown"
        );
    }

    #[tokio::test]
    async fn failed_or_stalled_probe_reports_unknown() {
        let unlocked = json!({"state":"unlocked","reason":null});
        assert_eq!(bounded(async { Ok(unlocked.clone()) }).await, unlocked);
        let failed =
            bounded(async { Err(Error::new("DEVICE_MEDIA_UNAVAILABLE", "Transport failed")) })
                .await;
        assert_eq!(failed["state"], "unknown");
        assert_eq!(failed["code"], "DEVICE_MEDIA_UNAVAILABLE");
        assert!(failed.get("nextAction").is_none());
        assert!(failed.get("manualAction").is_none());
        assert_eq!(failed["error"]["message"], "Transport failed");
        let stalled = bounded(std::future::pending()).await;
        assert_eq!(stalled["state"], "unknown");
        assert_eq!(stalled["code"], "TIMEOUT");
    }

    #[test]
    fn failed_probe_preserves_coredevice_diagnostics_without_claiming_a_lock() {
        let details = json!({"deviceError":{"domain":"CoreDevice.ActionError","code":2},"phase":"coredevice_transport","deviceState":"unknown"});
        let error = Error::new(
            "DEVICE_MEDIA_UNAVAILABLE",
            "CoreDevice rejected the lock probe",
        )
        .details(details.clone());
        let state = unknown(&error);
        assert_eq!(state["state"], "unknown");
        assert_eq!(state["error"]["details"], details);
        assert!(state.get("nextAction").is_none());
        assert!(state.get("manualAction").is_none());
    }
}
