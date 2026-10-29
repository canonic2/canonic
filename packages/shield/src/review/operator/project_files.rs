//! Capture project configuration and its contained artifacts before initialization.
use crate::{
    application::{self, Error},
    configuration::{self, Candidate},
};
use std::path::{Path, PathBuf};

/// Capture config.yml with the optional uncommitted config.local.yml beside it,
/// each file's bytes as read, with every artifact either layer names.
pub(super) fn capture(config: &Path) -> Result<configuration::Bundle, Error> {
    let path = regular_file(config)?;
    let root = path.parent().ok_or(Error::Context)?;
    let shared = application::read(&path)?;
    let local = root.join("config.local.yml");
    let local = if local.exists() {
        Some(application::read(&regular_file(&local)?)?)
    } else {
        None
    };
    let candidate =
        Candidate::from_yaml(&configuration::layers::merge(&shared, local.as_deref())?)?;
    let read = |paths: &mut dyn Iterator<Item = &str>| {
        paths
            .map(|path| read_artifact(root, path))
            .collect::<Result<Vec<_>, _>>()
    };
    let packs = read(&mut candidate.pack_paths())?;
    let disabled = read(&mut candidate.disabled_paths())?;
    let rules = read(&mut candidate.rules_path().into_iter())?.pop();
    let local_rules = read(&mut candidate.local_rules_path().into_iter())?.pop();
    configuration::Bundle::capture_layers(
        &shared,
        local.as_deref(),
        &packs,
        rules.as_deref(),
        local_rules.as_deref(),
        &disabled,
    )
    .map_err(Into::into)
}

fn read_artifact(root: &Path, path: &str) -> Result<String, Error> {
    let resolved = regular_file(&root.join(path))?;
    if !resolved.starts_with(root) {
        return Err(crate::policy::Error::Contract(
            "artifact must resolve to a file inside the configuration directory",
        )
        .into());
    }
    application::read(&resolved)
}

fn regular_file(path: &Path) -> Result<PathBuf, Error> {
    let resolved = path.canonicalize()?;
    if !resolved.is_file() {
        return Err(
            crate::policy::Error::Contract("candidate inputs must be regular files").into(),
        );
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;

    #[test]
    fn capture_retains_each_file_as_written() {
        let root = assert_fs::TempDir::new().expect("fixture");
        let config = "# Acme policy.\nschema_version: 1\npacks: []\nrules: rules.yml\n";
        let local = "# Personal roots.\nschema_version: 1\npacks: []\naccess_paths:\n  read:\n  - /srv/acme\n";
        let rules = "# Acme rules.\nschema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1]\nrules: []\n";
        for (name, text) in [
            ("config.yml", config),
            ("config.local.yml", local),
            ("rules.yml", rules),
        ] {
            root.child(".shield")
                .child(name)
                .write_str(text)
                .expect("file");
        }
        let bundle =
            serde_json::json!(capture(&root.path().join(".shield/config.yml")).expect("captured"));
        assert_eq!(bundle["config"], config);
        assert_eq!(bundle["local_config"], local);
        assert_eq!(bundle["rules"], rules);
        assert_eq!(
            configuration::layers::merged(&bundle).expect("merged")["local_access_paths"]["read"],
            serde_json::json!(["/srv/acme"])
        );
    }
}
