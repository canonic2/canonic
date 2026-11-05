//! Capture editable policy files without activating them or importing terminal UI.
use crate::review::{Context, Error, model::State};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub(in crate::review) struct Capture {
    pub(in crate::review) candidate: Value,
    pub(in crate::review) metadata: Value,
    pub(in crate::review) fingerprint: String,
}

type Reader<'a> = dyn FnMut(&str) -> Result<Option<String>, Error> + 'a;

/// Missing or linked projections require repair rather than file adoption.
pub(in crate::review) fn adoption_available(context: &Context) -> Result<bool, Error> {
    let state = crate::review::storage::Store::open(std::path::Path::new(&context.project_root))?
        .read_checkpoint()?;
    state.check(context)?;
    let Some(edited) =
        crate::review::publication::projections::edited_projections(context, &state)?
    else {
        return Ok(false);
    };
    let directory = crate::review::project::root(context).join(".shield");
    Ok(!edited.is_empty()
        && edited.iter().all(|name| {
            std::fs::symlink_metadata(directory.join(name)).is_ok_and(|metadata| metadata.is_file())
        }))
}

/// Bind the union of current and proposed output paths, including local files
/// that did not exist in the active generation. Missing files remain evidence.
pub(in crate::review) fn fingerprint(
    context: &Context,
    before: &State,
    candidate: &Value,
    meta: &Value,
) -> Result<String, Error> {
    let directory = crate::review::project::root(context).join(".shield");
    fingerprint_with(before, candidate, meta, &mut |name| {
        read_file(&directory, name)
    })
}

fn fingerprint_with(
    before: &State,
    candidate: &Value,
    meta: &Value,
    read: &mut Reader<'_>,
) -> Result<String, Error> {
    use crate::review::publication::projections::projections;
    let mut files = projections(&before.candidates["project"], &before.operator)?;
    files.extend(projections(candidate, meta)?);
    files.entry("config.local.yml".into()).or_default();
    files.entry("rules.local.yml".into()).or_default();
    let mut hashes = BTreeMap::new();
    for name in files.keys() {
        let hash = read(name)?.map(|bytes| crate::policy::fingerprint(bytes.as_bytes()));
        hashes.insert(name, hash);
    }
    Ok(crate::policy::fingerprint(
        json!(hashes).to_string().as_bytes(),
    ))
}

pub(in crate::review) fn capture(context: &Context, before: &State) -> Result<Capture, Error> {
    let directory = crate::review::project::root(context).join(".shield");
    capture_with(before, &mut |name| read_file(&directory, name))
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
    // Installed and locally disabled selections are both known releases.
    let known: Vec<Value> = meta["installed"]
        .as_array()
        .into_iter()
        .chain(meta["disabled"].as_array())
        .flatten()
        .cloned()
        .collect();
    let mut selected = Vec::new();
    let mut bytes = Vec::new();
    let mut disabled_rows = Vec::new();
    let mut disabled_bytes = Vec::new();
    for (list, rows, artifacts) in [
        ("packs", &mut selected, &mut bytes),
        ("disabled", &mut disabled_rows, &mut disabled_bytes),
    ] {
        for row in config[list].as_array().into_iter().flatten() {
            let original = known
                .iter()
                .find(|p| {
                    p["id"] == row["id"]
                        && (row["registry_identity"].is_null()
                            || p["source_identity"] == row["registry_identity"])
                        && p["version"] == row["version"]
                })
                .ok_or(Error::Invalid("use shield pack add for a new pack release"))?;
            let path = row["path"]
                .as_str()
                .ok_or(Error::Invalid("pack path missing"))?;
            let artifact = read(path)?.ok_or(Error::Invalid("project artifact unreadable"))?;
            if crate::policy::fingerprint(artifact.as_bytes()) != original["digest"] {
                return Err(Error::Invalid(
                    "installed artifact bytes changed; reacquire the pack",
                ));
            }
            let mut selection = original.clone();
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
    let rules = read("rules.yml")?.ok_or(Error::Invalid("manual rules unreadable"))?;
    let local_rules = config["local_rules"]
        .as_str()
        .map(|path| read(path)?.ok_or(Error::Invalid("project artifact unreadable")))
        .transpose()?;
    let candidate = json!(crate::configuration::Bundle::capture_layers(
        &source,
        local.as_deref(),
        &bytes,
        Some(&rules),
        local_rules.as_deref(),
        &disabled_bytes,
    )?);
    let fingerprint = fingerprint_with(before, &candidate, &meta, &mut read)?;
    Ok(Capture {
        candidate,
        metadata: meta,
        fingerprint,
    })
}

/// Capture both configuration layers, preserving comments when selecting rules.yml.
fn adopted_config(read: &mut Reader<'_>) -> Result<(String, Option<String>, Value), Error> {
    let source = read("config.yml")?.ok_or(Error::Invalid("project config unreadable"))?;
    let mut shared: Value =
        crate::policy::parse(&source).map_err(|_| Error::Invalid("invalid project config"))?;
    let source = if shared["rules"] == "rules.yml" {
        source
    } else {
        shared["rules"] = json!("rules.yml");
        crate::policy_files::write(Some(&source), &shared)?.text
    };
    let local = read("config.local.yml")?;
    let config: Value = crate::policy::parse(&crate::configuration::layers::merge(
        &source,
        local.as_deref(),
    )?)
    .map_err(|_| Error::Invalid("invalid project config"))?;
    Ok((source, local, config))
}

/// Read one contained regular file, preserving absence as fingerprint evidence.
fn read_file(directory: &Path, path: &str) -> Result<Option<String>, Error> {
    crate::configuration::portable_path(path)?;
    match std::fs::symlink_metadata(directory.join(path)) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err(Error::Invalid("project projection must be a regular file")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let resolved = directory.join(path).canonicalize()?;
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
    fn project_files_adopt_fingerprint_uses_candidate_reads_even_if_disk_changes_back() {
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
            "schema_version: 1\npacks: []\n",
        )
        .unwrap();
        std::fs::write(directory.join("rules.yml"), a).unwrap();
        let mut reads = 0;
        let captured = capture_with(&state, &mut |name| {
            let bytes = read_file(&directory, name)?;
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
        let live = fingerprint(&context, &state, &captured.candidate, &captured.metadata).unwrap();
        assert_ne!(captured.fingerprint, live);
        std::fs::write(directory.join("rules.yml"), a).unwrap();
        assert_eq!(
            captured.fingerprint,
            fingerprint(&context, &state, &captured.candidate, &captured.metadata).unwrap()
        );
        std::fs::write(directory.join("rules.yml"), &b).unwrap();
        assert_ne!(
            captured.fingerprint,
            fingerprint(&context, &state, &captured.candidate, &captured.metadata).unwrap()
        );
    }
}
