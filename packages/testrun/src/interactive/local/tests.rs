//! Real local socket, endpoint ownership and closing-state fixtures.
use super::*;
use super::{
    endpoint::{Endpoint, endpoint_bind_error},
    server::finish_with_endpoint,
    transport::{MAX_BYTES, check_owner},
};
use crate::{error::Error, qa::journal::Archive};
use serde_json::json;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use tokio::{
    io::AsyncWriteExt,
    net::{UnixListener, UnixStream},
};
#[test]
fn endpoint_permission_failure_is_not_a_collision_or_device_failure() {
    let denied = endpoint_bind_error(std::io::Error::from_raw_os_error(libc::EPERM));
    assert_eq!(denied.code, "HOST_ACCESS_REQUIRED");
    assert_eq!(
        denied.details.as_ref().unwrap()["deviceState"],
        "not_checked"
    );
    assert_eq!(denied.details.unwrap()["mediaAllocated"], false);
    let collision = endpoint_bind_error(std::io::Error::from(std::io::ErrorKind::AddrInUse));
    assert_eq!(collision.code, "SESSION_ENDPOINT_CONFLICT");
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("control.sock");
    let _foreign = std::os::unix::net::UnixListener::bind(&path).unwrap();
    let error = std::os::unix::net::UnixListener::bind(&path).unwrap_err();
    assert_eq!(endpoint_bind_error(error).code, "SESSION_ENDPOINT_CONFLICT");
    assert!(path.exists());
}
#[tokio::test]
async fn interrupted_reply_and_lost_endpoint_recover_completed_action_without_replay() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("owned");
    crate::artifacts::directory(&directory).unwrap();
    let archive = Archive::create(&directory).unwrap();
    let path = directory.join("control.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let metadata = std::fs::symlink_metadata(&path).unwrap();
    let endpoint = Endpoint {
        directory,
        socket: path.clone(),
        identity: Some((metadata.dev(), metadata.ino())),
    };
    let owner = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let input = read(&mut socket).await.unwrap();
        assert_eq!(input["commandId"], "route");
        archive
            .record(
                "route",
                &json!({"commandId":"route","status":"outcome_unknown","steps":[]}),
            )
            .unwrap();
        archive
            .record(
                "route:0",
                &json!({"commandId":"route:0","state":"dispatched"}),
            )
            .unwrap();
        // Synthetic adapter has returned, but the owner exits before observation/reply.
        archive.record("route:0", &json!({"commandId":"route:0","state":"completed","observed":false,"result":{"completion":"reports_sent","deliveryAcknowledged":false}})).unwrap();
        drop(endpoint);
    });
    let error = request(
        &path,
        &json!({"kind":"batch","commandId":"route","steps":[]}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "BATCH_OUTCOME_UNKNOWN");
    owner.await.unwrap();
    assert!(!path.exists());
    let status = request(&path, &json!({"kind":"status","commandId":"route"}))
        .await
        .unwrap();
    assert_eq!(status["data"]["journalSource"], "archive");
    assert_eq!(status["data"]["status"], "outcome_unknown");
    assert_eq!(status["data"]["actionOutcomes"][0]["state"], "completed");
    assert_eq!(status["data"]["actionOutcomes"][0]["observed"], false);
    assert_eq!(
        request(
            &path,
            &json!({"kind":"batch","commandId":"route","steps":[]})
        )
        .await
        .unwrap_err()
        .code,
        "SESSION_NOT_FOUND"
    );
}
#[tokio::test]
async fn local_clients_reconnect_without_replacing_the_endpoint_owner() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket_path = directory.path().join("control.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let path = socket_path.clone();
    let owner = tokio::spawn(async move {
        for sequence in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            check_owner(&socket).unwrap();
            let input = read(&mut socket).await.unwrap();
            assert_eq!(input["sequence"], sequence);
            write(&mut socket, &json!({"ok":true,"sequence":sequence}))
                .await
                .unwrap();
        }
    });
    for sequence in 0..2 {
        assert_eq!(
            request(&path, &json!({"sequence":sequence})).await.unwrap()["sequence"],
            sequence
        );
    }
    owner.await.unwrap();
}
#[tokio::test]
async fn oversized_messages_are_rejected_before_reading_payloads() {
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    writer.write_u32((MAX_BYTES + 1) as u32).await.unwrap();
    assert_eq!(
        read(&mut reader).await.unwrap_err().code,
        "INVALID_ARGUMENT"
    );
}
#[tokio::test]
async fn closing_endpoint_stays_queryable_and_preserves_cleanup_failure() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("owned");
    crate::artifacts::directory(&directory).unwrap();
    let archive = Archive::create(&directory).unwrap();
    archive
        .record(
            "route:0",
            &json!({"commandId":"route:0","state":"dispatched"}),
        )
        .unwrap();
    let path = directory.join("control.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let (finish, wait) = tokio::sync::oneshot::channel::<()>();
    let owner = tokio::spawn(async move {
        finish_with_endpoint(&listener, Some(&archive), None, async {
            wait.await.unwrap();
            Err(Error::new("CLEANUP_FAILED", "Fixture cleanup failed"))
        })
        .await
    });
    let status = request(&path, &json!({"kind":"status"})).await.unwrap();
    assert_eq!(status["data"]["state"], "closing");
    let command = request(&path, &json!({"kind":"status","commandId":"route:0"}))
        .await
        .unwrap();
    assert_eq!(command["data"]["state"], "outcome_unknown");
    assert_eq!(command["data"]["journalSource"], "archive");
    assert_eq!(
        request(&path, &json!({"kind":"batch","steps":[]}))
            .await
            .unwrap()["error"]["code"],
        "SESSION_CLOSING"
    );
    assert_eq!(
        request(&path, &json!({"kind":"close"})).await.unwrap()["data"]["closed"],
        false
    );
    finish.send(()).unwrap();
    assert_eq!(owner.await.unwrap().unwrap_err().code, "CLEANUP_FAILED");
}
#[tokio::test]
async fn endpoint_loss_is_detected_and_cleanup_preserves_a_replacement() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("owned");
    crate::artifacts::directory(&directory).unwrap();
    let socket = directory.join("control.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let metadata = std::fs::symlink_metadata(&socket).unwrap();
    let endpoint = Endpoint {
        directory: directory.clone(),
        socket: socket.clone(),
        identity: Some((metadata.dev(), metadata.ino())),
    };
    std::fs::remove_file(&socket).unwrap();
    assert_eq!(
        endpoint.lost().await.unwrap_err().code,
        "SESSION_ENDPOINT_LOST"
    );
    let replacement = UnixListener::bind(&socket).unwrap();
    drop(endpoint);
    drop(listener);
    assert!(socket.exists());
    drop(replacement);
}
#[test]
fn endpoint_cleanup_preserves_unrelated_files() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("owned");
    crate::artifacts::directory(&directory).unwrap();
    let socket = directory.join("control.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let endpoint = Endpoint {
        directory: directory.clone(),
        identity: Some({
            let metadata = std::fs::symlink_metadata(&socket).unwrap();
            (metadata.dev(), metadata.ino())
        }),
        socket,
    };
    std::fs::write(directory.join("unrelated"), b"preserve").unwrap();
    drop(listener);
    drop(endpoint);
    assert!(directory.join("unrelated").exists());
    assert!(!directory.join("control.sock").exists());
}
