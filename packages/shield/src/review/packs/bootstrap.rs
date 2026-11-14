//! Verified initial baseline closure, acquired before any enrollment or confirmation writes.
use super::{candidate_artifact, registries};
use crate::{
    policy::{self, Pack, Profile},
    review::Error,
};
use canonic_registry_kit::{
    acquisition::Fetcher,
    catalog::{Catalog, Entry},
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

type Artifacts = BTreeMap<(Instance, semver::Version), (String, Source, Entry, String)>;

#[derive(Default)]
struct Candidates {
    releases: Vec<Release<Profile>>,
    artifacts: Artifacts,
    pending: BTreeSet<(Instance, semver::Version)>,
    known: BTreeSet<Instance>,
    size: usize,
}

impl Candidates {
    fn enumerate(
        &mut self,
        instance: Instance,
        is_root: bool,
        fetcher: &Fetcher,
        source: &Source,
        alias: &str,
        catalog: &mut Catalog,
    ) -> Result<(), Error> {
        self.known.insert(instance.clone());
        if self.known.len() > 64 {
            return Err(policy::Error::Limit.into());
        }
        // A dependency-free current baseline needs no historical alternatives.
        // Every other instance is enumerated before the solver chooses a version.
        if is_root
            && let Some(head) = catalog
                .history(&instance.pack)
                .and_then(|_| catalog.select(&instance.pack, None))
                .cloned()
            && let Some(bytes) = candidate_artifact(fetcher, source, &head)?
        {
            let pack = Pack::from_yaml(&bytes)?;
            if pack.dependencies().is_empty() {
                self.size += bytes.len();
                self.releases.push(Release {
                    instance: instance.clone(),
                    version: head.version.clone(),
                    dependencies: Vec::new(),
                });
                self.artifacts.insert(
                    (instance, head.version.clone()),
                    (alias.into(), source.clone(), head, bytes),
                );
                return Ok(());
            }
            // Cache the already authenticated head for the later request.
            self.artifacts.insert(
                (instance.clone(), head.version.clone()),
                (alias.into(), source.clone(), head, bytes),
            );
        }
        registries::history(fetcher, source, catalog, &instance.pack)?;
        for entry in catalog.releases(&instance.pack) {
            self.pending
                .insert((instance.clone(), entry.version.clone()));
            self.releases.push(Release {
                instance: instance.clone(),
                version: entry.version.clone(),
                dependencies: Vec::new(),
            });
        }
        Ok(())
    }

    fn load(
        &mut self,
        instance: &Instance,
        version: &semver::Version,
        fetcher: &Fetcher,
        source: &Source,
        alias: &str,
        catalog: &Catalog,
    ) -> Result<(), Error> {
        let entry = catalog
            .release(&instance.pack, version)
            .expect("enumerated release")
            .clone();
        let key = (instance.clone(), version.clone());
        self.pending.remove(&key);
        let bytes = if let Some((_, _, _, bytes)) = self.artifacts.remove(&key) {
            Some(bytes)
        } else {
            candidate_artifact(fetcher, source, &entry)?
        };
        let Some(bytes) = bytes else {
            self.releases
                .retain(|r| r.instance != *instance || r.version != *version);
            if !self.releases.iter().any(|r| r.instance == *instance) {
                return Err(Error::UnsupportedReleases(instance.pack.to_string()));
            }
            return Ok(());
        };
        self.size = self.size.saturating_add(bytes.len());
        if self.size > crate::configuration::MAX_BUNDLE_BYTES {
            return Err(policy::Error::Limit.into());
        }
        let pack = Pack::from_yaml(&bytes)?;
        let candidate = self
            .releases
            .iter_mut()
            .find(|r| r.instance == *instance && r.version == *version)
            .expect("enumerated release");
        candidate.dependencies = pack
            .dependencies()
            .iter()
            .map(policy::Declaration::parsed)
            .collect::<Result<_, _>>()?;
        self.artifacts
            .insert(key, (alias.into(), source.clone(), entry, bytes));
        Ok(())
    }
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
    let mut candidates = Candidates::default();
    let enrolled = sources.keys().cloned().collect();
    let selected = loop {
        let request = match resolution::resolve_pending(
            &roots,
            &candidates.releases,
            &enrolled,
            &candidates.known,
            &candidates.pending,
        ) {
            Ok(selected) => break selected,
            Err(
                request @ (resolution::Error::CandidatesRequired(_)
                | resolution::Error::MetadataRequired(_, _)),
            ) => request,
            Err(error) => return Err(error.into()),
        };
        let instance = match &request {
            resolution::Error::CandidatesRequired(instance)
            | resolution::Error::MetadataRequired(instance, _) => instance.clone(),
            _ => unreachable!(),
        };
        let (alias, source) = sources.get(&instance.registry).ok_or(Error::Invalid(
            "baseline dependency registry is not enrolled",
        ))?;
        if !catalogs.contains_key(&instance.registry) {
            catalogs.insert(
                instance.registry.clone(),
                registries::catalog(fetcher, source)?,
            );
        }
        let catalog = catalogs
            .get_mut(&instance.registry)
            .expect("acquired catalog");
        match request {
            resolution::Error::CandidatesRequired(_) => {
                let is_root = instance == roots[0].instance;
                candidates.enumerate(instance, is_root, fetcher, source, alias, catalog)?;
            }
            resolution::Error::MetadataRequired(_, version) => {
                candidates.load(&instance, &version, fetcher, source, alias, catalog)?;
            }
            _ => unreachable!(),
        }
    };
    let (rows, bytes) = installed(project, &selected, &candidates.artifacts);
    let mut checkpoints = Checkpoints::default();
    for catalog in catalogs.values() {
        let protected = registries::protected_releases(&rows, catalog.source_identity())?;
        checkpoints
            .admit_retaining(catalog, &protected)
            .map_err(|error| registries::checkpoint_error(&error))?;
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
    artifacts: &Artifacts,
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
    use canonic_registry_kit::registrations::Registrations;
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
        let candidate =
            super::super::rebuild(&empty, &mut initial.rows.clone(), &initial.bytes, None).unwrap();
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
            .read_checkpoint()
            .unwrap();
        // The checkpoint keeps runtime state only; enrollment writes the policy
        // files that hold the baseline.
        assert!(state.policy_revision.is_empty());
        assert_eq!(state.registry_checkpoints, initial.checkpoints);
        assert_eq!(state.operator["installed"][0]["profile"], "recommended");
    }

    #[test]
    fn indexed_current_baseline_never_fetches_its_old_history() {
        let (root, source) = fixture();
        let path = root.path().join("catalog.json");
        let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        document["schema_version"] = json!(3);
        document["histories"] = json!({"canonic/baseline":{"path":"unavailable-history.json",
            "sha256":"a".repeat(64),"first":"0.0.1","last":"0.1.0"}});
        fs::write(&path, document.to_string()).unwrap();
        let initial = prepare(
            root.path(),
            Profile::Recommended,
            &registries::fetcher(),
            &registered(&source),
        )
        .unwrap();
        assert_eq!(initial.rows[0]["version"], "0.1.0");
        assert_eq!(initial.rows.len(), 1);
    }

    #[test]
    fn indexed_baseline_resolves_a_historical_dependency_below_its_current_head() {
        use canonic_registry_kit::testing::{Server, SignedRegistry};
        let mut registry = SignedRegistry::new("acme");
        let pack = |id: &str, version: &str, dependencies: Value| {
            json!({"schema_version":1,"kind":"pack","id":id,"version":version,
                "description":"Acme bootstrap dependency fixture","license":"UNLICENSED",
                "maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},
                "capabilities":["structured-rules-v1","dependencies-v1"],
                "dependencies":dependencies,"rules":[]})
            .to_string()
        };
        registry.release(
            "canonic/baseline",
            "1.0.0",
            pack(
                "canonic/baseline",
                "1.0.0",
                json!([{"pack":"acme/child","requirement":"=1.0.0"}]),
            )
            .as_bytes(),
        );
        for major in 1..=130 {
            let version = format!("{major}.0.0");
            registry.release(
                "acme/child",
                &version,
                pack("acme/child", &version, json!([])).as_bytes(),
            );
        }
        registry.publish_indexed(1, "9999-12-31T00:00:00Z");
        let fetcher = Server::fetcher(std::time::Duration::from_secs(2));
        let source = fetcher
            .discover(registry.server().url(), std::time::SystemTime::now())
            .unwrap()
            .source;
        let project = assert_fs::TempDir::new().unwrap();
        let initial = prepare(
            project.path(),
            Profile::Strict,
            &fetcher,
            &registered(&source),
        )
        .unwrap();
        assert_eq!(initial.rows.len(), 2);
        assert!(
            initial
                .rows
                .iter()
                .any(|row| row["id"] == "canonic/baseline"
                    && row["version"] == "1.0.0"
                    && row["direct"] == true)
        );
        assert!(initial.rows.iter().any(|row| row["id"] == "acme/child"
            && row["version"] == "1.0.0"
            && row["direct"] == false));
        assert!(initial.rows.iter().all(|row| row["profile"] == "strict"));
        let requests = registry.server().requests();
        assert_eq!(
            requests
                .iter()
                .filter(|path| path.contains("/packs/"))
                .count(),
            2,
            "only the baseline and exact child are downloaded across 130 child versions: {requests:?}"
        );
        assert!(!project.path().join(".shield").exists());
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
        publish_single_engine(root.path(), "0.2.0", "*");
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
