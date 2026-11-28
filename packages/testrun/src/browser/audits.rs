//! Explicit native Lighthouse audit options; reports are private evidence.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub device: Device,
    #[serde(default)]
    pub include_nodes: bool,
    #[serde(default = "audit_limit")]
    pub limit: usize,
}
fn audit_limit() -> usize {
    20
}
impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Default::default(),
            device: Default::default(),
            include_nodes: false,
            limit: audit_limit(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Navigation,
    Snapshot,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    #[default]
    Desktop,
    Mobile,
}
impl Options {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if !(1..=100).contains(&self.limit) {
            Err("AUDIT_BOUNDS")
        } else {
            Ok(())
        }
    }
}
pub(crate) const SCRIPT: &str = include_str!("reading/lighthouse.js");
pub(crate) fn schema(reference: &Value) -> Value {
    json!({"type":"object","properties":{"operation":{"const":"page_audit"},"page":reference,"options":{"type":"object","properties":{"mode":{"enum":["navigation","snapshot"],"default":"navigation"},"device":{"enum":["desktop","mobile"],"default":"desktop"},"includeNodes":{"type":"boolean","default":false},"limit":{"type":"integer","minimum":1,"maximum":100,"default":20}},"additionalProperties":false}},"required":["operation","page"],"additionalProperties":false})
}
