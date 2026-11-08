use super::*;

#[test]
fn bootstrap_nested_copy_cannot_use_the_original_enrollment_for_sdk_reads() {
    let root = assert_fs::TempDir::new().unwrap();
    let rules = crate::policy::rules::document(&[
        json!({"id":"read","decision":"allow","conditions":{"domain":"file","action":"read","sources":{"all":{"within":{"root":"project"}}}}}),
    ])
    .unwrap()
    .to_string();
    let bundle = crate::configuration::Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    let original = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    let canonical = root.path().canonicalize().unwrap();
    let ordinary = canonical.join("src/ordinary");
    fs::create_dir_all(&ordinary).unwrap();
    let path = ordinary.join("acme.txt");
    fs::write(&path, "Acme ordinary read").unwrap();
    let input = |cwd: &Path, path: &Path| json!({"protocol_version":1,"request_id":"read-nested-copy","expected_project_id":original.project_id,"cwd":cwd,"operation":{"kind":"read-text","path":path,"offset":null,"limit":null},"host":null});
    let (allowed, code) = evaluate(
        root.path(),
        &serde_json::to_vec(&input(&ordinary, &path)).unwrap(),
    );
    assert_eq!(code, 0, "{allowed}");
    assert_eq!(allowed["decision"], "allow", "{allowed}");
    let copy = canonical.join("copy");
    fs::create_dir_all(copy.join(".shield/review")).unwrap();
    fs::write(
        copy.join(".shield/config.yml"),
        "schema_version: 1\npacks: []\n",
    )
    .unwrap();
    fs::copy(
        root.path().join(".shield/review/state.json"),
        copy.join(".shield/review/state.json"),
    )
    .unwrap();
    let nested = copy.join("src");
    fs::create_dir(&nested).unwrap();
    let path = nested.join("acme.txt");
    fs::write(&path, "Acme copied read").unwrap();
    let (denied, code) = evaluate(
        root.path(),
        &serde_json::to_vec(&input(&nested, &path)).unwrap(),
    );
    assert_eq!(code, 2, "{denied}");
    assert_eq!(denied["decision"], "deny", "{denied}");
    assert_eq!(denied["code"], "invalid-context", "{denied}");
}

#[test]
fn bootstrap_nested_scope_cannot_inherit_the_original_paused_mode() {
    let root = assert_fs::TempDir::new().unwrap();
    let canonical = root.path().canonicalize().unwrap();
    let paused = crate::review::mode::Paused {
        root: canonical.clone(),
        cache: None,
        access: Ok(crate::review::path_access::PathAccess::default()),
    };
    let ordinary = canonical.join("ordinary/src");
    fs::create_dir_all(&ordinary).unwrap();
    // Web traffic has no protected file footprint and would otherwise defer
    // to the host while the original project is paused.
    let operation = json!({"kind":"web"});
    assert_eq!(
        paused_protection(&ordinary, &operation, &paused, None),
        Ok(())
    );
    for marker in ["config.yml", "review"] {
        let child = canonical.join(marker).join(".shield");
        fs::create_dir_all(&child).unwrap();
        if marker == "review" {
            fs::create_dir(child.join(marker)).unwrap();
        } else {
            fs::write(child.join(marker), "schema_version: 1\npacks: []\n").unwrap();
        }
        let cwd = child.parent().unwrap().join("src");
        fs::create_dir(&cwd).unwrap();
        assert_eq!(
            paused_protection(&cwd, &operation, &paused, None),
            Err("invalid-context")
        );
    }
}
