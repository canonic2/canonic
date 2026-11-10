//! Owned HTTPS fixtures verify review, immutable metadata CAS and final handoff.
use super::*;
use canonic_registry_kit::{
    testing::{Response, Server, SigningKey},
    trust::TrustedRoot,
};
use std::{cell::Cell, time::Duration};
mod native;

struct Fixture {
    project: assert_fs::TempDir,
    context: Context,
    server: Server,
    key: SigningKey,
    source: Source,
    pack: String,
}
fn key_id(key: &SigningKey) -> String {
    crate::policy::fingerprint(
        &canonic_registry_kit::digest::decode(&key.public_hex(), 32).unwrap(),
    )
}
fn signatures(keys: &[&SigningKey], bytes: &[u8]) -> Vec<u8> {
    json!({"schema_version":1,"kind":"registry-signatures","signatures":keys.iter().map(|key|json!({"key_id":key_id(key),"signature":key.sign_hex(bytes)})).collect::<Vec<_>>()} ).to_string().into_bytes()
}
impl Fixture {
    fn new() -> Self {
        let server = Server::new();
        let anchor = SigningKey::generate();
        let key = SigningKey::generate();
        let source = Source::signed(server.url(), &anchor.public_hex(), "acme-trust").unwrap();
        let id = key_id(&key);
        let role = json!({"key_ids":[id],"threshold":1});
        let payload=json!({"schema_version":1,"kind":"registry-root","source_identity":source.identity(),"location":source.location(),"registry_id":source.registry_id(),"version":1,"expires_at":"2100-01-01T00:00:00Z","keys":{id:key.public_hex()},"roles":{"root":role,"catalog":role,"revocations":role}}).to_string().into_bytes();
        let signed = signatures(&[&anchor, &key], &payload);
        TrustedRoot::bootstrap(&source, &payload, &signed).unwrap();
        server.set("trust/roots/1.json", Response::ok(payload));
        server.set("trust/roots/1.sig", Response::ok(signed));
        let pack=json!({"schema_version":1,"kind":"pack","id":"acme/tools","version":"1.0.0","description":"Acme trust fixture","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["structured-rules-v1","argument-rules-v1"],"rules":[{"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["status"]}}}]}).to_string();
        let config=json!({"schema_version":1,"packs":[{"id":"acme/tools","registry_identity":source.identity(),"version":"1.0.0","profile":"recommended","path":"tools.yml"}]}).to_string();
        let bundle =
            crate::configuration::Bundle::capture(&config, std::slice::from_ref(&pack), None)
                .unwrap();
        let project = assert_fs::TempDir::new().unwrap();
        let mut context = super::super::mcp::initialize_fixture(project.path(), &bundle);
        context.fetcher = Server::fetcher(Duration::from_secs(10));
        Store::open(project.path()).unwrap().update_checkpoint(&context,&||Ok(()),|state|{
            state.operator["sources"]["acme"]=json!(source);
            state.operator["installed"]=json!([{"selector":"pack:123e4567-e89b-42d3-a456-426614174000","id":"acme/tools","version":"1.0.0","profile":"recommended","digest":crate::policy::fingerprint(pack.as_bytes()),"source_identity":source.identity(),"source_alias":"acme","source":source,"pinned":true,"direct":true}]);
            Ok(json!({}))
        }).unwrap();
        std::fs::write(project.path().join(".shield/config.yml"), &config).unwrap();
        std::fs::write(project.path().join(".shield/tools.yml"), &pack).unwrap();
        let fixture = Self {
            project,
            context,
            server,
            key,
            source,
            pack,
        };
        fixture.publish(1, false);
        fixture
    }
    fn publish(&self, sequence: u64, revoked: bool) {
        let releases = if revoked {
            json!([{"id":"acme/tools","version":"1.0.0","sha256":crate::policy::fingerprint(self.pack.as_bytes())}])
        } else {
            json!([])
        };
        let revocations=json!({"schema_version":1,"kind":"registry-revocations","source_identity":self.source.identity(),"location":self.source.location(),"registry_id":self.source.registry_id(),"root_version":1,"sequence":sequence,"expires_at":"2100-01-01T00:00:00Z","releases":releases}).to_string().into_bytes();
        self.server
            .set("trust/revocations.json", Response::ok(revocations.clone()));
        self.server.set(
            "trust/revocations.sig",
            Response::ok(signatures(&[&self.key], &revocations)),
        );
        let catalog=json!({"schema_version":1,"registry_id":self.source.registry_id(),"sequence":sequence,"expires_at":"2100-01-01T00:00:00Z","packs":[{"id":"acme/tools","version":"1.0.0","path":"packs/tools.yml","sha256":crate::policy::fingerprint(self.pack.as_bytes())}]}).to_string().into_bytes();
        self.server
            .set("catalog.json", Response::ok(catalog.clone()));
        self.server.set(
            "catalog.sig",
            Response::ok(signatures(&[&self.key], &catalog)),
        );
        self.server.set(
            "packs/tools.yml",
            Response::ok(self.pack.as_bytes().to_vec()),
        );
    }
    fn request(&self) -> crate::policy::Request {
        crate::policy::Request {
            executable: "acme".into(),
            arguments: vec!["status".into()],
            cwd: self.context.project_root.clone(),
            project: self.context.project_root.clone(),
        }
    }
}

#[test]
fn warm_metadata_memo_rejects_replaced_head_counters() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::warm_metadata_memo_rejects_replaced_head_counters"
    )) {
        return;
    }
    let fixture = Fixture::new();
    Prepared::new(&fixture.context, "acme", &|| Ok(()))
        .unwrap()
        .commit(&fixture.context, &|| Ok(()))
        .unwrap();
    let namespace = Namespace::user().unwrap();
    let retained = namespace.read(fixture.source.identity()).unwrap().unwrap();
    let (head_path, _) = trust_record_paths(&fixture.source);
    let original: Value = serde_json::from_slice(&std::fs::read(&head_path).unwrap()).unwrap();
    for field in ["root_version", "revocation_sequence"] {
        let mut changed = original.clone();
        changed[field] = json!(original[field].as_u64().unwrap() + 1);
        super::super::storage::private_files::write(&head_path, &changed, 32 * 1024).unwrap();
        assert!(
            namespace.read(fixture.source.identity()).is_err(),
            "{field}"
        );
    }
    super::super::storage::private_files::write(&head_path, &original, 32 * 1024).unwrap();
    assert_eq!(
        namespace
            .read(fixture.source.identity())
            .unwrap()
            .unwrap()
            .object,
        retained.object
    );
}

fn trust_record_paths(source: &Source) -> (std::path::PathBuf, std::path::PathBuf) {
    let directory = std::path::PathBuf::from(std::env::var_os("SHIELD_ISOLATED_HOME").unwrap())
        .join(".shield/registry-trust");
    let key = crate::policy::fingerprint(source.identity().as_bytes());
    (
        directory.join(format!("{key}.head.json")),
        directory.join(format!("{key}.mode.json")),
    )
}

#[test]
fn interrupted_initial_adoption_recovers_only_exact_approved_target_on_runtime_use() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::interrupted_initial_adoption_recovers_only_exact_approved_target_on_runtime_use"
    )) {
        return;
    }
    let fixture = Fixture::new();
    Prepared::new(&fixture.context, "acme", &|| Ok(()))
        .unwrap()
        .commit(&fixture.context, &|| Ok(()))
        .unwrap();
    let (head_path, mode_path) = trust_record_paths(&fixture.source);
    let head: Value = serde_json::from_slice(&std::fs::read(&head_path).unwrap()).unwrap();
    let mut mode: Value = serde_json::from_slice(&std::fs::read(&mode_path).unwrap()).unwrap();
    mode["approved_initial"] = head.clone();
    // Persist the exact state after the approved mode rename but before head
    // publication. No network response participates in subsequent recovery.
    super::super::storage::private_files::write(&mode_path, &mode, 32 * 1024).unwrap();
    std::fs::remove_file(&head_path).unwrap();
    let namespace = Namespace::user().unwrap();
    assert!(namespace.read(fixture.source.identity()).is_err());
    super::super::project::checkpoint(&fixture.context).unwrap();
    assert!(!head_path.exists());
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&mode_path).unwrap()).unwrap(),
        mode
    );
    let mut corrupt = mode.clone();
    corrupt["approved_initial"]["revocation_sequence"] = json!(2);
    super::super::storage::private_files::write(&mode_path, &corrupt, 32 * 1024).unwrap();
    assert!(namespace.recover(fixture.source.identity()).is_err());
    assert!(!head_path.exists());
    corrupt = mode.clone();
    corrupt["approved_initial"]["intent"] = json!("0".repeat(64));
    super::super::storage::private_files::write(&mode_path, &corrupt, 32 * 1024).unwrap();
    assert!(namespace.recover(fixture.source.identity()).is_err());
    assert!(!head_path.exists());
    super::super::storage::private_files::write(&mode_path, &mode, 32 * 1024).unwrap();
    crate::runtime::Project::open(fixture.project.path())
        .unwrap()
        .snapshot()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&head_path).unwrap()).unwrap(),
        head
    );
    assert!(serde_json::from_slice::<Value>(&std::fs::read(&mode_path).unwrap()).unwrap()["approved_initial"].is_null());
}

#[test]
fn interrupted_initial_clear_precedes_higher_heads_and_never_recovers_over_them() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::interrupted_initial_clear_precedes_higher_heads_and_never_recovers_over_them"
    )) {
        return;
    }
    let fixture = Fixture::new();
    Prepared::new(&fixture.context, "acme", &|| Ok(()))
        .unwrap()
        .commit(&fixture.context, &|| Ok(()))
        .unwrap();
    let (head_path, mode_path) = trust_record_paths(&fixture.source);
    let initial: Value = serde_json::from_slice(&std::fs::read(&head_path).unwrap()).unwrap();
    let mut mode: Value = serde_json::from_slice(&std::fs::read(&mode_path).unwrap()).unwrap();
    mode["approved_initial"] = initial;
    // The head rename succeeded but clearing the initial target was interrupted.
    super::super::storage::private_files::write(&mode_path, &mode, 32 * 1024).unwrap();
    fixture.publish(2, true);
    let blocked_clear = mode_path.with_extension("pending");
    std::fs::create_dir(&blocked_clear).unwrap();
    assert!(catalog(&fixture.context.fetcher, &fixture.source).is_err());
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&head_path).unwrap()).unwrap(),
        mode["approved_initial"]
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&mode_path).unwrap()).unwrap(),
        mode
    );
    std::fs::remove_dir(blocked_clear).unwrap();
    catalog(&fixture.context.fetcher, &fixture.source)
        .unwrap()
        .unwrap();
    let latest = std::fs::read(&head_path).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&latest).unwrap()["revocation_sequence"],
        2
    );
    assert!(serde_json::from_slice::<Value>(&std::fs::read(&mode_path).unwrap()).unwrap()["approved_initial"].is_null());
    // A stale captured intent cannot replace a subsequently selected deny head.
    super::super::storage::private_files::write(&mode_path, &mode, 32 * 1024).unwrap();
    assert!(
        Namespace::user()
            .unwrap()
            .recover(fixture.source.identity())
            .is_err()
    );
    assert_eq!(std::fs::read(&head_path).unwrap(), latest);
}

#[test]
fn versioned_adoption_is_frozen_explicit_and_preserves_policy() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::versioned_adoption_is_frozen_explicit_and_preserves_policy"
    )) {
        return;
    }
    let fixture = Fixture::new();
    let before = super::super::project::checkpoint(&fixture.context).unwrap();
    let plan = Prepared::new(&fixture.context, "acme", &|| Ok(())).unwrap();
    assert!(
        Namespace::user()
            .unwrap()
            .read(fixture.source.identity())
            .unwrap()
            .is_none()
    );
    let shown = plan.review();
    assert_eq!(shown["scope"], "user");
    assert_eq!(shown["root"]["catalog"]["key_ids"][0], key_id(&fixture.key));
    // Acceptance uses retained verified bytes, not a new network response.
    fixture.server.set(
        "trust/roots/1.json",
        Response::ok(b"changed after review".to_vec()),
    );
    let committed = plan.commit(&fixture.context, &|| Ok(())).unwrap();
    assert_eq!(committed["committed"], true);
    let after = super::super::project::checkpoint(&fixture.context).unwrap();
    assert_eq!(after.policy_revision, before.policy_revision);
    assert_eq!(after.operator["installed"], before.operator["installed"]);
    assert_eq!(
        after.operator["registry_trust"][fixture.source.identity()]["mode"],
        "versioned-v1"
    );
    assert_eq!(
        Namespace::user()
            .unwrap()
            .read(fixture.source.identity())
            .unwrap()
            .unwrap()
            .root
            .source(),
        &fixture.source
    );
}

#[test]
fn revoked_metadata_invalidates_retained_handoff_and_stays_offline() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::revoked_metadata_invalidates_retained_handoff_and_stays_offline"
    )) {
        return;
    }
    let fixture = Fixture::new();
    Prepared::new(&fixture.context, "acme", &|| Ok(()))
        .unwrap()
        .commit(&fixture.context, &|| Ok(()))
        .unwrap();
    let project = crate::runtime::Project::open(fixture.project.path()).unwrap();
    let snapshot = project.snapshot().unwrap();
    let evidence = snapshot.evaluate(fixture.request()).unwrap();
    assert_eq!(
        evidence.evaluation().decision,
        crate::policy::Decision::Allow
    );
    fixture.publish(2, true);
    registries::checked_catalog(&fixture.context, &fixture.source).unwrap();
    let called = Cell::new(false);
    assert!(
        project
            .with_current(&evidence, |_, _| called.set(true))
            .is_err()
    );
    assert!(!called.get());
    assert!(project.snapshot().is_err());
    let entry = Entry {
        id: "acme/tools".parse().unwrap(),
        version: "1.0.0".parse().unwrap(),
        path: "packs/tools.yml".into(),
        sha256: crate::policy::fingerprint(fixture.pack.as_bytes()),
    };
    let requests = fixture.server.requests().len();
    assert!(check_entry(&fixture.source, &entry, false).is_err());
    assert_eq!(fixture.server.requests().len(), requests);
    // A fresh process reads the same owned user head, not this process's memo.
    let probe = concat!(module_path!(), "::registry_trust_restart_probe");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", probe.split_once("::").unwrap().1, "--nocapture"])
        .env("SHIELD_REGISTRY_TRUST_PROBE_ROOT", fixture.project.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
    assert_eq!(fixture.server.requests().len(), requests);
    // Config-only inspection and repair preparation can still see the retained policy.
    let state = super::super::project::checkpoint(&fixture.context).unwrap();
    assert_eq!(state.operator["installed"][0]["id"], "acme/tools");
    let mut unrelated = state.operator.clone();
    unrelated["installed"] = json!([]);
    guard(&unrelated).unwrap();
    Store::open(fixture.project.path())
        .unwrap()
        .update_checkpoint(&fixture.context, &|| Ok(()), |state| {
            state.operator["installed"] = json!([]);
            Ok(json!({}))
        })
        .unwrap();
    assert!(
        project.snapshot().is_err(),
        "compiled artifact identity covers old states without installed descriptions"
    );
}

#[test]
fn registry_trust_restart_probe() {
    let Some(root) = std::env::var_os("SHIELD_REGISTRY_TRUST_PROBE_ROOT") else {
        return;
    };
    assert!(
        std::env::var_os("SHIELD_ISOLATED_HOME").is_some(),
        "probe inherits the exact owned user namespace"
    );
    let project = crate::runtime::Project::open(Path::new(&root)).unwrap();
    assert!(matches!(
        project.snapshot(),
        Err(crate::review::runtime::consumer::RuntimeError::RevokedPolicy { .. })
    ));
}

#[test]
fn final_shared_guard_excludes_global_head_cas_and_old_evidence() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::final_shared_guard_excludes_global_head_cas_and_old_evidence"
    )) {
        return;
    }
    let fixture = Fixture::new();
    Prepared::new(&fixture.context, "acme", &|| Ok(()))
        .unwrap()
        .commit(&fixture.context, &|| Ok(()))
        .unwrap();
    let namespace = Namespace::user().unwrap();
    let previous = namespace.read(fixture.source.identity()).unwrap().unwrap();
    fixture.publish(2, false);
    let update = fetch(
        &fixture.context.fetcher,
        &fixture.source,
        Some(&previous),
        &json!({}),
        &|| Ok(()),
    )
    .unwrap();
    let project = crate::runtime::Project::open(fixture.project.path()).unwrap();
    let snapshot = project.snapshot().unwrap();
    let evidence = snapshot.evaluate(fixture.request()).unwrap();
    project
        .with_current(&evidence, |_, _| {
            assert!(matches!(
                namespace.publish(&update, Some(&previous)),
                Err(Error::Locked)
            ));
        })
        .unwrap();
    let published = namespace.publish(&update, Some(&previous)).unwrap();
    assert!(published.generation > previous.generation);
    assert!(project.with_current(&evidence, |_, _| ()).is_err());
    let replay = namespace.publish(&update, Some(&previous)).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.generation, published.generation);
}

#[test]
fn stale_or_cancelled_delegation_reviews_grant_no_mode() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::stale_or_cancelled_delegation_reviews_grant_no_mode"
    )) {
        return;
    }
    let fixture = Fixture::new();
    assert!(matches!(
        Prepared::new(&fixture.context, "acme", &|| Err(
            crate::policy::Error::Interrupted
        )),
        Err(Error::Cancelled)
    ));
    let plan = Prepared::new(&fixture.context, "acme", &|| Ok(())).unwrap();
    Store::open(fixture.project.path())
        .unwrap()
        .update_checkpoint(&fixture.context, &|| Ok(()), |state| {
            state.operator["sources"]
                .as_object_mut()
                .unwrap()
                .remove("acme");
            Ok(json!({}))
        })
        .unwrap();
    assert!(matches!(
        plan.commit(&fixture.context, &|| Ok(())),
        Err(Error::StaleSnapshot)
    ));
    assert!(
        Namespace::user()
            .unwrap()
            .read(fixture.source.identity())
            .unwrap()
            .is_none()
    );
    assert!(Prepared::new(&fixture.context, "main", &|| Ok(())).is_err());
}

#[test]
fn trust_head_cas_rejects_equal_value_aba_without_forgetting_catalog_pins() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::trust_head_cas_rejects_equal_value_aba_without_forgetting_catalog_pins"
    )) {
        return;
    }
    let fixture = Fixture::new();
    Prepared::new(&fixture.context, "acme", &|| Ok(()))
        .unwrap()
        .commit(&fixture.context, &|| Ok(()))
        .unwrap();
    let namespace = Namespace::user().unwrap();
    let first = namespace.read(fixture.source.identity()).unwrap().unwrap();
    let stale = fetch(
        &fixture.context.fetcher,
        &fixture.source,
        Some(&first),
        &json!({}),
        &|| Ok(()),
    )
    .unwrap();
    let pinned = json!({"installed":[{"source_identity":fixture.source.identity(),"id":"acme/old","version":"0.1.0","digest":"b".repeat(64)}]});
    let with_pin = fetch(
        &fixture.context.fetcher,
        &fixture.source,
        Some(&first),
        &pinned,
        &|| Ok(()),
    )
    .unwrap();
    namespace.publish(&with_pin, Some(&first)).unwrap();
    let second = namespace.read(fixture.source.identity()).unwrap().unwrap();
    assert_eq!(
        second
            .checkpoints
            .release_digest(fixture.source.identity(), "acme/old@0.1.0"),
        Some("b".repeat(64).as_str())
    );
    let without_pin = fetch(
        &fixture.context.fetcher,
        &fixture.source,
        Some(&second),
        &json!({}),
        &|| Ok(()),
    )
    .unwrap();
    namespace.publish(&without_pin, Some(&second)).unwrap();
    let third = namespace.read(fixture.source.identity()).unwrap().unwrap();
    assert_eq!(third.object, first.object);
    assert!(third.generation > first.generation);
    assert!(matches!(
        namespace.publish(&stale, Some(&first)),
        Err(Error::StaleSnapshot)
    ));
}

#[test]
fn source_only_reuse_keeps_revoked_policy_reviewed_but_inactive() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::source_only_reuse_keeps_revoked_policy_reviewed_but_inactive"
    )) {
        return;
    }
    let fixture = Fixture::new();
    Prepared::new(&fixture.context, "acme", &|| Ok(()))
        .unwrap()
        .commit(&fixture.context, &|| Ok(()))
        .unwrap();
    fixture.publish(2, true);
    registries::checked_catalog(&fixture.context, &fixture.source).unwrap();
    let clone = assert_fs::TempDir::new().unwrap();
    std::fs::create_dir(clone.path().join(".shield")).unwrap();
    for file in ["config.yml", "tools.yml"] {
        std::fs::copy(
            fixture.project.path().join(".shield").join(file),
            clone.path().join(".shield").join(file),
        )
        .unwrap();
    }
    let context = super::super::open(clone.path()).unwrap();
    let state = super::super::project::checkpoint(&context).unwrap();
    assert!(state.active.is_none());
    assert!(inactive_reviewed(&state));
    assert_eq!(state.operator["policy_consent"]["reason"], "known-revoked");
    assert!(super::super::bootstrap::forms_reviewed_at(clone.path()));
    let status = super::super::runtime::status_from(&state);
    assert!(status["active"].is_null());
    assert!(status["compiled_cache"].is_null());
    assert!(
        status["next_action"]
            .as_str()
            .unwrap()
            .contains("Update or remove")
    );
    assert!(!state.registry_checkpoints.is_empty());
    assert!(matches!(
        crate::runtime::Project::open(clone.path())
            .unwrap()
            .snapshot(),
        Err(crate::review::runtime::consumer::RuntimeError::RevokedPolicy { .. })
    ));
}

#[test]
fn missing_adopted_head_preserves_inspection_and_inactive_private_consent() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::missing_adopted_head_preserves_inspection_and_inactive_private_consent"
    )) {
        return;
    }
    let fixture = Fixture::new();
    Prepared::new(&fixture.context, "acme", &|| Ok(()))
        .unwrap()
        .commit(&fixture.context, &|| Ok(()))
        .unwrap();
    let directory = std::path::PathBuf::from(std::env::var_os("SHIELD_ISOLATED_HOME").unwrap())
        .join(".shield/registry-trust");
    let head = std::fs::read_dir(&directory)
        .unwrap()
        .map(|row| row.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".head.json")
        })
        .unwrap();
    std::fs::remove_file(head).unwrap();
    assert!(
        Namespace::user()
            .unwrap()
            .read(fixture.source.identity())
            .is_err()
    );
    assert!(
        crate::runtime::Project::open(fixture.project.path())
            .unwrap()
            .snapshot()
            .is_err()
    );
    super::super::project::checkpoint(&fixture.context).unwrap();
    let clone = assert_fs::TempDir::new().unwrap();
    std::fs::create_dir(clone.path().join(".shield")).unwrap();
    for file in ["config.yml", "tools.yml"] {
        std::fs::copy(
            fixture.project.path().join(".shield").join(file),
            clone.path().join(".shield").join(file),
        )
        .unwrap();
    }
    let context = super::super::open(clone.path()).unwrap();
    let state = super::super::project::checkpoint(&context).unwrap();
    assert!(state.active.is_none());
    assert!(inactive_reviewed(&state));
    assert_eq!(
        state.operator["policy_consent"]["reason"],
        "authority-unavailable"
    );
    assert!(super::super::bootstrap::forms_reviewed_at(clone.path()));
    assert!(matches!(inactive_error(&state), Some(Error::Invalid(_))));
}

#[test]
fn repaired_authority_reactivates_exact_consent_only_on_runtime_use() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::repaired_authority_reactivates_exact_consent_only_on_runtime_use"
    )) {
        return;
    }
    let fixture = Fixture::new();
    Prepared::new(&fixture.context, "acme", &|| Ok(()))
        .unwrap()
        .commit(&fixture.context, &|| Ok(()))
        .unwrap();
    let directory = std::path::PathBuf::from(std::env::var_os("SHIELD_ISOLATED_HOME").unwrap())
        .join(".shield/registry-trust");
    let head = std::fs::read_dir(&directory)
        .unwrap()
        .map(|row| row.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".head.json")
        })
        .unwrap();
    let backup: Value = serde_json::from_slice(&std::fs::read(&head).unwrap()).unwrap();
    std::fs::remove_file(&head).unwrap();
    let clone = assert_fs::TempDir::new().unwrap();
    std::fs::create_dir(clone.path().join(".shield")).unwrap();
    for file in ["config.yml", "tools.yml"] {
        std::fs::copy(
            fixture.project.path().join(".shield").join(file),
            clone.path().join(".shield").join(file),
        )
        .unwrap();
    }
    let context = super::super::open(clone.path()).unwrap();
    let before = super::super::project::checkpoint(&context).unwrap();
    assert!(may_reactivate(&before));
    {
        let _lease = super::super::storage::objects::Directory::try_enter(&directory).unwrap();
        super::super::storage::private_files::write(&head, &backup, 32 * 1024).unwrap();
    }
    let checkpoint = clone.path().join(".shield/review/state.json");
    let bytes = std::fs::read(&checkpoint).unwrap();
    let status = super::super::project::status(&context).unwrap();
    assert!(status["active"].is_null());
    assert_eq!(
        std::fs::read(&checkpoint).unwrap(),
        bytes,
        "diagnostics are observational after repair"
    );
    let requests = fixture.server.requests().len();
    let project = crate::runtime::Project::open(clone.path()).unwrap();
    let snapshot = project.snapshot().unwrap();
    assert_eq!(
        snapshot
            .evaluate(crate::policy::Request {
                executable: "acme".into(),
                arguments: vec!["status".into()],
                cwd: context.project_root.clone(),
                project: context.project_root.clone()
            })
            .unwrap()
            .evaluation()
            .decision,
        crate::policy::Decision::Allow
    );
    let active = super::super::project::checkpoint(&context).unwrap();
    assert!(active.active.is_some());
    assert!(active.operator["policy_consent"].is_null());
    assert_eq!(active.policy_revision, before.policy_revision);
    assert_eq!(
        super::super::storage::sources::map_digest(&active.candidates),
        super::super::storage::sources::map_digest(&before.candidates)
    );
    assert_eq!(
        fixture.server.requests().len(),
        requests,
        "repair reactivation uses retained bytes without acquisition"
    );
}

#[test]
fn retained_mode_floors_refuse_an_older_valid_user_head() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::retained_mode_floors_refuse_an_older_valid_user_head"
    )) {
        return;
    }
    let fixture = Fixture::new();
    Prepared::new(&fixture.context, "acme", &|| Ok(()))
        .unwrap()
        .commit(&fixture.context, &|| Ok(()))
        .unwrap();
    let directory = std::path::PathBuf::from(std::env::var_os("SHIELD_ISOLATED_HOME").unwrap())
        .join(".shield/registry-trust");
    let head = std::fs::read_dir(&directory)
        .unwrap()
        .map(|row| row.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".head.json")
        })
        .unwrap();
    let backup: Value = serde_json::from_slice(&std::fs::read(&head).unwrap()).unwrap();
    let old_object = directory.join(format!(
        "{}.object.json",
        backup["object"].as_str().unwrap()
    ));
    let object_backup: Value =
        serde_json::from_slice(&std::fs::read(&old_object).unwrap()).unwrap();
    fixture.publish(2, true);
    registries::checked_catalog(&fixture.context, &fixture.source).unwrap();
    // Restore a complete, formerly valid older backup. The process memo must not
    // make that older head acceptable to a policy with newer retained knowledge.
    let newer = super::super::project::checkpoint(&fixture.context).unwrap();
    {
        let _lease = super::super::storage::objects::Directory::try_enter(&directory).unwrap();
        super::super::storage::private_files::write(&old_object, &object_backup, 4 * 1024 * 1024)
            .unwrap();
        super::super::storage::private_files::write(&head, &backup, 32 * 1024).unwrap();
    }
    assert!(matches!(guard(&newer.operator), Err(Error::Invalid(_))));
    assert!(require_mode(&newer.operator, fixture.source.identity()).is_err());
    assert!(
        crate::runtime::Project::open(fixture.project.path())
            .unwrap()
            .snapshot()
            .is_err()
    );
}

fn repaired_inactive_clone(fixture: &Fixture) -> (assert_fs::TempDir, Context) {
    Prepared::new(&fixture.context, "acme", &|| Ok(()))
        .unwrap()
        .commit(&fixture.context, &|| Ok(()))
        .unwrap();
    let directory = std::path::PathBuf::from(std::env::var_os("SHIELD_ISOLATED_HOME").unwrap())
        .join(".shield/registry-trust");
    let head = std::fs::read_dir(&directory)
        .unwrap()
        .map(|row| row.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".head.json")
        })
        .unwrap();
    let backup: Value = serde_json::from_slice(&std::fs::read(&head).unwrap()).unwrap();
    std::fs::remove_file(&head).unwrap();
    let clone = assert_fs::TempDir::new().unwrap();
    std::fs::create_dir(clone.path().join(".shield")).unwrap();
    for file in ["config.yml", "tools.yml"] {
        std::fs::copy(
            fixture.project.path().join(".shield").join(file),
            clone.path().join(".shield").join(file),
        )
        .unwrap();
    }
    let context = super::super::open(clone.path()).unwrap();
    {
        let _lease = super::super::storage::objects::Directory::try_enter(&directory).unwrap();
        super::super::storage::private_files::write(&head, &backup, 32 * 1024).unwrap();
    }
    (clone, context)
}

#[test]
fn repaired_consent_cas_and_cache_failures_never_activate() {
    if !crate::test_support::isolated_home(concat!(
        module_path!(),
        "::repaired_consent_cas_and_cache_failures_never_activate"
    )) {
        return;
    }
    let fixture = Fixture::new();
    let (clone, context) = repaired_inactive_clone(&fixture);
    let store = Store::open(clone.path()).unwrap();
    let captured = super::super::project::checkpoint(&context).unwrap();
    let calls = Cell::new(0);
    let result = store.reactivate_reviewed(&context, captured, &|| {
        calls.set(calls.get() + 1);
        if calls.get() == 2 {
            Store::open(clone.path())
                .unwrap()
                .update_checkpoint(&context, &|| Ok(()), |current| {
                    current.operator["reactivation_race"] = json!(true);
                    Ok(json!({}))
                })
                .unwrap();
        }
        Ok(())
    });
    assert!(matches!(result, Err(Error::StaleSnapshot)));
    let state = super::super::project::checkpoint(&context).unwrap();
    assert!(state.active.is_none());
    assert!(inactive_reviewed(&state));
    let occupied = clone.path().join("occupied-cache");
    std::fs::write(&occupied, b"owned fixture file").unwrap();
    store
        .update_checkpoint(&context, &|| Ok(()), |current| {
            current.operator["compiled_cache_root"] = json!(occupied);
            Ok(json!({}))
        })
        .unwrap();
    assert!(
        crate::runtime::Project::open(clone.path())
            .unwrap()
            .snapshot()
            .is_err()
    );
    let state = super::super::project::checkpoint(&context).unwrap();
    assert!(state.active.is_none());
    assert!(inactive_reviewed(&state));
}
