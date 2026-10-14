//! A consuming product's install flow across a local and a signed registry.
//!
//! The product's part is played here: pack artifacts are JSON documents that
//! declare dependencies, and the lockfile is a small document embedding the
//! crate's locked entries.
use assert_fs::{TempDir, prelude::*};
use canonic_registry::{
    acquisition::Fetcher,
    cache::ArtifactCache,
    catalog::{Catalog, Entry},
    checkpoint::Checkpoints,
    lock::{self, LockedEdge, LockedPack},
    names::PackId,
    registrations::{Effective, GlobalFile, Registrations, Scope},
    resolution::{self, Dependency, Instance, Release, Root},
    selector::{self, InstalledSelector, Selector},
    source::Source,
    testing::{SignedRegistry, SigningKey},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::SystemTime};

const MAX_ARTIFACT: u64 = 65_536;

fn artifact(dependencies: &Value) -> Vec<u8> {
    json!({ "dependencies": dependencies })
        .to_string()
        .into_bytes()
}

/// Reads every listed release's dependencies, as a product would after validating its packs.
fn releases(fetcher: &Fetcher, source: &Source, catalog: &Catalog) -> Vec<Release<()>> {
    catalog
        .entries()
        .iter()
        .map(|entry| {
            let bytes = fetcher
                .artifact(source, entry, MAX_ARTIFACT)
                .expect("fixture artifact");
            let document: Value = serde_json::from_slice(&bytes).expect("fixture JSON");
            Release {
                instance: Instance {
                    registry: source.identity().to_owned(),
                    pack: entry.id.clone(),
                },
                version: entry.version.clone(),
                dependencies: document["dependencies"]
                    .as_array()
                    .expect("fixture dependencies")
                    .iter()
                    .map(|d| {
                        Dependency::parse(
                            d["pack"].as_str().expect("fixture pack"),
                            d["requirement"].as_str().expect("fixture requirement"),
                            d["registry"].as_str(),
                            None,
                        )
                        .expect("fixture dependency")
                    })
                    .collect(),
            }
        })
        .collect()
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one end-to-end install and offline reproduction"
)]
fn install_resolves_across_registries_locks_and_reproduces_offline() {
    let home = TempDir::new().unwrap();
    let now = SystemTime::now();
    let fetcher = crate::signed_fetcher();

    // A signed team registry, registered globally after first-use review.
    let mut team = SignedRegistry::new("acme-team");
    team.release("acme/design-tokens", "1.0.0", &artifact(&json!([])));
    team.release("acme/design-tokens", "1.3.0", &artifact(&json!([])));
    team.release("acme/design-tokens", "2.0.0", &artifact(&json!([])));
    team.publish(1, "9999-12-31T00:00:00Z");
    let discovered = fetcher.discover(team.server().url(), now).unwrap();
    let global_file = GlobalFile::new(home.path().join(".acme/registries.json"), "main");
    let before = global_file.read().unwrap();
    let mut global = before.clone();
    global
        .add("team", discovered.source.clone(), "main")
        .unwrap();
    global_file.replace(&before, &global).unwrap();
    let team_identity = discovered.source.identity().to_owned();

    // A local project registry whose pack depends on the team registry.
    let local = TempDir::new().unwrap();
    let workbench = artifact(&json!([
        {"pack":"acme/design-tokens","requirement":"^1","registry":team_identity},
        {"pack":"acme/review","requirement":"*"}
    ]));
    let review = artifact(&json!([]));
    local
        .child("packs/workbench.json")
        .write_binary(&workbench)
        .unwrap();
    local
        .child("packs/review.json")
        .write_binary(&review)
        .unwrap();
    local
        .child("catalog.json")
        .write_str(
            &json!({"schema_version":1,"packs":[
                {"id":"acme/workbench","version":"1.2.0","path":"packs/workbench.json","sha256":canonic_registry::digest::sha256_hex(&workbench)},
                {"id":"acme/review","version":"0.4.0","path":"packs/review.json","sha256":canonic_registry::digest::sha256_hex(&review)}
            ]})
            .to_string(),
        )
        .unwrap();
    let project_source = fetcher
        .discover(local.path().to_str().unwrap(), now)
        .unwrap()
        .source;
    let mut project = Registrations::default();
    project.add("acme", project_source.clone(), "main").unwrap();
    project.validate("main", Scope::Project).unwrap();

    let builtin = Source::pinned(
        "https://registry.example.com/",
        "main",
        &SigningKey::generate().public_hex(),
        "acme-main",
    )
    .unwrap();
    let effective = Effective::merge(
        ("main", &builtin),
        &global_file.read().unwrap(),
        Some(&project),
    )
    .unwrap();

    // `acme/workbench` names the project alias, so it is resolved in that registry.
    let Selector::Catalog(reference) =
        selector::parse("acme/workbench", |a| effective.get(a).is_some()).unwrap()
    else {
        panic!("catalog reference expected");
    };
    let source = effective.select(reference.alias.as_deref()).unwrap();
    assert_eq!(source, &project_source);
    let mut checkpoints = Checkpoints::default();
    let catalog = fetcher.catalog(source, now).unwrap();
    checkpoints.admit(&catalog).unwrap();
    let id = catalog.lookup(&reference.pack).unwrap();
    assert_eq!(id.to_string(), "acme/workbench");

    let team_catalog = fetcher.catalog(&discovered.source, now).unwrap();
    checkpoints.admit(&team_catalog).unwrap();
    let mut supplied = releases(&fetcher, source, &catalog);
    supplied.extend(releases(&fetcher, &discovered.source, &team_catalog));
    let selected = resolution::resolve(
        &[Root {
            instance: Instance {
                registry: source.identity().to_owned(),
                pack: id.clone(),
            },
            requirement: semver::VersionReq::STAR,
            pin: None,
            level: (),
        }],
        &supplied,
        &effective.identities(),
    )
    .unwrap();
    let chosen: BTreeSet<_> = selected
        .iter()
        .map(|s| format!("{}@{}", s.release.instance.pack, s.release.version))
        .collect();
    assert_eq!(
        chosen,
        BTreeSet::from([
            "acme/design-tokens@1.3.0".to_owned(),
            "acme/review@0.4.0".to_owned(),
            "acme/workbench@1.2.0".to_owned(),
        ])
    );

    // Retain verified bytes and write the product's lockfile.
    let cache = ArtifactCache::new(home.path().join(".acme/cache"));
    let entry_of = |release: &Release<()>| -> (Source, Entry) {
        let source = if release.instance.registry == team_identity {
            discovered.source.clone()
        } else {
            project_source.clone()
        };
        let catalog = if source == project_source {
            &catalog
        } else {
            &team_catalog
        };
        let entry = catalog
            .release(&release.instance.pack, &release.version)
            .unwrap()
            .clone();
        (source, entry)
    };
    let mut locked: Vec<LockedPack> = selected
        .iter()
        .map(|s| {
            let (source, entry) = entry_of(s.release);
            let bytes = fetcher.artifact(&source, &entry, MAX_ARTIFACT).unwrap();
            cache.put_expected(&entry.sha256, &bytes).unwrap();
            LockedPack {
                selector: InstalledSelector::from_seed(
                    format!("acme-project:{}", s.release.instance.pack).as_bytes(),
                ),
                registry: s.release.instance.registry.clone(),
                id: s.release.instance.pack.clone(),
                version: s.release.version.clone(),
                digest: entry.sha256,
                pinned: false,
                direct: s.direct,
                dependencies: s
                    .release
                    .dependencies
                    .iter()
                    .map(|d| LockedEdge {
                        registry: d
                            .registry
                            .clone()
                            .unwrap_or_else(|| s.release.instance.registry.clone()),
                        pack: d.pack.clone(),
                        requirement: d.requirement.clone(),
                    })
                    .collect(),
            }
        })
        .collect();
    lock::sort(&mut locked);
    lock::validate(&locked).unwrap();
    // Source identities differ per run, so the reviewed view is ordered by pack ID.
    let mut shown = locked.clone();
    shown.sort_by(|a, b| a.id.cmp(&b.id));
    let document = serde_json::to_string_pretty(&json!({"schema_version":1,"packs":shown}))
        .unwrap()
        // The workbench artifact names the team identity, so its digest varies too.
        .replace(
            &canonic_registry::digest::sha256_hex(&workbench),
            "[workbench digest]",
        )
        .replace(&team_identity, "[team identity]")
        .replace(project_source.identity(), "[project identity]");
    insta::assert_snapshot!("lockfile", document);

    // Offline: both registries are gone, and the lockfile alone reproduces the bytes.
    drop(team);
    drop(local);
    for pack in &locked {
        let bytes = cache.get(&pack.digest, MAX_ARTIFACT).unwrap().unwrap();
        assert_eq!(canonic_registry::digest::sha256_hex(&bytes), pack.digest);
    }
    assert!(fetcher.catalog(&discovered.source, now).is_err());
    let review: PackId = "acme/review".parse().unwrap();
    assert_eq!(
        lock::incoming(&locked, project_source.identity(), &review)[0]
            .0
            .id
            .to_string(),
        "acme/workbench"
    );
}

#[test]
fn an_unregistered_foreign_source_refuses_resolution() {
    let fetcher = crate::signed_fetcher();
    let local = TempDir::new().unwrap();
    let pack = artifact(
        &json!([{"pack":"acme/tokens","requirement":"*","registry":"unregistered-source"}]),
    );
    local.child("pack.json").write_binary(&pack).unwrap();
    local
        .child("catalog.json")
        .write_str(
            &json!({"schema_version":1,"packs":[{"id":"acme/workbench","version":"1.0.0","path":"pack.json","sha256":canonic_registry::digest::sha256_hex(&pack)}]})
                .to_string(),
        )
        .unwrap();
    let discovery = fetcher
        .discover(local.path().to_str().unwrap(), SystemTime::now())
        .unwrap();
    let supplied = releases(&fetcher, &discovery.source, &discovery.catalog);
    let root = Root {
        instance: Instance {
            registry: discovery.source.identity().to_owned(),
            pack: "acme/workbench".parse().unwrap(),
        },
        requirement: semver::VersionReq::STAR,
        pin: None,
        level: (),
    };
    assert_eq!(
        resolution::resolve(
            &[root],
            &supplied,
            &BTreeSet::from([discovery.source.identity().to_owned()])
        )
        .unwrap_err(),
        resolution::Error::Unenrolled("unregistered-source".into())
    );
}
