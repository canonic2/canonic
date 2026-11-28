use super::*;
fn write(root: &Path, path: &str, value: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, value).unwrap();
}
#[test]
fn onboarding_pages_reach_candidates_beyond_the_interactive_scan_cap() {
    let root = tempfile::tempdir().unwrap();
    for i in 0..301 {
        write(
            root.path(),
            &format!("app-{i:04}/package.json"),
            r#"{"dependencies":{"vite":"1"}}"#,
        );
    }
    let mut cursor = None;
    let mut paths = BTreeSet::new();
    loop {
        let page = detection::page(
            root.path(),
            &[],
            &canonic_mcp_kit::page::arguments(cursor.as_deref(), Some(100)),
        )
        .unwrap();
        for candidate in page["candidates"].as_array().unwrap() {
            assert!(paths.insert(candidate["path"].as_str().unwrap().to_owned()));
        }
        cursor = page["nextCursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(paths.len(), 301);
}
#[test]
fn onboarding_pages_keep_long_paths_inside_the_shared_cursor_bound() {
    let root = tempfile::tempdir().unwrap();
    let parent = "a".repeat(180);
    let child = format!("{parent}/{}", "b".repeat(180));
    for path in [&parent, &child] {
        write(
            root.path(),
            &format!("{path}/package.json"),
            r#"{"dependencies":{"vite":"1"}}"#,
        );
    }
    for paths in [vec![], vec![PathBuf::from(&parent), PathBuf::from(&child)]] {
        let mut cursor = None;
        let mut returned = BTreeSet::new();
        loop {
            let page = detection::page(
                root.path(),
                &paths,
                &canonic_mcp_kit::page::arguments(cursor.as_deref(), Some(1)),
            )
            .unwrap();
            for candidate in page["candidates"].as_array().unwrap() {
                assert!(returned.insert(candidate["path"].as_str().unwrap().to_owned()));
            }
            cursor = page["nextCursor"].as_str().map(str::to_owned);
            if let Some(cursor) = &cursor {
                assert!(cursor.len() <= 256);
            } else {
                break;
            }
        }
        assert_eq!(returned, BTreeSet::from([parent.clone(), child.clone()]));
    }
}
#[test]
fn onboarding_detects_monorepo_apps_without_generated_duplicates_or_scripts() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "package.json", r#"{"workspaces":["apps/*"]}"#);
    write(
        root.path(),
        "apps/web/package.json",
        r#"{"dependencies":{"vue":"1"},"devDependencies":{"vite":"1"},"scripts":{"preinstall":"must-not-run"}}"#,
    );
    write(
        root.path(),
        "apps/mobile/package.json",
        r#"{"dependencies":{"expo":"1","react-native":"1","react":"1"}}"#,
    );
    fs::create_dir_all(root.path().join("apps/mobile/ios/Acme.xcodeproj")).unwrap();
    write(
        root.path(),
        "apps/mobile/android/build.gradle",
        "ignored source",
    );
    write(
        root.path(),
        "node_modules/ignored/package.json",
        r#"{"dependencies":{"react":"1"}}"#,
    );
    write(
        root.path(),
        "apps/flutter/pubspec.yaml",
        "name: acme\ndependencies:\n  flutter:\n    sdk: flutter\n",
    );
    fs::create_dir_all(root.path().join("apps/native/Acme.xcodeproj")).unwrap();
    write(root.path(), "apps/android/build.gradle.kts", "plugins {}");
    write(
        root.path(),
        "apps/android/app/src/main/AndroidManifest.xml",
        "<manifest/>",
    );
    write(root.path(), "apps/static/index.html", "<!doctype html>");
    write(
        root.path(),
        "apps/flutter/web/index.html",
        "<!doctype html>",
    );
    let detected = discover(root.path(), &[]).unwrap();
    assert!(!detected.truncated);
    let mut cursor = None;
    let mut paged = Vec::new();
    loop {
        let page = detection::page(
            root.path(),
            &[],
            &canonic_mcp_kit::page::arguments(cursor.as_deref(), Some(1)),
        )
        .unwrap();
        paged.extend(page["candidates"].as_array().unwrap().iter().cloned());
        cursor = page["nextCursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    paged.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    assert_eq!(json!(paged), json!(detected.candidates));
    let rows: Vec<_> = detected
        .candidates
        .iter()
        .map(|c| (c.path.as_str(), c.kind.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("apps/android", "android-studio"),
            ("apps/flutter", "flutter"),
            ("apps/mobile", "expo"),
            ("apps/native", "xcode"),
            ("apps/static", "web"),
            ("apps/web", "vue")
        ]
    );
    assert!(
        detected
            .candidates
            .iter()
            .find(|c| c.kind == "xcode")
            .unwrap()
            .platforms
            .is_empty()
    );
    let plan = configuration(&detected.candidates).unwrap();
    assert_eq!(
        plan["projects"]["web"]["profiles"]["web"]["platform"],
        "web"
    );
    assert!(!root.path().join("testrun.yml").exists());
}
#[test]
fn onboarding_manual_paths_and_custom_platforms_stay_extensible() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("tool")).unwrap();
    let mut detected = discover(root.path(), &["tool".into()]).unwrap();
    detected.candidates[0].kind = "acme/board".into();
    detected.candidates[0].platforms = vec!["acme/firmware".into()];
    let plan = configuration(&detected.candidates).unwrap();
    assert_eq!(
        plan["projects"]["tool"]["profiles"]["acme-firmware"]["platform"],
        "acme/firmware"
    );
    assert!(discover(root.path(), &["../outside".into()]).is_err());
    let report = publish(root.path(), &plan).unwrap();
    assert_eq!(report["ignoreUpdated"], true);
    assert_eq!(load(root.path()).unwrap().document.projects.len(), 1);
    assert_eq!(
        fs::read_to_string(root.path().join(".gitignore")).unwrap(),
        "/testrun.local.yml\n"
    );
    assert!(publish(root.path(), &plan).is_err());
}
#[test]
fn onboarding_publication_preserves_existing_shared_local_and_ignore_files() {
    let root = tempfile::tempdir().unwrap();
    let plan = configuration(&[Candidate {
        path: ".".into(),
        kind: "custom".into(),
        platforms: Vec::new(),
        evidence: Vec::new(),
    }])
    .unwrap();
    write(
        root.path(),
        "testrun.local.yml",
        "# private overlay\nversion: 2\n",
    );
    assert!(publish(root.path(), &plan).is_err());
    assert!(!root.path().join("testrun.yml").exists());
    fs::remove_file(root.path().join("testrun.local.yml")).unwrap();
    write(root.path(), ".gitignore", "# existing\ndist/");
    publish(root.path(), &plan).unwrap();
    assert_eq!(
        fs::read_to_string(root.path().join(".gitignore")).unwrap(),
        "# existing\ndist/\n/testrun.local.yml\n"
    );
    let before = fs::read(root.path().join("testrun.yml")).unwrap();
    assert!(publish(root.path(), &plan).is_err());
    assert_eq!(fs::read(root.path().join("testrun.yml")).unwrap(), before);
}
#[test]
fn onboarding_duplicate_names_are_stable_and_invalid_manifests_are_reported() {
    let root = tempfile::tempdir().unwrap();
    for path in ["apps/one/package.json", "tools/one/package.json"] {
        write(root.path(), path, r#"{"dependencies":{"react":"1"}}"#);
    }
    write(root.path(), "broken/package.json", "not json");
    let detected = discover(root.path(), &[]).unwrap();
    assert_eq!(detected.warnings.len(), 1);
    let plan = configuration(&detected.candidates).unwrap();
    assert_eq!(plan["projects"]["one"]["root"], "apps/one");
    assert_eq!(plan["projects"]["one-2"]["root"], "tools/one");
    assert!(configuration(&[]).is_err());
    let plan = configuration(&[Candidate {
        path: ".".into(),
        kind: "custom".into(),
        platforms: vec![
            "42/board".into(),
            "acme/board".into(),
            "acme-board".into(),
            "acme-board".into(),
        ],
        evidence: vec![],
    }])
    .unwrap();
    assert_eq!(
        plan["projects"]["app"]["profiles"]
            .as_object()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        plan["projects"]["app"]["profiles"]["app-42-board"]["platform"],
        "42/board"
    );
}
#[cfg(unix)]
#[test]
fn onboarding_does_not_follow_symlinked_trees_or_manifests() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    write(
        external.path(),
        "package.json",
        r#"{"dependencies":{"react":"1"}}"#,
    );
    std::os::unix::fs::symlink(external.path(), root.path().join("linked")).unwrap();
    std::os::unix::fs::symlink(
        external.path().join("package.json"),
        root.path().join("package.json"),
    )
    .unwrap();
    assert!(discover(root.path(), &[]).unwrap().candidates.is_empty());
    assert!(discover(root.path(), &["linked".into()]).is_err());
}
#[test]
fn onboarding_bounded_manifests_and_depth_report_incomplete_discovery() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "package.json", &" ".repeat(262145));
    fs::create_dir_all(root.path().join("a/b/c/d/e/f/g")).unwrap();
    let detected = discover(root.path(), &[]).unwrap();
    assert!(detected.truncated);
    assert!(!detected.warnings.is_empty());
}
