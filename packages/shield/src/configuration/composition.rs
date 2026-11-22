//! Project artifact selection metadata without authorization.

use super::{Artifact, Lock, LockedPack};
use crate::policy::{self, Error, Profile, Scope};
use serde::{Deserialize, Serialize};

// SHA-256s of reviewed release artifacts from the Canonic registry catalog.
// Rule names alone do not identify the implementations this behavior depends on.
pub(super) const DOCKER_RUNTIME: &str =
    "62e6739e83df2cfcdbca8fc353e1efee75ff72b339b92787964d627ae8a2a230";
pub(super) const CONTAINER_BASELINES: [&str; 2] = [
    "f59a663e1bd6e3670148dea221bfa5128e73bfaa1084a1af8d6d9b1a96c04615", // 0.16.0
    "d87872dff6ceab06a2c08b947c141a028b9aa7e34fecce5a64ad30d1935aec64", // 0.17.0
];

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
    /// The original single-project composition encoded an empty shadow list.
    /// Retain it only when reconstructing that exact recorded identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "unshadowed"
    )]
    shadowed: Option<Vec<serde_json::Value>>,
}

fn unshadowed<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> Result<Option<Vec<serde_json::Value>>, D::Error> {
    let values = Vec::<serde_json::Value>::deserialize(input)?;
    if !values.is_empty() {
        return Err(serde::de::Error::custom(
            "historical shadowed scopes require separate review",
        ));
    }
    Ok(Some(values))
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
                shadowed: None,
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
    pub(super) fn has_pack_artifact(&self, source: &str, digests: &[&str]) -> bool {
        self.pack_artifacts()
            .any(|(id, _, digest)| id == source && digests.contains(&digest))
    }

    /// Exact selected pack identities and byte digests from the compiled lock.
    /// Metadata inspection is not a substitute for this active artifact binding.
    pub(crate) fn pack_artifacts(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.packs.iter().map(|pack| {
            (
                pack.selection.id.as_str(),
                pack.selection.version.as_str(),
                pack.selection.artifact.digest.as_str(),
            )
        })
    }
    /// Select only a known canonical encoding whose digest already matches the
    /// recorded revision. This does not reinterpret or drop another scope.
    pub(super) fn for_revision(mut self, expected: &str) -> Result<Self, Error> {
        if self.revision()? == expected {
            return Ok(self);
        }
        if self.scopes.len() != 1 || self.scopes[0].scope != Scope::Project {
            return Err(Error::Contract(
                "historical composition requires one project scope",
            ));
        }
        for pack in &mut self.packs {
            pack.shadowed = Some(Vec::new());
        }
        if self.revision()? != expected {
            return Err(Error::Contract(
                "recorded revision differs from retained source",
            ));
        }
        Ok(self)
    }

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
