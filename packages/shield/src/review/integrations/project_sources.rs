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
    selection_rows(config, candidate, "packs")
}

/// Disabled shared artifacts retain independent metadata for the shared lock.
pub(in crate::review) fn disabled(
    config: &Value,
    candidate: &Value,
) -> Result<Vec<Value>, crate::review::Error> {
    if config.get("disabled").is_none() {
        return Ok(Vec::new());
    }
    selection_rows(config, candidate, "disabled")
}

fn selection_rows(
    config: &Value,
    candidate: &Value,
    field: &str,
) -> Result<Vec<Value>, crate::review::Error> {
    let rows = config[field]
        .as_array()
        .ok_or(crate::review::Error::Invalid("packs missing"))?;
    Ok(rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            project_release(
                row,
                candidate[field][index].as_str().unwrap_or_default(),
                index,
            )
        })
        .collect())
}

/// Release metadata for a pack the policy files name with exact bytes but no
/// known registry release: its provenance is the project files, with no
/// registry signing trust. Hooks enforce such a pack from its bytes alone.
pub(in crate::review) fn project_release(row: &Value, artifact: &str, index: usize) -> Value {
    let project_identity = crate::policy::fingerprint(b"shield-project-files");
    let source_identity = row["registry_identity"]
        .as_str()
        .unwrap_or(&project_identity);
    let digest = crate::policy::fingerprint(artifact.as_bytes());
    let seed = crate::policy::fingerprint(format!("{source_identity}:{digest}:{index}").as_bytes());
    let mut installed = json!({"selector":format!("pack:{}-{}-{}-{}-{}",&seed[..8],&seed[8..12],&seed[12..16],&seed[16..20],&seed[20..32]),"id":row["id"],"version":row["version"],"profile":row["profile"],"digest":digest,"source_identity":source_identity,"source_alias":"project-files","source":{"identity":source_identity,"location":".shield","key":null},"pinned":true,"direct":true});
    if row["layer"] == "local" {
        installed["layer"] = json!("local");
    }
    installed
}

/// The bytes of a project's policy files, as read, before any validation.
pub(in crate::review) struct Sources {
    shared: String,
    local: Option<String>,
    packs: Vec<String>,
    rules: Option<String>,
    local_rules: Option<String>,
    disabled: Vec<String>,
    /// `shield.lock` and `shield.local.lock`, when present.
    locks: [Option<String>; 2],
}

impl Sources {
    /// Every file's bytes in a fixed order, for a content key.
    pub(in crate::review) fn parts(&self) -> impl Iterator<Item = Option<&str>> {
        [
            Some(self.shared.as_str()),
            self.local.as_deref(),
            self.rules.as_deref(),
            self.local_rules.as_deref(),
            self.locks[0].as_deref(),
            self.locks[1].as_deref(),
        ]
        .into_iter()
        .chain(self.packs.iter().map(|pack| Some(pack.as_str())))
        .chain([None])
        .chain(self.disabled.iter().map(|pack| Some(pack.as_str())))
    }

    /// The configuration layers' text, shared then local, for keys that
    /// treat their settings separately; [`Self::parts`] begins with these two.
    pub(in crate::review) fn config_layers(&self) -> (&str, Option<&str>) {
        (&self.shared, self.local.as_deref())
    }

    /// Validate the files into a bundle, including that every pack a lock
    /// file records has exactly the bytes the lock pins.
    pub(in crate::review) fn bundle(&self) -> Result<configuration::Bundle, Error> {
        self.validated().map(|(bundle, _)| bundle)
    }

    /// [`Self::bundle`] with the snapshot its single validation produced.
    pub(in crate::review) fn validated(
        &self,
    ) -> Result<(configuration::Bundle, configuration::Snapshot), Error> {
        self.check_locks()?;
        configuration::Bundle::capture_with_snapshot(
            &self.shared,
            self.local.as_deref(),
            &self.packs,
            self.rules.as_deref(),
            self.local_rules.as_deref(),
            &self.disabled,
        )
        .map_err(Into::into)
    }

    fn check_locks(&self) -> Result<(), Error> {
        let mut locked = Vec::new();
        for text in self.locks.iter().flatten() {
            let lock: Value = serde_json::from_str(text).map_err(|_| {
                crate::policy::Error::Contract("a Shield lock file is not valid JSON")
            })?;
            locked.extend(lock["packs"].as_array().into_iter().flatten().cloned());
        }
        if locked.is_empty() {
            return Ok(());
        }
        let config: Value = crate::policy::parse(&configuration::layers::merge(
            &self.shared,
            self.local.as_deref(),
        )?)?;
        for (list, artifacts) in [("packs", &self.packs), ("disabled", &self.disabled)] {
            let rows = config[list].as_array().into_iter().flatten();
            for (row, artifact) in rows.zip(artifacts) {
                let rows: Vec<_> = locked
                    .iter()
                    .filter(|lock| {
                        lock["id"] == row["id"]
                            && lock["version"] == row["version"]
                            && (row["registry_identity"].is_null()
                                || lock["source_identity"] == row["registry_identity"])
                    })
                    .collect();
                let digest = crate::policy::fingerprint(artifact.as_bytes());
                if !rows.is_empty() && !rows.iter().any(|lock| lock["digest"] == digest.as_str()) {
                    return Err(crate::policy::Error::Contract(
                        "a pack's bytes differ from the digest its lock file records",
                    )
                    .into());
                }
            }
        }
        Ok(())
    }
}

/// Read config.yml with the optional uncommitted config.local.yml beside it,
/// each file's bytes as read, with every artifact either layer names. Only the
/// configuration is parsed, to find those artifacts.
pub(in crate::review) fn read(config: &Path) -> Result<Sources, Error> {
    read_with(config, false)
}

pub(in crate::review) fn read_retaining(config: &Path) -> Result<Sources, Error> {
    read_with(config, true)
}

fn read_with(config: &Path, retain: bool) -> Result<Sources, Error> {
    crate::diagnostics::trace::count("policy.file_source_reads", 1);
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
    if candidate.rules_path().is_none() {
        refuse_unreferenced_rules(root)?;
    }
    let read = |paths: &mut dyn Iterator<Item = &str>| {
        paths
            .map(|path| read_artifact(root, path, retain))
            .collect::<Result<Vec<_>, _>>()
    };
    let lock = |name: &str| -> Result<Option<String>, Error> {
        let path = root.join(name);
        if path.exists() {
            Ok(Some(application::read(&regular_file(&path)?)?))
        } else {
            Ok(None)
        }
    };
    Ok(Sources {
        packs: read(&mut candidate.pack_paths())?,
        disabled: read(&mut candidate.disabled_paths())?,
        rules: read(&mut candidate.rules_path().into_iter())?.pop(),
        local_rules: read(&mut candidate.local_rules_path().into_iter())?.pop(),
        locks: [lock("shield.lock")?, lock("shield.local.lock")?],
        shared,
        local,
    })
}

/// [`read`] over file texts held in memory, such as a proposal's: `text`
/// returns a policy file's text by its path inside `.shield/`. Digest-named
/// pack artifacts it does not hold come from the per-user store, checked
/// against their digest.
pub(in crate::review) fn from_texts(
    shield: &Path,
    text: &dyn Fn(&str) -> Option<String>,
) -> Result<Sources, Error> {
    let missing = || -> Error { crate::policy::Error::Contract(MISSING_PACK).into() };
    let shared = text("config.yml").ok_or(crate::policy::Error::Contract(
        "project configuration missing",
    ))?;
    let local = text("config.local.yml");
    let candidate =
        Candidate::from_yaml(&configuration::layers::merge(&shared, local.as_deref())?)?;
    let read = |paths: &mut dyn Iterator<Item = &str>| {
        paths
            .map(|path| {
                text(path)
                    .or_else(|| stored_artifact(shield, path))
                    .ok_or_else(missing)
            })
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(Sources {
        packs: read(&mut candidate.pack_paths())?,
        disabled: read(&mut candidate.disabled_paths())?,
        rules: read(&mut candidate.rules_path().into_iter())?.pop(),
        local_rules: read(&mut candidate.local_rules_path().into_iter())?.pop(),
        locks: [text("shield.lock"), text("shield.local.lock")],
        shared,
        local,
    })
}

/// Whether `path` names a pack artifact by its digest, which the per-user
/// store holds for every checkout.
pub(in crate::review) fn digest_named(path: &str) -> Option<&str> {
    let path = Path::new(path);
    // Shield names these files itself, always with a lowercase `.yml`.
    path.extension()
        .filter(|extension| *extension == "yml")
        .and(path.file_stem())
        .and_then(|stem| stem.to_str())
        .filter(|stem| stem.len() == 64 && stem.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Keep a digest-named pack artifact in the per-user store.
pub(in crate::review) fn keep_artifact(path: &str, text: &str) {
    if let Some(digest) = digest_named(path)
        && crate::policy::fingerprint(text.as_bytes()) == digest
    {
        store_artifact(digest, text);
    }
}

/// Every pack, disabled and rules file a captured bundle's configuration names.
pub(in crate::review) fn referenced(bundle: &Value) -> Result<Vec<String>, Error> {
    let Some(shared) = bundle["config"].as_str() else {
        return Ok(Vec::new());
    };
    let candidate = Candidate::from_yaml(&configuration::layers::merge(
        shared,
        bundle["local_config"].as_str(),
    )?)?;
    Ok(candidate
        .pack_paths()
        .chain(candidate.disabled_paths())
        .chain(candidate.rules_path())
        .chain(candidate.local_rules_path())
        .map(str::to_owned)
        .collect())
}

/// Keep every digest-named pack and disabled artifact a captured bundle holds
/// in the per-user store, so the bundle can be rebuilt from its policy file
/// texts on any checkout.
pub(in crate::review) fn keep_bundle_artifacts(bundle: &Value) -> Result<(), Error> {
    let Some(shared) = bundle["config"].as_str() else {
        return Ok(());
    };
    let candidate = Candidate::from_yaml(&configuration::layers::merge(
        shared,
        bundle["local_config"].as_str(),
    )?)?;
    for (paths, texts) in [
        (candidate.pack_paths().collect::<Vec<_>>(), &bundle["packs"]),
        (candidate.disabled_paths().collect(), &bundle["disabled"]),
    ] {
        let texts = texts.as_array().map_or_default(Vec::as_slice);
        for (path, text) in paths.into_iter().zip(texts) {
            if let Some(text) = text.as_str() {
                keep_artifact(path, text);
            }
        }
    }
    Ok(())
}

/// Shield writes an empty `rules.yml` beside every configuration. When the
/// configuration does not name it, rules written there would never apply, so
/// a `rules.yml` that holds rules makes the policy files invalid.
fn refuse_unreferenced_rules(root: &Path) -> Result<(), Error> {
    let path = root.join("rules.yml");
    if !path.is_file() {
        return Ok(());
    }
    let rules: Value = crate::policy::parse(&application::read(&regular_file(&path)?)?)?;
    if rules["rules"]
        .as_array()
        .is_some_and(|rules| !rules.is_empty())
    {
        return Err(crate::policy::Error::Contract(
            "rules.yml holds rules that config.yml does not name; add `rules: rules.yml` to config.yml",
        )
        .into());
    }
    Ok(())
}

/// Capture and validate the policy files under a configuration path.
pub(in crate::review) fn capture(config: &Path) -> Result<configuration::Bundle, Error> {
    read(config)?.bundle()
}

/// The selected configuration, rules and lock files with Git conflict blocks.
pub(in crate::review) fn conflicted_files(config: &Path) -> Vec<(PathBuf, usize)> {
    let Ok(resolved) = regular_file(config) else {
        return Vec::new();
    };
    let Some(root) = resolved.parent() else {
        return Vec::new();
    };
    let shared = application::read(&resolved).ok();
    let local = read_project_artifact(root, "config.local.yml").ok();
    let mut files = Vec::new();
    if let Some(line) = shared.as_deref().and_then(git_conflict_line) {
        files.push((config.to_owned(), line));
    }
    let mut names: Vec<_> = ["rules.yml", "shield.lock"]
        .into_iter()
        .chain(crate::layout::LOCAL_FILES)
        .map(str::to_owned)
        .collect();
    for (layer, keys) in [
        (shared.as_deref(), &["rules", "local_rules"][..]),
        (local.as_deref(), &["rules"]),
    ] {
        let Some(document) = layer.and_then(|text| crate::policy::parse::<Value>(text).ok()) else {
            continue;
        };
        for name in keys.iter().filter_map(|key| document[*key].as_str()) {
            if configuration::portable_path(name).is_ok() && !names.iter().any(|path| path == name)
            {
                names.push(name.to_owned());
            }
        }
    }
    for name in names {
        let line = if name == "config.local.yml" {
            local.as_deref().and_then(git_conflict_line)
        } else {
            read_project_artifact(root, &name)
                .ok()
                .as_deref()
                .and_then(git_conflict_line)
        };
        if let Some(line) = line {
            files.push((root.join(name), line));
        }
    }
    files
}

fn git_conflict_line(text: &str) -> Option<usize> {
    const DEFAULT_MARKER_WIDTH: usize = 7;
    let marker = |line: &str, prefix| {
        let rest = line.trim_start_matches(prefix);
        let width = line.len() - rest.len();
        (width >= DEFAULT_MARKER_WIDTH && (rest.is_empty() || rest.starts_with(' ')))
            .then_some(width)
    };
    let mut opening = None;
    let mut separated = false;
    for (index, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        if let Some(width) = marker(line, '<') {
            opening = Some((index + 1, width));
            separated = false;
        } else if let Some((start, width)) = opening {
            if line.len() == width && line.bytes().all(|byte| byte == b'=') {
                separated = true;
            } else if separated && marker(line, '>') == Some(width) {
                return Some(start);
            }
        }
    }
    None
}

/// Read a pack artifact from the configuration directory. An artifact named by
/// its SHA-256 digest that is missing there, as in a fresh clone or worktree,
/// comes from the per-user artifact store or this folder's earlier retained
/// sources, checked against that digest; one read from the project is added to
/// the store for other checkouts.
pub(in crate::review) const MISSING_PACK: &str =
    "a locked pack is missing from this machine; use shield pack restore or pack_restore";

fn read_artifact(root: &Path, path: &str, retain: bool) -> Result<String, Error> {
    let digest = Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| stem.len() == 64 && stem.bytes().all(|b| b.is_ascii_hexdigit()));
    let Some(digest) = digest else {
        return read_project_artifact(root, path);
    };
    let text = if root.join(path).exists() {
        let text = read_project_artifact(root, path)?;
        // A pack named by its digest must have exactly those bytes.
        if crate::policy::fingerprint(text.as_bytes()) != digest {
            return Err(crate::policy::Error::Contract(
                "a pack's bytes differ from the digest in its file name",
            )
            .into());
        }
        text
    } else {
        stored_artifact(root, path).ok_or(crate::policy::Error::Contract(MISSING_PACK))?
    };
    if retain {
        store_artifact(digest, &text);
    }
    Ok(text)
}

/// A digest-named pack artifact that is not in the configuration directory
/// `shield`, from the per-user store or this folder's retained sources, checked
/// against its digest. Every reader of the policy files resolves missing
/// artifacts through this one function.
pub(in crate::review) fn stored_artifact(shield: &Path, path: &str) -> Option<String> {
    let digest = Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| stem.len() == 64 && stem.bytes().all(|b| b.is_ascii_hexdigit()))?;
    let store = crate::review::files::artifact_store().ok();
    // Retained sources were written by earlier releases into the state
    // directory beside a root `.shield/` bundle, under either state name.
    let blob = format!("{digest}.blob");
    let candidates = [
        store.map(|store| store.join(format!("{digest}.yml"))),
        Some(shield.join("state/sources").join(&blob)),
        Some(shield.join("review/sources").join(&blob)),
    ];
    candidates.into_iter().flatten().find_map(|candidate| {
        std::fs::read_to_string(candidate)
            .ok()
            .filter(|text| crate::policy::fingerprint(text.as_bytes()) == digest)
    })
}

fn store_artifact(digest: &str, text: &str) {
    let Ok(store) = crate::review::files::artifact_store() else {
        return;
    };
    let path = store.join(format!("{digest}.yml"));
    if !path.exists() {
        // The store is a cache: a failed write only means a later fetch.
        let _ = crate::review::files::write_private(&path, text.as_bytes());
    }
}

fn read_project_artifact(root: &Path, path: &str) -> Result<String, Error> {
    let resolved = regular_file(&root.join(path))?;
    if !resolved.starts_with(root) {
        return Err(crate::policy::Error::Contract(
            "artifact must resolve to a file inside the configuration directory",
        )
        .into());
    }
    application::read(&resolved)
}

pub(in crate::review) fn regular_file(path: &Path) -> Result<PathBuf, Error> {
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
    fn conflict_diagnostics_require_at_least_the_default_marker_width() {
        assert_eq!(git_conflict_line("< HEAD\n{}\n=\n{}\n> topic\n"), None);
        assert_eq!(
            git_conflict_line("\n<<<<<<<<< HEAD\n{}\n=========\n{}\n>>>>>>>>> topic\n"),
            Some(2)
        );
    }

    #[test]
    fn conflict_diagnostics_follow_rule_paths_in_each_readable_layer() {
        let root = assert_fs::TempDir::new().unwrap();
        let shared = root.child(".shield/config.yml");
        shared
            .write_str("schema_version: 1\npacks: []\nrules: custom.json\n")
            .unwrap();
        root.child(".shield/config.local.yml")
            .write_str("schema_version: 1\nrules: personal.yml\n")
            .unwrap();
        let marker = "<<<<<<< HEAD\n{}\n=======\n{}\n>>>>>>> topic\n";
        root.child(".shield/custom.json")
            .write_str(&format!("\n{marker}"))
            .unwrap();
        root.child(".shield/personal.yml")
            .write_str(marker)
            .unwrap();
        let check = || {
            crate::review::files::check(root.path(), None)
                .unwrap_err()
                .to_string()
        };
        let both = check();
        assert!(both.contains("custom.json (line 2)"), "{both}");
        assert!(both.contains("personal.yml (line 1)"), "{both}");
        shared.write_str(marker).unwrap();
        let local = check();
        assert!(local.contains("config.yml (line 1)"), "{local}");
        assert!(local.contains("personal.yml (line 1)"), "{local}");
        assert!(!local.contains("custom.json"), "{local}");
    }

    #[test]
    fn source_only_metadata_preserves_shared_and_local_replacement_identities() {
        let config = json!({"packs":[{"id":"acme/tools","registry_identity":"acme","version":"2.0.0","profile":"strict","layer":"local"}],
            "disabled":[{"id":"acme/tools","registry_identity":"acme","version":"1.0.0","profile":"recommended"}]});
        let candidate = json!({"packs":["local artifact"],"disabled":["shared artifact"]});
        let active = installed(&config, &candidate).unwrap();
        let shared = disabled(&config, &candidate).unwrap();
        assert_eq!(active[0]["source_identity"], "acme");
        assert_eq!(active[0]["source"]["identity"], "acme");
        assert_eq!(active[0]["layer"], "local");
        assert_eq!(shared[0]["source_identity"], "acme");
        assert!(shared[0].get("layer").is_none());
        assert_eq!(
            shared[0]["digest"],
            crate::policy::fingerprint(b"shared artifact")
        );
    }

    #[test]
    fn a_missing_artifact_comes_from_retained_sources_or_the_store_by_digest() {
        let root = assert_fs::TempDir::new().expect("fixture");
        let shield = root.child(".shield");
        shield.create_dir_all().expect("folder");
        let text = format!("acme artifact {}\n", std::process::id());
        let digest = crate::policy::fingerprint(text.as_bytes());
        let path = format!("cache/{digest}.yml");
        let shield = shield.path().canonicalize().expect("canonical");

        assert!(
            read_artifact(&shield, &path, false).is_err(),
            "nothing holds it yet"
        );
        let sources = shield.join("review/sources");
        std::fs::create_dir_all(&sources).expect("sources");
        std::fs::write(sources.join(format!("{digest}.blob")), "tampered").expect("tampered");
        assert!(
            read_artifact(&shield, &path, false).is_err(),
            "bytes must match the digest"
        );

        std::fs::write(sources.join(format!("{digest}.blob")), &text).expect("blob");
        assert_eq!(
            read_artifact(&shield, &path, false).expect("from sources"),
            text
        );
        assert!(
            !crate::review::files::artifact_store()
                .unwrap()
                .join(format!("{digest}.yml"))
                .exists()
        );
        assert_eq!(
            read_artifact(&shield, &path, true).expect("retain from sources"),
            text
        );
        std::fs::remove_dir_all(&sources).expect("forget sources");
        assert_eq!(
            read_artifact(&shield, &path, false).expect("from store"),
            text
        );
    }

    #[test]
    fn a_pack_named_by_a_digest_must_have_those_bytes() {
        let root = assert_fs::TempDir::new().expect("fixture");
        let digest = crate::policy::fingerprint(b"acme original");
        root.child(format!(".shield/cache/{digest}.yml"))
            .write_str("acme substituted")
            .expect("artifact");
        let shield = root
            .path()
            .join(".shield")
            .canonicalize()
            .expect("canonical");
        let error = read_artifact(&shield, &format!("cache/{digest}.yml"), false).unwrap_err();
        assert!(
            error.to_string().contains("digest in its file name"),
            "{error}"
        );
    }

    #[test]
    fn lock_rows_pin_the_bytes_of_the_packs_they_record() {
        let root = assert_fs::TempDir::new().expect("fixture");
        root.child(".shield/config.yml")
            .write_str("schema_version: 1\npacks:\n- id: acme/tools\n  version: 1.0.0\n  profile: recommended\n  path: cache/acme.yml\n")
            .expect("config");
        root.child(".shield/cache/acme.yml")
            .write_str("acme substituted")
            .expect("artifact");
        let lock = |digest: &str| {
            json!({"schema_version":1,"packs":[{"id":"acme/tools","version":"1.0.0","digest":digest}]})
                .to_string()
        };
        root.child(".shield/shield.lock")
            .write_str(&lock(&crate::policy::fingerprint(b"acme original")))
            .expect("lock");
        let config = root.path().join(".shield/config.yml");
        let Err(error) = read(&config).expect("sources").bundle() else {
            panic!("bytes the lock does not pin must be refused");
        };
        assert!(error.to_string().contains("lock file records"), "{error}");
        root.child(".shield/shield.lock")
            .write_str(&lock(&crate::policy::fingerprint(b"acme substituted")))
            .expect("lock");
        // Matching bytes pass the lock check; validation then reaches the pack.
        let result = read(&config).expect("sources").bundle();
        assert!(
            result
                .as_ref()
                .err()
                .is_none_or(|error| !error.to_string().contains("lock file")),
            "{:?}",
            result.err()
        );
    }

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
