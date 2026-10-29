//! Editable project-file projections, drift fingerprints and bounded repair.
use crate::{
    configuration, policy, policy_files,
    review::{
        self, Context, Error,
        model::State,
        project::{metadata, root},
        storage::Store,
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, io::Write, path::Path};

/// The editable .shield files of a candidate: the shared configuration, rules
/// and lock, the local layer's files when it has content, and pack artifacts.
/// Each configuration and rules file is its retained source.
pub(in crate::review) fn projections(
    candidate: &Value,
    meta: &Value,
) -> Result<BTreeMap<String, String>, Error> {
    let config = configuration::layers::merged(candidate)?;
    let shared = candidate["config"]
        .as_str()
        .ok_or(Error::Invalid("candidate config missing"))?;
    let mut files = BTreeMap::new();
    files.insert("config.yml".into(), policy_files::projected(shared)?);
    files.insert(
        "rules.yml".into(),
        policy_files::projected(
            candidate["rules"]
                .as_str()
                .unwrap_or(r#"{"schema_version":1,"kind":"custom","capabilities":["structured-rules-v1"],"rules":[]}"#),
        )?,
    );
    let rows = meta["installed"].as_array().cloned().unwrap_or_default();
    let layer = |local: bool| {
        let selected: Vec<_> = rows
            .iter()
            .enumerate()
            .filter(|(index, _)| (config["packs"][*index]["layer"] == "local") == local)
            .map(|(_, row)| portable(row))
            .collect();
        let mut locked = crate::review::packs::summaries(&json!({"installed":selected}));
        locked["schema_version"] = json!(1);
        (selected.is_empty(), locked.to_string())
    };
    files.insert("shield.lock".into(), layer(false).1);
    if let Some(local) = candidate["local_config"].as_str() {
        files.insert("config.local.yml".into(), policy_files::projected(local)?);
        let (none, locked) = layer(true);
        if !none {
            files.insert("shield.local.lock".into(), locked);
        }
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
    policy::fingerprint(json!(files).to_string().as_bytes())
}

pub(in crate::review) fn projection_changed(
    context: &Context,
    state: &State,
) -> Result<bool, Error> {
    if let Some(expected) = state.operator["projection_file_hashes"].as_object() {
        // A local file created by hand is an edit too.
        for name in ["config.local.yml", "rules.local.yml"] {
            if !expected.contains_key(name) && root(context).join(".shield").join(name).exists() {
                return Ok(true);
            }
        }
        for (name, hash) in expected {
            let path = root(context).join(".shield").join(name);
            if !path.is_file() || fs::symlink_metadata(&path)?.file_type().is_symlink() {
                return Ok(true);
            }
            if json!(policy::fingerprint(
                crate::application::read(&path)
                    .map_err(|_| Error::Invalid("project projection unreadable"))?
                    .as_bytes()
            )) != *hash
            {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    let expected = metadata(state)["projection"].as_str().map(str::to_owned);
    let Some(expected) = expected else {
        return Ok(false);
    };
    let captured = Store::open(Path::new(&context.project_root))?
        .captured_candidate(&state.candidates["project"])?;
    let mut actual = projections(&captured, &metadata(state))?;
    for (name, bytes) in &mut actual {
        let path = root(context).join(".shield").join(name);
        if !path.is_file() || fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Ok(true);
        }
        *bytes = crate::application::read(&path)
            .map_err(|_| Error::Invalid("project projection unreadable"))?;
    }
    Ok(digest(&actual) != expected)
}
pub(in crate::review) fn file_hashes(files: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|(name, bytes)| (name.clone(), policy::fingerprint(bytes.as_bytes())))
        .collect()
}

pub(in crate::review) fn projection_fingerprint(
    context: &Context,
    state: &State,
) -> Result<String, Error> {
    let mut actual = projections(&state.candidates["project"], &metadata(state))?;
    for (name, bytes) in &mut actual {
        *bytes = crate::application::read(&root(context).join(".shield").join(name))
            .map_err(|_| Error::Invalid("project projection unreadable"))?;
    }
    Ok(digest(&actual))
}

pub(in crate::review) fn materialize(
    context: &Context,
    files: &BTreeMap<String, String>,
) -> Result<(), Error> {
    let directory = root(context).join(".shield");
    for (name, bytes) in files {
        crate::configuration::Candidate::from_yaml(
            &json!({"schema_version":1,"packs":[],"rules":name}).to_string(),
        )?;
        let path = directory.join(name);
        let parent = path.parent().ok_or(Error::Invalid("projection parent"))?;
        for ancestor in parent.ancestors().take_while(|p| p.starts_with(&directory)) {
            if fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(Error::Invalid("projection parent cannot be a link"));
            }
        }
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
        fs::rename(temp, path)
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
    for name in files.keys() {
        let path = root(context).join(".shield").join(name);
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

pub(in crate::review) fn repair_projections(
    context: &Context,
    files: &BTreeMap<String, String>,
    meta: &Value,
) -> Result<(), Error> {
    let actual = projection_hashes(context, files)?;
    for (name, bytes) in files {
        if !actual[name].is_null()
            && actual[name] != policy::fingerprint(bytes.as_bytes())
            && actual[name] != meta["projection_previous"][name]
        {
            return Err(Error::Invalid(
                "manual projections differ; review changes through shield policy review",
            ));
        }
    }
    materialize(context, files)
}
