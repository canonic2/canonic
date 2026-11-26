//! Editable project-file projections, drift fingerprints and bounded repair.
use crate::{
    configuration, policy, policy_files,
    review::{self, Context, Error, project::root},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
};

/// The editable .shield files of a candidate: the shared configuration, rules
/// and lock, the local layer's files when it has content, and pack artifacts.
/// Each configuration and rules file is its retained source.
pub(in crate::review) fn projections(
    candidate: &Value,
    meta: &Value,
) -> Result<BTreeMap<String, String>, Error> {
    let _trace = crate::diagnostics::trace::span("projection.generate");
    let config = configuration::layers::merged(candidate)?;
    let shared = candidate["config"]
        .as_str()
        .ok_or(Error::Invalid("candidate config missing"))?;
    let mut files = locks(candidate, meta)?;
    files.insert("config.yml".into(), policy_files::projected(shared)?);
    files.insert(
        "rules.yml".into(),
        policy_files::projected(
            candidate["rules"]
                .as_str()
                .unwrap_or(r#"{"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1"],"rules":[]}"#),
        )?,
    );
    if let Some(local) = candidate["local_config"].as_str() {
        files.insert("config.local.yml".into(), policy_files::projected(local)?);
    }
    if let (Some(path), Some(source)) = (
        config["local_rules"].as_str(),
        candidate["local_rules"].as_str(),
    ) {
        files.insert(path.into(), policy_files::projected(source)?);
    }
    for (selections, artifacts) in [("packs", "packs"), ("disabled", "disabled")] {
        for (index, row) in config[selections]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let path = row["path"]
                .as_str()
                .ok_or(Error::Invalid("pack path missing"))?;
            files.insert(
                path.into(),
                candidate[artifacts][index]
                    .as_str()
                    .ok_or(Error::Invalid("pack bytes missing"))?
                    .into(),
            );
        }
    }
    Ok(files)
}

/// The projection closure without cloning or rendering any artifact bodies.
pub(in crate::review) fn paths(candidate: &Value, meta: &Value) -> Result<BTreeSet<String>, Error> {
    let config = configuration::layers::merged(candidate)?;
    let mut paths = BTreeSet::from([
        "config.yml".into(),
        "rules.yml".into(),
        "shield.lock".into(),
    ]);
    if candidate["local_config"].is_string() {
        paths.insert("config.local.yml".into());
        if local_rows(&config, meta).next().is_some() {
            paths.insert("shield.local.lock".into());
        }
    }
    if candidate["local_rules"].is_string()
        && let Some(path) = config["local_rules"].as_str()
    {
        paths.insert(path.into());
    }
    for selections in ["packs", "disabled"] {
        for row in config[selections].as_array().into_iter().flatten() {
            paths.insert(
                row["path"]
                    .as_str()
                    .ok_or(Error::Invalid("pack path missing"))?
                    .into(),
            );
        }
    }
    Ok(paths)
}

/// Portable selection locks need authoring documents and reviewed metadata,
/// never artifact bodies. Consent and publication use this one projection codec.
pub(in crate::review) fn locks(
    candidate: &Value,
    meta: &Value,
) -> Result<BTreeMap<String, String>, Error> {
    crate::diagnostics::trace::count("projection.lock_sets_rendered", 1);
    let config = configuration::layers::merged(candidate)?;
    let render = |rows: Vec<Value>| json!({"packs":rows,"schema_version":1}).to_string();
    let shared_rows: Vec<_> = shared_rows(candidate, meta)?.iter().map(portable).collect();
    let mut files = BTreeMap::from([("shield.lock".into(), render(shared_rows))]);
    if candidate["local_config"].is_string() {
        let selected: Vec<_> = local_rows(&config, meta).map(portable).collect();
        if !selected.is_empty() {
            files.insert("shield.local.lock".into(), render(selected));
        }
    }
    Ok(files)
}

/// Metadata for the effective selections in the local layer, in lockfile order.
fn local_rows<'a>(config: &'a Value, meta: &'a Value) -> impl Iterator<Item = &'a Value> {
    meta["installed"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(move |(index, row)| (config["packs"][index]["layer"] == "local").then_some(row))
}

/// Shared graph records are independent of the local graph's incoming edges.
pub(in crate::review) fn shared_rows(candidate: &Value, meta: &Value) -> Result<Vec<Value>, Error> {
    let shared: Value = policy::parse(
        candidate["config"]
            .as_str()
            .ok_or(Error::Invalid("candidate config missing"))?,
    )?;
    let effective = configuration::layers::merged(candidate)?;
    let retained: Vec<_> = if let Some(rows) = meta["shared_installed"].as_array() {
        rows.iter().collect()
    } else {
        meta["installed"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(index, row)| {
                (effective["packs"][index]["layer"] != "local").then_some(row)
            })
            .chain(meta["disabled"].as_array().into_iter().flatten())
            .collect()
    };
    Ok(shared["packs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|selection| {
            retained
                .iter()
                .copied()
                .find(|row| {
                    row["id"] == selection["id"]
                        && (selection["registry_identity"].is_null()
                            || row["source_identity"] == selection["registry_identity"])
                        && (row["version"].is_null() || row["version"] == selection["version"])
                        && (row["profile"].is_null() || row["profile"] == selection["profile"])
                })
                .cloned()
        })
        .collect())
}

/// A lock row without machine-specific paths: a local-directory source, which
/// has no signing key or registry ID, keeps its identity and alias but not its
/// location. Retained state keeps the full row.
pub(in crate::review) fn portable(row: &Value) -> Value {
    let mut row = row.clone();
    if let Some(source) = row["source"].as_object_mut()
        && source.get("key").is_none_or(Value::is_null)
        && source.get("registry_id").is_none_or(Value::is_null)
    {
        source.remove("location");
    }
    row
}

pub(in crate::review) fn digest(files: &BTreeMap<String, String>) -> String {
    crate::json::fingerprint(files).expect("projection files serialize")
}

pub(in crate::review) fn file_hashes(files: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|(name, bytes)| (name.clone(), policy::fingerprint(bytes.as_bytes())))
        .collect()
}

pub(in crate::review) fn materialize(
    context: &Context,
    files: &BTreeMap<String, String>,
) -> Result<(), Error> {
    let project = root(context);
    let directory = crate::layout::bundle(&project);
    for (name, bytes) in files {
        crate::configuration::portable_path(name)?;
        let path = crate::layout::bundle_file(&project, name);
        let parent = path.parent().ok_or(Error::Invalid("projection parent"))?;
        check_parent(&directory, parent)?;
        fs::create_dir_all(parent)?;
        if !parent
            .canonicalize()?
            .starts_with(directory.canonicalize()?)
        {
            return Err(Error::Invalid("projection escapes project"));
        }
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Invalid("projection cannot replace a link"));
        }
        // Projections are also written outside the writer lock, so only a
        // temporary whose writer has exited is removed.
        review::storage::remove_dead_temporaries(parent, temporary_writer);
        let temp = path.with_extension(format!("shield-{}.tmp", std::process::id()));
        replace(&temp, &path, bytes.as_bytes())?;
        crate::diagnostics::trace::count("projection.files_written", 1);
    }
    Ok(())
}

fn check_parent(directory: &Path, parent: &Path) -> Result<(), Error> {
    for ancestor in parent.ancestors().take_while(|p| p.starts_with(directory)) {
        if fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Invalid("projection parent cannot be a link"));
        }
    }
    Ok(())
}

/// Writes `bytes` to the new file `temp`, flushes it and renames it to `path`.
/// Once created, `temp` is removed on any failure.
fn replace(temp: &Path, path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)?;
    let written = (|| {
        file.write_all(bytes)?;
        review::storage::sync(&file)?;
        drop(file);
        crate::review::storage::replace(temp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(temp);
    }
    written.map_err(Error::from)
}

/// The writer PID of a `<name>.shield-<pid>.tmp` projection temporary.
fn temporary_writer(name: &str) -> Option<u32> {
    let (_, stamp) = name.strip_suffix(".tmp")?.rsplit_once('.')?;
    review::storage::stamp_pid(stamp.strip_prefix("shield-")?, 1)
}

pub(in crate::review) fn projection_hashes(
    context: &Context,
    files: &BTreeMap<String, String>,
) -> Result<Value, Error> {
    let mut hashes = serde_json::Map::new();
    let project = root(context);
    let directory = crate::layout::bundle(&project);
    for name in files.keys() {
        crate::configuration::portable_path(name)?;
        let path = crate::layout::bundle_file(&project, name);
        check_parent(
            &directory,
            path.parent().ok_or(Error::Invalid("projection parent"))?,
        )?;
        let hash = match fs::symlink_metadata(&path) {
            Ok(m) if m.is_file() && !m.file_type().is_symlink() => json!(policy::fingerprint(
                crate::application::read(&path)
                    .map_err(|_| Error::Invalid("projection unreadable"))?
                    .as_bytes()
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Null,
            Ok(_) => return Err(Error::Invalid("projection must be a regular file")),
            Err(e) => return Err(e.into()),
        };
        hashes.insert(name.clone(), hash);
    }
    Ok(Value::Object(hashes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_paths_include_the_local_lock_only_for_local_selections() {
        let selection = json!({"id":"acme/read","version":"1.0.0","profile":"recommended","path":"cache/acme.yml"});
        for local in [false, true] {
            let candidate = json!({
                "config": json!({"schema_version":1,"packs":[]}).to_string(),
                "local_config": if local {
                    json!({"schema_version":1,"packs":[selection]}).to_string()
                } else {
                    "schema_version: 1\nenforcement: paused\n".to_owned()
                },
                "packs": if local { vec!["Acme test artifact"] } else { Vec::new() },
            });
            let meta = json!({"installed": if local { vec![&selection] } else { Vec::new() }});
            let paths = paths(&candidate, &meta).unwrap();
            let files = projections(&candidate, &meta).unwrap();
            assert_eq!(paths, files.into_keys().collect::<BTreeSet<_>>());
            assert_eq!(paths.contains("shield.local.lock"), local);
        }
    }

    #[test]
    fn projection_fingerprint_preserves_the_persisted_json_format() {
        let files = BTreeMap::from([
            ("rules.yml".into(), "# Acme Δ\nmessage: \"quoted\"\n".into()),
            ("config.yml".into(), "schema_version: 1\npacks: []\n".into()),
        ]);
        assert_eq!(
            digest(&files),
            policy::fingerprint(json!(files).to_string().as_bytes())
        );
    }
}
