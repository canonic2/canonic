//! Discovery listings with the live capabilities of connected devices.
//! Discovery runs in the client's process, while capabilities a device actor
//! verifies, such as a registered keyboard, live in the manager; `status`
//! carries them across.
use super::remote::Client;
use serde_json::{Value, json};
use std::time::Duration;

impl Client {
    /// `list` with each connected device's capabilities replaced by the ones
    /// its device actor reports. `status` never starts a manager or renews a
    /// device's idle timeout; when it fails or is slow, the discovery
    /// capabilities stay.
    pub(crate) async fn with_held_capabilities(&mut self, mut list: Value) -> Value {
        if let Ok(Ok(status)) =
            tokio::time::timeout(Duration::from_secs(3), self.call("status", json!({}))).await
        {
            overlay(&mut list, &status);
        }
        list
    }
}

/// Replace the capabilities of listed targets that are connected. `status`
/// describes one connected target at its top level, or several under `targets`.
fn overlay(list: &mut Value, status: &Value) {
    let held: Vec<&Value> = match status["targets"].as_array() {
        Some(targets) => targets.iter().collect(),
        None => vec![status],
    };
    let Some(targets) = list["targets"].as_array_mut() else {
        return;
    };
    for entry in held {
        let (Some(id), Some(capabilities)) = (
            entry["targetId"].as_str(),
            entry["capabilities"].as_object(),
        ) else {
            continue;
        };
        if entry["nextAction"] == "connect" {
            continue;
        }
        for target in targets.iter_mut() {
            if target["targetId"] == id {
                target["capabilities"] = Value::Object(capabilities.clone());
                target["capabilitiesSource"] = json!("connected_session");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Value {
        json!({"targets":[
            {"targetId":"device:Acme-1","capabilities":{"key":{"state":"unknown"}}},
            {"targetId":"device:Acme-2","capabilities":{"key":{"state":"unknown"}}},
        ]})
    }

    #[test]
    fn held_targets_take_their_session_capabilities() {
        let mut one = list();
        overlay(
            &mut one,
            &json!({"targetId":"device:Acme-1","nextAction":"observe","capabilities":{"key":{"state":"supported"}}}),
        );
        assert_eq!(
            one["targets"][0]["capabilities"]["key"]["state"],
            "supported"
        );
        assert_eq!(one["targets"][0]["capabilitiesSource"], "connected_session");
        assert_eq!(one["targets"][1]["capabilities"]["key"]["state"], "unknown");

        let mut several = list();
        overlay(
            &mut several,
            &json!({"targets":[
                {"targetId":"device:Acme-2","nextAction":"observe","capabilities":{"key":{"state":"supported"}}},
                {"targetId":"device:Acme-1","error":{"code":"STATE_IO"}},
            ]}),
        );
        assert_eq!(
            several["targets"][1]["capabilities"]["key"]["state"],
            "supported"
        );
        assert_eq!(
            several["targets"][0]["capabilities"]["key"]["state"],
            "unknown"
        );
    }

    #[test]
    fn released_or_unreported_targets_keep_discovery_capabilities() {
        for status in [
            json!({"targetId":"device:Acme-1","nextAction":"connect","capabilities":{"key":{"state":"supported"}}}),
            json!({"state":"disconnected","nextAction":"connect"}),
            json!({}),
        ] {
            let mut listed = list();
            overlay(&mut listed, &status);
            assert_eq!(listed, list());
        }
    }
}
