//! Bounded, private registration records; none is an operation capability.
use crate::review::Error;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(super) const PROFILE_LIMIT: usize = 1024;
pub(super) const CONTEXT_LIMIT: usize = 128;
pub(super) const CONTEXT_TTL_MS: u64 = 3_600_000;
pub(super) const INDEX_BYTES: usize = 256 * 1024;

pub(super) fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Location-independent consent, scoped to this developer's private namespace.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Profile {
    pub lineage: String,
    pub binary_digest: String,
    pub mapping_digest: String,
}

impl Profile {
    pub(super) fn key(&self) -> Result<String, Error> {
        self.validate()?;
        crate::json::fingerprint(&(
            "reviewed-sdk-read-profile-v1",
            &self.lineage,
            &self.binary_digest,
            &self.mapping_digest,
        ))
        .map_err(|_| Error::Invalid("SDK read profile encoding"))
    }
    pub(super) fn validate(&self) -> Result<(), Error> {
        if ![&self.lineage, &self.binary_digest, &self.mapping_digest]
            .into_iter()
            .all(|value| digest(value))
        {
            return Err(Error::Invalid("SDK read profile is invalid"));
        }
        Ok(())
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Profiles {
    pub schema_version: u8,
    pub profiles: BTreeMap<String, Profile>,
}

impl Profiles {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.schema_version != 1 || self.profiles.len() > PROFILE_LIMIT {
            return Err(Error::Invalid("SDK read profile index is invalid"));
        }
        for (key, profile) in &self.profiles {
            if profile.key()? != *key {
                return Err(Error::Invalid("SDK read profile identity differs"));
            }
        }
        Ok(())
    }
}

/// Ephemeral checkout authority. Only the hash of the random launch token is
/// retained; its private binding is independent of portable profile consent.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ContextRecord {
    pub consumer_id: String,
    pub project_id: String,
    /// Written by an earlier format; such a context is refused as stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incarnation: Option<String>,
    pub project_root: String,
    pub profile: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub binary: super::binary::Receipt,
}

impl ContextRecord {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if ![&self.consumer_id, &self.project_id, &self.profile]
            .into_iter()
            .all(|value| digest(value))
            || self.incarnation.is_some()
            || self.project_root.len() > 4096
            || self.project_root.contains('\0')
            || !std::path::Path::new(&self.project_root).is_absolute()
            || self.expires_at_ms.checked_sub(self.created_at_ms) != Some(CONTEXT_TTL_MS)
        {
            return Err(Error::Invalid("SDK read context is invalid"));
        }
        Ok(())
    }
    pub(super) fn binding(&self) -> Result<String, Error> {
        self.validate()?;
        crate::json::fingerprint(&("sdk-read-context-v1", self))
            .map_err(|_| Error::Invalid("SDK read context encoding"))
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Contexts {
    pub schema_version: u8,
    pub contexts: BTreeMap<String, ContextRecord>,
}

impl Contexts {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.schema_version != 1 || self.contexts.len() > CONTEXT_LIMIT {
            return Err(Error::Invalid("SDK read context index is invalid"));
        }
        for (token_hash, record) in &self.contexts {
            if !digest(token_hash) {
                return Err(Error::Invalid("SDK read token hash is invalid"));
            }
            record.validate()?;
        }
        Ok(())
    }
}
