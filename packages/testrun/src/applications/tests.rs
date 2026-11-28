//! App deployment fixtures replace only the platform executables, never device ownership.
use super::*;

struct Fixture {
    root: tempfile::TempDir,
    tools: adapter::Tools,
}

#[tokio::test]
async fn inventory_uses_the_same_app_identity_for_simulator_and_physical() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.root.path().join("installed.json"),
        json!({"bundleId":"com.acme.staging","version":"2.0","build":"42"}).to_string(),
    )
    .unwrap();
    let simulator = fixture.tools.inventory("simulator:ACMESIM").await.unwrap();
    let physical = fixture.tools.inventory("device:ACMEUSB").await.unwrap();
    assert_eq!(simulator, physical);
    assert_eq!(physical["apps"][0]["appId"], "com.acme.staging");
    assert_eq!(physical["apps"][0]["build"], "42");
    std::fs::remove_file(fixture.root.path().join("installed.json")).unwrap();
    assert_eq!(
        fixture.tools.inventory("device:ACMEUSB").await.unwrap()["apps"][0]["appId"],
        "com.apple.Preferences"
    );
    assert_eq!(
        fixture
            .tools
            .installed("device:ACMEUSB", "com.acme.staging")
            .await
            .unwrap()
            .map(|identity| identity.build),
        None
    );
    // A physical inventory always includes system apps; an empty one is unreadable, not empty.
    std::fs::write(fixture.root.path().join("unreadable"), b"yes").unwrap();
    let error = fixture.tools.inventory("device:ACMEUSB").await.unwrap_err();
    assert_eq!(error.code, "CAPABILITY_UNAVAILABLE");
    let error = fixture
        .tools
        .installed("device:ACMEUSB", "com.acme.staging")
        .await
        .unwrap_err();
    assert_eq!(error.code, "CAPABILITY_UNAVAILABLE");
    assert!(fixture.mutations().is_empty());
}

#[test]
fn deployment_catalog_defaults_to_the_connected_device_and_requires_write_authority() {
    let tools = crate::catalog::tools();
    for (verb, args) in [
        ("app_install", json!({"path":"/tmp/staging.apk"})),
        ("app_uninstall", json!({"appId":"com.acme.staging"})),
    ] {
        // Without targetId, app tools act on the task's connected device.
        let tool = tools.iter().find(|t| t["name"] == verb).unwrap();
        assert_eq!(tool["annotations"]["readOnlyHint"], false);
        assert_eq!(tool["annotations"]["destructiveHint"], true);
        assert!(
            crate::schema::validate(&tool["inputSchema"], &json!({"arbitrary":"shell"})).is_err()
        );
        crate::schema::validate(&tool["inputSchema"], &args).unwrap();
        let mut explicit = args;
        explicit["targetId"] = json!("android:Acme");
        crate::schema::validate(&tool["inputSchema"], &explicit).unwrap();
    }
}
impl Fixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let helper = r#"#!/usr/bin/env -S python3 -I -S
import sys,json,pathlib,plistlib,shlex,time,shutil,hashlib
r=pathlib.Path(__file__).parent
a=sys.argv[1:];tool=pathlib.Path(__file__).name
with (r/'calls.jsonl').open('a') as f:f.write(json.dumps([tool,a])+'\n')
state=r/'installed.json'
app=json.loads(state.read_text()) if state.exists() else None
def mutate(install,path=None):
 if (r/'pause').exists():
  (r/'entered').write_text('yes')
  while (r/'pause').exists():time.sleep(.01)
 if (r/'fail').exists():print('signature mismatch',file=sys.stderr);sys.exit(1)
 if install:
  p=pathlib.Path(path)
  if p.is_dir():
   v=plistlib.loads((p/'Info.plist').read_bytes());v={'bundleId':v['CFBundleIdentifier'],'version':v['CFBundleShortVersionString'],'build':v['CFBundleVersion']}
   if (r/'minimal-inventory').exists():shutil.copytree(p,r/'Installed.app',dirs_exist_ok=True)
  else:v=json.loads(p.read_text());shutil.copyfile(p,r/'base.apk')
  state.write_text(json.dumps(v))
  if (r/'fail-after').exists():sys.exit(1)
 elif state.exists():state.unlink()
if tool=='aapt2':
 v=json.loads(pathlib.Path(a[-1]).read_text());print("package: name='%s' versionCode='%s' versionName='%s'"%(v['bundleId'],v['build'],v['version']))
elif tool=='adb':
 if a==['devices','-l']:print('List of devices attached\nAcme device model:Acme transport_id:7')
 elif a[:2]!=['-s','Acme']:sys.exit(3)
 elif a[2]=='install':
  if (r/'protect').exists():
   (r/'focus-read').unlink(missing_ok=True);end=time.time()+10
   while not (r/'focus-read').exists() and time.time()<end:time.sleep(.02)
   (r/'prompt').write_text('yes');end=time.time()+10
   while (r/'prompt').exists():
    if time.time()>end:sys.exit(9)
    time.sleep(.02)
  mutate(True,a[-1]);print('Success')
 elif a[2]=='uninstall':mutate(False);print('Success')
 elif a[2:4]==['shell','-T']:
  sys.stdin.buffer.read()
  print('<hierarchy window-id="1"><node class="android.widget.FrameLayout" text="" content-desc="" clickable="false" bounds="[0,0][1080,2280]"><node class="android.widget.TextView" text="Send app for security check?" content-desc="" clickable="false" bounds="[100,1000][1000,1100]"/><node class="android.widget.TextView" text="Don&#8217;t send" content-desc="" clickable="true" bounds="[100,1500][500,1600]"/><node class="android.widget.Button" text="Send" content-desc="" clickable="true" bounds="[600,1500][1000,1600]"/></node></hierarchy>')
 elif a[2]=='shell':
  c=shlex.split(a[3])
  if c[:4]==['cmd','package','list','packages']:
   if app:print('package:'+app['bundleId'])
  elif c[:2]==['dumpsys','package'] and app:print('versionCode='+app['build']+' minSdk=21\nversionName='+app['version'])
  elif c[:2]==['dumpsys','window']:print('  mCurrentFocus=Window{1a u0 '+('com.android.vending/com.google.android.finsky.verifier.impl.ConsentDialog' if (r/'prompt').exists() else 'com.example.home/.Main')+'}',flush=True);(r/'focus-read').touch()
  elif c[:2]==['input','tap']:
   with (r/'taps.jsonl').open('a') as f:f.write(json.dumps(c[2:])+'\n')
   (r/'prompt').unlink(missing_ok=True)
  elif c[:2]==['pm','path']:
   if app:print('package:/data/app/~~acme/'+c[2]+'/base.apk')
  elif c[0]=='sha256sum':print(hashlib.sha256((r/'base.apk').read_bytes()).hexdigest()+'  '+c[1])
  else:sys.exit(4)
 else:sys.exit(5)
elif tool=='xcrun':
 if a[:4]==['simctl','list','devices','--json']:print(json.dumps({'devices':{'iOS':[{'udid':'ACMESIM','state':'Booted'}]}}))
 elif a[:2]==['simctl','listapps']:
  apps={'com.apple.Preferences':{'CFBundleDisplayName':'Settings','CFBundleShortVersionString':'27.0','CFBundleVersion':'1'}}
  if app:apps[app['bundleId']]={'CFBundleShortVersionString':app['version'],'CFBundleVersion':app['build']}
  if app and (r/'minimal-inventory').exists():apps[app['bundleId']]={'CFBundleVersion':int(app['build'])}
  print(plistlib.dumps(apps).decode())
 elif a[:2]==['simctl','get_app_container']:print(r/'Installed.app')
 elif a[0]=='simctl':mutate(a[1]=='install',a[-1] if a[1]=='install' else None)
 elif a[0]=='devicectl':
  result={}
  if a[1:3]==['list','devices']:result={'devices':[{'hardwareProperties':{'udid':'ACMEUSB','platform':'iOS'},'connectionProperties':{'transportType':'wired'}}]}
  elif a[1:4]==['device','info','apps']:
   apps=[] if '--bundle-id' in a else [{'bundleIdentifier':'com.apple.Preferences','name':'Settings','version':'27.0','bundleVersion':'1'}]
   if app:apps.append({'bundleIdentifier':app['bundleId'],'version':app['version'],'bundleVersion':app['build']})
   result={'apps':[] if (r/'unreadable').exists() else apps}
  elif a[1] in ['device'] and a[2] in ['install','uninstall']:mutate(a[2]=='install',a[6] if a[2]=='install' else None)
  else:sys.exit(6)
  pathlib.Path(a[a.index('--json-output')+1]).write_text(json.dumps({'info':{'outcome':'success'},'result':result}))
 else:sys.exit(7)
"#;
        for tool in ["adb", "xcrun", "aapt2"] {
            let path = root.path().join(tool);
            std::fs::write(&path, helper).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let tools = adapter::Tools {
            native_inventory: false,
            xcrun: root.path().join("xcrun"),
            adb: root.path().join("adb"),
            aapt2: Some(root.path().join("aapt2")),
            play_protect: super::play_protect::Cadence {
                delay: std::time::Duration::ZERO,
                interval: std::time::Duration::from_millis(20),
            },
        };
        Self { root, tools }
    }
    fn package(&self, target: &str, version: &str, build: &str) -> PathBuf {
        if target.starts_with("android:") {
            let path = self.root.path().join("staging build.apk");
            std::fs::write(
                &path,
                json!({"bundleId":"com.acme.staging","version":version,"build":build}).to_string(),
            )
            .unwrap();
            path
        } else {
            let path = self.root.path().join("Staging build.app");
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("Staging"), b"Acme executable").unwrap();
            let value = plist::Value::Dictionary(plist::Dictionary::from_iter([
                (
                    "CFBundleIdentifier",
                    plist::Value::String("com.acme.staging".into()),
                ),
                ("CFBundleExecutable", plist::Value::String("Staging".into())),
                (
                    "CFBundleShortVersionString",
                    plist::Value::String(version.into()),
                ),
                ("CFBundleVersion", plist::Value::String(build.into())),
                (
                    "CFBundleSupportedPlatforms",
                    plist::Value::Array(vec![plist::Value::String(
                        if target.starts_with("simulator:") {
                            "iPhoneSimulator"
                        } else {
                            "iPhoneOS"
                        }
                        .into(),
                    )]),
                ),
            ]));
            value.to_file_xml(path.join("Info.plist")).unwrap();
            path
        }
    }
    fn request(&self, target: &str, path: Option<PathBuf>, replace: bool) -> Request {
        Request {
            target_id: target.into(),
            bundle_id: path.is_none().then(|| "com.acme.staging".into()),
            path,
            expected_sha256: None,
            replace,
            play_protect: super::play_protect::Answer::Fail,
        }
    }
    fn taps(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.path().join("taps.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
    async fn call(&self, id: &str, operation: &str, request: Request) -> Result<Value> {
        execute_with(
            &self.root.path().join("records"),
            id,
            operation,
            request,
            &self.tools,
        )
        .await
    }
    fn mutations(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.path().join("calls.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .filter(|v| {
                v[1].as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a == "install" || a == "uninstall")
            })
            .collect()
    }
}

#[tokio::test]
async fn simulator_install_verifies_the_bundle_when_inventory_omits_version_strings() {
    let fixture = Fixture::new();
    std::fs::write(fixture.root.path().join("minimal-inventory"), b"yes").unwrap();
    let path = fixture.package("simulator:ACMESIM", "1.2", "42");
    let result = fixture
        .call(
            "app-minimal-install",
            "install",
            fixture.request("simulator:ACMESIM", Some(path), false),
        )
        .await
        .unwrap();
    assert_eq!(result["version"], "1.2");
    assert_eq!(result["build"], "42");
    assert_eq!(result["buildVerified"], true);
    assert_eq!(fixture.mutations().len(), 1);
}

#[tokio::test]
async fn install_update_uninstall_verify_identity_and_deduplicate_on_all_adapters() {
    for target in ["android:Acme", "simulator:ACMESIM", "device:ACMEUSB"] {
        let fixture = Fixture::new();
        let path = fixture.package(target, "1.2", "42");
        let request = fixture.request(target, Some(path.clone()), false);
        let installed = fixture
            .call("app-install", "install", request.clone())
            .await
            .unwrap();
        assert_eq!(installed["appId"], "com.acme.staging");
        assert_eq!(installed["build"], "42");
        assert_eq!(installed["verified"], true);
        assert_eq!(installed["launched"], false);
        assert_eq!(
            fixture
                .call("app-install", "install", request.clone())
                .await
                .unwrap(),
            installed
        );
        assert_eq!(
            fixture
                .call("app-refuse", "install", request)
                .await
                .unwrap_err()
                .code,
            "APP_ALREADY_INSTALLED"
        );
        assert_eq!(fixture.mutations().len(), 1);
        let path = fixture.package(target, "2.0", "43");
        let updated = fixture
            .call(
                "app-update",
                "install",
                fixture.request(target, Some(path), true),
            )
            .await
            .unwrap();
        assert_eq!(updated["updated"], true);
        assert_eq!(updated["build"], "43");
        if target.starts_with("android:") {
            let calls = fixture.mutations();
            let args = calls[1][1].as_array().unwrap();
            assert!(args.contains(&json!("--no-streaming")));
            assert!(args.contains(&json!("-r")));
            assert!(!args.contains(&json!("-g")));
            assert!(!args.contains(&json!("-d")));
        }
        assert_eq!(
            fixture
                .call(
                    "app-uninstall",
                    "uninstall",
                    fixture.request(target, None, false)
                )
                .await
                .unwrap()["removed"],
            true
        );
        assert_eq!(
            fixture
                .call(
                    "app-absent",
                    "uninstall",
                    fixture.request(target, None, false)
                )
                .await
                .unwrap()["changed"],
            false
        );
        assert_eq!(fixture.mutations().len(), 3);
        assert_eq!(
            last(&fixture.root.path().join("records")).unwrap()["status"],
            "completed"
        );
    }
}
#[tokio::test]
async fn package_failures_and_wrong_platform_never_dispatch() {
    let fixture = Fixture::new();
    let path = fixture.package("simulator:ACMESIM", "1.0", "1");
    let error = fixture
        .call(
            "app-wrong",
            "install",
            fixture.request("device:ACMEUSB", Some(path), false),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "PACKAGE_PLATFORM_MISMATCH");
    let error = fixture
        .call(
            "app-path",
            "install",
            fixture.request("android:Acme", Some("relative.apk".into()), false),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "INVALID_ARGUMENT");
    assert!(fixture.mutations().is_empty());
    for target in ["android:Acme", "device:ACMEUSB", "simulator:Acme"] {
        validate(target).unwrap();
    }
    for target in ["Acme", "android:", "browser:Acme"] {
        assert_eq!(validate(target).unwrap_err().code, "INVALID_ARGUMENT");
    }
}
#[tokio::test]
async fn rejection_and_interruption_remain_queryable_without_automatic_replay() {
    let fixture = std::sync::Arc::new(Fixture::new());
    let target = "android:Acme";
    let path = fixture.package(target, "1.0", "1");
    let request = fixture.request(target, Some(path), false);
    std::fs::write(fixture.root.path().join("fail"), b"yes").unwrap();
    let error = fixture
        .call("app-failed", "install", request.clone())
        .await
        .unwrap_err();
    assert_eq!(error.code, "APP_OUTCOME_UNKNOWN");
    assert!(
        serde_json::to_string(&error)
            .unwrap()
            .contains("signature mismatch")
    );
    fixture
        .call("app-failed", "install", request.clone())
        .await
        .unwrap_err();
    assert_eq!(fixture.mutations().len(), 1);
    std::fs::remove_file(fixture.root.path().join("fail")).unwrap();
    std::fs::write(fixture.root.path().join("pause"), b"yes").unwrap();
    let worker = fixture.clone();
    let pending_request = request.clone();
    let mut operation = tokio::spawn(async move {
        worker
            .call("app-interrupted", "install", pending_request)
            .await
    });
    let entered = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            tokio::select! {
                result = &mut operation => panic!("Expected a pending operation: {result:?}"),
                _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                    if fixture.root.path().join("entered").exists() { break; }
                }
            }
        }
    })
    .await;
    if let Err(error) = entered {
        operation.abort();
        let _ = operation.await;
        panic!(
            "Mutation did not enter pause: {error}; calls: {}; status: {:?}",
            std::fs::read_to_string(fixture.root.path().join("calls.jsonl")).unwrap_or_default(),
            status(&fixture.root.path().join("records"), "app-interrupted")
        );
    }
    operation.abort();
    assert!(operation.await.unwrap_err().is_cancelled());
    assert_eq!(
        status(&fixture.root.path().join("records"), "app-interrupted").unwrap()["status"],
        "outcome_unknown"
    );
    assert_eq!(
        status(&fixture.root.path().join("records"), "app-interrupted").unwrap()["dispatched"],
        true
    );
    fixture
        .call("app-interrupted", "install", request)
        .await
        .unwrap_err();
    assert_eq!(fixture.mutations().len(), 2);
}
#[test]
fn snapshots_are_private_stable_and_reject_escaping_links() {
    let fixture = Fixture::new();
    let path = fixture.package("android:Acme", "1.0", "1");
    let snapshot = package::Snapshot::create(&path).unwrap();
    let bytes = std::fs::read(&snapshot.path).unwrap();
    {
        use sha2::{Digest, Sha256};
        assert_eq!(snapshot.sha256, format!("{:x}", Sha256::digest(&bytes)));
    }
    std::fs::write(&path, b"modified").unwrap();
    assert_eq!(std::fs::read(&snapshot.path).unwrap(), bytes);
    let owned = snapshot.path.parent().unwrap().to_path_buf();
    drop(snapshot);
    assert!(!owned.exists());
    let app = fixture.package("simulator:ACMESIM", "1.0", "1");
    std::os::unix::fs::symlink(&path, app.join("outside")).unwrap();
    assert!(package::Snapshot::create(&app).is_err());
}
#[tokio::test]
async fn play_protect_prompt_stops_the_install_unless_an_answer_was_chosen() {
    let fixture = Fixture::new();
    let target = "android:Acme";
    std::fs::write(fixture.root.path().join("protect"), b"yes").unwrap();
    let path = fixture.package(target, "1.0", "1");
    let error = fixture
        .call(
            "app-prompted",
            "install",
            fixture.request(target, Some(path.clone()), false),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "INSTALL_AWAITING_PLAY_PROTECT");
    let details = error.details.unwrap();
    let labels: Vec<_> = details["buttons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["label"].clone())
        .collect();
    assert_eq!(labels, [json!("Don\u{2019}t send"), json!("Send")]);
    assert_eq!(details["outcome"], "unknown");
    assert_eq!(details["reconciliation"]["targetBuildInstalled"], false);
    assert!(
        fixture.taps().is_empty(),
        "The default answer never touches the prompt"
    );
    assert_eq!(
        status(&fixture.root.path().join("records"), "app-prompted").unwrap()["status"],
        "outcome_unknown"
    );

    // A fresh device: the unanswered install's adb may still be waiting on its
    // prompt and could otherwise finish installing before this call starts.
    let fixture = Fixture::new();
    std::fs::write(fixture.root.path().join("protect"), b"yes").unwrap();
    let path = fixture.package(target, "1.0", "1");
    let mut request = fixture.request(target, Some(path), false);
    request.play_protect = super::play_protect::Answer::DontSend;
    let installed = fixture
        .call("app-answered", "install", request)
        .await
        .unwrap();
    assert_eq!(installed["verified"], true);
    assert_eq!(
        installed["playProtect"]["answers"][0]["answer"],
        "dont_send"
    );
    assert_eq!(
        installed["playProtect"]["answers"][0]["label"],
        "Don\u{2019}t send"
    );
    assert_eq!(fixture.taps(), [json!(["300", "1550"])]);
}
#[tokio::test]
async fn an_uncertain_android_install_reports_whether_the_submitted_apk_is_installed() {
    let fixture = Fixture::new();
    let target = "android:Acme";
    std::fs::write(fixture.root.path().join("fail-after"), b"yes").unwrap();
    let path = fixture.package(target, "1.0", "1");
    let error = fixture
        .call(
            "app-uncertain",
            "install",
            fixture.request(target, Some(path), false),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "APP_OUTCOME_UNKNOWN");
    let reconciliation = &error.details.as_ref().unwrap()["reconciliation"];
    assert_eq!(
        reconciliation["targetBuildInstalled"], true,
        "{reconciliation}"
    );
    assert_eq!(
        reconciliation["installedSha256"],
        reconciliation["submittedSha256"]
    );
    let record = inspect_with(
        &fixture.root.path().join("records"),
        "app-uncertain",
        &fixture.tools,
    )
    .await
    .unwrap();
    assert_eq!(record["status"], "outcome_unknown");
    assert_eq!(record["reconciliation"]["targetBuildInstalled"], true);
}
