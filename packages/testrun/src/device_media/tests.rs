//! CoreDevice prerequisites and explicitly selected USB acceptance.
use super::*;
#[test]
fn transport_errors_distinguish_host_access_device_state_and_transient_sockets() {
    #[cfg(unix)]
    let permission_error = libc::EPERM;
    #[cfg(windows)]
    let permission_error = windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED as i32;
    let denied = platform_at(
        idevice::IdeviceError::Socket(std::io::Error::from_raw_os_error(permission_error)),
        "usb_daemon_connect",
    );
    assert_eq!(denied.code, "HOST_ACCESS_REQUIRED");
    assert!(!denied.retryable);
    assert_eq!(
        denied.details.as_ref().unwrap()["osError"],
        permission_error
    );
    assert_eq!(denied.details.unwrap()["deviceState"], "unknown");
    let unavailable = platform_at(
        idevice::IdeviceError::Socket(std::io::Error::from(std::io::ErrorKind::ConnectionRefused)),
        "coredevice_proxy_connect",
    );
    assert_eq!(unavailable.code, "DEVICE_MEDIA_UNAVAILABLE");
    assert!(unavailable.retryable);
    assert_eq!(
        unavailable.details.unwrap()["blockingState"],
        "socket_unavailable"
    );
    let locked = platform(idevice::IdeviceError::DeviceLocked);
    assert_eq!(locked.code, "DEVICE_LOCKED");
    assert_eq!(locked.details.unwrap()["deviceState"], "locked");
    let disconnected = platform(idevice::IdeviceError::DeviceNotFound);
    assert_eq!(disconnected.code, "DEVICE_DISCONNECTED");
    assert_eq!(disconnected.details.unwrap()["deviceState"], "disconnected");
}
#[test]
fn minimum_os_does_not_accept_unknown_versions() {
    assert!(require_version("27.0.1").is_ok());
    for version in ["26.6.2", "", "unknown"] {
        assert_eq!(
            require_version(version).unwrap_err().code,
            "CAPABILITY_UNAVAILABLE"
        );
    }
}
#[tokio::test]
#[cfg(feature = "acceptance-ios-device")]
async fn selected_device_media_preflight() {
    let serial = std::env::var("TESTRUN_TEST_DEVICE").expect("Select an owned USB fixture");
    let report = inspect(&serial).await.unwrap();
    println!("{report}");
    assert_eq!(report["status"]["running"], false);
}
#[cfg(target_os = "macos")]
#[tokio::test]
#[cfg(feature = "acceptance-ios-device")]
async fn selected_device_stream_lifecycle() {
    let serial = std::env::var("TESTRUN_TEST_DEVICE").expect("Select an owned USB fixture");
    let mut sources = Sources::default();
    let source = sources
        .acquire(
            &serial,
            &json!({"targetId":format!("device:{serial}"),"generation":"live-acceptance"}),
        )
        .await
        .unwrap();
    let mut frames = source.frames.subscribe();
    source.keyframe();
    let mut count = 0;
    let mut key = false;
    for _ in 0..60 {
        let frame = tokio::time::timeout(Duration::from_secs(5), frames.recv())
            .await
            .unwrap()
            .unwrap();
        count += 1;
        key |= frame.key;
        assert_eq!(frame.media.metadata["codec"], "hevc");
    }
    let metadata = source
        .latest
        .borrow()
        .as_ref()
        .unwrap()
        .media
        .metadata
        .clone();
    println!(
        "{}",
        json!({"receivedFrames":count,"keyframe":key,"width":metadata["width"],"height":metadata["height"],"sourceFps":metadata["sourceFps"],"droppedAccessUnits":metadata["droppedAccessUnits"]})
    );
    drop(source);
    sources.release_unused().await.unwrap();
    assert_eq!(inspect(&serial).await.unwrap()["status"]["running"], false);
}
