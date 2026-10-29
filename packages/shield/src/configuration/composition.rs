//! Project artifact selection metadata without authorization.

use super::{Artifact, Lock, LockedPack};
use crate::policy::{self, Error, Profile, Scope};
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

/// Validated project candidate metadata, including the complete artifact lock.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Effective {
    schema_version: u8,
    kind: String,
    scopes: Vec<ContributingScope>,
    packs: Vec<EffectivePack>,
    custom_rules: Vec<ScopedRules>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ContributingScope {
    scope: Scope,
    lock_revision: String,
    lock: Lock,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct EffectivePack {
    scope: Scope,
    selection: LockedPack,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ScopedRules {
    scope: Scope,
    artifact: Artifact,
}

/// Describe exactly one validated project lock and its rule artifacts.
///
/// # Errors
/// Rejects absent or multiple project candidates.
pub fn compose(inputs: &[(Scope, Lock)]) -> Result<Effective, Error> {
    let [(scope, lock)] = inputs else {
        return Err(Error::Contract("exactly one project candidate required"));
    };
    Ok(Effective {
        schema_version: 1,
        kind: "effective-candidate".into(),
        scopes: vec![ContributingScope {
            scope: *scope,
            lock_revision: lock.revision()?,
            lock: lock.clone(),
        }],
        packs: lock
            .document
            .packs
            .iter()
            .map(|selection| EffectivePack {
                scope: *scope,
                selection: selection.clone(),
            })
            .collect(),
        custom_rules: lock
            .document
            .rules
            .iter()
            .map(|artifact| ScopedRules {
                scope: *scope,
                artifact: artifact.clone(),
            })
            .collect(),
    })
}

impl Effective {
    pub(crate) fn mcp_connections(&self) -> &crate::mcp_connections::Connections {
        &self.scopes[0].lock.document.mcp
    }
    pub(crate) fn access_paths(&self) -> &super::AccessPaths {
        &self.scopes[0].lock.document.access_paths
    }

    pub(super) fn selections(&self) -> impl Iterator<Item = (&str, Profile)> {
        self.packs
            .iter()
            .map(|pack| (pack.selection.id.as_str(), pack.selection.profile))
    }

    /// Fingerprint of canonical project metadata and all retained artifact identities.
    ///
    /// # Errors
    /// Returns an error if the candidate cannot be serialized.
    pub fn revision(&self) -> Result<String, Error> {
        let bytes = serde_json::to_vec(self)
            .map_err(|_| Error::Contract("effective candidate serialization failed"))?;
        Ok(policy::fingerprint(&bytes))
    }
}
