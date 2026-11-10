//! Exact variant selection and monotonic optional consent observations.
use super::{BTreeMap, Error, Namespace, Value, json};

pub(super) fn locks_for_source(
    namespace: &Namespace,
    source: &Value,
    metadata: &Value,
) -> Result<Value, Error> {
    let candidate = json!({"config":namespace.field(source,"config",None)?,"local_config":namespace.field(source,"local_config",None)?});
    locks(&candidate, metadata)
}
fn locks(candidate: &Value, metadata: &Value) -> Result<Value, Error> {
    let projected = crate::review::publication::projections::locks(candidate, metadata)?;
    let mut values = serde_json::Map::new();
    for (name, text) in projected {
        values.insert(
            name,
            crate::json::parse(text.as_bytes())
                .map_err(|_| Error::Invalid("portable lock encoding"))?,
        );
    }
    Ok(Value::Object(values))
}
fn matches_locks(config: &std::path::Path, expected: &Value) -> Result<bool, Error> {
    let expected = expected
        .as_object()
        .ok_or(Error::Invalid("private consent locks invalid"))?;
    if expected
        .keys()
        .any(|key| !["shield.lock", "shield.local.lock"].contains(&key.as_str()))
        || !expected.contains_key("shield.lock")
    {
        return Err(Error::Invalid("private consent locks invalid"));
    }
    let root = config
        .parent()
        .ok_or(Error::Invalid("project configuration parent"))?
        .canonicalize()?;
    for name in ["shield.lock", "shield.local.lock"] {
        let path = root.join(name);
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
            Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                return Err(Error::Invalid("portable lock must be a regular file"));
            }
            Ok(_) => {}
        }
        let file = std::fs::File::open(path)?;
        if file.metadata()?.len() > super::super::super::MAX_STATE as u64 {
            return Err(crate::policy::Error::Limit.into());
        }
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(
            &mut std::io::Read::take(file, super::super::super::MAX_STATE as u64 + 1),
            &mut bytes,
        )?;
        if bytes.len() > super::super::super::MAX_STATE {
            return Err(crate::policy::Error::Limit.into());
        }
        let actual: Value = crate::json::parse(&bytes)
            .map_err(|_| Error::Invalid("portable lock invalid; review project files"))?;
        if expected.get(name) != Some(&actual) {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(super) fn matches(
    config: &std::path::Path,
    namespace: &Namespace,
    source: &Value,
    authored: &Value,
    expected: &Value,
) -> Result<bool, Error> {
    if !matches_locks(config, expected)? {
        return Ok(false);
    }
    let root = config
        .parent()
        .ok_or(Error::Invalid("project configuration parent"))?;
    let candidate =
        crate::configuration::Candidate::from_yaml(&crate::configuration::layers::merge(
            authored["config"]
                .as_str()
                .ok_or(Error::Invalid("config missing"))?,
            authored["local_config"].as_str(),
        )?)?;
    let mut partial = authored.clone();
    for (field, paths) in [
        ("packs", candidate.pack_paths().collect::<Vec<_>>()),
        ("disabled", candidate.disabled_paths().collect::<Vec<_>>()),
    ] {
        let mut rows = Vec::with_capacity(paths.len());
        for (index, path) in paths.iter().enumerate() {
            rows.push(match std::fs::symlink_metadata(root.join(path)) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Null,
                Err(error) => return Err(error.into()),
                Ok(_) => namespace.field(source, field, Some(index))?,
            });
        }
        partial[field] = Value::Array(rows);
    }
    crate::review::integrations::project_sources::matches_artifacts(config, &partial)
        .map_err(|_| Error::Invalid("cannot validate project artifacts against reviewed policy"))
}
pub(in crate::review::storage::portability) fn matches_legacy(
    config: &std::path::Path,
    candidate: &Value,
    metadata: &Value,
) -> Result<bool, Error> {
    matches_locks(config, &locks(candidate, metadata)?)
}

/// Conflicting authenticated observations refuse refresh; stale checkpoints
/// retain the strongest prior evidence instead of replacing it with a floor.
pub(super) fn merge_observations(current: &mut Value, previous: &Value) -> Result<(), Error> {
    let old_modes = previous["metadata"]["registry_trust"].as_object();
    if let Some(old_modes) = old_modes {
        if current["metadata"]["registry_trust"].is_null() {
            current["metadata"]["registry_trust"] = json!({});
        }
        for (source, old) in old_modes {
            let new = &current["metadata"]["registry_trust"][source];
            let mut old_higher = false;
            let mut new_higher = false;
            if !new.is_null() {
                for (version, revision) in [
                    ("generation", "head_revision"),
                    ("root_version", "root_revision"),
                    ("revocation_sequence", "revocation_revision"),
                ] {
                    let old_version = old[version]
                        .as_u64()
                        .ok_or(Error::Invalid("private consent trust floor invalid"))?;
                    let new_version = new[version]
                        .as_u64()
                        .ok_or(Error::Invalid("private consent trust floor invalid"))?;
                    if old_version == new_version && old[revision] != new[revision] {
                        return Err(Error::Invalid(
                            "private consent trust observations conflict",
                        ));
                    }
                    old_higher |= old_version > new_version;
                    new_higher |= new_version > old_version;
                }
                if old_higher && new_higher {
                    return Err(Error::Invalid(
                        "private consent trust observations conflict",
                    ));
                }
            }
            if new.is_null() || old_higher {
                current["metadata"]["registry_trust"][source] = old.clone();
            }
        }
    }
    let old: BTreeMap<String, canonic_registry_kit::checkpoint::Checkpoint> =
        serde_json::from_value(previous["registry_checkpoints"].clone())
            .map_err(|_| Error::Invalid("private consent checkpoints invalid"))?;
    let mut new: BTreeMap<String, canonic_registry_kit::checkpoint::Checkpoint> =
        serde_json::from_value(current["registry_checkpoints"].clone())
            .map_err(|_| Error::Invalid("private consent checkpoints invalid"))?;
    for (source, retained) in old {
        if let Some(observed) = new.get_mut(&source) {
            if retained.sequence > observed.sequence {
                observed
                    .check_observation(&retained)
                    .map_err(|_| Error::Invalid("private consent catalog observations conflict"))?;
                *observed = retained;
            } else {
                retained
                    .check_observation(observed)
                    .map_err(|_| Error::Invalid("private consent catalog observations conflict"))?;
            }
        } else {
            new.insert(source, retained);
        }
    }
    if new.len() > canonic_registry_kit::checkpoint::MAX_SOURCES {
        return Err(crate::policy::Error::Limit.into());
    }
    current["registry_checkpoints"] = serde_json::to_value(new)
        .map_err(|_| Error::Invalid("private consent checkpoints encoding"))?;
    Ok(())
}
