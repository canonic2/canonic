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
fn update_reports_pinned_roots_it_skips() {
    let row = |selector: &str, id: &str, pinned: bool, direct: bool| json!({"selector":selector,"id":id,"version":"1.0.0","source_identity":"main","source_alias":"main","pinned":pinned,"direct":direct});
    let rows = [
        row("pack:1", "acme/fixed", true, true),
        row("pack:2", "acme/tracking", false, true),
        row("pack:3", "acme/child", true, false),
    ];
    let ids = |skipped: Vec<Value>| -> Vec<Value> {
        skipped.into_iter().map(|row| row["id"].clone()).collect()
    };
    let all = mutations::skipped_pinned(&rows, &[], None).expect("update all");
    assert_eq!(ids(all), [json!("acme/fixed")]);
    let named = mutations::skipped_pinned(&rows, &[], Some("acme/fixed")).expect("named pinned");
    assert_eq!(named[0]["selector"], "pack:1");
    // A current alias bound to the row's source identity also names it.
    let renamed = [("renamed".to_owned(), "main".to_owned())];
    let aliased = mutations::skipped_pinned(&rows, &renamed, Some("renamed/acme/fixed"))
        .expect("current alias");
    assert_eq!(aliased[0]["selector"], "pack:1");
    assert_eq!(
        mutations::skipped_pinned(&rows, &[], Some("acme/tracking")).expect("named tracking"),
        Vec::<Value>::new()
    );
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
