//! Alias rules, scope precedence and locked atomic global writes.
use super::*;
use crate::test_support::{builtin, local};
use assert_fs::{TempDir, prelude::*};
use std::fs;

#[test]
fn aliases_are_validated_reserved_and_unique() {
    let mut registrations = Registrations::default();
    registrations
        .add("acme", local("/srv/acme"), "main")
        .unwrap();
    assert!(matches!(
        registrations.add("acme", local("/srv/other"), "main"),
        Err(RegistrationError::Duplicate)
    ));
    assert!(matches!(
        registrations.add("main", local("/srv/other"), "main"),
        Err(RegistrationError::Reserved(_))
    ));
    assert!(matches!(
        registrations.add("Acme", local("/srv/other"), "main"),
        Err(RegistrationError::InvalidAlias)
    ));
    assert!(matches!(
        registrations.remove("main", "main"),
        Err(RegistrationError::Reserved(_))
    ));
    assert!(matches!(
        registrations.remove("missing", "main"),
        Err(RegistrationError::Missing(_))
    ));
    for i in 1..MAX_REGISTRATIONS {
        registrations
            .add(&format!("source-{i}"), local(&format!("/srv/{i}")), "main")
            .unwrap();
    }
    assert!(matches!(
        registrations.add("one-more", local("/srv/x"), "main"),
        Err(RegistrationError::Limit)
    ));
}

#[test]
fn registrations_cannot_claim_another_sources_identity() {
    // A pinned source carries a chosen identity, such as the built-in `main`.
    let impostor = Source::pinned(
        "https://attacker.example.com/",
        "main",
        crate::test_support::EXAMPLE_KEY,
        "acme-main",
    )
    .unwrap();
    let mut registrations = Registrations::default();
    assert!(matches!(
        registrations.add("a-team", impostor.clone(), "main"),
        Err(RegistrationError::Identity(_))
    ));
    // A stored file that bypassed `add` is refused when read.
    registrations.sources.insert("a-team".into(), impostor);
    assert!(matches!(
        registrations.validate("main", Scope::Global),
        Err(RegistrationError::Identity(_))
    ));
    let home = TempDir::new().unwrap();
    home.child("registries.json")
        .write_str(&serde_json::to_string(&registrations).unwrap())
        .unwrap();
    assert!(matches!(
        GlobalFile::new(home.path().join("registries.json"), "main").read(),
        Err(RegistrationError::Identity(_))
    ));
    // A local source whose identity names another path is refused as well.
    let mut moved = serde_json::to_value(local("/srv/acme")).unwrap();
    moved["location"] = serde_json::json!("/srv/other");
    let moved: Source = serde_json::from_value(moved).unwrap();
    assert!(!moved.has_derived_identity());
    assert!(matches!(
        Registrations::default().add("acme", moved, "main"),
        Err(RegistrationError::Identity(_))
    ));
}

#[test]
fn removing_the_default_inherits_the_next_scope() {
    let mut global = Registrations::default();
    global.add("team", local("/srv/team"), "main").unwrap();
    global.default = Some("team".into());
    let mut project = Registrations::default();
    project.add("acme", local("/srv/acme"), "main").unwrap();
    project.default = Some("acme".into());
    let effective = Effective::merge(("main", &builtin()), &global, Some(&project)).unwrap();
    assert_eq!(effective.default_alias(), "acme");
    project.remove("acme", "main").unwrap();
    assert_eq!(project.default, None);
    let effective = Effective::merge(("main", &builtin()), &global, Some(&project)).unwrap();
    assert_eq!(effective.default_alias(), "team");
    global.remove("team", "main").unwrap();
    let effective = Effective::merge(("main", &builtin()), &global, None).unwrap();
    assert_eq!(effective.default_alias(), "main");
    assert_eq!(effective.select(None).unwrap(), &builtin());
}

#[test]
fn project_shadows_global_but_never_the_builtin() {
    let mut global = Registrations::default();
    global.add("acme", local("/srv/global"), "main").unwrap();
    global.add("team", local("/srv/team"), "main").unwrap();
    let mut project = Registrations::default();
    project.add("acme", local("/srv/project"), "main").unwrap();
    // A malformed project scope that bypassed validation is refused, so it
    // can neither shadow main nor claim its identity.
    let mut shadowing = project.clone();
    shadowing.sources.insert("main".into(), local("/srv/evil"));
    assert!(matches!(
        Effective::merge(("main", &builtin()), &global, Some(&shadowing)),
        Err(RegistrationError::Reserved(_))
    ));
    let mut impostor = project.clone();
    impostor.sources.insert(
        "evil".into(),
        Source::pinned(
            "https://attacker.example.com/",
            "main",
            crate::test_support::EXAMPLE_KEY,
            "acme-main",
        )
        .unwrap(),
    );
    assert!(matches!(
        Effective::merge(("main", &builtin()), &global, Some(&impostor)),
        Err(RegistrationError::Identity(_))
    ));
    let effective = Effective::merge(("main", &builtin()), &global, Some(&project)).unwrap();
    assert_eq!(effective.get("acme").unwrap().location(), "/srv/project");
    assert_eq!(effective.scope("acme"), Some(Scope::Project));
    assert_eq!(effective.scope("team"), Some(Scope::Global));
    assert_eq!(effective.get("main").unwrap(), &builtin());
    assert_eq!(effective.scope("main"), Some(Scope::Builtin));
    assert!(matches!(
        effective.select(Some("missing")),
        Err(RegistrationError::Missing(_))
    ));
    assert_eq!(
        effective
            .iter()
            .map(|(alias, _, _)| alias)
            .collect::<Vec<_>>(),
        ["acme", "main", "team"]
    );
    assert!(effective.identities().contains("main"));
    assert_eq!(effective.identities().len(), 3);
}

#[test]
fn defaults_must_name_an_available_registration() {
    let mut global = Registrations::default();
    global.add("team", local("/srv/team"), "main").unwrap();
    let available = Effective::merge(("main", &builtin()), &global, None).unwrap();
    let mut updated = global.clone();
    updated.set_default("team", &available).unwrap();
    updated.set_default("main", &available).unwrap();
    assert!(matches!(
        updated.set_default("missing", &available),
        Err(RegistrationError::Missing(_))
    ));
    assert!(matches!(
        updated.set_default("Team", &available),
        Err(RegistrationError::InvalidAlias)
    ));
    assert_eq!(updated.default.as_deref(), Some("main"));
    updated.validate("main", Scope::Global).unwrap();
}

#[test]
fn registrations_are_found_by_source_identity() {
    let shared = local("/srv/shared");
    let mut global = Registrations::default();
    global.add("alpha", shared.clone(), "main").unwrap();
    let mut project = Registrations::default();
    project.add("zeta", shared.clone(), "main").unwrap();
    let effective = Effective::merge(("main", &builtin()), &global, Some(&project)).unwrap();
    // Alias order decides, whatever the scope.
    let (alias, source, scope) = effective.by_identity(shared.identity()).unwrap();
    assert_eq!((alias, scope), ("alpha", Scope::Global));
    assert_eq!(source, &shared);
    assert_eq!(
        effective
            .by_identity("main")
            .map(|(alias, _, scope)| (alias, scope)),
        Some(("main", Scope::Builtin))
    );
    assert!(effective.by_identity("unknown").is_none());
}

#[test]
fn project_defaults_may_name_global_aliases_but_global_defaults_may_not_dangle() {
    let registrations = Registrations {
        sources: BTreeMap::new(),
        default: Some("team".into()),
    };
    registrations.validate("main", Scope::Project).unwrap();
    assert!(matches!(
        registrations.validate("main", Scope::Global),
        Err(RegistrationError::InvalidDefault)
    ));
    let main = Registrations {
        sources: BTreeMap::new(),
        default: Some("main".into()),
    };
    main.validate("main", Scope::Global).unwrap();
}

#[test]
fn global_file_round_trips_atomically_and_refuses_stale_reviews() {
    let home = TempDir::new().unwrap();
    let file = GlobalFile::new(home.path().join(".acme/registries.json"), "main");
    let empty = file.read().unwrap();
    assert_eq!(empty, Registrations::default());
    let mut after = empty.clone();
    after.add("team", local("/srv/team"), "main").unwrap();
    after.default = Some("team".into());
    file.replace(&empty, &after).unwrap();
    assert_eq!(file.read().unwrap(), after);
    assert!(!home.child(".acme/registries.lock").exists());
    assert!(!home.child(".acme/registries.new").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(file.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    assert!(matches!(
        file.replace(&empty, &Registrations::default()),
        Err(RegistrationError::Changed)
    ));
    assert_eq!(file.read().unwrap(), after);
}

#[test]
fn a_held_lock_refuses_and_a_stale_temporary_file_is_replaced() {
    let home = TempDir::new().unwrap();
    let file = GlobalFile::new(home.path().join("registries.json"), "main");
    home.child("registries.lock").touch().unwrap();
    assert!(matches!(
        file.replace(&Registrations::default(), &Registrations::default()),
        Err(RegistrationError::Locked(_))
    ));
    fs::remove_file(home.path().join("registries.lock")).unwrap();
    home.child("registries.new").write_str("partial").unwrap();
    file.replace(&Registrations::default(), &Registrations::default())
        .unwrap();
    assert_eq!(file.read().unwrap(), Registrations::default());
}

#[test]
fn valid_registrations_that_would_serialize_beyond_the_bound_are_not_written() {
    // Control characters are valid in registry IDs but escape to six bytes in JSON.
    let registry_id = "\u{1}".repeat(crate::names::MAX_TEXT);
    let mut after = Registrations::default();
    for i in 0..MAX_REGISTRATIONS {
        let source = Source::signed(
            &format!("https://registry.example.com/{i}/"),
            crate::test_support::EXAMPLE_KEY,
            &registry_id,
        )
        .unwrap();
        after.add(&format!("team-{i}"), source, "main").unwrap();
    }
    after.validate("main", Scope::Global).unwrap();
    let home = TempDir::new().unwrap();
    let file = GlobalFile::new(home.path().join("registries.json"), "main");
    let mut small = Registrations::default();
    small.add("acme", local("/srv/acme"), "main").unwrap();
    file.replace(&Registrations::default(), &small).unwrap();
    assert!(matches!(
        file.replace(&small, &after),
        Err(RegistrationError::TooLarge)
    ));
    // The previous file is untouched, still readable, and the lock is released.
    assert_eq!(file.read().unwrap(), small);
    assert!(!home.child("registries.lock").exists());
    assert!(!home.child("registries.new").exists());
}

#[cfg(unix)]
#[test]
fn a_committed_write_that_cannot_be_synced_reports_it_without_rolling_back() {
    use std::os::unix::fs::PermissionsExt;
    let home = TempDir::new().unwrap();
    let dir = home.path().join("config");
    fs::create_dir(&dir).unwrap();
    let file = GlobalFile::new(dir.join("registries.json"), "main");
    let mut after = Registrations::default();
    after.add("acme", local("/srv/acme"), "main").unwrap();
    // Writable and searchable, but not readable: the directory cannot be opened to sync.
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o300)).unwrap();
    if fs::File::open(&dir).is_ok() {
        // Privileges that bypass permissions make the failure impossible to provoke.
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    let result = file.replace(&Registrations::default(), &after);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        matches!(result, Err(RegistrationError::NotDurable(_))),
        "{result:?}"
    );
    // The new value is in place and the lock was released.
    assert_eq!(file.read().unwrap(), after);
    assert!(!dir.join("registries.lock").exists());
}

#[test]
fn paths_that_collide_with_their_lock_or_temporary_file_refuse() {
    let home = TempDir::new().unwrap();
    // Case-insensitive filesystems treat these as the lock or temporary file itself.
    for name in [
        "registries.lock",
        "registries.new",
        "registries.LOCK",
        "registries.New",
        "registries.loc\u{212A}",
    ] {
        let existing = home.path().join(name);
        fs::write(&existing, "{\"sources\":{}}\n").unwrap();
        let file = GlobalFile::new(&existing, "main");
        assert!(matches!(
            file.replace(&Registrations::default(), &Registrations::default()),
            Err(RegistrationError::Path)
        ));
        // Refused before anything on disk changes: the file is never removed.
        assert_eq!(fs::read_to_string(&existing).unwrap(), "{\"sources\":{}}\n");
        fs::remove_file(&existing).unwrap();
    }
}

#[test]
fn invalid_or_linked_files_refuse() {
    let home = TempDir::new().unwrap();
    let file = GlobalFile::new(home.path().join("registries.json"), "main");
    for content in [
        "not json",
        r#"{"sources":{},"default":"missing"}"#,
        r#"{"sources":{},"extra":true}"#,
    ] {
        home.child("registries.json").write_str(content).unwrap();
        assert!(file.read().is_err(), "{content}");
    }
    home.child("registries.json")
        .write_str(&"x".repeat(1_048_577))
        .unwrap();
    assert!(matches!(file.read(), Err(RegistrationError::Malformed)));
    #[cfg(unix)]
    {
        fs::remove_file(home.path().join("registries.json")).unwrap();
        home.child("target.json").write_str("{}").unwrap();
        std::os::unix::fs::symlink(
            home.path().join("target.json"),
            home.path().join("registries.json"),
        )
        .unwrap();
        assert!(matches!(file.read(), Err(RegistrationError::Link)));
    }
}
