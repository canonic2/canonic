//! Retains unverified project artifact bytes with a recomputable lock, never approval.

use super::{Candidate, LockDocument, Snapshot, layers};
use crate::{
    policy::{self, Error},
    policy_files,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Maximum encoded retained artifact size, including JSON escaping.
pub const MAX_BUNDLE_BYTES: usize = 512 * 1024 * 1024;
const MAX_CONTENT_BYTES: usize = crate::policy::MAX_DOCUMENT_BYTES;

/// Retained project source bytes and their artifact lock. All fields remain untrusted.
///
/// Each configuration layer keeps its own file's source; the merged
/// configuration is derived from them and never retained.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    schema_version: u8,
    kind: String,
    trust: String,
    config: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    local_config: Option<String>,
    packs: Vec<String>,
    #[serde(deserialize_with = "nullable_string")]
    rules: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    local_rules: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    disabled: Vec<String>,
    lock: LockDocument,
}

fn nullable_string<'de, D: serde::Deserializer<'de>>(input: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(input)
}

impl Bundle {
    /// Return retained custom-rule source for a proposal, without activating it.
    #[must_use]
    pub fn rules_source(&self) -> Option<&str> {
        self.rules.as_deref()
    }

    /// Capture validated source bytes and their reproducible lock without filesystem I/O.
    ///
    /// # Errors
    /// Rejects invalid configs/artifacts, mismatched identities, and aggregate limits.
    pub fn capture(config: &str, packs: &[String], rules: Option<&str>) -> Result<Self, Error> {
        Self::capture_layers(config, None, packs, rules, None, &[])
    }

    /// Capture both configuration layers' sources with their local custom rules
    /// and the retained artifacts of shared selections the local layer disables.
    /// Pack and disabled artifacts follow the merged configuration's order.
    ///
    /// # Errors
    /// Rejects invalid configs/artifacts, mismatched identities, and aggregate limits.
    pub fn capture_layers(
        config: &str,
        local_config: Option<&str>,
        packs: &[String],
        rules: Option<&str>,
        local_rules: Option<&str>,
        disabled: &[String],
    ) -> Result<Self, Error> {
        let merged = layers::merge(config, local_config)?;
        let bundle = Self {
            schema_version: 1,
            kind: "candidate-bundle".into(),
            trust: "unverified".into(),
            config: config.into(),
            local_config: local_config.map(str::to_owned),
            packs: packs.to_vec(),
            rules: rules.map(str::to_owned),
            local_rules: local_rules.map(str::to_owned),
            disabled: disabled.to_vec(),
            lock: Candidate::from_yaml(&merged)?
                .capture_layers(packs, rules, local_rules, disabled)?
                .lock
                .document,
        };
        bundle.content_limit()?;
        Ok(bundle)
    }

    /// Capture a changed candidate whose files are `previous`'s sources edited
    /// to their new documents, keeping each file's comments and layout where
    /// the edit allows. `merged` is the complete new merged configuration;
    /// `rules` and `local_rules` are the new custom-rule documents' sources.
    ///
    /// # Errors
    /// Rejects invalid documents, configs and artifacts, mismatched identities,
    /// and aggregate limits.
    pub fn revise(
        previous: &Value,
        merged: &Value,
        packs: &[String],
        rules: Option<&str>,
        local_rules: Option<&str>,
        disabled: &[String],
    ) -> Result<Self, Error> {
        let (shared, local) = layers::split(
            &merged.to_string(),
            (
                previous["config"].as_str(),
                previous["local_config"].as_str(),
            ),
        )?;
        let config = edited(previous, "config", Some(&shared))?
            .ok_or(Error::Contract("candidate configuration missing"))?;
        Self::capture_layers(
            &config,
            edited(previous, "local_config", local.as_deref())?.as_deref(),
            packs,
            edited(previous, "rules", rules)?.as_deref(),
            edited(previous, "local_rules", local_rules)?.as_deref(),
            disabled,
        )
    }

    /// Parse strict JSON and recompute all embedded contents against the captured lock.
    ///
    /// # Errors
    /// Rejects malformed/unknown/duplicate fields, incompatible metadata, tampering,
    /// invalid policy contents, and encoded/decoded resource limits.
    pub fn from_json(bytes: &[u8]) -> Result<Self, Error> {
        let bundle = Self::decode(bytes)?;
        bundle.snapshot()?;
        Ok(bundle)
    }

    /// Reconstruct the snapshot while validating retained bytes once. Callers
    /// needing the snapshot do not need to validate and discard it first.
    pub(crate) fn snapshot_from_json(bytes: &[u8]) -> Result<Snapshot, Error> {
        Self::decode(bytes)?.snapshot()
    }

    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_BUNDLE_BYTES {
            return Err(Error::Limit);
        }
        serde_json::from_slice(bytes).map_err(|_| Error::Syntax)
    }

    /// Reconstruct a validated in-memory preview snapshot; this creates no active state.
    ///
    /// # Errors
    /// Rejects incompatible metadata, altered source/lock bytes, invalid policy, and limits.
    pub fn snapshot(&self) -> Result<Snapshot, Error> {
        if self.schema_version != 1 || self.kind != "candidate-bundle" || self.trust != "unverified"
        {
            return Err(Error::Contract("invalid candidate bundle metadata"));
        }
        self.content_limit()?;
        let merged = layers::merge(&self.config, self.local_config.as_deref())?;
        let snapshot = Candidate::from_yaml(&merged)?.capture_layers(
            &self.packs,
            self.rules.as_deref(),
            self.local_rules.as_deref(),
            &self.disabled,
        )?;
        if snapshot.lock.document != self.lock {
            return Err(Error::Contract(
                "candidate bundle lock differs from retained bytes",
            ));
        }
        Ok(snapshot)
    }
}

/// One file's new source: the previous candidate's source for `field` edited
/// to the document `target` holds, or `target`'s bytes when they are the
/// previous source itself.
fn edited(previous: &Value, field: &str, target: Option<&str>) -> Result<Option<String>, Error> {
    let Some(target) = target else {
        return Ok(None);
    };
    let source = previous[field].as_str();
    if source == Some(target) {
        return Ok(Some(target.to_owned()));
    }
    let document: Value = policy::parse(target)?;
    Ok(Some(policy_files::write(source, &document)?.text))
}

impl Bundle {
    fn content_limit(&self) -> Result<(), Error> {
        let artifacts = self.packs.iter().chain(&self.disabled);
        if self.packs.len() + self.disabled.len() > 64
            || artifacts.map(String::len).sum::<usize>()
                + self.config.len()
                + self.local_config.as_ref().map_or(0, String::len)
                + self.rules.as_ref().map_or(0, String::len)
                + self.local_rules.as_ref().map_or(0, String::len)
                > MAX_CONTENT_BYTES
        {
            return Err(Error::Limit);
        }
        Ok(())
    }
}
