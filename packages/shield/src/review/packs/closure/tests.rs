//! Source admission for updates of retained selections and foreign dependencies.
use super::*;
use canonic_registry::{
    registrations::Registrations,
    testing::{Response, Server, SigningKey},
};
use std::time::Duration;

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
