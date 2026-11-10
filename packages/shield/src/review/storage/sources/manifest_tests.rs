use super::*;
use std::collections::BTreeMap;
use std::io::Write as _;
fn fixture() -> (assert_fs::TempDir, Store) {
    let (root, _) = crate::review::tests::enrolled();
    let store = Store::open(root.path()).unwrap();
    (root, store)
}
fn candidate(rules: &str, packs: bool) -> Value {
    let mut value = json!(
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap()
    );
    value["rules"] = json!(rules);
    if packs {
        value["packs"] = json!(["Acme unchanged artifact ".repeat(2048)]);
    }
    value
}
fn retain(store: &Store, raw: &Value) -> (Value, super::super::objects::Pins) {
    let mut reference = raw.clone();
    let pins = compact(store, &mut reference).unwrap();
    (reference, pins)
}
fn names(store: &Store) -> BTreeSet<String> {
    fs::read_dir(store.directory.join("sources"))
        .unwrap()
        .map(|v| v.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn source_v2_preserves_original_identity_and_narrow_reads_skip_unselected_artifacts() {
    let (_root, store) = fixture();
    let mut raw = candidate("quoted \"rules\"\n😃\té", true);
    raw["local_config"] = Value::Null;
    let (reference, pins) = retain(&store, &raw);
    assert_eq!(
        pins.len(),
        1,
        "one manifest descriptor, not one per artifact"
    );
    assert_eq!(candidate_identity(&reference), candidate_identity(&raw));
    assert_eq!(
        map_digest(&BTreeMap::from([("project".into(), reference.clone())])),
        map_digest(&BTreeMap::from([("project".into(), raw.clone())]))
    );
    let mut hydrated = reference.clone();
    hydrate(&store, &mut hydrated).unwrap();
    assert_eq!(hydrated, raw);
    let blob = store.directory.join("sources").join(format!(
        "{}.blob",
        crate::policy::fingerprint(raw["packs"][0].as_str().unwrap().as_bytes())
    ));
    let hidden = blob.with_extension("hidden");
    fs::rename(&blob, &hidden).unwrap();
    assert_eq!(
        captured_field(&store, &reference, "config", None).unwrap(),
        raw["config"]
    );
    assert!(hydrate(&store, &mut reference.clone()).is_err());
    fs::rename(hidden, blob).unwrap();
}

#[test]
fn source_v1_remains_readable_with_the_same_identity() {
    let (_root, store) = fixture();
    let raw = candidate("Acme legacy", false);
    let bytes = serde_json::to_vec(&raw).unwrap();
    let digest = crate::policy::fingerprint(&bytes);
    let path = store
        .directory
        .join("sources")
        .join(format!("{digest}.json"));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    private(&mut options);
    let mut file = options.open(path).unwrap();
    file.write_all(&bytes).unwrap();
    drop(file);
    let reference = json!({"kind":KIND,"digest":digest,"candidate_digest":map_digest(&BTreeMap::from([("project".into(),raw.clone())])),"length":bytes.len()});
    let pins = pin(&store, &reference).unwrap();
    assert_eq!(pins.len(), 1);
    let mut hydrated = reference.clone();
    hydrate(&store, &mut hydrated).unwrap();
    assert_eq!(hydrated, raw);
    assert_eq!(candidate_identity(&reference), candidate_identity(&raw));
}

#[test]
fn changed_candidates_share_artifacts_and_only_store_changed_components() {
    let (_root, store) = fixture();
    let (first, first_pin) = retain(&store, &candidate("Acme first", true));
    let before = names(&store);
    let (second, second_pin) = retain(&store, &candidate("Acme second", true));
    let after = names(&store);
    assert_eq!(
        after.difference(&before).count(),
        2,
        "only changed text blob and manifest"
    );
    assert_ne!(first["digest"], second["digest"]);
    drop(first_pin);
    let (_third, _third_pin) = retain(&store, &candidate("Acme third", false));
    assert_eq!(
        captured_field(&store, &second, "packs", Some(0)).unwrap(),
        candidate("", true)["packs"][0]
    );
    drop(second_pin);
}

#[test]
fn manifest_only_pin_protects_descendants_before_any_blob_reclamation() {
    let (_root, store) = fixture();
    let raw = candidate("Acme pinned", true);
    let (reference, pins) = retain(&store, &raw);
    let blob = store.directory.join("sources").join(format!(
        "{}.blob",
        crate::policy::fingerprint(raw["packs"][0].as_str().unwrap().as_bytes())
    ));
    // The blob precedes its sole pin's manifest in publication order. Scanning
    // deletion batches before discovering that pin would delete live content.
    let (_other, other_pin) = retain(&store, &candidate("Acme no pack", false));
    assert!(blob.exists());
    assert_eq!(pins.len(), 1);
    let mut hydrated = reference.clone();
    hydrate(&store, &mut hydrated).unwrap();
    assert_eq!(hydrated, raw);
    drop(pins);
    drop(other_pin);
    let (_next, _pin) = retain(&store, &candidate("Acme collect", false));
    assert!(!blob.exists());
}

#[test]
fn manifest_substitution_cannot_retarget_the_original_identity() {
    let (_root, store) = fixture();
    let (mut first, _a) = retain(&store, &candidate("Acme original", false));
    let (second, _b) = retain(&store, &candidate("Acme other", false));
    first["manifest_digest"] = second["manifest_digest"].clone();
    first["manifest_length"] = second["manifest_length"].clone();
    assert!(captured_field(&store, &first, "rules", None).is_err());
    assert!(hydrate(&store, &mut first).is_err());
}

#[test]
fn existing_source_verification_stops_at_declared_length_plus_one() {
    struct Growing<'a>(&'a std::cell::Cell<usize>);
    impl Read for Growing<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            bytes.fill(0);
            self.0.set(self.0.get() + bytes.len());
            Ok(bytes.len())
        }
    }
    let read = std::cell::Cell::new(0);
    assert!(
        manifest::verify_existing(Growing(&read), 16, &crate::policy::fingerprint(&[0; 16]))
            .is_err()
    );
    assert_eq!(read.get(), 17);
}

#[test]
fn malformed_manifest_aggregate_is_refused_before_any_blob_hydration() {
    let (_root, store) = fixture();
    let raw = candidate("Acme aggregate", true);
    let (mut reference, _pin) = retain(&store, &raw);
    let directory = store.directory.join("sources");
    let original = directory.join(manifest::name(&reference).unwrap());
    let mut wire: Value = serde_json::from_slice(&fs::read(original).unwrap()).unwrap();
    let mut blob = wire["fields"]["packs"]["value"][0].clone();
    blob["length"] = json!(reference["length"].as_u64().unwrap() / 2 + 1);
    wire["fields"]["packs"]["value"] = json!([blob, blob]);
    let bytes = serde_json::to_vec(&wire).unwrap();
    let digest = crate::policy::fingerprint(&bytes);
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    private(&mut options);
    options
        .open(directory.join(format!("{digest}.manifest.json")))
        .unwrap()
        .write_all(&bytes)
        .unwrap();
    reference["manifest_digest"] = json!(digest);
    reference["manifest_length"] = json!(bytes.len());
    assert!(matches!(
        captured_field(&store, &reference, "config", None),
        Err(Error::Invalid("source manifest aggregate exceeds bundle"))
    ));
}

#[test]
fn private_namespace_import_and_metadata_roots_survive_independent_gc() {
    let (_root, store) = fixture();
    let raw = candidate("Acme namespace", true);
    let (reference, project_pin) = retain(&store, &raw);
    let home = assert_fs::TempDir::new().unwrap();
    let directory = home.path().join("private-consent");
    super::super::private_directory(&directory).unwrap();
    let user = Namespace::private(&directory).unwrap();
    let (imported, user_pin) = user
        .import(&Namespace::project(&store), &reference)
        .unwrap();
    assert_eq!(imported, reference);
    assert_eq!(user_pin.len(), 1);
    let metadata = json!({"policy_lineage":"Acme lineage","sources":{}});
    let (context, context_pin) = user.retain_json(&metadata).unwrap();
    assert_eq!(context_pin.len(), 1);
    super::super::private_files::write(
        &directory.join("state.json"),
        &json!({"candidates":{"approved":{"source":imported,"context":context}}}),
        2 * 1024 * 1024,
    )
    .unwrap();
    drop(project_pin);
    drop(user_pin);
    drop(context_pin);
    let (_other, _other_pin) = user.retain_owned(candidate("Acme next", false)).unwrap();
    assert_eq!(user.captured(&reference).unwrap(), raw);
    assert_eq!(user.read_json(&context).unwrap(), metadata);
}
