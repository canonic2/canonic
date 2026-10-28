//! Independent control v1 envelopes, admission acknowledgements and capability checks.

use crate::{Error, Result, core::Snapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Write;

/// Control protocol version, independent of package and product protocol versions.
pub const CONTROL_VERSION: u32 = 1;

/// Supports semantic-version checks and retirement once existing work settles.
pub const VERSION_RETIREMENT: &str = "retire-if-older-v1";

/// Explicit, opt-in process termination without graceful resource cleanup.
pub const FORCE_STOP: &str = "force-stop-v1";

/// A product's bounded structured failure, framed without interpreting its contents.
#[derive(Debug, thiserror::Error)]
#[error("product request failed")]
pub struct ProductError(pub Value);

/// Public, secret-free identity supplied by the product at initialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub manager_id: String,
    /// Opaque build label; opt-in versioned connections require a complete semantic version.
    pub build: String,
    pub protocol: u32,
    pub control_version: u32,
    pub capabilities: Vec<String>,
}
impl Identity {
    /// Create a fresh 128-bit random manager incarnation with caller-owned metadata.
    ///
    /// # Errors
    /// Returns I/O failure if the operating system cannot supply randomness.
    pub fn new(build: String, protocol: u32, capabilities: Vec<String>) -> Result<Self> {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(|_| {
            Error::Io(std::io::Error::other(
                "manager incarnation randomness unavailable",
            ))
        })?;
        let mut manager_id = String::with_capacity(32);
        for byte in bytes {
            let _ = write!(manager_id, "{byte:02x}");
        }
        Ok(Self {
            manager_id,
            build,
            protocol,
            control_version: CONTROL_VERSION,
            capabilities,
        })
    }
    /// Check the product protocol and required capabilities without admitting work.
    ///
    /// # Errors
    /// Returns `Incompatible` for an unsupported version or capability.
    pub fn require(&self, protocol: u32, required: &[String]) -> Result<()> {
        if protocol != self.protocol || required.iter().any(|r| !self.capabilities.contains(r)) {
            return Err(Error::Incompatible);
        }
        Ok(())
    }
}

/// A control request is decoded before any product payload or protocol check.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub control_version: u32,
    pub request_id: String,
    /// Built-ins include inspection, retirement and drain; extensions are advertised names.
    pub action: String,
    #[serde(default)]
    pub requires: Vec<String>,
    /// Required for retirement and drain; prevents acting on a replacement incarnation.
    pub manager_id: Option<String>,
    #[serde(default)]
    pub arguments: Value,
}

/// Product envelope. Payload interpretation and durable deduplication belong to the product.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: u32,
    pub request_id: String,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub payload: Value,
}

/// Stable refusal codes; refusals have no product effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    Draining,
    Starting,
    Incompatible,
    UnknownAction,
    StaleManager,
    Capacity,
    FrameTooLarge,
    Invalid,
    Product,
    InvalidVersion,
}

/// Admission or result frame. Every reply preserves the original request ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub control_version: u32,
    pub manager_id: String,
    pub request_id: String,
    /// True on the admission acknowledgement; false on a definite refusal.
    /// None on the final result after admission.
    pub accepted: Option<bool>,
    pub code: Option<Code>,
    pub data: Value,
}

/// Secret-free status mechanics. Product blocker contents are provided separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub identity: Identity,
    pub lifetime: Snapshot,
}

pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
}
