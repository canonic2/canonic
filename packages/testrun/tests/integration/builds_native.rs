//! Explicit native build acceptance against owned source projects and optional selected targets.
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
mod targets;

fn command(workspace: &crate::Workspace, project: &Path, args: &[&str]) -> std::process::Command {
    let executable = std::env::var_os("TESTRUN_TEST_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_testrun").into());
    let mut command = std::process::Command::new(executable);
    if workspace.path().join("simulators").exists() {
        command.env(
            "TESTRUN_SIMULATOR_DEVICE_SET",
            workspace.path().join("simulators"),
        );
    }
    if workspace.path().join("avd").exists() {
        command.env("ANDROID_AVD_HOME", workspace.path().join("avd"));
    }
    command
        .arg("--json")
        .args(args)
        .current_dir(project)
        .env("TESTRUN_DATA_ROOT", workspace.data_root())
        .env_remove("CODEX_THREAD_ID")
        .env("TESTRUN_TASK_ID", "Acme-native-build");
    command
}

async fn invoke(
    workspace: &crate::Workspace,
    project: &Path,
    args: &[&str],
) -> std::process::Output {
    tokio::time::timeout(
        Duration::from_secs(1200),
        tokio::process::Command::from(command(workspace, project, args))
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("Native build deadline")
    .unwrap()
}

/// Disconnects the scenario's session when it ends, including by a failed
/// step's panic: a booted emulator outlives its owner until disconnect stops it.
/// Declare it after the disposable target so it drops before the target and the
/// workspace that stops the owner.
struct Release<'a> {
    workspace: &'a crate::Workspace,
    project: &'a Path,
}
impl Drop for Release<'_> {
    fn drop(&mut self) {
        let _ = command(self.workspace, self.project, &["disconnect"]).output();
    }
}

async fn cli(workspace: &crate::Workspace, project: &Path, args: &[&str]) -> Value {
    let output = invoke(workspace, project, args).await;
    if !output.status.success() {
        let error: Value = serde_json::from_slice(&output.stderr).unwrap_or(Value::Null);
        let retained = tempfile::Builder::new()
            .prefix("testrun-native-build-failure-")
            .tempdir()
            .unwrap()
            .keep();
        if let Some(log) = error["error"]["details"]["operation"]["logPath"].as_str() {
            let _ = std::fs::copy(log, retained.join("build.log"));
        }
        std::fs::write(retained.join("result.json"), &output.stderr).unwrap();
        #[cfg(target_os = "macos")]
        if workspace.path().join("simulators").exists() {
            simulator_failure(workspace, &retained).await;
        }
        if let Some(target) = args
            .windows(2)
            .find(|pair| pair[0] == "--target")
            .map(|pair| pair[1])
            && target.starts_with("simulator:")
        {
            // A disposable simulator lives in the isolated set; an explicitly
            // selected one is in the default set.
            let set = workspace.path().join("simulators");
            let mut inventory = tokio::process::Command::new("/usr/bin/xcrun");
            inventory.arg("simctl");
            if set.exists() {
                inventory.arg("--set").arg(set);
            }
            let inventory = inventory
                .args(["listapps", target.rsplit(':').next().unwrap()])
                .output()
                .await
                .unwrap();
            std::fs::write(retained.join("simulator-apps.plist"), inventory.stdout).unwrap();
        }
        panic!(
            "Native CLI {} failed ({}); private evidence: {}",
            args.first().unwrap_or(&"command"),
            error["error"]["code"],
            retained.display()
        );
    }
    serde_json::from_slice(&output.stdout).unwrap()
}

#[cfg(target_os = "macos")]
async fn simulator_failure(workspace: &crate::Workspace, retained: &Path) {
    let set = workspace.path().join("simulators");
    let listing = tokio::process::Command::new("/usr/bin/xcrun")
        .args(["simctl", "--set"])
        .arg(&set)
        .args(["list", "devices", "--json"])
        .output()
        .await
        .unwrap();
    std::fs::write(retained.join("simulator-devices.json"), &listing.stdout).unwrap();
    let inventory: Value = serde_json::from_slice(&listing.stdout).unwrap_or(Value::Null);
    let Some(devices) = inventory["devices"].as_object() else {
        return;
    };
    let booted = devices
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .find(|device| device["state"] == "Booted");
    let Some(uuid) = booted.and_then(|device| device["udid"].as_str()) else {
        return;
    };
    let developer = tokio::process::Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .output()
        .await
        .unwrap();
    let executable = std::env::var_os("TESTRUN_TEST_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_testrun").into());
    let mut command = tokio::process::Command::new(executable);
    command
        .env("TESTRUN_SIMULATOR_DEVICE_SET", &set)
        .args([
            "--json",
            "native-accessibility",
            "--developer",
            String::from_utf8_lossy(&developer.stdout).trim(),
            "--id",
            uuid,
        ])
        .kill_on_drop(true);
    if let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(10), command.output()).await {
        std::fs::write(retained.join("accessibility.stdout.json"), output.stdout).unwrap();
        std::fs::write(retained.join("accessibility.stderr.json"), output.stderr).unwrap();
    }
}

async fn ready(workspace: &crate::Workspace, project: &Path, target: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    loop {
        let output = invoke(
            workspace,
            project,
            &["connect", "--target", target, "--boot"],
        )
        .await;
        if output.status.success() {
            let result: Value = serde_json::from_slice(&output.stdout).unwrap();
            if result["state"] == "ready" {
                return;
            }
        } else {
            let result: Value = serde_json::from_slice(&output.stderr).unwrap();
            assert!(
                matches!(
                    result["error"]["code"].as_str(),
                    Some("TIMEOUT" | "RECOVERING")
                ),
                "Target preparation failed: {result}"
            );
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Disposable target did not become ready"
        );
        // Observe recovery before using the idempotent connection route again;
        // no build, install or app action is replayed by target preparation.
        let _ = invoke(workspace, project, &["status"]).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[tokio::test]
async fn selected_native_build_profiles() {
    let project = std::env::var("TESTRUN_BUILD_PROJECT").expect("Select TESTRUN_BUILD_PROJECT");
    let profiles = std::env::var("TESTRUN_BUILD_PROFILES").expect("Select TESTRUN_BUILD_PROFILES");
    // Check every name before either flow creates a target or starts a build.
    let profiles: Vec<&str> = profiles.split(',').collect();
    for profile in &profiles {
        assert!(
            !profile.is_empty()
                && profile
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "TESTRUN_BUILD_PROFILES needs comma-separated profile names of lowercase letters, digits and hyphens: {profile:?}"
        );
    }
    let project = Path::new(&project).canonicalize().unwrap();
    let workspace = crate::Workspace::new();
    let selected = std::env::var("TESTRUN_BUILD_TARGET")
        .ok()
        .filter(|v| !v.is_empty());
    let flow = std::env::var("TESTRUN_BUILD_FLOW").unwrap_or_default();
    assert!(matches!(flow.as_str(), "" | "composed" | "separate"));
    let mut built = Vec::new();
    let mut owned = match selected.as_deref() {
        #[cfg(target_os = "macos")]
        Some("disposable:ios") => Some(targets::Target::ios(workspace.path())),
        Some("disposable:android") => Some(targets::Target::android(
            workspace.path(),
            Path::new(&std::env::var("ANDROID_HOME").expect("Select ANDROID_HOME")),
        )),
        _ => None,
    };
    let _release = Release {
        workspace: &workspace,
        project: &project,
    };
    // Define the isolated set before the owner starts, but boot only after
    // compilation: the owner and every CLI call must see the same host scope.
    if flow == "separate" {
        for &profile in &profiles {
            let result = cli(&workspace, &project, &["build", "start", profile]).await;
            assert_eq!(result["state"], "completed", "{result}");
            eprintln!("Native profile {profile}: build completed before target preparation");
            built.push(result);
        }
    }
    if let Some(target) = &mut owned {
        if selected.as_deref() == Some("disposable:ios") {
            target.select_simulator(
                &cli(
                    &workspace,
                    &project,
                    &["target", "list", "--kind", "simulator", "--compact"],
                )
                .await,
            );
        }
        ready(&workspace, &project, &target.id).await;
    }
    let target = owned.as_ref().map(|target| target.id.clone()).or(selected);
    if flow == "separate" {
        if let Some(target) = &target {
            // App installation runs once on a disconnected target without
            // connecting it, but launch and foreground checks need a connected
            // one. An explicit target is never booted; a disposable one is
            // already connected by ready().
            if owned.is_none() {
                cli(&workspace, &project, &["connect", "--target", target]).await;
            }
            deploy_artifacts(&workspace, &project, built, target).await;
        }
        cli(&workspace, &project, &["disconnect"]).await;
        return;
    }
    for profile in profiles {
        let mut args = vec!["build", "start", profile];
        if let Some(target) = &target {
            args.extend(["--install", "--launch", "--replace", "--target", target]);
            if owned.is_some() {
                args.push("--boot");
            }
        }
        let result = cli(&workspace, &project, &args).await;
        assert_eq!(result["state"], "completed", "{result}");
        assert!(result["result"]["artifact"]["sha256"].as_str().is_some());
        if target.is_some() {
            let conditions = json!([{"kind":"foreground","appId":result["result"]["artifact"]["identity"]["appId"]}]).to_string();
            cli(
                &workspace,
                &project,
                &["wait", &conditions, "--timeout-ms", "15000"],
            )
            .await;
        }
        eprintln!("Native profile {profile}: completed");
    }
    cli(&workspace, &project, &["disconnect"]).await;
}

async fn deploy_artifacts(
    workspace: &crate::Workspace,
    project: &Path,
    built: Vec<Value>,
    target: &str,
) {
    for result in built {
        let reference = result["result"]["artifact"]["reference"].as_str().unwrap();
        let installed = cli(
            workspace,
            project,
            &[
                "app",
                "install",
                "--artifact-ref",
                reference,
                "--replace",
                "--target",
                target,
            ],
        )
        .await;
        assert_eq!(installed["verified"], true);
        let actions = json!([{"action":"app_launch","appId":result["result"]["artifact"]["identity"]["appId"]}]).to_string();
        cli(workspace, project, &["act", &actions, "--target", target]).await;
        let conditions = json!([{"kind":"foreground","appId":result["result"]["artifact"]["identity"]["appId"]}]).to_string();
        cli(
            workspace,
            project,
            &[
                "wait",
                &conditions,
                "--timeout-ms",
                "15000",
                "--target",
                target,
            ],
        )
        .await;
        eprintln!("Native artifact {reference}: installed, launched and foreground-verified");
    }
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn native_owned_xcode_debug_release() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let ios = root.join("ios");
    std::fs::create_dir_all(ios.join("Acme.xcodeproj")).unwrap();
    std::fs::write(
        ios.join("Acme.xcodeproj/project.pbxproj"),
        include_str!("../fixtures/builds/project.pbxproj"),
    )
    .unwrap();
    std::fs::write(
        ios.join("main.m"),
        include_str!("../fixtures/builds/main.m"),
    )
    .unwrap();
    std::fs::write(root.join("testrun.yml"),"version: 1\nbuilds:\n  ios-debug:\n    builder: xcode\n    platform: ios\n    target_kind: simulator\n    cwd: ios\n    xcode:\n      project: Acme.xcodeproj\n      scheme: Acme\n      configuration: Debug\n  ios-release:\n    builder: xcode\n    platform: ios\n    target_kind: simulator\n    cwd: ios\n    xcode:\n      project: Acme.xcodeproj\n      scheme: Acme\n      configuration: Release\n").unwrap();
    let mut config: Value =
        serde_saphyr::from_str(&std::fs::read_to_string(root.join("testrun.yml")).unwrap())
            .unwrap();
    for profile in ["ios-debug", "ios-release"] {
        config["builds"][profile]["launch"] =
            json!({"args":["--acme-qa"],"env":{"API_URL":"https://staging.example.com"}});
    }
    std::fs::write(
        root.join("testrun.yml"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let workspace = crate::Workspace::new();
    let mut owned = if std::env::var("TESTRUN_BUILD_DEPLOY").as_deref() == Ok("1") {
        Some(targets::Target::ios(workspace.path()))
    } else {
        None
    };
    let _release = Release {
        workspace: &workspace,
        project: root,
    };
    if let Some(target) = &mut owned {
        target.select_simulator(
            &cli(
                &workspace,
                root,
                &["target", "list", "--kind", "simulator", "--compact"],
            )
            .await,
        );
        ready(&workspace, root, &target.id).await;
    }
    let mut marker = None;
    let mut last_artifact = Value::Null;
    for (index, profile) in ["ios-debug", "ios-release"].into_iter().enumerate() {
        let mut args = vec!["build", "start", profile];
        if let Some(target) = &owned {
            args.extend([
                "--install",
                "--launch",
                "--replace",
                "--boot",
                "--target",
                &target.id,
            ]);
        }
        let result = cli(&workspace, root, &args).await;
        assert_eq!(result["state"], "completed", "{result}");
        last_artifact = result["result"]["artifact"].clone();
        assert_eq!(
            result["result"]["artifact"]["identity"]["appId"],
            "com.example.acme.buildfixture"
        );
        if let Some(target) = &owned {
            marker = Some(ios_evidence(target, index + 1, marker.as_ref()).await);
        }
        eprintln!("Owned native {profile}: compiled and identity-verified");
    }
    if let Some(target) = &owned {
        ios_custom(&workspace, root, target, &last_artifact, marker.as_ref()).await;
    }
    cli(&workspace, root, &["disconnect"]).await;
}

#[cfg(target_os = "macos")]
async fn ios_custom(
    workspace: &crate::Workspace,
    project: &Path,
    target: &targets::Target,
    artifact: &Value,
    marker: Option<&Value>,
) {
    cli(
        workspace,
        project,
        &[
            "act",
            "[{\"action\":\"app_terminate\",\"appId\":\"com.example.acme.buildfixture\"}]",
        ],
    )
    .await;
    let inline = json!({"builder":"custom","platform":"ios","target_kind":"simulator","custom":{"command":["/usr/bin/ditto",artifact["path"],project.join("staged.app")]},"artifact":{"path":"staged.app"},"launch":{"args":["--acme-qa"],"env":{"API_URL":"https://staging.example.com"}}}).to_string();
    let result = cli(
        workspace,
        project,
        &[
            "build",
            "start",
            "--inline",
            &inline,
            "--install",
            "--launch",
            "--replace",
            "--boot",
            "--target",
            &target.id,
        ],
    )
    .await;
    assert_eq!(result["state"], "completed", "{result}");
    ios_evidence(target, 3, marker).await;
    eprintln!(
        "Owned native custom command: retained package installed and launched with preserved data"
    );
}

#[cfg(target_os = "macos")]
async fn ios_evidence(target: &targets::Target, count: usize, marker: Option<&Value>) -> Value {
    let evidence = target
        .app_data("com.example.acme.buildfixture")
        .join("Documents/build-acceptance.json");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut observed = Value::Null;
    let data: Value = loop {
        if let Ok(bytes) = std::fs::read(&evidence) {
            let data: Value = serde_json::from_slice(&bytes).unwrap();
            if data["count"] == count {
                break data;
            }
            observed = data;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Launched fixture did not publish count {count}; last evidence: {observed}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(data["apiUrl"], "https://staging.example.com");
    assert_eq!(data["argument"], true);
    if let Some(previous) = marker {
        assert_eq!(&data["marker"], previous, "Replacement lost app data");
    }
    data["marker"].clone()
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One owned native scenario prepares sources/signing, exercises both configurations and verifies deployment before releasing its resources"
)]
async fn native_owned_gradle_debug_release() {
    let gradle = std::env::var("TESTRUN_TEST_GRADLE").expect("Select cached Gradle 9.3.1");
    let sdk = std::env::var("TESTRUN_TEST_ANDROID_SDK").expect("Select SDK 36");
    let network = std::env::var("TESTRUN_TEST_BUILD_NETWORK").unwrap_or_default();
    assert!(
        matches!(network.as_str(), "" | "1"),
        "Network opt-in must be 1 or absent"
    );
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    std::fs::create_dir_all(root.join("app/src/main/java/com/example/acme/buildfixture")).unwrap();
    for (path, contents) in [
        (
            "settings.gradle",
            include_str!("../fixtures/builds/android-settings.gradle"),
        ),
        (
            "build.gradle",
            include_str!("../fixtures/builds/android-root.gradle"),
        ),
        (
            "app/build.gradle",
            include_str!("../fixtures/builds/android-app.gradle"),
        ),
        (
            "app/src/main/AndroidManifest.xml",
            include_str!("../fixtures/builds/AndroidManifest.xml"),
        ),
        (
            "app/src/main/java/com/example/acme/buildfixture/MainActivity.java",
            include_str!("../fixtures/builds/MainActivity.java"),
        ),
    ] {
        std::fs::write(root.join(path), contents).unwrap();
    }
    let key = tokio::process::Command::new("keytool")
        .args(["-genkeypair", "-storetype", "JKS", "-keystore"])
        .arg(root.join("fixture.keystore"))
        .args([
            "-storepass",
            "acme-fixture",
            "-keypass",
            "acme-fixture",
            "-alias",
            "acme",
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "2",
            "-dname",
            "CN=Acme,OU=QA,O=Acme,L=Example,ST=Example,C=US",
            "-noprompt",
        ])
        .output()
        .await
        .unwrap();
    assert!(
        key.status.success(),
        "{}",
        String::from_utf8_lossy(&key.stderr)
    );
    let wrapper = tokio::time::timeout(
        Duration::from_secs(120),
        tokio::process::Command::new(gradle)
            .args(["--offline", "--no-daemon", "--console=plain", "wrapper"])
            .current_dir(root)
            .env("ANDROID_HOME", &sdk)
            .env("API_URL", "https://staging.example.com")
            .env("ACME_FIXTURE_NETWORK", &network)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        wrapper.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&wrapper.stdout),
        String::from_utf8_lossy(&wrapper.stderr)
    );
    let mut profiles = serde_json::Map::new();
    for (name, task, artifact) in [
        (
            "android-debug",
            "assembleDebug",
            "app/build/outputs/apk/debug/app-debug.apk",
        ),
        (
            "android-release",
            "assembleRelease",
            "app/build/outputs/apk/release/app-release.apk",
        ),
    ] {
        profiles.insert(name.into(),json!({"builder":"gradle","platform":"android","target_kind":"emulator","env":{"ANDROID_HOME":sdk,"API_URL":"https://staging.example.com","ACME_FIXTURE_NETWORK":network},"gradle":{"tasks":[task],"properties":{"android.aapt2FromMavenOverride":Path::new(&sdk).join("build-tools/36.0.0/aapt2")}},"artifact":{"path":artifact}}));
    }
    std::fs::write(
        root.join("testrun.yml"),
        serde_json::to_vec(&json!({"version":1,"builds":profiles})).unwrap(),
    )
    .unwrap();
    let workspace = crate::Workspace::new();
    let owned = if std::env::var("TESTRUN_BUILD_DEPLOY").as_deref() == Ok("1") {
        Some(targets::Target::android(workspace.path(), Path::new(&sdk)))
    } else {
        None
    };
    let _release = Release {
        workspace: &workspace,
        project: root,
    };
    if let Some(target) = &owned {
        ready(&workspace, root, &target.id).await;
    }
    for (index, profile) in ["android-debug", "android-release"].into_iter().enumerate() {
        let mut args = vec!["build", "start", profile];
        if let Some(target) = &owned {
            args.extend([
                "--install",
                "--launch",
                "--replace",
                "--boot",
                "--target",
                &target.id,
            ]);
        }
        let result = cli(&workspace, root, &args).await;
        assert_eq!(result["state"], "completed", "{result}");
        assert_eq!(
            result["result"]["artifact"]["identity"]["appId"],
            "com.example.acme.buildfixture"
        );
        if owned.is_some() {
            cli(
                &workspace,
                root,
                &[
                    "wait",
                    "[{\"kind\":\"foreground\",\"appId\":\"com.example.acme.buildfixture\"}]",
                    "--timeout-ms",
                    "15000",
                ],
            )
            .await;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
            loop {
                let tree = cli(&workspace, root, &["ui", "snapshot"]).await.to_string();
                if tree.contains("https://staging.example.com")
                    && tree.contains(&format!("launches={}", index + 1))
                {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "Expected configured app and retained launch counter: {tree}"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        eprintln!("Owned native {profile}: compiled, signed and identity-verified");
    }
    cli(&workspace, root, &["disconnect"]).await;
}
