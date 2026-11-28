//! Process-boundary fixtures for source sharing, cancellation and teardown.
use super::*;
use std::{
    process::Stdio,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio::process::Command;

const WORKER: &str = r#"
import json,os,select,struct,sys,time
def send(meta, payload=b''):
    data=json.dumps(meta).encode()
    sys.stdout.buffer.write(struct.pack('>II',len(data),len(payload))+data+payload)
    sys.stdout.buffer.flush()
time.sleep(float(sys.argv[1]))
send({'event':'ready','epoch':str(os.getpid())})
profiles={'jpeg':False,'h264':False}; seq=0; recording=None
while True:
    if select.select([sys.stdin],[],[],0.02)[0]:
        line=sys.stdin.buffer.raw.readline()
        if not line: break
        command=json.loads(line); op=command['operation']
        if op=='close':
            time.sleep(0.15)
            break
        if op=='screenshot': send({'event':'reply','id':command['id']},b'fresh image')
        if op=='profiles': profiles=command
        if op=='record': recording=command['id']
        if op=='stop_recording' and command['id']==recording:
            send({'event':'reply','id':recording,'result':{'status':'complete'}})
            recording=None
    seq+=1
    for codec in ('jpeg','h264'):
        if profiles[codec]: send({'event':'frame','codec':codec,'seq':str(seq)},b'frame')
    send({'event':'heartbeat'})
"#;

async fn fixture(delay: &str) -> Result<Arc<Source>> {
    let child = Command::new("python3")
        .args(["-u", "-c", WORKER, delay])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    Source::attach(child).await
}

#[tokio::test]
async fn startup_is_single_flight_and_cancellation_does_not_cancel_another_waiter() {
    let sources = Arc::new(Sources::default());
    let starts = Arc::new(AtomicUsize::new(0));
    let registry = sources.clone();
    let count = starts.clone();
    let first = tokio::spawn(async move {
        registry
            .acquire_with("target-generation".into(), move || async move {
                count.fetch_add(1, Ordering::SeqCst);
                fixture("0.15").await
            })
            .await
    });
    while starts.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    first.abort();
    let _ = first.await;
    let source = sources
        .acquire_with("target-generation".into(), || async {
            panic!("A second startup must not run")
        })
        .await
        .unwrap();
    let another = sources
        .acquire_with("target-generation".into(), || async {
            panic!("A compatible consumer must share")
        })
        .await
        .unwrap();
    assert!(Arc::ptr_eq(&source, &another));
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    let (a, b) = tokio::join!(source.screenshot(), another.screenshot());
    assert_eq!(a.unwrap(), b"fresh image");
    assert_eq!(b.unwrap(), b"fresh image");
    let mut jpeg = source.subscribe("jpeg");
    let mut h264 = another.subscribe("h264");
    tokio::time::timeout(Duration::from_secs(2), jpeg.frames.changed())
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), h264.frames.changed())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        jpeg.frames.borrow().as_ref().unwrap().metadata["codec"],
        "jpeg"
    );
    drop(jpeg);
    drop(source);
    drop(another);
    sources.release_unused().await.unwrap();
    // A remaining subscriber keeps the process alive and can request a keyframe.
    h264.keyframe();
    tokio::time::timeout(Duration::from_secs(2), h264.frames.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(h264.frames.borrow().is_some());
    drop(h264);
    sources.release_unused().await.unwrap();
    assert!(sources.entries.lock().unwrap().is_empty());
}

#[tokio::test]
async fn recording_stop_is_ordered_and_scoped_and_close_survives_a_cancelled_waiter() {
    let source = fixture("0").await.unwrap();
    {
        let recording = source.record("owned", "unused.mp4".into(), 1000);
        tokio::pin!(recording);
        tokio::select! { biased; _ = &mut recording => panic!("Recording should wait for stop"), _ = tokio::task::yield_now() => {} }
        source.stop_recording("foreign").unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(60), &mut recording)
                .await
                .is_err()
        );
        source.stop_recording("owned").unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), recording)
                .await
                .unwrap()
                .unwrap()["status"],
            "complete"
        );
    }
    let cleanup = source.cleanup.clone();
    drop(source);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), cleanup.close())
            .await
            .is_err()
    );
    cleanup.close().await.unwrap();
    cleanup.close().await.unwrap();
}

#[tokio::test]
async fn bounded_wire_refuses_oversized_payload_before_allocating() {
    let header = [
        1_u32.to_be_bytes(),
        (64_u32 * 1024 * 1024 + 1).to_be_bytes(),
    ]
    .concat();
    let error = match ipc::read(&mut header.as_slice()).await {
        Ok(_) => panic!("Oversized packet admitted"),
        Err(error) => error,
    };
    assert_eq!(error.code, "OUTPUT_LIMIT");
}

#[tokio::test]
async fn last_consumer_cleanup_fences_reacquisition_and_other_targets_continue() {
    let sources = Sources::default();
    let a = sources
        .acquire_with("a".into(), || fixture("0"))
        .await
        .unwrap();
    let b = sources
        .acquire_with("b".into(), || fixture("0"))
        .await
        .unwrap();
    let old_epoch = a.epoch().to_owned();
    drop(a);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), sources.release_unused())
            .await
            .is_err()
    );
    assert_eq!(b.screenshot().await.unwrap(), b"fresh image");
    let replacement = sources
        .acquire_with("a".into(), || fixture("0"))
        .await
        .unwrap();
    assert_ne!(old_epoch, replacement.epoch());
    assert_eq!(replacement.screenshot().await.unwrap(), b"fresh image");
    drop(replacement);
    drop(b);
    sources.shutdown().await.unwrap();
}

#[tokio::test]
async fn capture_failure_preserves_error_but_reaping_allows_clean_reacquisition() {
    let sources = Sources::default();
    let source = sources
        .acquire_with("failed".into(), || async {
            let worker = r#"import json,struct,sys
data=json.dumps({'event':'ready','epoch':'owned-failure'}).encode()
sys.stdout.buffer.write(struct.pack('>II',len(data),0)+data);sys.stdout.buffer.flush()
sys.stdin.readline();sys.exit(9)
"#;
            let child = Command::new("python3")
                .args(["-u", "-c", worker])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            Source::attach(child).await
        })
        .await
        .unwrap();
    let error = source.screenshot().await.unwrap_err();
    assert_eq!(error.code, "NATIVE_STREAM_ENDED");
    assert!(source.validate().is_err());
    let replacement = sources
        .acquire_with("failed".into(), || fixture("0"))
        .await
        .unwrap();
    assert_eq!(replacement.screenshot().await.unwrap(), b"fresh image");
    drop(source);
    drop(replacement);
    sources.shutdown().await.unwrap();
}
