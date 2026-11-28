//! Target identity and capability data without discovery or device I/O.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetRef {
    pub target_id: String,
    pub generation: String,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub identity: TargetRef,
    pub name: String,
    pub kind: String,
    pub state: String,
    pub runtime: String,
    pub metadata: BTreeMap<String, serde_json::Value>,
    pub capabilities: BTreeMap<String, serde_json::Value>,
    pub platform_id: String,
    pub usb_id: Option<u32>,
    pub(crate) epoch: String,
}
impl Target {
    /// `ios` for simulators and physical iPhones, `android` otherwise.
    pub fn platform(&self) -> &'static str {
        if self.kind == "android" {
            "android"
        } else {
            "ios"
        }
    }
}

/// The published shape of a target: its identity, `platform`, `kind` and capabilities.
impl Serialize for Target {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Published<'a> {
            #[serde(flatten)]
            identity: &'a TargetRef,
            name: &'a str,
            platform: &'static str,
            kind: &'a str,
            state: &'a str,
            runtime: &'a str,
            #[serde(flatten)]
            metadata: &'a BTreeMap<String, serde_json::Value>,
            capabilities: &'a BTreeMap<String, serde_json::Value>,
        }
        Published {
            identity: &self.identity,
            name: &self.name,
            platform: self.platform(),
            kind: &self.kind,
            state: &self.state,
            runtime: &self.runtime,
            metadata: &self.metadata,
            capabilities: &self.capabilities,
        }
        .serialize(serializer)
    }
}
