//! Broken optional history cannot prevent its own trusted administrative form.
use super::*;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

#[test]
fn history_control_shipping_permission_drift_accepts_or_declines_on_the_same_connection() {
    for agent in ["claude", "codex"] {
        for modern in [false, true] {
            for args in [
                json!({"enabled":false}),
                json!({"purge":true}),
                json!({"limit":10}),
            ] {
                let (server, project) = fixture(Some(agent));
                let context = project.review.as_ref();
                let path = Path::new(&context.project_root).join(".shield/review/history.lock");
                let inode = fs::metadata(&path).unwrap().ino();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
                let state_path = Path::new(&context.project_root).join(".shield/review/state.json");
                let before: Value =
                    serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
                let transport = connect(server);
                for action in ["decline", "accept"] {
                    let form = begin(&transport, 10, &args, modern);
                    assert_eq!(
                        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                        0o644
                    );
                    let result = answer(&transport, 11, &args, &form, action, modern);
                    let value = &result["result"]["structuredContent"];
                    let after: Value =
                        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
                    if action == "accept" {
                        assert!(value["error"].is_null(), "{result}");
                        assert_eq!(
                            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                            0o600
                        );
                        if args["enabled"] == false {
                            assert_eq!(after["history_enabled"], false);
                        }
                        if args["purge"] == true || args["limit"].is_number() {
                            assert_ne!(after["history_generation"], before["history_generation"]);
                        }
                    } else {
                        assert_eq!(value["error"]["code"], "approval_declined", "{result}");
                        assert_eq!(
                            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                            0o644
                        );
                        assert_eq!(after["history_enabled"], before["history_enabled"]);
                        assert_eq!(after["history_generation"], before["history_generation"]);
                        assert_eq!(after["history_limit"], before["history_limit"]);
                    }
                    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
                    assert_eq!(after["policy_revision"], before["policy_revision"]);
                    assert_eq!(after["active"], before["active"]);
                    let status = transport.call(json!(12), "status", &json!({}));
                    assert_eq!(status["result"]["isError"], false, "{status}");
                }
            }
        }
    }
}
