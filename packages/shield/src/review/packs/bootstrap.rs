//! Verified initial baseline closure, acquired before any enrollment or confirmation writes.
use super::{candidate_artifact, registries};
use crate::{
    policy::{self, Pack, Profile},
    review::Error,
};
use canonic_registry::{
    acquisition::Fetcher,
    catalog::Entry,
    checkpoint::Checkpoints,
    registrations::Effective,
    resolution::{self, Instance, Release, Root},
    selector::InstalledSelector,
    source::Source,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(in crate::review) struct Initial {
    pub(in crate::review) rows: Vec<Value>,
    pub(in crate::review) bytes: Vec<String>,
    pub(in crate::review) checkpoints: BTreeMap<String, Value>,
}
pub(in crate::review) fn acquire(
    project: &Path,
    profile: Profile,
    fetcher: &Fetcher,
) -> Result<Initial, Error> {
    prepare(project, profile, fetcher, &registries::effective(None)?)
}
fn prepare(
    project: &Path,
    profile: Profile,
    fetcher: &Fetcher,
    registered: &Effective,
) -> Result<Initial, Error> {
    let main = registered
        .select(Some(registries::BUILTIN))
        .map_err(|_| Error::Invalid("main registry source invalid"))?;
    let instance = Instance {
        registry: main.identity().to_owned(),
        pack: "canonic/baseline"
            .parse()
            .map_err(|_| Error::Invalid("baseline identity"))?,
    };
    let roots = vec![Root {
        instance: instance.clone(),
        requirement: "*"
            .parse()
            .map_err(|_| Error::Invalid("baseline requirement"))?,
        pin: None,
        level: profile,
    }];
    let sources = by_identity(main, registered);
    let mut catalogs = BTreeMap::new();
    let mut releases = Vec::new();
    let mut artifacts = BTreeMap::new();
    let mut pending = BTreeSet::from([instance]);
    let mut visited = BTreeSet::new();
    let mut size = 0usize;
    while let Some(instance) = pending.pop_first() {
        if !visited.insert(instance.clone()) {
            continue;
        }
        if visited.len() > 64 {
            return Err(policy::Error::Limit.into());
        }
        let (alias, source) = sources.get(&instance.registry).ok_or(Error::Invalid(
            "baseline dependency registry is not enrolled",
        ))?;
        if !catalogs.contains_key(&instance.registry) {
            catalogs.insert(
                instance.registry.clone(),
                registries::catalog(fetcher, source)?,
            );
        }
        let catalog = &catalogs[&instance.registry];
        let mut published = false;
        for entry in catalog.entries().iter().filter(|e| e.id == instance.pack) {
            published = true;
            let Some(bytes) = candidate_artifact(fetcher, source, entry)? else {
                continue;
            };
            size = size.saturating_add(bytes.len());
            if size > crate::configuration::MAX_BUNDLE_BYTES || releases.len() >= 4096 {
                return Err(policy::Error::Limit.into());
            }
            let pack = Pack::from_yaml(&bytes)?;
            let release = Release {
                instance: instance.clone(),
                version: entry.version.clone(),
                dependencies: pack
                    .dependencies()
                    .iter()
                    .map(policy::Declaration::parsed)
                    .collect::<Result<_, _>>()?,
            };
            pending.extend(release.dependencies.iter().map(|d| {
                Instance {
                    registry: d
                        .registry
                        .clone()
                        .unwrap_or_else(|| instance.registry.clone()),
                    pack: d.pack.clone(),
                }
            }));
            artifacts.insert(
                (instance.clone(), release.version.clone()),
                (alias.clone(), source.clone(), entry.clone(), bytes),
            );
            releases.push(release);
        }
        if published && !releases.iter().any(|r| r.instance == instance) {
            return Err(Error::UnsupportedReleases(instance.pack.to_string()));
        }
    }
    let selected = resolution::resolve(&roots, &releases, &sources.keys().cloned().collect())?;
    let (rows, bytes) = installed(project, &selected, &artifacts);
    let mut checkpoints = Checkpoints::default();
    for catalog in catalogs.values() {
        registries::admit(&mut checkpoints, catalog)?;
    }
    Ok(Initial {
        rows,
        bytes,
        checkpoints: registries::retained(&checkpoints)?,
    })
}

/// Installed rows and exact bytes for the selected closure.
fn installed(
    project: &Path,
    selected: &[resolution::Selection<'_, Profile>],
    artifacts: &BTreeMap<(Instance, semver::Version), (String, Source, Entry, String)>,
) -> (Vec<Value>, Vec<String>) {
    let mut rows = Vec::new();
    let mut bytes = Vec::new();
    for selection in selected {
        let release = selection.release;
        let (alias, source, entry, content) =
            &artifacts[&(release.instance.clone(), release.version.clone())];
        let seed = format!("{}:{}:{}", project.display(), source.identity(), entry.id);
        let selector = InstalledSelector::from_seed(seed.as_bytes());
        rows.push(json!({"selector":selector,"id":entry.id,"version":entry.version,"profile":selection.level,"digest":entry.sha256,"source_identity":source.identity(),"source_alias":alias,"source":source,"pinned":false,"direct":selection.direct}));
        bytes.push(content.clone());
    }
    (rows, bytes)
}
/// Every registered source with its alias, keyed by identity.
fn by_identity(main: &Source, registered: &Effective) -> BTreeMap<String, (String, Source)> {
    let mut sources = BTreeMap::from([(
        main.identity().to_owned(),
        (registries::BUILTIN.to_owned(), main.clone()),
    )]);
    for (alias, source, _) in registered.iter() {
        if alias != registries::BUILTIN {
            sources.insert(
                source.identity().to_owned(),
                (alias.to_owned(), source.clone()),
            );
        }
    }
    sources
}

#[cfg(test)]
mod tests {
    use super::*;
    use canonic_registry::registrations::Registrations;
    use std::fs;
    fn fixture() -> (assert_fs::TempDir, Source) {
        let root = assert_fs::TempDir::new().unwrap();
        let baseline=json!({"schema_version":1,"kind":"pack","id":"canonic/baseline","version":"0.1.0","description":"Acme test baseline","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1"],"rules":[]}).to_string();
        fs::write(root.path().join("baseline.yml"), &baseline).unwrap();
        fs::write(root.path().join("catalog.json"),json!({"schema_version":1,"packs":[{"id":"canonic/baseline","version":"0.1.0","path":"baseline.yml","sha256":policy::fingerprint(baseline.as_bytes())}]}).to_string()).unwrap();
        let source = Source::local(&root.path().canonicalize().unwrap()).unwrap();
        (root, source)
    }
    /// Registrations whose built-in registry is the local fixture.
    fn registered(main: &Source) -> Effective {
        Effective::merge((registries::BUILTIN, main), &Registrations::default(), None).unwrap()
    }
    #[test]
    fn initial_baseline_is_verified_removable_tracking_and_has_no_preconfirmation_writes() {
        let (root, source) = fixture();
        let identity = source.identity().to_owned();
        let initial = prepare(
            root.path(),
            Profile::Recommended,
            &registries::fetcher(),
            &registered(&source),
        )
        .unwrap();
        assert_eq!(initial.rows.len(), 1);
        assert_eq!(initial.rows[0]["id"], "canonic/baseline");
        assert_eq!(initial.rows[0]["profile"], "recommended");
        assert_eq!(initial.rows[0]["direct"], true);
        assert_eq!(initial.rows[0]["pinned"], false);
        assert_eq!(
            initial.rows[0]["digest"],
            policy::fingerprint(initial.bytes[0].as_bytes())
        );
        assert!(
            initial.checkpoints[&identity]["releases"]
                .get("canonic/baseline@0.1.0")
                .is_some()
        );
        assert!(!root.path().join(".shield").exists());
        let empty = json!(
            crate::configuration::Bundle::capture(r#"{"schema_version":1,"packs":[]}"#, &[], None)
                .unwrap()
        );
        let candidate = super::super::rebuild(&empty, &initial.rows, &initial.bytes, None).unwrap();
        let candidates = BTreeMap::from([("project".into(), candidate)]);
        let evaluator =
            crate::configuration::Evaluator::new(crate::review::snapshots(&candidates).unwrap())
                .unwrap();
        crate::review::storage::Store::initialize_verified(
            root.path(),
            candidates,
            evaluator.revision(),
            json!({"sources":{},"default":null,"projection":null,"installed":initial.rows,"compiled_cache_root":root.path().join("cache")}),
            initial.checkpoints.clone(),
        )
        .unwrap();
        let state = crate::review::storage::Store::open(root.path())
            .unwrap()
            .read()
            .unwrap();
        assert!(state.active.is_some());
        assert_eq!(state.registry_checkpoints, initial.checkpoints);
        assert_eq!(state.operator["installed"][0]["profile"], "recommended");
    }
    /// Publishes a release in a policy schema this Shield version cannot read.
    fn publish_unreadable(root: &Path, version: &str) {
        let bytes = format!(
            "{{\"schema_version\":2,\"kind\":\"pack\",\"id\":\"canonic/baseline\",\"version\":\"{version}\",\"engine\":\">=0.1.0\"}}"
        );
        let path = format!("baseline-{version}.yml");
        fs::write(root.join(&path), &bytes).unwrap();
        let catalog = root.join("catalog.json");
        let mut document: Value = serde_json::from_slice(&fs::read(&catalog).unwrap()).unwrap();
        document["packs"].as_array_mut().unwrap().push(json!({"id":"canonic/baseline","version":version,"path":path,"sha256":policy::fingerprint(bytes.as_bytes())}));
        fs::write(&catalog, document.to_string()).unwrap();
    }
    #[test]
    fn releases_in_unreadable_formats_are_skipped() {
        let (root, source) = fixture();
        publish_unreadable(root.path(), "0.0.9");
        publish_unreadable(root.path(), "0.2.0");
        let initial = prepare(
            root.path(),
            Profile::Recommended,
            &registries::fetcher(),
            &registered(&source),
        )
        .unwrap();
        assert_eq!(initial.rows.len(), 1);
        assert_eq!(initial.rows[0]["version"], "0.1.0");
    }
    /// Publishes a schema-1 release in the format that declared one `engine` range.
    fn publish_single_engine(root: &Path, version: &str, range: &str) {
        let bytes = json!({"schema_version":1,"kind":"pack","id":"canonic/baseline","version":version,"description":"Acme historical baseline","license":"MIT","maintainers":["Acme"],"engine":range,"capabilities":["structured-rules-v1"],"rules":[]}).to_string();
        let path = format!("baseline-{version}.yml");
        fs::write(root.join(&path), &bytes).unwrap();
        let catalog = root.join("catalog.json");
        let mut document: Value = serde_json::from_slice(&fs::read(&catalog).unwrap()).unwrap();
        document["packs"].as_array_mut().unwrap().push(json!({"id":"canonic/baseline","version":version,"path":path,"sha256":policy::fingerprint(bytes.as_bytes())}));
        fs::write(&catalog, document.to_string()).unwrap();
    }
    #[test]
    fn single_engine_releases_outside_this_version_are_skipped() {
        let (root, source) = fixture();
        publish_single_engine(root.path(), "0.0.9", ">=0.0.1, <0.0.2");
        let initial = prepare(
            root.path(),
            Profile::Recommended,
            &registries::fetcher(),
            &registered(&source),
        )
        .unwrap();
        assert_eq!(initial.rows.len(), 1);
        assert_eq!(initial.rows[0]["version"], "0.1.0");
    }
    #[test]
    fn malformed_release_within_its_single_engine_range_still_refuses() {
        let (root, source) = fixture();
        publish_single_engine(root.path(), "0.0.9", "*");
        assert!(
            prepare(
                root.path(),
                Profile::Recommended,
                &registries::fetcher(),
                &registered(&source),
            )
            .is_err()
        );
        assert!(!root.path().join(".shield").exists());
    }
    #[test]
    fn no_readable_release_names_the_pack_and_leaves_the_project_unenrolled() {
        let (root, source) = fixture();
        fs::write(
            root.path().join("catalog.json"),
            r#"{"schema_version":1,"packs":[]}"#,
        )
        .unwrap();
        publish_unreadable(root.path(), "0.0.9");
        let error = prepare(
            root.path(),
            Profile::Recommended,
            &registries::fetcher(),
            &registered(&source),
        )
        .err()
        .unwrap();
        assert!(
            matches!(error, Error::UnsupportedReleases(ref pack) if pack == "canonic/baseline")
        );
        assert!(!root.path().join(".shield").exists());
    }
    #[test]
    fn initial_digest_failure_leaves_the_project_unenrolled() {
        let (root, source) = fixture();
        fs::write(root.path().join("baseline.yml"), "Acme tampering").unwrap();
        assert!(
            prepare(
                root.path(),
                Profile::Strict,
                &registries::fetcher(),
                &registered(&source)
            )
            .is_err()
        );
        assert!(!root.path().join(".shield").exists());
    }
}
