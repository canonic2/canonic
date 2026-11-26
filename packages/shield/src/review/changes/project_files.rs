//! Capture the editable policy files without writing them or importing terminal UI.
use crate::review::{Context, Error, model::State};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub(in crate::review) struct Capture {
    pub(in crate::review) candidate: Value,
    pub(in crate::review) snapshot: crate::configuration::Snapshot,
    pub(in crate::review) metadata: Value,
    pub(in crate::review) paths: BTreeMap<String, Option<String>>,
}

type Reader<'a> = dyn FnMut(&str) -> Result<Option<String>, Error> + 'a;

pub(in crate::review) fn capture(context: &Context, before: &State) -> Result<Capture, Error> {
    crate::diagnostics::trace::count("policy.file_source_reads", 1);
    let root = crate::review::project::root(context);
    let directory = crate::layout::bundle(&root);
    // Missing pack artifacts resolve exactly as they do for enforcement.
    capture_with(before, &mut |name| {
        Ok(read_file(&root, name)?.or_else(|| {
            crate::review::integrations::project_sources::stored_artifact(&directory, name)
        }))
    })
}

fn capture_with(before: &State, source: &mut Reader<'_>) -> Result<Capture, Error> {
    // Each path, including an absent optional file, is observed once. Both
    // policy parsing and the confirmation binding consume that observation.
    let mut files = BTreeMap::new();
    let mut read = |name: &str| {
        if !files.contains_key(name) {
            files.insert(name.to_owned(), source(name)?);
        }
        Ok(files[name].clone())
    };
    let (source, local, config) = adopted_config(&mut read)?;
    let mut meta = crate::review::project::metadata(before);
    crate::review::packs::repair_disabled_metadata(&before.candidates["project"], &mut meta)?;
    let known = known_releases(&meta, &mut read)?;
    let mut selected = Vec::new();
    let mut bytes = Vec::new();
    let mut disabled_rows = Vec::new();
    let mut disabled_bytes = Vec::new();
    for (list, rows, artifacts) in [
        ("packs", &mut selected, &mut bytes),
        ("disabled", &mut disabled_rows, &mut disabled_bytes),
    ] {
        for (index, row) in config[list].as_array().into_iter().flatten().enumerate() {
            let path = row["path"]
                .as_str()
                .ok_or(Error::Invalid("pack path missing"))?;
            let artifact = read(path)?.ok_or(Error::Invalid("project artifact unreadable"))?;
            let digest = crate::policy::fingerprint(artifact.as_bytes());
            // A known release with these exact bytes keeps its registry
            // provenance; any other pack the files name is, as for
            // enforcement, the project files' own release.
            let original = known
                .iter()
                .find(|p| {
                    p["id"] == row["id"]
                        && (row["registry_identity"].is_null()
                            || p["source_identity"] == row["registry_identity"])
                        && p["version"] == row["version"]
                        && p["digest"] == digest.as_str()
                })
                .cloned()
                .unwrap_or_else(|| {
                    crate::review::integrations::project_sources::project_release(
                        row, &artifact, index,
                    )
                });
            let mut selection = original;
            selection["profile"] = row["profile"].clone();
            if let Some(fields) = selection.as_object_mut() {
                fields.remove("layer");
            }
            if row["layer"] == "local" {
                selection["layer"] = json!("local");
            }
            rows.push(selection);
            artifacts.push(artifact);
        }
    }
    meta["installed"] = json!(selected);
    if disabled_rows.is_empty() {
        if let Some(fields) = meta.as_object_mut() {
            fields.remove("disabled");
        }
    } else {
        meta["disabled"] = json!(disabled_rows);
    }
    let rules = config["rules"]
        .as_str()
        .map(|path| read(path)?.ok_or(Error::Invalid("manual rules unreadable")))
        .transpose()?;
    let local_rules = config["local_rules"]
        .as_str()
        .map(|path| read(path)?.ok_or(Error::Invalid("project artifact unreadable")))
        .transpose()?;
    let (bundle, snapshot) = crate::configuration::Bundle::capture_with_snapshot(
        &source,
        local.as_deref(),
        &bytes,
        rules.as_deref(),
        local_rules.as_deref(),
        &disabled_bytes,
    )?;
    let candidate = json!(bundle);
    // Keep the same bound as the encoded bundle reader without allocating and
    // decoding JSON just to reconstruct the snapshot we already have.
    let encoded =
        crate::json::encoded_len(&candidate).map_err(|_| Error::Invalid("candidate encoding"))?;
    if encoded > crate::configuration::MAX_BUNDLE_BYTES {
        return Err(crate::policy::Error::Limit.into());
    }
    update_shared_metadata(before, &source, &candidate, &mut meta)?;
    let paths = files
        .into_iter()
        .map(|(name, source)| {
            (
                name,
                source.map(|bytes| crate::policy::fingerprint(bytes.as_bytes())),
            )
        })
        .collect();
    Ok(Capture {
        candidate,
        snapshot,
        metadata: meta,
        paths,
    })
}

/// Installed and locally disabled selections are both known releases, and so
/// is every release the lock files name: a pull or merge that brings a new
/// release brings its lock row with it.
fn known_releases(meta: &Value, read: &mut Reader<'_>) -> Result<Vec<Value>, Error> {
    let mut known: Vec<Value> = meta["installed"]
        .as_array()
        .into_iter()
        .chain(meta["disabled"].as_array())
        .chain(meta["shared_installed"].as_array())
        .flatten()
        .cloned()
        .collect();
    for lock in ["shield.lock", "shield.local.lock"] {
        if let Some(text) = read(lock)? {
            let locked: Value = serde_json::from_str(&text)
                .map_err(|_| Error::Invalid("project lock file is not valid JSON"))?;
            known.extend(locked["packs"].as_array().into_iter().flatten().cloned());
        }
    }
    Ok(known)
}

fn update_shared_metadata(
    before: &State,
    source: &str,
    candidate: &Value,
    meta: &mut Value,
) -> Result<(), Error> {
    if before.candidates["project"]["config"]
        .as_str()
        .map(crate::policy::parse::<Value>)
        .transpose()?
        == Some(crate::policy::parse::<Value>(source)?)
    {
        meta["shared_installed"] = json!(crate::review::publication::projections::shared_rows(
            &before.candidates["project"],
            &before.operator
        )?);
    } else {
        crate::review::packs::refresh_shared_metadata(candidate, meta)?;
    }
    Ok(())
}

/// Capture both configuration layers exactly as written: the files mean what
/// they say, as they do for every request.
fn adopted_config(read: &mut Reader<'_>) -> Result<(String, Option<String>, Value), Error> {
    let source = read("config.yml")?.ok_or(Error::Invalid("project config unreadable"))?;
    let _: Value =
        crate::policy::parse(&source).map_err(|_| Error::Invalid("invalid project config"))?;
    let local = read("config.local.yml")?;
    let config: Value = crate::policy::parse(&crate::configuration::layers::merge(
        &source,
        local.as_deref(),
    )?)
    .map_err(|_| Error::Invalid("invalid project config"))?;
    Ok((source, local, config))
}

/// Read one contained regular file, preserving absence as fingerprint evidence.
/// Read the policy file `path` of the project at `root`, named relative to its
/// selected bundle, as [`crate::layout::bundle_file`] places it.
pub(in crate::review) fn read_file(root: &Path, path: &str) -> Result<Option<String>, Error> {
    crate::configuration::portable_path(path)?;
    let directory = crate::layout::bundle(root);
    let file = crate::layout::bundle_file(root, path);
    match std::fs::symlink_metadata(&file) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err(Error::Invalid("project projection must be a regular file")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let resolved = file.canonicalize()?;
    if !resolved.starts_with(directory.canonicalize()?) {
        return Err(Error::Invalid("manual artifact escapes project"));
    }
    crate::application::read(&resolved)
        .map(Some)
        .map_err(|_| Error::Invalid("project artifact unreadable"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_disabled_release_can_be_reenabled_by_local_file_adoption() {
        let root = assert_fs::TempDir::new().unwrap();
        let artifact = json!({"schema_version":1,"kind":"pack","id":"acme/read","version":"1.0.0","description":"Retained source fixture","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1"],"rules":[]}).to_string();
        let source = json!({"schema_version":1,"rules":"rules.yml","packs":[{"id":"acme/read","registry_identity":"main","version":"1.0.0","profile":"recommended","path":"read.yml"}]}).to_string();
        let local = "schema_version: 1\npacks:\n  - id: acme/read\n    registry_identity: main\n    disabled: true\n";
        let rules =
            "schema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1]\nrules: []\n";
        let bundle = crate::configuration::Bundle::capture_layers(
            &source,
            Some(local),
            &[],
            Some(rules),
            None,
            std::slice::from_ref(&artifact),
        )
        .unwrap();
        let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
        let mut state = crate::review::project::load(&context).unwrap();
        state.operator.as_object_mut().unwrap().remove("disabled");
        state
            .operator
            .as_object_mut()
            .unwrap()
            .remove("shared_installed");
        let previous = crate::review::publication::projections::projections(
            &state.candidates["project"],
            &state.operator,
        )
        .unwrap();
        let captured = capture_with(&state, &mut |name| {
            Ok(match name {
                "config.yml" => Some(source.clone()),
                "rules.yml" => Some(rules.into()),
                "read.yml" => Some(artifact.clone()),
                _ => previous
                    .get(name)
                    .cloned()
                    .filter(|_| name != "config.local.yml"),
            })
        })
        .unwrap();
        assert_eq!(captured.metadata["installed"][0]["trust"], "unverified");
        assert_eq!(
            captured.metadata["installed"][0]["source_alias"],
            "project-files"
        );
        assert!(captured.metadata["installed"][0]["source"]["key"].is_null());
        let next = crate::review::publication::projections::projections(
            &captured.candidate,
            &captured.metadata,
        )
        .unwrap();
        assert_eq!(next["shield.lock"], previous["shield.lock"]);
        assert_eq!(next["config.yml"], previous["config.yml"]);
    }

    #[test]
    fn capture_reads_each_file_once_and_records_what_it_read() {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle =
            crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None)
                .unwrap();
        let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
        let state = crate::review::project::load(&context).unwrap();
        let a = "schema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1]\nrules: []\n";
        let b = format!("{a}# unreviewed edit B\n");
        let directory = root.path().join(".shield");
        std::fs::write(
            directory.join("config.yml"),
            "schema_version: 1\npacks: []\nrules: rules.yml\n",
        )
        .unwrap();
        std::fs::write(directory.join("rules.yml"), a).unwrap();
        let mut reads = 0;
        let captured = capture_with(&state, &mut |name| {
            let bytes = read_file(root.path(), name)?;
            if name == "rules.yml" {
                reads += 1;
                // The reader observed A; immediately after it, disk is B.
                std::fs::write(directory.join(name), &b).unwrap();
            }
            Ok(bytes)
        })
        .unwrap();
        assert_eq!(reads, 1);
        assert_eq!(captured.candidate["rules"], a);
        // The recorded hash is of the bytes read, not of the later edit.
        assert_eq!(
            captured.paths["rules.yml"],
            Some(crate::policy::fingerprint(a.as_bytes()))
        );
    }
}
