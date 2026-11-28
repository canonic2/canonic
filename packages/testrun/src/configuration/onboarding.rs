//! Bounded, read-only app discovery and create-only project initialization.
use super::*;
use std::{collections::BTreeSet, fs, io::Read};
pub(crate) mod api;
mod detection;
pub(crate) use detection::discover;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Candidate {
    pub path: String,
    pub kind: String,
    pub platforms: Vec<String>,
    pub evidence: Vec<String>,
}
#[derive(Serialize)]
pub(crate) struct Discovery {
    pub root: PathBuf,
    pub candidates: Vec<Candidate>,
    pub warnings: Vec<String>,
    pub truncated: bool,
}
pub(crate) fn configuration(candidates: &[Candidate]) -> Result<Value> {
    if candidates.is_empty() || candidates.len() > 128 {
        return Err(Error::invalid("Select 1..128 projects"));
    }
    let mut result = json!({"version":2,"projects":{}});
    let mut used = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for candidate in candidates {
        if !paths.insert(&candidate.path) {
            return Err(Error::invalid("Select each application path once"));
        }
        if !projects::identifier(&candidate.kind) {
            return Err(Error::invalid("Invalid project kind identifier"));
        }
        let base = Path::new(&candidate.path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("app");
        let name = unique_name(base, &mut used);
        let mut profiles = serde_json::Map::new();
        let mut profile_names = BTreeSet::new();
        let mut platforms = BTreeSet::new();
        for platform in &candidate.platforms {
            if !projects::identifier(platform) {
                return Err(Error::invalid("Invalid platform identifier"));
            }
            if !platforms.insert(platform) {
                continue;
            }
            let profile = unique_name(platform, &mut profile_names);
            let target = match platform.as_str() {
                "web" => "browser",
                "ios" => "device",
                "android" => "emulator",
                "macos" | "windows" | "linux" => "desktop",
                _ => "process",
            };
            profiles.insert(profile, json!({"platform":platform,"target_kind":target}));
        }
        let project = json!({"root":candidate.path,"kind":candidate.kind,"profiles":profiles});
        projects::decode_project(&project)?;
        result["projects"][&name] = project;
    }
    projects::schema::validate_root(&result, false)?;
    Ok(result)
}
fn unique_name(base: &str, used: &mut BTreeSet<String>) -> String {
    let mut name: String = base
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(48)
        .collect();
    name = name.trim_matches('-').to_owned();
    if name.is_empty() || !name.as_bytes()[0].is_ascii_lowercase() {
        name = format!("app-{name}");
    }
    let base = name.clone();
    let mut suffix = 2;
    while !used.insert(name.clone()) {
        name = format!("{base}-{suffix}");
        suffix += 1;
    }
    name
}

/// The canonical root and destination file, refusing existing configuration
/// in either default location or at an explicit destination.
pub(crate) fn available(root: &Path, config: Option<&Path>) -> Result<(PathBuf, PathBuf)> {
    super::files::destination(root, config)
}

#[cfg(test)]
pub(crate) fn publish(root: &Path, value: &Value) -> Result<Value> {
    publish_at(root, None, value)
}
pub(crate) fn publish_at(root: &Path, config: Option<&Path>, value: &Value) -> Result<Value> {
    let (root, destination) = available(root, config)?;
    projects::schema::validate_root(value, false)?;
    for project in value["projects"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(_, p)| p)
    {
        projects::decode_project(project)?;
    }
    // JSON is a YAML subset: deterministic quoted scalars avoid YAML ambiguity.
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|_| Error::new("CONFIG_INVALID", "Cannot encode project configuration"))?;
    if bytes.len() > 1_048_576 {
        return Err(Error::invalid("Configuration exceeds 1 MiB"));
    }
    let directory = destination.parent().unwrap_or(&root).to_path_buf();
    // The temporary file shares the destination's directory so the hard link stays on one filesystem.
    let temporary = directory.join(format!(".testrun-init-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        use std::io::Write;
        fs::create_dir_all(&directory)
            .map_err(|_| Error::new("CONFIG_IO", "Cannot create the configuration directory"))?;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|_| Error::new("CONFIG_IO", "Cannot create configuration temporary file"))?;
        file.write_all(&bytes)
            .and_then(|()| file.write_all(b"\n"))
            .map_err(|_| Error::new("CONFIG_IO", "Cannot write project configuration"))?;
        drop(file);
        // A hard link publishes complete bytes atomically without replacing a raced file.
        fs::hard_link(&temporary, &destination).map_err(|error| {
            Error::new(
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    "CONFIG_EXISTS"
                } else {
                    "CONFIG_IO"
                },
                "Cannot publish configuration; existing files are preserved",
            )
        })?;
        Ok(())
    })();
    let cleanup = fs::remove_file(&temporary);
    result?;
    if cleanup.is_err() {
        return Err(Error::new(
            "CONFIG_IO",
            "Configuration was created; temporary-file cleanup failed",
        )
        .details(json!({"created":destination,"temporary":temporary})));
    }
    // The shared initializer's ignore update is also used by config init.
    let ignore =
        super::files::ignore_local(&root, &destination.with_file_name(super::files::LOCAL))
            .map_err(|e| e.details(json!({"created":destination,"ignoreUpdated":false})))?;
    Ok(
        json!({"created":destination,"ignoreUpdated":true,"ignore":ignore,"projects":value["projects"].as_object().unwrap().keys().collect::<Vec<_>>() }),
    )
}
