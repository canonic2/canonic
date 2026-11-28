use super::*;
fn fixture(value: Value) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("testrun.yml"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    dir
}
#[test]
fn project_pages_reach_every_project_at_the_configuration_bound() {
    let projects: serde_json::Map<String, Value> = (0..128)
        .map(|i| (format!("app-{i:04}"), json!({"root":".","kind":"vite"})))
        .collect();
    let dir = fixture(json!({"version":2,"projects":projects}));
    let loaded = load(dir.path()).unwrap();
    let mut cursor = None;
    let mut count = 0;
    loop {
        let page = list(
            &loaded,
            &canonic_mcp_kit::page::arguments(cursor.as_deref(), Some(100)),
        )
        .unwrap();
        count += page["projects"].as_array().unwrap().len();
        cursor = page["nextCursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(count, 128);
}
#[test]
fn project_keysets_survive_insertion_and_deletion_without_invalidating_the_query() {
    let dir = fixture(
        json!({"version":2,"projects":{"a":{"root":".","kind":"vite"},"c":{"root":".","kind":"vite"}}}),
    );
    let first = list(&load(dir.path()).unwrap(), &json!({"limit":1})).unwrap();
    std::fs::write(dir.path().join("testrun.yml"),json!({"version":2,"projects":{"b":{"root":".","kind":"vite"},"c":{"root":".","kind":"vite"},"d":{"root":".","kind":"vite"}}}).to_string()).unwrap();
    let next = list(
        &load(dir.path()).unwrap(),
        &json!({"cursor":first["nextCursor"]}),
    )
    .unwrap();
    assert_eq!(
        next["projects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|project| project["project"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["b", "c", "d"]
    );
    assert!(next["nextCursor"].is_null());
}
#[test]
fn projects_accept_core_and_extension_platforms_without_adapters() {
    for kind in CORE_KINDS.iter().copied().chain(["acme/firmware"]) {
        let dir = fixture(
            json!({"version":2,"projects":{"app":{"root":".","kind":kind,"profiles":{"windows":{"platform":"windows","target_kind":"desktop"},"linux":{"platform":"linux","target_kind":"process"},"macos":{"platform":"macos","target_kind":"desktop"},"hardware":{"platform":"acme/board","target_kind":"acme/usb","driver":{"adapter":"acme/usb","options":{"baud":115200}}}}}}}),
        );
        let selected = select(dir.path(), "app").unwrap();
        assert_eq!(selected.project.profiles.len(), 4);
        assert_eq!(
            selected.native("hardware").err().unwrap().code,
            "CAPABILITY_UNAVAILABLE"
        );
        assert_eq!(inspect(&selected.loaded, None).unwrap()["valid"], true);
    }
}
#[test]
fn projects_isolate_invalid_siblings_and_validate_selected_references() {
    let dir = fixture(
        json!({"version":2,"projects":{"good":{"root":".","kind":"vue","profiles":{"web":{"platform":"web","target_kind":"browser"}}},"bad":{"root":".","kind":"react","typo":true}}}),
    );
    assert!(select(dir.path(), "good").is_ok());
    assert!(
        list(&load(dir.path()).unwrap(), &json!({"limit":1})).unwrap()["nextCursor"].is_string()
    );
    assert_eq!(
        inspect(&load(dir.path()).unwrap(), None).unwrap()["valid"],
        false
    );
    assert!(select(dir.path(), "bad").is_err());
    let invalid = json!({"root":".","kind":"expo","profiles":{"dev":{"platform":"ios","target_kind":"device","prepare":["missing"]}}});
    assert_eq!(
        decode_project(&invalid).err().unwrap().code,
        "CONFIG_REFERENCE"
    );
}
#[test]
fn projects_build_selection_uses_application_root_and_local_profile() {
    let dir = fixture(
        json!({"version":2,"projects":{"mobile":{"root":"app","kind":"flutter","profiles":{"debug":{"platform":"android","target_kind":"emulator","build":{"builder":"custom","custom":{"command":["make","apk"]},"artifact":{"path":"dist/app.apk","kind":"android-apk"}}}}},"web":{"root":".","kind":"vite","profiles":{"debug":{"platform":"web","target_kind":"browser"}}}}}),
    );
    std::fs::create_dir(dir.path().join("app")).unwrap();
    let request: crate::builds::Request =
        serde_json::from_value(json!({"root":dir.path(),"project":"mobile","profile":"debug"}))
            .unwrap();
    let prepared = crate::builds::prepare(&request).unwrap();
    assert_eq!(
        prepared.root,
        dir.path().canonicalize().unwrap().join("app")
    );
    assert_eq!(prepared.source["project"], "mobile");
    assert_eq!(prepared.profile.platform, "android");
    assert!(prepared.source.get("inline").is_none());
    assert_eq!(
        prepared.source["effectiveProvenance"]["/custom/command"],
        "testrun.yml"
    );
}
#[test]
fn projects_overlay_preserves_identity_and_redacts_execution_values() {
    let dir = fixture(
        json!({"version":2,"projects":{"app":{"root":".","kind":"react","tasks":{"prepare":{"command":["tool","private-argument"],"env":{"TOKEN":"private-token"}}}}}}),
    );
    std::fs::write(dir.path().join("testrun.local.yml"),r#"{"version":2,"projects":{"app":{"tasks":{"prepare":{"env":{"TOKEN":null,"LOCAL":"private-local"}}}}}}"#).unwrap();
    let selected = select(dir.path(), "app").unwrap();
    assert!(!selected.project.tasks["prepare"].env.contains_key("TOKEN"));
    let shown = selected.show().to_string();
    assert!(!shown.contains("private-"));
    std::fs::write(
        dir.path().join("testrun.local.yml"),
        r#"{"version":2,"projects":{"app":{"kind":"vue"}}}"#,
    )
    .unwrap();
    assert!(load(dir.path()).is_err());
}
#[test]
fn projects_legacy_read_is_lossless_and_does_not_rewrite_files() {
    let value = json!({"version":1,"builds":{"debug":{"builder":"custom","platform":"android","target_kind":"emulator","custom":{"command":["make","apk"]},"artifact":{"path":"app.apk"}}}});
    let dir = fixture(value.clone());
    let selected = select(dir.path(), "legacy").unwrap();
    assert_eq!(selected.native("debug").unwrap(), value["builds"]["debug"]);
    assert_eq!(
        std::fs::read(dir.path().join("testrun.yml")).unwrap(),
        serde_json::to_vec(&value).unwrap()
    );
}

#[test]
fn projects_validate_service_graphs_resources_and_execution_owners() {
    let value = json!({"version":2,"services":{"api":{"adapter":"http","execution":{"kind":"external"}}},"resources":{"shared":{"path":"libs"}},"projects":{"web":{"root":".","kind":"vite","tasks":{"sync":{"command":["make","sync"],"resources":["shared"]}},"services":{"vite":{"adapter":"http","execution":{"kind":"command","run":{"command":["pnpm","dev"]}},"depends_on":["workspace/api"]}},"profiles":{"dev":{"platform":"web","target_kind":"browser","services":["vite"],"open":{"kind":"browser","service":"vite"}}}}}});
    let dir = fixture(value.clone());
    assert!(select(dir.path(), "web").is_ok());
    for change in ["cycle", "resource", "owner"] {
        let mut invalid = value.clone();
        match change {
            "cycle" => {
                invalid["projects"]["web"]["services"]["vite"]["depends_on"] = json!(["vite"])
            }
            "resource" => {
                invalid["projects"]["web"]["tasks"]["sync"]["resources"] = json!(["missing"])
            }
            _ => {
                invalid["projects"]["web"]["services"]["vite"]["execution"]["workspace"] =
                    json!("acme")
            }
        }
        let invalid = fixture(invalid);
        assert!(select(invalid.path(), "web").is_err(), "{change}");
    }
}

#[test]
fn projects_cannot_repair_an_invalid_shared_project_with_an_overlay() {
    let dir = fixture(
        json!({"version":2,"projects":{"app":{"root":".","kind":"react","tasks":{"prepare":{"command":[]}}}}}),
    );
    std::fs::write(
        dir.path().join("testrun.local.yml"),
        r#"{"version":2,"projects":{"app":{"tasks":{"prepare":{"command":["make","prepare"]}}}}}"#,
    )
    .unwrap();
    assert!(select(dir.path(), "app").is_err());
    assert_eq!(
        inspect(&load(dir.path()).unwrap(), None).unwrap()["valid"],
        false
    );
}

#[test]
fn projects_surface_defaults_to_v2_and_accepts_explicit_v1_schema() {
    for (version, field) in [(1, "builds"), (2, "projects")] {
        let schema =
            crate::builds::config("config_schema", &json!({"root":"/","version":version})).unwrap();
        assert!(schema["properties"].get(field).is_some());
    }
    assert!(crate::builds::config("project_list", &json!({"root":"relative"})).is_err());
}

#[test]
fn projects_known_builders_validate_required_fields_and_partial_local_artifacts() {
    let invalid = json!({"root":".","kind":"xcode","profiles":{"dev":{"platform":"ios","target_kind":"simulator","build":{"builder":"xcode","xcode":{"project":"Acme.xcodeproj"}}}}});
    assert!(decode_project(&invalid).is_err());
    let overlay = json!({"version":2,"projects":{"app":{"profiles":{"dev":{"build":{"artifact":{"kind":"android-apk"}}}}}}});
    assert!(canonic_mcp_kit::schema::validate(&schema::document(true), &overlay).is_ok());
}

#[test]
fn projects_adapter_options_can_specialize_kind_and_execution_fields() {
    let dir = fixture(
        json!({"version":2,"projects":{"app":{"root":".","kind":"acme/hardware","options":{"kind":"original","execution":{"kind":"serial","port":1}}}}}),
    );
    std::fs::write(dir.path().join("testrun.local.yml"),r#"{"version":2,"projects":{"app":{"options":{"kind":"local","execution":{"kind":"network"}}}}}"#).unwrap();
    let selected = select(dir.path(), "app").unwrap();
    assert_eq!(selected.project.options["kind"], "local");
    assert_eq!(selected.project.options["execution"]["port"], 1);
}

#[test]
fn projects_legacy_development_normalizes_tasks_and_service_without_execution() {
    let profile = json!({"builder":"expo","platform":"ios","target_kind":"device","expo":{"profile":"development"},"env":{"MODE":"staging"},"development":{"app_id":"com.example.acme","variant":"staging","expo":{"url":"http://127.0.0.1:8081/","slug":"acme"},"prepare":{"command":["make","prepare"]},"server":{"command":["pnpm","start"]},"checks":{"Types":{"command":["pnpm","types"]}}}});
    let dir = fixture(json!({"version":1,"builds":{"dev":profile}}));
    let selected = select(dir.path(), "legacy").unwrap();
    assert_eq!(selected.native("dev").unwrap(), profile);
    assert_eq!(selected.project.tasks["p0-prepare"].env["MODE"], "staging");
    assert_eq!(
        selected.project.services["p0-expo"]["execution"]["kind"],
        "command"
    );
    assert_eq!(
        selected.project.profiles["dev"].checks["Types"],
        "p0-check-0"
    );
}

#[test]
fn projects_native_identity_is_preserved_and_missing_platform_does_not_panic() {
    let value = json!({"version":2,"projects":{"mobile":{"root":".","kind":"react-native","applications":{"staging":{"android":{"app_id":"com.example.acme"}}},"profiles":{"dev":{"platform":"android","target_kind":"emulator","application":"staging","build":{"builder":"custom","custom":{"command":["make","apk"]},"artifact":{"path":"app.apk"}}}}}}});
    let dir = fixture(value.clone());
    let selected = select(dir.path(), "mobile").unwrap();
    assert_eq!(
        selected.native("dev").unwrap()["development"]["app_id"],
        "com.example.acme"
    );
    let mut missing = value;
    missing["projects"]["mobile"]["applications"]["staging"] =
        json!({"ios":{"app_id":"com.example.acme"}});
    let dir = fixture(missing);
    assert_eq!(
        select(dir.path(), "mobile")
            .unwrap()
            .native("dev")
            .err()
            .unwrap()
            .code,
        "CONFIG_REFERENCE"
    );
}
