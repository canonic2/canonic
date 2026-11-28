//! Configuration-free runs, goal ownership and private evidence finalization.
use super::Server;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
#[tokio::test]
async fn runs_validate_goals_and_finalize_private_evidence_without_configuration() {
    let mut server = Server::new();
    for obsolete in ["env", "allowWrites", "authorizeExternal"] {
        let mut arguments = json!({"goalIds":["qa"]});
        arguments[obsolete] = json!("unused");
        assert_eq!(
            server.call("run.begin", arguments).await.unwrap_err().code,
            "INVALID_ARGUMENT"
        );
    }
    assert_eq!(
        server
            .call("run.begin", json!({"goalIds":[]}))
            .await
            .unwrap_err()
            .code,
        "INVALID_ARGUMENT"
    );
    let run = server
        .call("run.begin", json!({"goalIds":["qa"]}))
        .await
        .unwrap();
    assert_eq!(
        server
            .call("goal.begin", json!({"runId":run["runId"],"goalId":"other"}))
            .await
            .unwrap_err()
            .code,
        "GOAL_NOT_DECLARED"
    );
    let directory = PathBuf::from(run["evidenceDir"].as_str().unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    server
        .call("run.end", json!({"runId":run["runId"]}))
        .await
        .unwrap();
    assert!(directory.join("summary.json").is_file());
    assert_eq!(
        server
            .call("goal.begin", json!({"runId":run["runId"],"goalId":"qa"}))
            .await
            .unwrap_err()
            .code,
        "RUN_NOT_FOUND"
    );
    fs::remove_dir_all(directory).unwrap();
}
