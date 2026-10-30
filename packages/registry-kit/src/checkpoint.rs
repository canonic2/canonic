//! Retained catalog checkpoints that refuse rollback and release replacement.
//!
//! Callers persist [`Checkpoints`] with their own state and call
//! [`Checkpoints::admit`] for every authenticated catalog before using it.
//! Admission is all-or-nothing: a refusal leaves the checkpoints unchanged.

use crate::catalog::Catalog;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Maximum distinct sources with retained checkpoints: every source two full
/// scopes and the built-in source can enroll, with room for sources whose
/// registrations were removed.
pub const MAX_SOURCES: usize = 256;
/// Maximum retained releases per source.
pub const MAX_RELEASES: usize = 4096;

/// Checkpoints keyed by source identity.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Checkpoints(BTreeMap<String, Checkpoint>);

/// The last admitted catalog of one source and every release digest it has listed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    /// Last admitted sequence; local sources may have none.
    pub sequence: Option<u64>,
    /// SHA-256 of the last admitted catalog bytes.
    pub digest: String,
    /// Retained digests by `publisher/name@version`.
    pub releases: BTreeMap<String, String>,
    /// Advisory command index from the latest catalog only, not release history.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub commands: BTreeMap<String, Vec<String>>,
}

/// Why a catalog was not admitted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckpointError {
    /// The sequence went backwards, was omitted after sequencing began, or
    /// the same sequence has different bytes.
    #[error("registry rollback, missing retained sequence or same-sequence replacement")]
    Rollback,
    /// A retained release now lists a different digest.
    #[error("registry replaced bytes for retained release {0}")]
    Replaced(String),
    /// Source or release retention bounds are exhausted.
    #[error("registry checkpoint limit exceeded")]
    Limit,
}

impl Checkpoints {
    /// Admits `catalog` under the identity of the source that authenticated
    /// it, retaining its releases. The identity comes from the catalog itself,
    /// so one source's catalog can never be admitted under another's.
    ///
    /// A sequenced catalog must not go below the retained sequence, and an
    /// equal sequence must have identical bytes. Once a source has admitted a
    /// sequence, later catalogs must keep sequencing. A release seen before
    /// must keep its digest, even after it leaves the catalog.
    ///
    /// # Errors
    /// Returns [`CheckpointError`] and leaves the checkpoints unchanged.
    pub fn admit(&mut self, catalog: &Catalog) -> Result<(), CheckpointError> {
        let identity = catalog.source_identity();
        let previous = self.0.get(identity);
        if let Some(previous) = previous
            && previous.sequence.is_some()
            && (catalog.sequence() < previous.sequence
                || (catalog.sequence() == previous.sequence && previous.digest != catalog.digest()))
        {
            return Err(CheckpointError::Rollback);
        }
        if previous.is_none() && self.0.len() >= MAX_SOURCES {
            return Err(CheckpointError::Limit);
        }
        let mut releases = previous.map(|p| p.releases.clone()).unwrap_or_default();
        for entry in catalog.entries() {
            let release = format!("{}@{}", entry.id, entry.version);
            match releases.get(&release) {
                Some(retained) if *retained != entry.sha256 => {
                    return Err(CheckpointError::Replaced(release));
                }
                Some(_) => {}
                None if releases.len() >= MAX_RELEASES => return Err(CheckpointError::Limit),
                None => {
                    releases.insert(release, entry.sha256.clone());
                }
            }
        }
        let mut commands: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (id, names) in catalog.commands() {
            for name in names {
                commands
                    .entry(name.clone())
                    .or_default()
                    .push(id.to_string());
            }
        }
        self.0.insert(
            identity.to_owned(),
            Checkpoint {
                sequence: catalog.sequence(),
                digest: catalog.digest().to_owned(),
                releases,
                commands,
            },
        );
        Ok(())
    }

    /// The retained checkpoint of one source.
    #[must_use]
    pub fn get(&self, identity: &str) -> Option<&Checkpoint> {
        self.0.get(identity)
    }

    /// The digest retained for one release, if it was ever listed.
    #[must_use]
    pub fn release_digest(&self, identity: &str, release: &str) -> Option<&str> {
        self.0
            .get(identity)
            .and_then(|c| c.releases.get(release))
            .map(String::as_str)
    }

    /// Number of sources with retained checkpoints.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether no source has a retained checkpoint.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{local, local_catalog, now, release};
    use serde_json::json;

    #[test]
    fn discovery_index_is_current_and_does_not_accumulate_with_release_history() {
        let source = local("/srv/acme");
        let mut checkpoints = Checkpoints::default();
        let mut document = json!({"schema_version":2,"sequence":1,
            "packs":[release("acme/runtime","1.0.0",'1'),release("acme/tools","1.0.0",'2')],
            "commands":{"acme/runtime":["node","npm"],"acme/tools":["npm"]}});
        let catalog =
            Catalog::verify(&source, document.to_string().as_bytes(), None, now()).unwrap();
        checkpoints.admit(&catalog).unwrap();
        let index = &checkpoints.get(source.identity()).unwrap().commands;
        assert_eq!(index["npm"], ["acme/runtime", "acme/tools"]);
        assert_eq!(index["node"], ["acme/runtime"]);
        let retained = serde_json::to_value(&checkpoints).unwrap();
        assert_eq!(
            serde_json::from_value::<Checkpoints>(retained).unwrap(),
            checkpoints
        );
        document["sequence"] = json!(2);
        document["commands"] = json!({});
        let catalog =
            Catalog::verify(&source, document.to_string().as_bytes(), None, now()).unwrap();
        checkpoints.admit(&catalog).unwrap();
        assert!(
            checkpoints
                .get(source.identity())
                .unwrap()
                .commands
                .is_empty()
        );
        assert_eq!(
            checkpoints.get(source.identity()).unwrap().releases.len(),
            2
        );
    }

    fn catalog(sequence: Option<u64>, packs: &[(&str, &str, char)]) -> Catalog {
        let packs: Vec<_> = packs
            .iter()
            .map(|(id, version, digest)| release(id, version, *digest))
            .collect();
        let mut document = json!({"schema_version":1,"packs":packs});
        if let Some(sequence) = sequence {
            document["sequence"] = json!(sequence);
        }
        local_catalog(&document).unwrap()
    }

    /// The identity of the source behind `catalog`.
    fn identity() -> String {
        local("/srv/acme-registry").identity().to_owned()
    }

    /// An empty catalog authenticated against the local source at `path`.
    fn empty_at(path: &str) -> Catalog {
        Catalog::verify(
            &local(path),
            br#"{"schema_version":1,"packs":[]}"#,
            None,
            now(),
        )
        .unwrap()
    }

    #[test]
    fn rollback_and_replacement_preserve_retained_checkpoints() {
        let mut checkpoints = Checkpoints::default();
        checkpoints
            .admit(&catalog(Some(2), &[("acme/a", "1.0.0", '0')]))
            .unwrap();
        let before = checkpoints.clone();
        for (candidate, error) in [
            (
                catalog(Some(1), &[("acme/a", "1.0.0", '0')]),
                CheckpointError::Rollback,
            ),
            (
                catalog(Some(2), &[("acme/a", "1.0.0", '1')]),
                CheckpointError::Rollback,
            ),
            (
                catalog(
                    Some(3),
                    &[("acme/b", "1.0.0", '0'), ("acme/a", "1.0.0", '1')],
                ),
                CheckpointError::Replaced("acme/a@1.0.0".into()),
            ),
        ] {
            assert_eq!(checkpoints.admit(&candidate), Err(error));
            assert_eq!(checkpoints, before);
        }
        assert_eq!(
            checkpoints.release_digest(&identity(), "acme/a@1.0.0"),
            Some("0".repeat(64).as_str())
        );
    }

    #[test]
    fn removed_releases_stay_retained_and_identical_catalogs_readmit() {
        let mut checkpoints = Checkpoints::default();
        let first = catalog(Some(1), &[("acme/a", "1.0.0", '0')]);
        checkpoints.admit(&first).unwrap();
        checkpoints.admit(&first).unwrap();
        checkpoints
            .admit(&catalog(Some(2), &[("acme/b", "1.0.0", '1')]))
            .unwrap();
        assert_eq!(
            checkpoints.admit(&catalog(Some(3), &[("acme/a", "1.0.0", '2')])),
            Err(CheckpointError::Replaced("acme/a@1.0.0".into()))
        );
        assert_eq!(checkpoints.get(&identity()).unwrap().releases.len(), 2);
        assert_eq!(checkpoints.len(), 1);
    }

    #[test]
    fn unsequenced_local_catalogs_skip_sequence_checks_but_keep_digests() {
        let mut checkpoints = Checkpoints::default();
        checkpoints
            .admit(&catalog(None, &[("acme/a", "1.0.0", '0')]))
            .unwrap();
        checkpoints
            .admit(&catalog(
                None,
                &[("acme/a", "1.0.0", '0'), ("acme/b", "1.0.0", '1')],
            ))
            .unwrap();
        assert!(matches!(
            checkpoints.admit(&catalog(None, &[("acme/a", "1.0.0", '9')])),
            Err(CheckpointError::Replaced(_))
        ));
    }

    #[test]
    fn local_sequencing_cannot_be_dropped_or_reset_after_persistence() {
        let mut checkpoints = Checkpoints::default();
        checkpoints
            .admit(&catalog(None, &[("acme/a", "1.0.0", '0')]))
            .unwrap();
        let sequenced = catalog(Some(4), &[("acme/a", "1.0.0", '0')]);
        checkpoints.admit(&sequenced).unwrap();
        let stored = serde_json::to_vec(&checkpoints).unwrap();
        let mut restored: Checkpoints = serde_json::from_slice(&stored).unwrap();

        for candidate in [
            catalog(None, &[]),
            catalog(None, &[("acme/a", "1.0.0", '0'), ("acme/b", "1.0.0", '1')]),
            catalog(Some(3), &[("acme/a", "1.0.0", '0')]),
            catalog(Some(4), &[]),
        ] {
            assert_eq!(restored.admit(&candidate), Err(CheckpointError::Rollback));
            assert_eq!(restored, checkpoints);
        }
        restored.admit(&sequenced).unwrap();
        assert_eq!(restored, checkpoints);
        restored.admit(&catalog(Some(5), &[])).unwrap();
        assert_eq!(restored.get(&identity()).unwrap().sequence, Some(5));
        assert_eq!(
            restored.admit(&catalog(Some(6), &[("acme/a", "1.0.0", '1')])),
            Err(CheckpointError::Replaced("acme/a@1.0.0".into()))
        );
    }

    #[test]
    fn source_bound_refuses_new_identities_only() {
        let mut checkpoints = Checkpoints::default();
        for i in 0..MAX_SOURCES {
            checkpoints
                .admit(&empty_at(&format!("/srv/source-{i}")))
                .unwrap();
        }
        assert_eq!(
            checkpoints.admit(&empty_at("/srv/one-more")),
            Err(CheckpointError::Limit)
        );
        checkpoints.admit(&empty_at("/srv/source-0")).unwrap();
    }

    #[test]
    fn serialized_shape_round_trips() {
        let mut checkpoints = Checkpoints::default();
        checkpoints
            .admit(&catalog(Some(4), &[("acme/a", "1.0.0", '0')]))
            .unwrap();
        // A catalog from another source is admitted under its own identity.
        checkpoints.admit(&empty_at("/srv/other")).unwrap();
        assert_eq!(
            checkpoints
                .get(local("/srv/other").identity())
                .unwrap()
                .releases
                .len(),
            0
        );
        assert_eq!(checkpoints.get(&identity()).unwrap().sequence, Some(4));
        let value = serde_json::to_value(&checkpoints).unwrap();
        assert_eq!(value[identity()]["sequence"], json!(4));
        assert_eq!(
            serde_json::from_value::<Checkpoints>(value).unwrap(),
            checkpoints
        );
    }
}
