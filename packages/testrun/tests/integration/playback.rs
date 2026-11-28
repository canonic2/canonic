//! Published media workflows against an owned adb/helper protocol fixture, without hardware.
use serde_json::{Value, json};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

struct Fixture {
    root: crate::Workspace,
    adb: PathBuf,
    task: String,
    target: String,
}
impl Fixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = crate::Workspace::new();
        image::RgbImage::new(100, 200)
            .save(root.path().join("screen.png"))
            .unwrap();
        let serial = format!("Acme-{}", uuid::Uuid::new_v4().simple());
        std::fs::write(root.path().join("serial"), &serial).unwrap();
        let adb = root.path().join("adb");
        std::fs::write(&adb,r#"#!/usr/bin/env python3
import sys,pathlib,json,shlex,os,time,hashlib,base64
r=pathlib.Path(__file__).parent;a=sys.argv[1:];serial=(r/'serial').read_text()
def log(name,value):
 with (r/name).open('a') as f:f.write(json.dumps(value)+'\n')
log('calls',a)
if a==['devices','-l']:print('List of devices attached\n'+serial+' device model:Acme transport_id:7');sys.exit()
if a[:2]!=['-s',serial]:sys.exit(2)
if a[2]=='exec-out':sys.stdout.buffer.write((r/'screen.png').read_bytes());sys.exit()
if a[2]=='push':
 if (r/'bad_push').exists():sys.exit(8)
 assert pathlib.Path(a[3]).read_bytes().startswith(b'dex\n')
 (r/'digest').write_text(hashlib.sha256(pathlib.Path(a[3]).read_bytes()).hexdigest())
 sys.exit()
if a[2]!='shell':sys.exit(3)
if a[3]=='-T':
 (r/'pid').write_text(str(os.getpid()))
 print(json.dumps({'version':2 if (r/'wrong_version').exists() else 1,'ready':True,'uid':2000,'pid':os.getpid(),'helperEpoch':'fixture'}),flush=True)
 for line in sys.stdin:
  q=json.loads(line);op=q['operation'];args=q.get('arguments',{});reply={'version':1,'id':q['id'],'ok':True}
  log('requests',q)
  if op=='quit':reply['data']={'closed':True};print(json.dumps(reply),flush=True);break
  error=None;ref=args.get('sessionRef',args.get('volumeTarget',{}).get('sessionRef'))
  if ref and ref not in ['fixture:a','fixture:b']:error='MEDIA_SESSION_STALE'
  if (r/'refuse').exists() and op=='media_control':error='MEDIA_UNSUPPORTED'
  if args.get('value',0)>15:error='INVALID_ARGUMENT'
  if error:
   reply.update(ok=False,error={'code':error,'message':'Fixture preflight refusal','dispatched':False})
  elif q.get('prepare'):reply['data']={'before':{'state':'playing'}}
  elif op=='media_state':reply['data']={'source':'android_media_controller','helperEpoch':'fixture','sessions':[{'sessionRef':x,'packageName':'com.example.acme','tag':'duplicate','reportedPlayback':{'state':'playing','positionMs':123,'positionSource':'service_extrapolated'}} for x in ['fixture:a','fixture:b']],'count':2,'partial':False}
  elif op=='volume_get':reply['data']={'source':'android_audio_manager','volumeTarget':args['volumeTarget'],'min':0,'max':15,'current':5,'muted':False}
  elif op=='audio_probe':
   log('dispatches',{'operation':op,'arguments':args})
   if (r/'probe_block').exists():
    (r/'probe_started').write_text('owned')
    while (r/'probe_block').exists():time.sleep(0.01)
   if (r/'probe_error_payload').exists():reply.update(ok=False,error={'code':'AUDIO_PROBE_FAILED','message':'Acme recorder read failed','dispatched':True,'details':{'phase':'read','cleanupVerified':True,'pcmBase64':base64.b64encode(bytes(args.get('durationMs',2000)*48*4)).decode(),'partial':{'sampleCount':0,'pcmBase64':'private-payload','health':{'reads':0,'extra':'private-payload'}}}})
   elif (r/'probe_failure').exists():reply.update(ok=False,error={'code':'AUDIO_PROBE_FAILED','message':'Acme recorder initialization failed','dispatched':True,'details':{'phase':'initialization','cleanupVerified':True,'partial':{'sampleCount':0,'digitalZero':False}}})
   else:
    n=args.get('durationMs',2000)*48
    reply['data']={'backend':'android_remote_submix','helperEpoch':'fixture','sampleCount':n*2,'frameCount':n,'decodedDurationMs':args.get('durationMs',2000),'sampleRate':48000,'channels':2,'digitalZero':True,'rmsDbfs':None,'peakDbfs':None,'cleanupVerified':True,'observed':True}
    if args.get('retainClip'):reply['data']['pcmBase64']=base64.b64encode(bytes(n*4)).decode()
    if (r/'probe_bad_count').exists():reply['data']['sampleCount']=0
    if (r/'probe_unrequested_clip').exists():reply['data']['pcmBase64']=base64.b64encode(bytes(4)).decode()
  else:
   log('dispatches',{'operation':op,'arguments':args})
   if (r/'crash').exists():os._exit(8)
   reply['data']={'source':'android_media_controller','dispatch':{'state':'sent','command':args.get('command'),'destination':ref or args.get('volumeTarget'),'deliveryAcknowledged':False},'effect':{'state':'not_checked'},'observed':False}
  if (r/'malformed').exists():print('not json',flush=True)
  else:print(json.dumps(reply),flush=True)
 sys.exit()
c=a[3]
if c.startswith('sha256sum '):print(('0'*64 if (r/'bad_digest').exists() else (r/'digest').read_text())+'  media-helper.jar');sys.exit()
if c.startswith('cat '):print((r/'pid').read_text());sys.exit()
if c.startswith('umask 077; mkdir '):sys.exit()
if c.startswith('if [ ! -d '):
 log('cleanup',c)
 if (r/'fail_cleanup').exists():sys.exit(7)
 sys.exit()
if shlex.split(c)[:1]==['input']:sys.exit()
sys.exit(4)
"#).unwrap();
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            root,
            adb,
            task: format!("media-fixture-{}", uuid::Uuid::new_v4()),
            target: format!("android:{serial}"),
        }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_testrun"));
        command.arg("--json");
        command
            .env("TESTRUN_DATA_ROOT", self.root.data_root())
            .env_remove("CODEX_THREAD_ID")
            .env("TESTRUN_TASK_ID", &self.task)
            .env("TESTRUN_ADB", &self.adb)
            .env("ANDROID_AVD_HOME", crate::NO_AVDS)
            .kill_on_drop(true);
        command
    }
    async fn raw(&self, args: &[&str]) -> std::process::Output {
        tokio::time::timeout(Duration::from_secs(40), self.command().args(args).output())
            .await
            .unwrap()
            .unwrap()
    }
    async fn call(&self, args: &[&str]) -> Value {
        let output = self.raw(args).await;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    async fn target_status(&self) -> Value {
        self.call(&["status", "--target", &self.target]).await
    }
    fn dispatches(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.path().join("dispatches"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
    async fn connect(&self) {
        self.call(&["connect", "--target", &self.target]).await;
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.root.path().join("fail_cleanup"));
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
async fn a_prepared_media_read_does_not_hold_the_input_or_status_queue() {
    struct Block(PathBuf);
    impl Drop for Block {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let f = Fixture::new();
    let source = std::fs::read_to_string(&f.adb).unwrap();
    std::fs::write(&f.adb,source.replace("  error=None;ref=", "  if op=='media_state' and (r/'state_block').exists():\n   (r/'state_started').touch()\n   while (r/'state_block').exists():time.sleep(0.01)\n  error=None;ref=")).unwrap();
    f.connect().await;
    assert_eq!(f.call(&["media", "status"]).await["count"], 2);
    let block = Block(f.root.path().join("state_block"));
    std::fs::write(&block.0, "").unwrap();
    let mut read = f
        .command()
        .args(["media", "status"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !f.root.path().join("state_started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // Both independent commands must complete while the media read is pending.
    let early_read = tokio::select! {
        biased;
        result = read.wait() => Some(result),
        () = async {
            assert_eq!(f.target_status().await["state"], "ready");
            f.call(&["act", r#"{"action":"tap","point":{"x":0.5,"y":0.5}}"#])
                .await;
        } => None,
    };
    // Abandon only the read client; the helper and its pending native reply
    // remain owned until their bounded protocol operation completes.
    if early_read.is_none() {
        read.kill().await.unwrap();
    }
    drop(block);
    assert!(
        early_read.is_none(),
        "media read finished before input/status: {early_read:?}"
    );
    assert_eq!(f.target_status().await["lastAction"]["status"], "completed");
    f.call(&["disconnect"]).await;
}

#[tokio::test]
async fn a_failed_helper_release_retries_only_the_unfinished_stage_and_retains_errors() {
    let f = Fixture::new();
    f.connect().await;
    f.call(&["media", "send", "next", "--media-session", "fixture:a"])
        .await;
    f.call(&["act", r#"{"action":"tap","point":{"x":0.5,"y":0.5}}"#])
        .await;
    let foreign = f.root.path().join("foreign-resource");
    std::fs::write(&foreign, "Acme neighboring owner").unwrap();
    std::fs::write(f.root.path().join("fail_cleanup"), "").unwrap();
    assert!(!f.raw(&["disconnect"]).await.status.success());
    let failed = f.target_status().await;
    assert_eq!(failed["state"], "releasing", "{failed}");
    assert_eq!(failed["release"]["retrying"], true, "{failed}");
    assert!(
        failed["cleanup"]
            .to_string()
            .contains("Android media helper cleanup is unverified")
    );
    let history = failed["lastAction"].clone();
    assert!(
        !f.raw(&["act", r#"{"action":"tap","point":{"x":0.5,"y":0.5}}"#])
            .await
            .status
            .success()
    );
    std::fs::remove_file(f.root.path().join("fail_cleanup")).unwrap();
    // A CLI disconnect or the manager's own retry finishes the release.
    let closed = f.call(&["disconnect"]).await;
    assert_eq!(closed["closed"], true, "{closed}");
    let refused = f.raw(&["status", "--target", &f.target]).await;
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("NOT_CONNECTED"));
    let cleanup = f.root.closed_cleanup();
    assert_eq!(cleanup["complete"], true);
    assert!(
        cleanup
            .to_string()
            .contains("Android media helper cleanup is unverified")
    );
    assert_eq!(
        f.call(&[
            "status",
            "--action-id",
            history["actionId"].as_str().unwrap()
        ])
        .await["action"],
        history
    );
    assert_eq!(
        std::fs::read_to_string(foreign).unwrap(),
        "Acme neighboring owner"
    );
    assert_eq!(f.dispatches().len(), 1);
    // Every attempt released the same helper; none restarted it.
    let releases = std::fs::read_to_string(f.root.path().join("cleanup")).unwrap();
    let releases: Vec<_> = releases.lines().collect();
    assert!(releases.len() >= 2, "{releases:?}");
    assert!(releases.iter().all(|release| *release == releases[0]));
    let requests = std::fs::read_to_string(f.root.path().join("requests")).unwrap();
    assert_eq!(
        requests
            .lines()
            .filter(|line| line.contains("\"operation\": \"quit\""))
            .count(),
        1
    );
    let calls = std::fs::read_to_string(f.root.path().join("calls")).unwrap();
    assert_eq!(
        calls
            .lines()
            .filter(|line| line.contains("'input' 'tap'"))
            .count(),
        1
    );
    // A repeated disconnect has nothing left to release.
    assert_eq!(f.call(&["disconnect"]).await["closed"], true);
}
#[tokio::test]
async fn cli_media_uses_selected_controller_and_preserves_refusals_cleanup_and_status() {
    let f = Fixture::new();
    assert!(!f.raw(&["media", "status"]).await.status.success());
    f.connect().await;
    let state = f.call(&["media", "status"]).await;
    assert_eq!(state["count"], 2);
    assert_eq!(
        state["sessions"][0]["reportedPlayback"]["positionSource"],
        "service_extrapolated"
    );
    assert_eq!(state["targetId"], f.target);
    let volume = f.call(&["volume", "get"]).await;
    assert_eq!(volume["current"], 5);
    assert_eq!(volume["destination"], json!({"kind":"music_stream"}));
    let result = f
        .call(&["media", "send", "pause", "--media-session", "fixture:b"])
        .await;
    assert_eq!(result["dispatch"]["state"], "sent");
    assert_eq!(
        result["dispatch"]["destination"],
        json!({"kind":"media_session","id":"fixture:b"})
    );
    assert_eq!(result["observed"], false);
    let reference = result["actionId"].as_str().unwrap();
    let volume = f.call(&["volume", "set", "--value", "0"]).await;
    assert_eq!(volume["dispatch"]["command"], "volume_set");
    assert_eq!(volume["dispatch"]["destination"]["kind"], "music_stream");
    assert_eq!(volume["dispatch"]["value"], 0);
    for args in [
        vec!["media", "send", "next", "--media-session", "stale"],
        vec!["volume", "set", "--value", "16"],
        vec!["volume", "set", "--value", "1", "--adjust", "up"],
    ] {
        assert!(!f.raw(&args).await.status.success());
    }
    assert_eq!(f.dispatches().len(), 2);
    assert_eq!(f.dispatches()[0]["arguments"]["sessionRef"], "fixture:b");
    f.call(&["act", r#"{"action":"tap","point":{"x":0.5,"y":0.5}}"#])
        .await;
    assert_eq!(
        f.call(&["status", "--action-id", reference]).await["action"]["state"],
        "completed"
    );
    f.call(&["disconnect"]).await;
    assert_eq!(
        f.call(&[
            "status",
            "--action-id",
            volume["actionId"].as_str().unwrap()
        ])
        .await["action"]["state"],
        "completed"
    );
    assert!(f.root.path().join("cleanup").exists());
    let pid: i32 = std::fs::read_to_string(f.root.path().join("pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // SAFETY: query only the PID recorded by this fixture's owned helper.
    assert_ne!(unsafe { libc::kill(pid, 0) }, 0);
}

#[tokio::test]
async fn mcp_media_deduplicates_and_refuses_wrong_platform_or_nested_arguments() {
    let f = Fixture::new();
    f.connect().await;
    let mut child = f
        .command()
        .args(["mcp", "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    async fn request(
        input: &mut tokio::process::ChildStdin,
        output: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
        request: Value,
    ) -> Value {
        input
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        if request.get("id").is_none() {
            return Value::Null;
        }
        serde_json::from_str(
            &tokio::time::timeout(Duration::from_secs(30), output.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    }
    request(&mut input,&mut output,json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme media fixture","version":"1"}}})).await;
    request(
        &mut input,
        &mut output,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await;
    let command = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"media_send","arguments":{"destination":{"kind":"media_session","id":"fixture:a"},"command":"next"}}});
    let first = request(&mut input, &mut output, command.clone()).await;
    assert_eq!(first["result"]["isError"], false, "{first}");
    let duplicate = request(&mut input, &mut output, command.clone()).await;
    assert_eq!(
        first["result"]["structuredContent"]["data"],
        duplicate["result"]["structuredContent"]["data"]
    );
    for (id, name, args, code) in [
        (
            3,
            "media_send",
            json!({"destination":{"kind":"system_player"},"command":"pause"}),
            "CAPABILITY_UNAVAILABLE",
        ),
        (
            4,
            "volume_set",
            json!({"destination":{"kind":"music_stream","id":"fixture:a"},"value":1}),
            "INVALID_ARGUMENT",
        ),
    ] {
        let response=request(&mut input,&mut output,json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}})).await;
        assert_eq!(response["result"]["isError"], true, "{response}");
        assert_eq!(
            response["result"]["structuredContent"]["code"], code,
            "{response}"
        );
    }
    assert_eq!(f.dispatches().len(), 1);
    let probe = json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"audio_probe","arguments":{"acceptRouteChange":true,"durationMs":100}}});
    let measured = request(&mut input, &mut output, probe.clone()).await;
    assert_eq!(measured["result"]["isError"], false, "{measured}");
    let duplicate = request(&mut input, &mut output, probe).await;
    assert_eq!(
        measured["result"]["structuredContent"]["data"],
        duplicate["result"]["structuredContent"]["data"]
    );
    assert_eq!(f.dispatches().len(), 2);
    let closed=request(&mut input,&mut output,json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"disconnect","arguments":{}}})).await;
    assert_eq!(closed["result"]["isError"], false, "{closed}");
    let recovered = request(&mut input, &mut output, command).await;
    assert_eq!(
        first["result"]["structuredContent"]["data"],
        recovered["result"]["structuredContent"]["data"]
    );
    assert_eq!(f.dispatches().len(), 2);
    drop(input);
    tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn helper_exit_after_dispatch_is_unknown_and_never_replayed() {
    let f = Fixture::new();
    f.connect().await;
    f.call(&["media", "status"]).await;
    std::fs::write(f.root.path().join("crash"), "").unwrap();
    let output = f
        .raw(&["media", "send", "next", "--media-session", "fixture:a"])
        .await;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("MEDIA_OUTCOME_UNKNOWN"));
    assert_eq!(
        f.target_status().await["lastMediaOperation"]["state"],
        "outcome_unknown"
    );
    assert!(
        !f.raw(&["media", "send", "next", "--media-session", "fixture:a"])
            .await
            .status
            .success()
    );
    assert_eq!(f.dispatches().len(), 1);
    f.call(&["disconnect"]).await;
}

#[tokio::test]
async fn helper_setup_and_protocol_failures_never_become_empty_or_silent_success() {
    for failure in ["bad_push", "bad_digest", "wrong_version", "malformed"] {
        let f = Fixture::new();
        f.connect().await;
        std::fs::write(f.root.path().join(failure), "").unwrap();
        let output = f.raw(&["media", "status"]).await;
        assert!(!output.status.success(), "{failure}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("MEDIA_HELPER_FAILED"),
            "{failure}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(f.dispatches().is_empty());
        f.call(&["observe"]).await;
        f.call(&["disconnect"]).await;
        assert!(f.root.path().join("cleanup").exists());
    }
}

#[tokio::test]
async fn bounded_probe_requires_acknowledgement_retains_private_clip_and_preserves_other_tools() {
    let f = Fixture::new();
    f.connect().await;
    assert!(!f.raw(&["audio", "probe"]).await.status.success());
    assert!(
        !f.raw(&[
            "audio",
            "probe",
            "--accept-route-change",
            "--duration-ms",
            "10001"
        ])
        .await
        .status
        .success()
    );
    assert!(f.dispatches().is_empty());
    let measured = f
        .call(&[
            "audio",
            "probe",
            "--accept-route-change",
            "--duration-ms",
            "100",
        ])
        .await;
    assert_eq!(measured["sampleCount"], 9600);
    assert_eq!(measured["experimental"], true);
    assert_eq!(measured["integrity"], "not_fully_verifiable");
    assert_eq!(measured["audibility"], "unverified");
    assert!(measured.get("clip").is_none());
    let measured = f
        .call(&[
            "audio",
            "probe",
            "--accept-route-change",
            "--duration-ms",
            "10000",
            "--retain-clip",
        ])
        .await;
    assert_eq!(measured["sampleCount"], 960000);
    assert!(measured.get("pcmBase64").is_none());
    let path = std::path::Path::new(measured["clip"]["path"].as_str().unwrap());
    assert_eq!(std::fs::metadata(path).unwrap().len(), 1920000);
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(path.parent().unwrap().join("manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["complete"], true);
    assert_eq!(manifest["evidence"]["attribution"], "system_mix");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    for marker in ["probe_bad_count", "probe_unrequested_clip"] {
        std::fs::write(f.root.path().join(marker), "").unwrap();
        let refused = f
            .raw(&[
                "audio",
                "probe",
                "--accept-route-change",
                "--duration-ms",
                "100",
            ])
            .await;
        assert!(!refused.status.success());
        assert!(String::from_utf8_lossy(&refused.stderr).contains("AUDIO_PROBE_FAILED"));
        std::fs::remove_file(f.root.path().join(marker)).unwrap();
    }
    std::fs::write(f.root.path().join("probe_failure"), "").unwrap();
    let failed = f.raw(&["audio", "probe", "--accept-route-change"]).await;
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("AUDIO_PROBE_FAILED"));
    assert_eq!(
        f.target_status().await["lastMediaOperation"]["state"],
        "failed"
    );
    f.call(&["media", "status"]).await;
    f.call(&["volume", "get"]).await;
    f.call(&["observe"]).await;
    f.call(&["disconnect"]).await;
    assert!(f.root.path().join("cleanup").exists());
}

#[tokio::test]
async fn overlapping_probe_refuses_before_acquisition_instead_of_queuing_another_capture() {
    let f = Fixture::new();
    f.connect().await;
    std::fs::write(f.root.path().join("probe_block"), "").unwrap();
    let mut first = f
        .command()
        .args([
            "audio",
            "probe",
            "--accept-route-change",
            "--duration-ms",
            "100",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !f.root.path().join("probe_started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let refused = f.raw(&["audio", "probe", "--accept-route-change"]).await;
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("CAPTURE_CONFLICT"));
    assert_eq!(f.dispatches().len(), 1);
    // Killing only this CLI does not cancel the controller's admitted probe or its cleanup.
    first.kill().await.unwrap();
    std::fs::remove_file(f.root.path().join("probe_block")).unwrap();
    assert_eq!(
        f.target_status().await["lastMediaOperation"]["state"],
        "completed"
    );
    assert_eq!(f.call(&["media", "status"]).await["count"], 2);
    f.call(&["disconnect"]).await;
    assert!(f.root.path().join("cleanup").exists());
}

#[tokio::test]
async fn malformed_probe_clips_never_enter_errors_or_break_archived_status() {
    let f = Fixture::new();
    f.connect().await;
    for marker in ["probe_bad_count", "probe_error_payload"] {
        std::fs::write(f.root.path().join(marker), "").unwrap();
        for duration in ["100", "10000"] {
            let output = f
                .raw(&[
                    "audio",
                    "probe",
                    "--accept-route-change",
                    "--duration-ms",
                    duration,
                    "--retain-clip",
                ])
                .await;
            assert!(!output.status.success());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(error.contains("AUDIO_PROBE_FAILED"), "{error}");
            assert!(!error.contains("pcmBase64"));
            assert!(!error.contains("private-payload"));
            assert!(error.len() < 16384);
            let status = f.target_status().await;
            assert_eq!(status["lastMediaOperation"]["state"], "failed");
            assert_eq!(
                status["lastMediaOperation"]["error"]["code"],
                "AUDIO_PROBE_FAILED"
            );
            let archived = status["lastMediaOperation"].to_string();
            assert!(!archived.contains("pcmBase64"));
            assert!(!archived.contains("private-payload"));
            assert!(archived.len() < 16384);
        }
        std::fs::remove_file(f.root.path().join(marker)).unwrap();
    }
    f.call(&["media", "status"]).await;
    f.call(&["disconnect"]).await;
}
