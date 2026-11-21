//! Source admission for updates of retained selections and foreign dependencies.
use super::*;
use canonic_registry_kit::{
    registrations::Registrations,
    testing::{Response, Server, SigningKey},
};
use std::time::Duration;

fn historical_installed(context: &Context) -> (State, Vec<Value>, Vec<String>) {
    let mut before =
        crate::review::storage::Store::open(std::path::Path::new(&context.project_root))
            .unwrap()
            .read_checkpoint()
            .unwrap();
    let mut rows = Vec::new();
    let mut selections = Vec::new();
    let mut artifacts = Vec::new();
    let mut locks = Vec::new();
    for name in ["keep", "remove"] {
        // The actual old matcher/profile schema, including the obsolete engine
        // range, must remain usable only as exact retained source.
        let bytes = json!({"schema_version":1,"id":format!("acme/{name}"),"version":"0.1.0",
            "description":"Historical maintenance fixture","license":"MIT","maintainers":["Acme"],
            "engine":"^0.1.0","capabilities":["argv-v1"],"rules":[{
                "id":"inspect","matcher":{"executable":"git","arguments":{"kind":"exact","values":["status"]}},
                "reason":"Inspect","profiles":{"loose":"allow","recommended":"ask","strict":"deny"}}]}).to_string();
        assert!(
            Pack::from_yaml(&bytes).is_err(),
            "fresh acquisition stays strict"
        );
        let digest = policy::fingerprint(bytes.as_bytes());
        let path = format!("{name}.yml");
        selections.push(json!({"id":format!("acme/{name}"),"registry_identity":"main","version":"0.1.0","profile":"recommended","path":path}));
        locks.push(json!({"id":format!("main/acme/{name}"),"version":"0.1.0","profile":"recommended","artifact":{"path":path,"digest":digest}}));
        rows.push(
            json!({"selector":format!("pack:{name}"),"id":format!("acme/{name}"),
            "version":"0.1.0","profile":"recommended","source_identity":"main",
            "source_alias":"project-files","digest":digest,"direct":true,"pinned":true}),
        );
        artifacts.push(bytes);
    }
    let config = json!({"schema_version":1,"packs":selections}).to_string();
    before.candidates.insert("project".into(), json!({"schema_version":1,"kind":"candidate-bundle",
        "trust":"unverified","config":config,"packs":artifacts,"rules":null,
        "lock":{"schema_version":1,"kind":"local-candidate","config_digest":policy::fingerprint(config.as_bytes()),"packs":locks,"rules":null}}));
    before.operator["installed"] = json!(rows);
    (before, rows, artifacts)
}

#[test]
fn historical_installed_packs_support_removal_and_offline_profile_changes() {
    let (_root, context) = crate::review::tests::enrolled();
    let (mut before, rows, artifacts) = historical_installed(&context);
    let (candidate, meta) = mutations::remove(&before, "acme/remove").unwrap();
    assert_eq!(candidate["packs"], json!([artifacts[0]]));
    assert_eq!(meta["installed"].as_array().unwrap().len(), 1);
    assert_eq!(meta["installed"][0]["selector"], "pack:keep");
    assert_eq!(meta["installed"][0]["digest"], rows[0]["digest"]);
    assert_eq!(meta["installed"][0]["dependencies"], json!([]));
    before.candidates.insert("project".into(), candidate);
    before.operator = meta.clone();
    let mut strict = meta;
    strict["installed"][0]["profile"] = json!("strict");
    let (candidate, meta) = prepare(
        &context,
        &before,
        strict,
        &[artifacts[0].clone()],
        0,
        false,
        None,
    )
    .unwrap();
    assert_eq!(candidate["packs"], json!([artifacts[0]]));
    assert_eq!(meta["installed"][0]["selector"], "pack:keep");
    assert_eq!(meta["installed"][0]["profile"], "strict");
    let bundle: crate::configuration::Bundle = serde_json::from_value(candidate).unwrap();
    let evaluator = crate::configuration::Evaluator::new(vec![(
        policy::Scope::Project,
        bundle.snapshot().unwrap(),
    )])
    .unwrap();
    let request = policy::Request {
        executable: "git".into(),
        arguments: vec!["status".into()],
        cwd: context.project_root.clone(),
        project: context.project_root.clone(),
    };
    assert_eq!(
        evaluator.evaluate(&request).unwrap().decision,
        policy::Decision::Deny
    );
    let mut changed = rows[0].clone();
    changed["digest"] = json!("0".repeat(64));
    assert!(
        release(&changed, &artifacts[0]).is_err(),
        "retained source still requires its exact digest"
    );
}

#[test]
fn local_profile_replacement_and_removal_preserve_the_shared_configuration_and_lock() {
    let (_root, context) = crate::review::tests::enrolled();
    let (mut before, _, _) = historical_installed(&context);
    // Normalize the historical fixture into a complete, captured bundle.
    let rows = before.operator["installed"].as_array_mut().unwrap();
    let bytes: Vec<String> =
        serde_json::from_value(before.candidates["project"]["packs"].clone()).unwrap();
    let source = super::super::rebuild(&before.candidates["project"], rows, &bytes, None).unwrap();
    before.candidates.insert("project".into(), source.clone());
    let shared: Value = policy::parse(source["config"].as_str().unwrap()).unwrap();
    let (candidate, metadata) = super::super::prepare_add(
        &context,
        &before,
        "acme/keep",
        Some(Profile::Strict),
        true,
        |_, _| panic!("an installed selection already has a profile"),
    )
    .unwrap();
    assert_eq!(
        policy::parse::<Value>(candidate["config"].as_str().unwrap()).unwrap(),
        shared
    );
    assert_eq!(candidate["disabled"], json!([bytes[0]]));
    let local: Value = policy::parse(candidate["local_config"].as_str().unwrap()).unwrap();
    assert_eq!(local["packs"].as_array().unwrap().len(), 1);
    assert_eq!(local["packs"][0]["profile"], "strict");
    let files =
        crate::review::publication::projections::projections(&source, &before.operator).unwrap();
    let changed =
        crate::review::publication::projections::projections(&candidate, &metadata).unwrap();
    assert_eq!(changed["shield.lock"], files["shield.lock"]);
    assert_eq!(changed["config.yml"], files["config.yml"]);
    before.candidates.insert("project".into(), candidate);
    before.operator = metadata;
    let (removed, metadata) = super::super::prepare_remove_local(&before, "acme/keep").unwrap();
    let local: Value = policy::parse(removed["local_config"].as_str().unwrap()).unwrap();
    assert_eq!(local["packs"][0]["disabled"], true);
    assert!(local["packs"][0]["version"].is_null());
    let changed =
        crate::review::publication::projections::projections(&removed, &metadata).unwrap();
    assert_eq!(changed["shield.lock"], files["shield.lock"]);
    assert_eq!(changed["config.yml"], files["config.yml"]);
}

#[test]
fn a_local_release_replacement_retains_the_exact_shared_artifact() {
    let (_root, context) = crate::review::tests::enrolled();
    let (before, _, bytes) = historical_installed(&context);
    let replacement = json!({"schema_version":1,"kind":"pack","id":"acme/keep","version":"2.0.0","description":"Acme replacement","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1"],"rules":[]}).to_string();
    let mut artifacts = bytes.clone();
    artifacts[0] = replacement.clone();
    let mut metadata = before.operator.clone();
    metadata["installed"][0]["version"] = json!("2.0.0");
    metadata["installed"][0]["digest"] = json!(policy::fingerprint(replacement.as_bytes()));
    let candidate = super::super::rebuild(
        &before.candidates["project"],
        metadata["installed"].as_array_mut().unwrap(),
        &artifacts,
        None,
    )
    .unwrap();
    let (candidate, metadata) =
        super::super::add_layer(&before, candidate, metadata, true).unwrap();
    assert_eq!(
        policy::parse::<Value>(candidate["config"].as_str().unwrap()).unwrap(),
        policy::parse::<Value>(before.candidates["project"]["config"].as_str().unwrap()).unwrap()
    );
    assert_eq!(candidate["disabled"], json!([bytes[0]]));
    let merged = crate::configuration::layers::merged(&candidate).unwrap();
    let index = merged["packs"]
        .as_array()
        .unwrap()
        .iter()
        .position(|selection| selection["id"] == "acme/keep")
        .unwrap();
    assert_eq!(merged["packs"][index]["version"], "2.0.0");
    assert_eq!(candidate["packs"][index], replacement);
    assert_eq!(metadata["disabled"][0]["version"], "0.1.0");
    serde_json::from_value::<crate::configuration::Bundle>(candidate)
        .unwrap()
        .snapshot()
        .unwrap();
}

#[test]
fn legacy_disabled_metadata_is_recovered_without_changing_the_shared_lock() {
    let (_root, context) = crate::review::tests::enrolled();
    let (mut before, _, artifacts) = historical_installed(&context);
    let (candidate, metadata) = super::super::prepare_remove_local(&before, "acme/remove").unwrap();
    before.candidates.insert("project".into(), candidate);
    before.operator = metadata;
    let mut known = before.operator.clone();
    known["disabled"][0]["source"] = json!({"identity":"main","key":"retained-signing-key","location":"https://registry.example.com"});
    let original = known["disabled"][0].clone();
    super::super::repair_disabled_metadata(&before.candidates["project"], &mut known).unwrap();
    assert_eq!(
        known["disabled"][0], original,
        "existing provenance is retained"
    );
    before.operator.as_object_mut().unwrap().remove("disabled");
    before
        .operator
        .as_object_mut()
        .unwrap()
        .remove("shared_installed");
    let old = crate::review::publication::projections::projections(
        &before.candidates["project"],
        &before.operator,
    )
    .unwrap();
    let (candidate, metadata) = super::super::prepare_add(
        &context,
        &before,
        "acme/keep",
        Some(Profile::Strict),
        true,
        |_, _| panic!("existing profile"),
    )
    .unwrap();
    let recovered = metadata["disabled"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "acme/remove")
        .unwrap();
    assert_eq!(recovered["id"], "acme/remove");
    assert_eq!(
        recovered["digest"],
        policy::fingerprint(artifacts[1].as_bytes())
    );
    assert_eq!(recovered["source_alias"], "project-files");
    assert_eq!(recovered["source_identity"], "main");
    assert!(recovered["source"]["key"].is_null());
    assert_eq!(recovered["trust"], "unverified");
    assert_eq!(recovered["recovered_project_files"], true);
    let files =
        crate::review::publication::projections::projections(&candidate, &metadata).unwrap();
    assert_eq!(files["config.yml"], old["config.yml"]);
    assert_eq!(files["shield.lock"], old["shield.lock"]);
    before.candidates.insert("project".into(), candidate);
    before.operator = metadata;
    let (candidate, metadata) = super::super::prepare_remove_local(&before, "acme/keep").unwrap();
    let files =
        crate::review::publication::projections::projections(&candidate, &metadata).unwrap();
    assert_eq!(files["shield.lock"], old["shield.lock"]);
    assert_eq!(files["config.yml"], old["config.yml"]);

    let mut broken = before.candidates["project"].clone();
    let config = crate::configuration::layers::merged(&broken).unwrap();
    let index = config["disabled"]
        .as_array()
        .unwrap()
        .iter()
        .position(|row| row["id"] == "acme/remove")
        .unwrap();
    broken["disabled"][index] = json!(artifacts[0]);
    let mut missing = before.operator.clone();
    missing.as_object_mut().unwrap().remove("disabled");
    missing.as_object_mut().unwrap().remove("shared_installed");
    missing["installed"]
        .as_array_mut()
        .unwrap()
        .retain(|row| row["id"] != "acme/keep");
    assert!(super::super::repair_disabled_metadata(&broken, &mut missing).is_err());
    broken["disabled"] = json!([]);
    assert!(super::super::repair_disabled_metadata(&broken, &mut missing).is_err());
}

#[test]
fn local_removal_disables_shared_selections_and_removes_local_only_selections() {
    let (_root, context) = crate::review::tests::enrolled();
    let (before, _, bytes) = historical_installed(&context);
    let (candidate, metadata) = super::super::prepare_remove_local(&before, "acme/remove").unwrap();
    let shared: Value = policy::parse(candidate["config"].as_str().unwrap()).unwrap();
    assert_eq!(shared["packs"].as_array().unwrap().len(), 2);
    assert_eq!(candidate["disabled"], json!([bytes[1]]));
    assert_eq!(metadata["installed"].as_array().unwrap().len(), 1);
    let (mut local_before, _, _) = historical_installed(&context);
    local_before.operator["installed"][1]["layer"] = json!("local");
    let rows = local_before.operator["installed"].as_array_mut().unwrap();
    let mut original = before.candidates["project"].clone();
    let mut shared: Value = policy::parse(original["config"].as_str().unwrap()).unwrap();
    shared["packs"].as_array_mut().unwrap().remove(1);
    original["config"] = json!(shared.to_string());
    let candidate = super::super::rebuild(&original, rows, &bytes, None).unwrap();
    local_before.candidates.insert("project".into(), candidate);
    let (candidate, _) = super::super::prepare_remove_local(&local_before, "acme/remove").unwrap();
    assert!(candidate.get("local_config").is_none());
    assert!(candidate.get("disabled").is_none());
}

#[test]
fn local_dependency_graph_changes_preserve_the_complete_shared_lock() {
    let (_root, context) = crate::review::tests::enrolled();
    let (mut before, _, _) = historical_installed(&context);
    let artifact = |name: &str, parent: bool| {
        let mut pack = json!({"schema_version":1,"kind":"pack","id":format!("acme/{name}"),"version":"1.0.0","description":"Acme graph fixture","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1"],"rules":[]});
        if parent {
            pack["capabilities"] = json!(["structured-rules-v1", "dependencies-v1"]);
            pack["dependencies"] = json!([{"pack":"acme/child","requirement":"^1"}]);
        }
        pack.to_string()
    };
    let bytes = vec![artifact("root", true), artifact("child", false)];
    let config = json!({"schema_version":1,"packs":[
        {"id":"acme/root","registry_identity":"main","version":"1.0.0","profile":"recommended","path":"root.yml"},
        {"id":"acme/child","registry_identity":"main","version":"1.0.0","profile":"recommended","path":"child.yml"}
    ]});
    let source =
        json!(crate::configuration::Bundle::capture(&config.to_string(), &bytes, None).unwrap());
    before.candidates.insert("project".into(), source.clone());
    before.operator["installed"] = json!(["root", "child"].iter().enumerate().map(|(index, name)| {
        json!({"selector":format!("pack:{name}"),"id":format!("acme/{name}"),"source_identity":"main","source_alias":"main","version":"1.0.0","profile":"recommended","digest":policy::fingerprint(bytes[index].as_bytes()),"direct":index == 0,"pinned":true})
    }).collect::<Vec<_>>());
    super::super::refresh_shared_metadata(&source, &mut before.operator).unwrap();
    let shared_lock = crate::review::publication::projections::projections(
        &source,
        &before.operator,
    )
    .unwrap()["shield.lock"]
        .clone();
    let mine = artifact("mine", true);
    let mut added_bytes = bytes.clone();
    added_bytes.push(mine.clone());
    let mut added_meta = before.operator.clone();
    added_meta["installed"].as_array_mut().unwrap().push(json!({"selector":"pack:mine","id":"acme/mine","source_identity":"main","source_alias":"main","version":"1.0.0","profile":"recommended","digest":policy::fingerprint(mine.as_bytes()),"direct":true,"pinned":true}));
    added_meta["installed"][1]["incoming"] = json!([
        {"parent":"acme/root","registry_identity":"main","requirement":"^1"},
        {"parent":"acme/mine","registry_identity":"main","requirement":"^1"}
    ]);
    let candidate = super::super::rebuild(
        &source,
        added_meta["installed"].as_array_mut().unwrap(),
        &added_bytes,
        None,
    )
    .unwrap();
    let (candidate, metadata) =
        super::super::add_layer(&before, candidate, added_meta, true).unwrap();
    assert_eq!(
        crate::review::publication::projections::projections(&candidate, &metadata).unwrap()["shield.lock"],
        shared_lock
    );
    before.candidates.insert("project".into(), candidate);
    before.operator = metadata;
    let (candidate, metadata) = super::super::prepare_remove_local(&before, "acme/mine").unwrap();
    assert_eq!(
        crate::review::publication::projections::projections(&candidate, &metadata).unwrap()["shield.lock"],
        shared_lock
    );
    before.candidates.insert("project".into(), candidate);
    before.operator = metadata;
    let (candidate, metadata) = super::super::prepare_add(
        &context,
        &before,
        "acme/root",
        Some(Profile::Strict),
        true,
        |_, _| panic!("retained profile"),
    )
    .unwrap();
    assert_eq!(metadata["disabled"].as_array().unwrap().len(), 2);
    assert_eq!(
        crate::review::publication::projections::projections(&candidate, &metadata).unwrap()["shield.lock"],
        shared_lock
    );
    before.candidates.insert("project".into(), candidate);
    before.operator = metadata;
    let (candidate, metadata) = super::super::prepare_remove_local(&before, "acme/root").unwrap();
    assert_eq!(
        crate::review::publication::projections::projections(&candidate, &metadata).unwrap()["shield.lock"],
        shared_lock
    );
    assert_eq!(
        metadata["installed"].as_array().unwrap().as_slice(),
        [] as [serde_json::Value; 0]
    );
}

#[test]
fn historical_installed_pack_configuration_preserves_sources_and_applies_overrides() {
    let (_root, context) = crate::review::tests::enrolled();
    let (mut before, _, artifacts) = historical_installed(&context);
    // Remove the other matching pack so a disabled rule has an observable effect.
    let (candidate, meta) = mutations::remove(&before, "acme/remove").unwrap();
    before.candidates.insert("project".into(), candidate);
    before.operator = meta;
    let (candidate, meta) =
        super::super::prepare_configure(&before, "acme/keep", |source, settings| {
            assert_eq!(Pack::from_retained_yaml(source)?.id(), "acme/keep");
            settings.edit(false, |binding| {
                binding
                    .overrides
                    .insert("inspect".into(), policy::Override::Off);
                Ok(())
            })
        })
        .unwrap();
    assert_eq!(candidate["packs"], json!([artifacts[0]]));
    assert_eq!(meta["installed"][0]["selector"], "pack:keep");
    let config: Value = policy::parse(candidate["config"].as_str().unwrap()).unwrap();
    assert_eq!(config["packs"][0]["overrides"]["inspect"], "off");
    let bundle: crate::configuration::Bundle = serde_json::from_value(candidate).unwrap();
    let evaluator = crate::configuration::Evaluator::new(vec![(
        policy::Scope::Project,
        bundle.snapshot().unwrap(),
    )])
    .unwrap();
    let evaluation = evaluator
        .evaluate(&policy::Request {
            executable: "git".into(),
            arguments: vec!["status".into()],
            cwd: context.project_root.clone(),
            project: context.project_root.clone(),
        })
        .unwrap();
    assert_eq!(evaluation.decision, policy::Decision::Ask);
    assert!(evaluation.unmatched_ask());
}

#[test]
fn builtin_tracking_root_acquires_and_selects_a_verified_new_release() {
    let (_root, mut context) = crate::review::tests::enrolled();
    let server = Server::new();
    let key = SigningKey::generate();
    let source = Source::pinned(server.url(), "main", &key.public_hex(), "acme")
        .expect("pinned fixture source");
    let registered = Effective::merge(("main", &source), &Registrations::default(), None)
        .expect("built-in fixture");
    context.fetcher = Server::fetcher(Duration::from_secs(2));
    let mut entries = Vec::new();
    // Obsolete releases cannot be selected by this direct-root update and must
    // not require the current engine to understand their retired policy format.
    let obsolete = "obsolete policy format";
    server.set("obsolete.json", Response::ok(obsolete));
    entries.push(json!({"id":"acme/inspect","version":"0.1.0","path":"obsolete.json","sha256":policy::fingerprint(obsolete.as_bytes())}));
    let mut previous = None;
    for version in ["1.0.0", "2.0.0"] {
        let bytes = json!({"schema_version":1,"kind":"pack","id":"acme/inspect","version":version,"description":"Acme update fixture","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1"],"rules":[]}).to_string();
        let digest = policy::fingerprint(bytes.as_bytes());
        let path = format!("inspect-{version}.json");
        server.set(&path, Response::ok(bytes.clone()));
        entries.push(json!({"id":"acme/inspect","version":version,"path":path,"sha256":digest}));
        if previous.is_none() {
            previous = Some((
                json!({"id":"acme/inspect","version":version,"digest":digest,"source_identity":"main","source":source,"direct":true,"profile":"strict","pinned":false}),
                bytes,
            ));
        }
    }
    let catalog = json!({"schema_version":1,"registry_id":"acme","sequence":1,"expires_at":"9999-12-31T00:00:00Z","packs":entries}).to_string();
    server.set(
        "catalog.sig",
        Response::ok(key.sign_hex(catalog.as_bytes())),
    );
    server.set("catalog.json", Response::ok(catalog));
    let (row, bytes) = previous.expect("retained release");
    let instance = identity(&row).expect("identity");
    let old = release(&row, &bytes).expect("release");
    let minimum_versions = BTreeMap::from([(instance.clone(), old.version.clone())]);
    let mut artifacts = BTreeMap::from([(
        (instance.clone(), old.version.clone()),
        (row.clone(), bytes),
    )]);
    let mut releases = vec![old];
    acquire_closure(
        &context,
        Some(&registered),
        &[row],
        &[Root {
            instance: instance.clone(),
            requirement: "*".parse().unwrap(),
            pin: None,
            level: Profile::Strict,
        }],
        Acquisition {
            exact_versions: BTreeMap::new(),
            pending: BTreeSet::from([instance.clone()]),
            locked: BTreeSet::new(),
            offline: false,
            catalogs: BTreeMap::new(),
            minimum_versions,
        },
        &mut releases,
        &mut artifacts,
    )
    .expect("verified update acquisition");
    let roots = [Root {
        instance,
        requirement: "*".parse().expect("requirement"),
        pin: None,
        level: Profile::Strict,
    }];
    let selected = resolution::resolve(&roots, &releases, &BTreeSet::from(["main".into()]))
        .expect("tracking update");
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].release.version.to_string(), "2.0.0");
    assert_eq!(selected[0].level, Profile::Strict);
    assert!(selected[0].direct);
    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|p| p.ends_with("catalog.json"))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|p| p.ends_with("catalog.sig"))
            .count(),
        1
    );
    assert!(requests.iter().all(|p| !p.ends_with("catalog-key.pub")));
    assert!(requests.iter().all(|p| !p.ends_with("obsolete.json")));
}

#[test]
fn recovered_direct_main_updates_select_the_verified_builtin_source() {
    let (_root, context) = crate::review::tests::enrolled();
    let registered = registries::effective(Some(&context)).unwrap();
    let row = json!({"id":"acme/inspect","source_identity":"main",
        "source_alias":"project-files","source":{"identity":"main","location":".shield","key":null},
        "pinned":true,"direct":true});
    let instance = identity(&row).unwrap();
    let (_, source) = enrolled_source(&registered, std::slice::from_ref(&row), &instance).unwrap();
    assert_eq!(source, registries::main_source().unwrap());
    for (field, value) in [
        ("key", json!("0".repeat(64))),
        ("location", json!("elsewhere")),
    ] {
        let mut changed = row.clone();
        changed["source"][field] = value;
        assert!(enrolled_source(&registered, &[changed], &instance).is_err());
    }
}

#[test]
fn direct_main_updates_use_the_binary_pinned_source_without_enrollment() {
    let (_root, context) = crate::review::tests::enrolled();
    let registered = registries::effective(Some(&context)).expect("registries");
    let source = registries::main_source().expect("pinned source");
    for direct in [json!(true), Value::Null] {
        let row =
            json!({"id":"acme/inspect","source_identity":"main","source":source,"direct":direct});
        let instance = identity(&row).expect("retained identity");
        let (alias, selected) =
            enrolled_source(&registered, &[row], &instance).expect("update source");
        assert_eq!(alias, "main");
        assert_eq!(selected, source);
    }
}

#[test]
fn main_dependency_lookup_does_not_inherit_an_unrelated_direct_selection() {
    let (_root, context) = crate::review::tests::enrolled();
    let registered = registries::effective(Some(&context)).expect("registries");
    let source = registries::main_source().expect("pinned source");
    let rows = [
        json!({"id":"acme/inspect","source_identity":"main","source":source,"direct":true}),
        json!({"id":"acme/child","source_identity":"main","source":source,"direct":false}),
    ];
    for pack in ["acme/child", "acme/missing"] {
        let instance = Instance {
            registry: "main".into(),
            pack: pack.parse().expect("pack"),
        };
        assert!(matches!(
            enrolled_source(&registered, &rows, &instance),
            Err(Error::Invalid(
                "dependency registry identity is not enrolled"
            ))
        ));
    }
}

#[test]
fn direct_update_refuses_a_changed_retained_binding_or_unknown_registry() {
    let (_root, context) = crate::review::tests::enrolled();
    let registered = registries::effective(Some(&context)).expect("registries");
    let mut row = json!({"id":"acme/inspect","source_identity":"main","source":registries::main_source().expect("source"),"direct":true});
    row["source"]["key"] = json!("0".repeat(64));
    let instance = identity(&row).expect("identity");
    assert!(matches!(
        enrolled_source(&registered, &[row.clone()], &instance),
        Err(Error::Invalid("retained registry identity binding changed"))
    ));
    row["source_identity"] = json!("not-enrolled");
    let instance = identity(&row).expect("identity");
    assert!(matches!(
        enrolled_source(&registered, &[row], &instance),
        Err(Error::Invalid(
            "dependency registry identity is not enrolled"
        ))
    ));
}
