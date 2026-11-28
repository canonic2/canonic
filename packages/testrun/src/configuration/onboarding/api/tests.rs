use super::*;
fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("web")).unwrap();
    fs::write(
        root.path().join("web/package.json"),
        r#"{"dependencies":{"vue":"1"},"devDependencies":{"vite":"1"}}"#,
    )
    .unwrap();
    fs::create_dir(root.path().join("device")).unwrap();
    root
}

#[test]
fn initialization_preview_has_separate_permissions_and_cannot_create_files() {
    let root = fixture();
    let definitions = tools();
    let preview = definitions
        .iter()
        .find(|tool| tool["name"] == "project_init_preview")
        .unwrap();
    assert_eq!(preview["annotations"]["readOnlyHint"], true);
    assert_eq!(preview["annotations"]["destructiveHint"], false);
    let args = json!({"root":root.path(),"projects":[{"path":"web"}]});
    let plan = call("project_init_preview", &args).unwrap();
    assert_eq!(plan["dryRun"], true);
    let mut apply_args = args.clone();
    apply_args["planDigest"] = plan["planDigest"].clone();
    assert!(call("project_init_preview", &apply_args).is_err());
    assert!(
        call("project_init", &args).is_err(),
        "creation requires the preview digest"
    );
    for tool in &definitions {
        assert!(tool["inputSchema"]["properties"].get("dryRun").is_none());
    }
    for value in [false, true] {
        let mut invalid = args.clone();
        invalid["dryRun"] = json!(value);
        assert!(call("project_init_preview", &invalid).is_err());
        invalid["planDigest"] = plan["planDigest"].clone();
        assert!(call("project_init", &invalid).is_err());
    }
    assert!(!root.path().join("testrun.yml").exists());
    assert!(!root.path().join(".gitignore").exists());
}
#[test]
fn onboarding_mcp_discovery_matches_cli_review_and_does_not_write() {
    let root = fixture();
    let discovered = discover(root.path(), &[]).unwrap();
    let page = call("project_discover", &json!({"root":root.path()})).unwrap();
    let expected = review(&discovered, &discovered.candidates, None).unwrap();
    for field in ["candidates", "configuration", "canCreate", "planDigest"] {
        assert_eq!(page[field], expected[field], "{field}");
    }
    assert!(page["nextCursor"].is_null());
    assert!(!root.path().join("testrun.yml").exists());
    assert!(!root.path().join(".gitignore").exists());
    assert_eq!(
        call("project_discover", &json!({"root":root.path()})).unwrap()["configuration"]["projects"]
            ["web"]["kind"],
        "vue"
    );
}
#[test]
fn onboarding_mcp_selected_overrides_preview_and_apply_share_one_configuration() {
    let root = fixture();
    let mut args = json!({"root":root.path(),"projects":[{"path":"web","kind":"react","platforms":[]},{"path":"device","kind":"acme/board","platforms":["acme/firmware"]}]});
    let plan = call("project_init_preview", &args).unwrap();
    assert_eq!(plan["dryRun"], true);
    assert_eq!(plan["canCreate"], true);
    assert_eq!(
        plan["configuration"]["projects"]["web"]["profiles"],
        json!({})
    );
    assert_eq!(
        plan["configuration"]["projects"]["device"]["profiles"]["acme-firmware"]["platform"],
        "acme/firmware"
    );
    assert!(!root.path().join("testrun.yml").exists());
    args["planDigest"] = plan["planDigest"].clone();
    let applied = call("project_init", &args).unwrap();
    assert_eq!(applied["configuration"], plan["configuration"]);
    assert_eq!(load(root.path()).unwrap().value, plan["configuration"]);
    let before = fs::read(root.path().join("testrun.yml")).unwrap();
    assert_eq!(
        call("project_init", &args).err().unwrap().code,
        "CONFIG_EXISTS"
    );
    assert_eq!(fs::read(root.path().join("testrun.yml")).unwrap(), before);
}
#[test]
fn onboarding_mcp_creates_the_selected_file_and_ignores_its_overlay() {
    let root = fixture();
    let grouped = root.path().join(".config/testrun.yml");
    let mut args = json!({"root":root.path(),"config":grouped,"projects":[{"path":"web"}]});
    let plan = call("project_init_preview", &args).unwrap();
    let canonical = root.path().canonicalize().unwrap();
    assert_eq!(
        plan["writes"][0]["path"],
        json!(canonical.join(".config/testrun.yml"))
    );
    assert_eq!(plan["writes"][1]["entry"], "/.config/testrun.local.yml");
    // The destination is part of the reviewed plan.
    let default = call(
        "project_init_preview",
        &json!({"root":root.path(),"projects":[{"path":"web"}]}),
    )
    .unwrap();
    assert_ne!(default["planDigest"], plan["planDigest"]);
    args["planDigest"] = default["planDigest"].clone();
    assert_eq!(
        call("project_init", &args).err().unwrap().code,
        "INIT_PLAN_CHANGED"
    );
    args["planDigest"] = plan["planDigest"].clone();
    call("project_init", &args).unwrap();
    assert!(!root.path().join("testrun.yml").exists());
    assert_eq!(
        fs::read_to_string(root.path().join(".gitignore")).unwrap(),
        "/.config/testrun.local.yml\n"
    );
    // Both default locations now block a default initialization.
    let blocked = call(
        "project_init_preview",
        &json!({"root":root.path(),"projects":[{"path":"web"}]}),
    )
    .unwrap();
    assert_eq!(blocked["blocker"]["code"], "CONFIG_EXISTS");
    assert_eq!(
        load(root.path()).unwrap().location.base,
        canonical.join(".config/testrun.yml")
    );
    assert!(
        call(
            "project_discover",
            &json!({"root":root.path(),"config":"relative.yml"})
        )
        .is_err()
    );
}
#[test]
fn onboarding_mcp_apply_requires_exact_reviewed_selections_and_root() {
    let root = fixture();
    let args = json!({"root":root.path(),"projects":[{"path":"web"}]});
    let plan = call("project_init_preview", &args).unwrap();
    let mut changed = args.clone();
    assert!(
        call("project_init", &changed).is_err(),
        "creation requires the preview digest"
    );
    changed["planDigest"] = plan["planDigest"].clone();
    changed["projects"][0]["kind"] = json!("react");
    assert_eq!(
        call("project_init", &changed).err().unwrap().code,
        "INIT_PLAN_CHANGED"
    );
    let other = fixture();
    changed["root"] = json!(other.path());
    changed["projects"][0]
        .as_object_mut()
        .unwrap()
        .remove("kind");
    assert_eq!(
        call("project_init", &changed).err().unwrap().code,
        "INIT_PLAN_CHANGED"
    );
    assert!(!root.path().join("testrun.yml").exists());
    assert!(!other.path().join("testrun.yml").exists());
}
#[test]
fn onboarding_mcp_refuses_outside_and_duplicate_paths_and_malformed_arguments() {
    let root = fixture();
    for args in [
        json!({"root":root.path(),"projects":[{"path":"web","kind":"not/a/valid/id"}]}),
        json!({"root":root.path(),"projects":[{"path":"web"},{"path":"./web"}]}),
        json!({"root":root.path(),"projects":[{"path":"web","command":["must-not-run"]}]}),
        json!({"root":root.path(),"dryRun":null}),
        json!({"root":"relative"}),
        json!({"root":root.path(),"projects":[]}),
    ] {
        assert!(call("project_init_preview", &args).is_err(), "{args}");
    }
    let outside = fixture();
    assert!(
        call(
            "project_init_preview",
            &json!({"root":root.path(),"projects":[{"path":outside.path()}]})
        )
        .is_err()
    );
    assert!(!root.path().join("testrun.yml").exists());
}
#[test]
fn onboarding_mcp_empty_existing_and_truncated_projects_remain_inspectable() {
    let root = tempfile::tempdir().unwrap();
    let empty = call("project_discover", &json!({"root":root.path()})).unwrap();
    assert_eq!(empty["canCreate"], false);
    assert!(empty["configuration"].is_null());
    fs::write(
        root.path().join("testrun.yml"),
        "# preserve\nversion: 2\nprojects: {}\n",
    )
    .unwrap();
    let existing = call(
        "project_init_preview",
        &json!({"root":root.path(),"projects":[{"path":".","kind":"custom"}]}),
    )
    .unwrap();
    assert_eq!(existing["canCreate"], false);
    assert_eq!(existing["blocker"]["code"], "CONFIG_EXISTS");
    let root = fixture();
    fs::create_dir_all(root.path().join("a/b/c/d/e/f/g")).unwrap();
    let truncated = call("project_init_preview", &json!({"root":root.path()})).unwrap();
    assert_eq!(truncated["truncated"], true);
    assert_eq!(
        call(
            "project_init",
            &json!({"root":root.path(),"planDigest":truncated["planDigest"]})
        )
        .err()
        .unwrap()
        .code,
        "DISCOVERY_LIMIT"
    );
    assert!(!root.path().join("testrun.yml").exists());
}
