//! Capture project configuration and its contained artifacts before initialization.
use crate::{
    application::{self, Error},
    configuration::{self, Candidate},
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Local project artifacts acquire no registry trust during initial review.
pub(in crate::review) fn installed(
    config: &Value,
    candidate: &Value,
) -> Result<Vec<Value>, crate::review::Error> {
    let source_identity = crate::policy::fingerprint(b"shield-project-files");
    let rows = config["packs"]
        .as_array()
        .ok_or(crate::review::Error::Invalid("packs missing"))?;
    Ok(rows.iter().enumerate().map(|(index, row)| {
        let digest = crate::policy::fingerprint(candidate["packs"][index].as_str().unwrap_or_default().as_bytes());
        let seed = crate::policy::fingerprint(format!("{source_identity}:{digest}:{index}").as_bytes());
        json!({"selector":format!("pack:{}-{}-{}-{}-{}",&seed[..8],&seed[8..12],&seed[12..16],&seed[16..20],&seed[20..32]),"id":row["id"],"version":row["version"],"profile":row["profile"],"digest":digest,"source_identity":source_identity,"source_alias":"project-files","source":{"identity":source_identity,"location":".shield","key":null},"pinned":true,"direct":true})
    }).collect())
}

/// Capture config.yml with the optional uncommitted config.local.yml beside it,
/// each file's bytes as read, with every artifact either layer names.
pub(in crate::review) fn capture(config: &Path) -> Result<configuration::Bundle, Error> {
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
        let local = format!(
            "# Personal roots.\nschema_version: 1\npacks: []\naccess_paths:\n  read:\n  - {}\n",
            crate::test_support::absolute("/srv/acme")
        );
        let rules = "# Acme rules.\nschema_version: 1\nkind: custom\ncapabilities: [structured-rules-v1]\nrules: []\n";
        for (name, text) in [
            ("config.yml", config),
            ("config.local.yml", local.as_str()),
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
            serde_json::json!([crate::test_support::absolute("/srv/acme")])
        );
    }
}
