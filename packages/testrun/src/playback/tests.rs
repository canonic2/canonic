//! Public media admission tests prevent ambiguous or broadened native dispatch.
use super::*;

#[cfg(unix)]
#[tokio::test]
async fn device_cleanup_requires_the_allocation_marker_and_valid_pid_before_removal() {
    for (marker, pid, removed) in [
        (Some("testrun-media-acme"), None, true),
        (Some("foreign-owner"), None, false),
        (None, None, false),
        (Some("testrun-media-acme"), Some("1; invalid"), false),
    ] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("owned");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("payload"), "preserve unless owned").unwrap();
        if let Some(marker) = marker {
            std::fs::write(directory.join("owner"), marker).unwrap();
        }
        if let Some(pid) = pid {
            std::fs::write(directory.join("pid"), pid).unwrap();
        }
        let command = android::cleanup_command(directory.to_str().unwrap(), "testrun-media-acme");
        let result = crate::process::run(
            "/bin/sh",
            &["-c", &command],
            std::time::Duration::from_secs(2),
            4096,
        )
        .await;
        assert_eq!(result.is_ok(), removed);
        assert_eq!(!directory.exists(), removed);
    }
}
#[test]
fn media_validation_rejects_nested_authority_ambiguous_volume_and_wrong_platform() {
    let session = json!({"kind":"media_session","id":"fixture"});
    let music = json!({"kind":"music_stream"});
    let player = json!({"kind":"system_player"});
    for (name, args) in [
        (
            "volume_set",
            json!({"destination":music,"value":5,"adjust":"up"}),
        ),
        ("volume_set", json!({"destination":music})),
        (
            "volume_get",
            json!({"destination":{"kind":"media_session"}}),
        ),
        (
            "volume_get",
            json!({"destination":{"kind":"music_stream","id":"other"}}),
        ),
        (
            "volume_get",
            json!({"destination":{"kind":"music_stream","shell":"id"}}),
        ),
        ("volume_get", json!({"destination":player})),
        (
            "media_send",
            json!({"destination":session,"command":"play","observeForMs":2001}),
        ),
        (
            "media_send",
            json!({"command":"pause","destination":"system_player"}),
        ),
        ("media_send", json!({"command":"pause","destination":music})),
        ("media_status", json!({"destination":player})),
    ] {
        assert!(validate(name, &args).is_err(), "{name} {args}");
    }
    for (name, args) in [
        ("media_status", json!({})),
        (
            "media_send",
            json!({"destination":session,"command":"pause","observeForMs":1000}),
        ),
        ("volume_set", json!({"destination":music,"value":0})),
        ("volume_set", json!({"destination":session,"adjust":"down"})),
        (
            "media_send",
            json!({"destination":player,"command":"previous"}),
        ),
    ] {
        validate(name, &args).unwrap();
    }
    // One schema serves both platforms; each refuses what its adapter cannot do.
    for (name, args, android) in [
        ("media_status", json!({}), false),
        ("volume_get", json!({"destination":music}), false),
        (
            "media_send",
            json!({"destination":player,"command":"pause"}),
            true,
        ),
        (
            "media_send",
            json!({"destination":session,"command":"pause"}),
            false,
        ),
        (
            "media_send",
            json!({"destination":player,"command":"pause","observeForMs":100}),
            false,
        ),
        (
            "media_send",
            json!({"destination":session,"command":"volume_up"}),
            true,
        ),
    ] {
        assert_eq!(
            validate_platform(name, &args, android).unwrap_err().code,
            "CAPABILITY_UNAVAILABLE",
            "{name} {args} {android}"
        );
    }
}

#[test]
fn probe_admission_enforces_bounds_route_acknowledgement_and_write_authority() {
    for args in [
        json!({"acceptRouteChange":false}),
        json!({"durationMs":99}),
        json!({"durationMs":10001}),
        json!({"durationMs":100.5}),
        json!({"targetId":"device:foreign"}),
        json!({"acceptRouteChange":true,"retainClip":"yes"}),
    ] {
        assert!(validate("audio_probe", &args).is_err(), "{args}");
    }
    let accepted = json!({"acceptRouteChange":true,"durationMs":10000,"retainClip":true});
    validate("audio_probe", &json!({})).unwrap();
    validate("audio_probe", &accepted).unwrap();
    // Only Android capture reroutes output, so only Android needs the acknowledgement.
    validate_platform("audio_probe", &json!({}), false).unwrap();
    assert_eq!(
        validate_platform("audio_probe", &json!({}), true)
            .unwrap_err()
            .code,
        "INVALID_ARGUMENT"
    );
    validate_platform("audio_probe", &accepted, true).unwrap();
    for definition in schema::tools()
        .iter()
        .filter(|t| t["name"].as_str().unwrap().ends_with("audio_probe"))
    {
        assert!(mutation(definition["name"].as_str().unwrap()));
        assert_eq!(definition["annotations"]["readOnlyHint"], false);
    }
}
