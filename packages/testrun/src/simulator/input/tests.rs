//! Real owned pipe/process failures verify release, no replay and retained cleanup.
use super::*;
use std::path::Path;

const WORKER: &str = r#"
import json,os,struct,sys,time
path,mode,delay=sys.argv[1:]
def update(**changes):
    try:
        with open(path) as f: state=json.load(f)
    except FileNotFoundError: state={}
    for key,value in changes.items(): state[key]=state.get(key,0)+value
    temporary=path+'.partial'
    with open(temporary,'w') as f: json.dump(state,f)
    os.replace(temporary,path)
def send(value):
    data=json.dumps(value).encode()
    sys.stdout.buffer.write(struct.pack('>II',len(data),0)+data);sys.stdout.buffer.flush()
update(starts=1)
if mode=='bound-set':
    assert os.environ.get('TESTRUN_SIMULATOR_DEVICE_SET')==os.path.realpath(os.path.dirname(path))
if mode=='unsupported':
    send({'event':'error','error':{'code':'CAPABILITY_UNAVAILABLE','message':'Synthetic unsupported service identity'}})
    sys.exit(1)
time.sleep(float(delay));send({'event':'ready'})
for line in sys.stdin:
    command=json.loads(line);action=command['input']['action']
    if action=='gesture':
        update(down=1)
        if mode in ('exit','release-fails'): os._exit(9)
        if mode=='hang': time.sleep(60)
        update(up=1)
    if action=='release':
        if mode=='release-fails':
            send({'event':'reply','id':command['id'],'error':{'code':'OUTCOME_UNKNOWN','message':'Synthetic release failed'}})
            continue
        update(up=1,release=1)
    send({'event':'reply','id':command['id']})
    if action=='close': time.sleep(.15);break
"#;

fn start(path: &Path, mode: &str, delay: &str, identity: Incarnation) -> Arc<Input> {
    Input::launch(
        Factory {
            program: "python3".into(),
            args: vec![
                "-u".into(),
                "-c".into(),
                WORKER.into(),
                path.to_str().unwrap().into(),
                mode.into(),
                delay.into(),
            ],
        },
        identity,
    )
}
fn gesture() -> Action {
    Action::Gesture {
        start: [0.5, 0.5],
        end: [0.5, 0.5],
        duration: 170,
    }
}
async fn state(path: &Path, key: &str, expected: u64) -> serde_json::Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(bytes) = std::fs::read(path)
                && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
                && value[key] == expected
            {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn worker_receives_the_session_device_set_without_process_environment_changes() {
    let (temp, identity) = super::super::incarnation::tests::fixture();
    let _binding = crate::simulator::device_set::DeviceSet::bind(Some(temp.path().to_owned()));
    let path = temp.path().join("state.json");
    let input = start(&path, "bound-set", "0", identity);
    input
        .perform(gesture(), [10, 20], Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(state(&path, "up", 1).await["starts"], 1);
    input.close().await.unwrap();
}
#[tokio::test]
async fn healthy_commands_reuse_one_worker_and_close_is_retained_across_cancelled_waiters() {
    let (temp, identity) = super::super::incarnation::tests::fixture();
    let path = temp.path().join("state.json");
    let input = start(&path, "ok", "0", identity);
    for _ in 0..3 {
        input
            .perform(gesture(), [10, 20], Duration::from_secs(2))
            .await
            .unwrap();
    }
    let observed = state(&path, "up", 3).await;
    assert_eq!(observed["starts"], 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), input.close())
            .await
            .is_err()
    );
    input.close().await.unwrap();
    input.close().await.unwrap();
    assert!(input.requests.is_closed());
}
#[tokio::test]
async fn killed_worker_reports_unknown_releases_once_without_replay_and_disconnect_succeeds() {
    let (temp, identity) = super::super::incarnation::tests::fixture();
    let path = temp.path().join("state.json");
    let input = start(&path, "exit", "0", identity);
    let error = input
        .perform(gesture(), [10, 20], Duration::from_secs(2))
        .await
        .unwrap_err();
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    input.close().await.unwrap();
    let observed = state(&path, "release", 1).await;
    assert_eq!(observed["down"], 1);
    assert_eq!(observed["up"], 1);
    assert_eq!(observed["starts"], 2);
    assert!(
        input
            .perform(gesture(), [10, 20], Duration::from_secs(1))
            .await
            .is_err()
    );
}
#[tokio::test]
async fn cancelled_request_releases_owned_contact_and_startup_waiter_cannot_orphan_worker() {
    let (temp, identity) = super::super::incarnation::tests::fixture();
    let path = temp.path().join("state.json");
    let input = start(&path, "hang", "0", identity.clone());
    let caller = input.clone();
    let request = tokio::spawn(async move {
        caller
            .perform(gesture(), [10, 20], Duration::from_secs(10))
            .await
    });
    state(&path, "down", 1).await;
    request.abort();
    let _ = request.await;
    input.close().await.unwrap();
    let observed = state(&path, "release", 1).await;
    assert_eq!(observed["down"], 1);
    assert_eq!(observed["up"], 1);
    let startup_path = temp.path().join("startup.json");
    let input = start(&startup_path, "ok", "0.15", identity);
    assert!(
        tokio::time::timeout(Duration::from_millis(10), input.ready())
            .await
            .is_err()
    );
    input.close().await.unwrap();
    assert!(input.requests.is_closed());
    assert_eq!(state(&startup_path, "starts", 1).await.get("down"), None);
}

#[tokio::test]
async fn unresolved_release_is_retained_as_cleanup_failure_and_never_replayed() {
    let (temp, identity) = super::super::incarnation::tests::fixture();
    let path = temp.path().join("state.json");
    let input = start(&path, "release-fails", "0", identity);
    assert_eq!(
        input
            .perform(gesture(), [10, 20], Duration::from_secs(2))
            .await
            .unwrap_err()
            .code,
        "OUTCOME_UNKNOWN"
    );
    let first = input.close().await.unwrap_err();
    assert_eq!(first.code, "CLEANUP_FAILED");
    assert_eq!(input.close().await.unwrap_err().code, first.code);
    let observed = state(&path, "down", 1).await;
    assert!(observed.get("up").is_none());
    assert_eq!(observed["starts"], 2);
}

#[tokio::test]
async fn unsupported_startup_preserves_reason_and_deadline_release_does_not_replay() {
    let (temp, identity) = super::super::incarnation::tests::fixture();
    let startup = temp.path().join("unsupported.json");
    let input = start(&startup, "unsupported", "0", identity.clone());
    assert_eq!(
        input.ready().await.unwrap_err().code,
        "CAPABILITY_UNAVAILABLE"
    );
    input.close().await.unwrap();
    assert!(state(&startup, "starts", 1).await.get("down").is_none());
    let path = temp.path().join("timeout.json");
    let input = start(&path, "hang", "0", identity);
    assert_eq!(
        input
            .perform(gesture(), [10, 20], Duration::from_secs(2))
            .await
            .unwrap_err()
            .code,
        "OUTCOME_UNKNOWN"
    );
    input.close().await.unwrap();
    let observed = state(&path, "release", 1).await;
    assert_eq!(observed["down"], 1);
    assert_eq!(observed["up"], 1);
}
