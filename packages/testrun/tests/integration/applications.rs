//! Published app lifecycle commands against an owned adb/SDK fixture, including a live controller.
use serde_json::{Value, json};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::process::Command;

struct Fixture {
    root: crate::Workspace,
    task: String,
    adb: PathBuf,
    aapt: PathBuf,
    target: String,
}
impl Fixture {
    fn build_project(&self) {
        let sdk = self.root.path().join("sdk/build-tools/36.0.0");
        std::fs::create_dir_all(&sdk).unwrap();
        std::fs::copy(&self.aapt, sdk.join("aapt2")).unwrap();
        std::fs::copy(self.root.path().join("serial"), sdk.join("serial")).unwrap();
        std::fs::write(self.root.path().join("build.py"),"import os,pathlib,json\nr=pathlib.Path(__file__).parent\nassert os.environ['API_URL']=='https://staging.example.com'\n(r/'output.apk').write_text(json.dumps({'version':'1.0','build':'1'}))\np=r/'build-count'\np.write_text((p.read_text() if p.exists() else '')+'once')\n").unwrap();
        std::fs::write(self.root.path().join("testrun.yml"),format!("version: 1\nbuilds:\n  staging:\n    builder: custom\n    platform: android\n    target_kind: device\n    env:\n      ANDROID_HOME: {}\n      API_URL: https://staging.example.com\n    custom:\n      command: [python3, build.py]\n    artifact:\n      path: output.apk\n",self.root.path().join("sdk").display())).unwrap();
    }
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = crate::Workspace::new();
        let serial = format!("Acme-{}", uuid::Uuid::new_v4().simple());
        std::fs::write(root.path().join("serial"), &serial).unwrap();
        image::RgbImage::new(100, 200)
            .save(root.path().join("screen.png"))
            .unwrap();
        let script = r#"#!/usr/bin/env python3
import sys,pathlib,json,shlex
r=pathlib.Path(__file__).parent;a=sys.argv[1:]
serial=(r/'serial').read_text()
with (r/'calls.jsonl').open('a') as f:f.write(json.dumps(a)+'\n')
p=r/'package.json';app=json.loads(p.read_text()) if p.exists() else None
if pathlib.Path(__file__).name=='aapt2':
 v=json.loads(pathlib.Path(a[-1]).read_text());print("package: name='com.acme.staging' versionCode='%s' versionName='%s'"%(v['build'],v['version']))
elif a==['devices','-l']:print('List of devices attached\n'+serial+' device model:Acme transport_id:7')
elif a[:2]!=['-s',serial]:sys.exit(2)
elif a[2]=='exec-out':sys.stdout.buffer.write((r/'screen.png').read_bytes())
elif a[2]=='install':p.write_text(pathlib.Path(a[-1]).read_text());print('Success')
elif a[2]=='uninstall':p.unlink();print('Success')
elif a[2]=='shell':
 c=shlex.split(a[3])
 if c[:4]==['cmd','package','list','packages']:
  if app:print('package:com.acme.staging')
 elif c[:2]==['dumpsys','package'] and app:print('versionCode='+app['build']+' minSdk=21\nversionName='+app['version'])
 elif c[:3]==['cmd','package','resolve-activity']:print('com.acme.staging/.MainActivity')
 elif c[:2]==['am','start']:print('Status: ok')
 elif c[:1]==['input']:pass
 else:sys.exit(3)
else:sys.exit(4)
"#;
        let adb = root.path().join("adb");
        let aapt = root.path().join("aapt2");
        for path in [&adb, &aapt] {
            std::fs::write(path, script).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self {
            root,
            task: format!("app-fixture-{}", uuid::Uuid::new_v4()),
            adb,
            aapt,
            target: format!("android:{serial}"),
        }
    }
    fn package(&self, version: &str, build: &str) -> String {
        let path = self.root.path().join("staging after.apk");
        std::fs::write(&path, json!({"version":version,"build":build}).to_string()).unwrap();
        path.to_str().unwrap().into()
    }
    async fn raw(&self, args: &[&str], task: &str) -> std::process::Output {
        tokio::time::timeout(
            Duration::from_secs(30),
            Command::new(env!("CARGO_BIN_EXE_testrun"))
                .arg("--json")
                .args(args)
                .env("TESTRUN_DATA_ROOT", self.root.data_root())
                .env_remove("CODEX_THREAD_ID")
                .env("TESTRUN_TASK_ID", task)
                .env("TESTRUN_ADB", &self.adb)
                .env("ANDROID_AVD_HOME", crate::NO_AVDS)
                .env("TESTRUN_AAPT2", &self.aapt)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap()
    }
    async fn call(&self, args: &[&str]) -> Value {
        let output = self.raw(args, &self.task).await;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

struct Mcp {
    child: tokio::process::Child,
    input: tokio::process::ChildStdin,
    output: tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>,
    next: u64,
}
impl Mcp {
    async fn start(fixture: &Fixture) -> Self {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let mut child = Command::new(env!("CARGO_BIN_EXE_testrun"))
            .args(["--json", "mcp", "serve"])
            .env("TESTRUN_DATA_ROOT", fixture.root.data_root())
            .env_remove("CODEX_THREAD_ID")
            .env("TESTRUN_TASK_ID", &fixture.task)
            .env("TESTRUN_ADB", &fixture.adb)
            .env("TESTRUN_AAPT2", &fixture.aapt)
            .env("ANDROID_AVD_HOME", crate::NO_AVDS)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
        let mut connection = Self {
            child,
            input,
            output,
            next: 2,
        };
        let initialized=connection.exchange(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme build fixture","version":"1"}}})).await;
        assert!(initialized.get("error").is_none(), "{initialized}");
        connection
            .input
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .unwrap();
        connection
    }
    async fn exchange(&mut self, request: Value) -> Value {
        use tokio::io::AsyncWriteExt;
        self.input
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        loop {
            let line = tokio::time::timeout(Duration::from_secs(15), self.output.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["id"] == request["id"] {
                return response;
            }
        }
    }
    async fn tool(&mut self, name: &str, args: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let response=self.exchange(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}})).await;
        assert!(response.get("error").is_none(), "{response}");
        response["result"].clone()
    }
    async fn data(&mut self, name: &str, args: Value) -> Value {
        let result = self.tool(name, args).await;
        assert_eq!(result["isError"], false, "{result}");
        result["structuredContent"]["data"].clone()
    }
    async fn settled(&mut self, id: &str) -> Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            let record = self.data("build_status", json!({"operationId":id})).await;
            if record["settled"] == true {
                return record;
            }
            assert!(tokio::time::Instant::now() < deadline, "{record}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    async fn close(mut self) {
        drop(self.input);
        tokio::time::timeout(Duration::from_secs(10), self.child.wait())
            .await
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn mcp_build_flow_recovers_lost_reply_replays_after_yaml_change_and_cancels() {
    let fixture = Fixture::new();
    fixture.build_project();
    let mut mcp = Mcp::start(&fixture).await;
    let args = json!({"project":fixture.root.path(),"profile":"staging","submissionId":"acme-staging","baseline":{"PATH":std::env::var("PATH").unwrap()},"install":true,"launch":true,"targetId":fixture.target});
    // Deliberately recover through the caller's stable key rather than the submission reply.
    mcp.data("build_start", args.clone()).await;
    let recovered = mcp
        .data("build_status", json!({"submissionId":"acme-staging"}))
        .await;
    let id = recovered["operationId"].as_str().unwrap().to_owned();
    let completed = mcp.settled(&id).await;
    assert_eq!(completed["state"], "completed", "{completed}");
    assert_eq!(completed["stages"]["launch"]["state"], "completed");
    std::fs::write(fixture.root.path().join("testrun.yml"), "invalid: [").unwrap();
    let replay = mcp.data("build_start", args).await;
    assert_eq!(replay["operationId"], id);
    assert_eq!(replay["state"], "completed");
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("build-count")).unwrap(),
        "once"
    );
    let history = mcp.data("build_status", json!({})).await;
    assert_eq!(history["operations"].as_array().unwrap().len(), 1);
    let submitted=mcp.data("build_start",json!({"project":fixture.root.path(),"submissionId":"acme-cancel","baseline":{"PATH":"/usr/bin:/bin"},"inline":{"builder":"custom","platform":"android","target_kind":"device","custom":{"command":["/usr/bin/python3","-c","import pathlib,time; pathlib.Path('cancel.started').write_text('ready'); time.sleep(30)"]},"artifact":{"path":"never.apk"}}})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !fixture.root.path().join("cancel.started").exists() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    mcp.data("build_cancel", json!({"submissionId":"acme-cancel"}))
        .await;
    let cancelled = mcp
        .settled(submitted["operationId"].as_str().unwrap())
        .await;
    assert_eq!(cancelled["state"], "cancelled", "{cancelled}");
    mcp.data("disconnect", json!({})).await;
    mcp.close().await;
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::process::Command::new(env!("CARGO_BIN_EXE_testrun"))
            .arg("--json")
            .arg("disconnect")
            .env("TESTRUN_DATA_ROOT", self.root.data_root())
            .env_remove("CODEX_THREAD_ID")
            .env("TESTRUN_TASK_ID", &self.task)
            .env("TESTRUN_ADB", &self.adb)
            .env("ANDROID_AVD_HOME", crate::NO_AVDS)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[tokio::test]
async fn test_build_install_launch_flow_retains_stages_and_reuses_artifact() {
    let fixture = Fixture::new();
    fixture.build_project();
    let project = fixture.root.path().to_str().unwrap();
    let args = [
        "build",
        "start",
        "staging",
        "--submission-id",
        "acme-cli-build",
        "--project",
        project,
        "--install",
        "--launch",
        "--target",
        &fixture.target,
    ];
    let operation = fixture.call(&args).await;
    assert_eq!(operation["state"], "completed", "{operation}");
    for stage in ["build", "artifact", "connect", "install", "launch"] {
        assert_eq!(
            operation["stages"][stage]["state"], "completed",
            "{operation}"
        );
    }
    let id = operation["operationId"].as_str().unwrap();
    std::fs::write(fixture.root.path().join("testrun.yml"), "invalid: [").unwrap();
    assert_eq!(fixture.call(&args).await["operationId"], id);
    assert_eq!(
        fixture
            .call(&["build", "status", "--submission-id", "acme-cli-build"])
            .await["operationId"],
        id
    );
    assert_eq!(
        fixture.call(&["build", "status", id]).await["state"],
        "completed"
    );
    let artifact = operation["result"]["artifact"]["reference"]
        .as_str()
        .unwrap();
    let installed = fixture
        .call(&[
            "app",
            "install",
            "--artifact-ref",
            artifact,
            "--replace",
            "--target",
            &fixture.target,
        ])
        .await;
    assert_eq!(installed["verified"], true);
    let retained = operation["result"]["artifact"]["path"].as_str().unwrap();
    std::fs::write(retained, b"changed retained bytes").unwrap();
    let refused = fixture
        .raw(
            &[
                "app",
                "install",
                "--artifact-ref",
                artifact,
                "--replace",
                "--target",
                &fixture.target,
            ],
            &fixture.task,
        )
        .await;
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("BUILD_ARTIFACT_CHANGED"));
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("build-count")).unwrap(),
        "once"
    );
    fixture.call(&["disconnect"]).await;
}

#[tokio::test]
async fn published_mcp_install_deduplicates_and_status_observes_without_replay() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let fixture = Fixture::new();
    let package = fixture.package("1.0", "1");
    let mut child = Command::new(env!("CARGO_BIN_EXE_testrun"))
        .arg("--json")
        .args(["mcp", "serve"])
        .env("TESTRUN_DATA_ROOT", fixture.root.data_root())
        .env_remove("CODEX_THREAD_ID")
        .env("TESTRUN_TASK_ID", &fixture.task)
        .env("TESTRUN_ADB", &fixture.adb)
        .env("ANDROID_AVD_HOME", crate::NO_AVDS)
        .env("TESTRUN_AAPT2", &fixture.aapt)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    let install = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"app_install","arguments":{"targetId":fixture.target,"path":package}}});
    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme deployment fixture","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        install.clone(),
        install,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"status","arguments":{}}}),
    ];
    let mut installed: Option<Value> = None;
    for mut request in requests {
        if request["id"] == 3 {
            request["params"]["arguments"] =
                json!({"actionId":installed.as_ref().unwrap()["actionId"]});
        }
        input
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        if request.get("id").is_none() {
            continue;
        }
        let response: Value = serde_json::from_str(
            &tokio::time::timeout(Duration::from_secs(15), output.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert!(response.get("error").is_none(), "{response}");
        if response["id"] == 2 {
            assert_eq!(response["result"]["isError"], false, "{response}");
            let data = &response["result"]["structuredContent"]["data"];
            if let Some(previous) = &installed {
                assert_eq!(data, previous);
            } else {
                installed = Some(data.clone());
            }
        }
        if response["id"] == 3 {
            assert_eq!(
                response["result"]["structuredContent"]["data"]["appOperation"]["status"],
                "completed"
            );
        }
    }
    drop(input);
    tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .unwrap()
        .unwrap();
    let calls = std::fs::read_to_string(fixture.root.path().join("calls.jsonl")).unwrap();
    assert_eq!(
        calls
            .lines()
            .filter(|line| serde_json::from_str::<Value>(line).unwrap()[2] == "install")
            .count(),
        1
    );
}
#[tokio::test]
async fn app_commands_work_before_connect_and_through_the_connected_device_for_every_agent() {
    let fixture = Fixture::new();
    let path = fixture.package("1.0", "1");
    let installed = fixture
        .call(&["app", "install", "--target", &fixture.target, &path])
        .await;
    assert_eq!(installed["verified"], true);
    assert_eq!(installed["updated"], false);
    let reference = installed["actionId"].as_str().unwrap();
    let status = fixture.call(&["status", "--action-id", reference]).await;
    assert_eq!(status["appOperation"]["status"], "completed");
    assert_eq!(
        fixture
            .call(&["connect", "--target", &fixture.target])
            .await["state"],
        "ready"
    );
    let path = fixture.package("2.0", "2");
    let updated = fixture.call(&["app", "install", "--replace", &path]).await;
    assert_eq!(updated["updated"], true);
    assert_eq!(updated["build"], "2");
    // Another agent's app operation runs through the same connected device.
    let other = fixture
        .raw(
            &[
                "app",
                "uninstall",
                "--target",
                &fixture.target,
                "com.acme.staging",
            ],
            &format!("{}-other", fixture.task),
        )
        .await;
    assert!(
        other.status.success(),
        "{}",
        String::from_utf8_lossy(&other.stderr)
    );
    let removed: Value = serde_json::from_slice(&other.stdout).unwrap();
    assert_eq!(removed["removed"], true, "{removed}");
    assert_eq!(
        fixture.call(&["status", "--target", &fixture.target]).await["state"],
        "ready"
    );
    assert_eq!(fixture.call(&["observe"]).await["state"], "ready");
    let calls = std::fs::read_to_string(fixture.root.path().join("calls.jsonl")).unwrap();
    let mutations: Vec<Value> = calls
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|a| matches!(a[2].as_str(), Some("install" | "uninstall")))
        .collect();
    assert_eq!(mutations.len(), 3);
    assert!(mutations[1].as_array().unwrap().contains(&json!("-r")));
    assert_eq!(fixture.call(&["disconnect"]).await["closed"], true);
}
