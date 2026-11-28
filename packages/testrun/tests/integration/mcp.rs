//! The shipping Tokio transport drives shared admission without device resources.
#[cfg(unix)]
use canonic_process_kit::{
    core::{Admission, IdleGrace},
    endpoint::{Lease, Root},
    protocol::{Control, Identity, ProductError, Request},
    server::{self, Context, Handler},
};
use serde_json::{Value, json};
#[cfg(unix)]
use std::sync::{Arc, Condvar, Mutex};
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

async fn exchange(frames: Vec<Vec<u8>>) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_testrun"))
        .args(["mcp", "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = tokio::spawn(async move {
        for frame in frames {
            if input.write_all(&frame).await.is_err() {
                break;
            }
            if input.write_all(b"\n").await.is_err() {
                break;
            }
        }
    });
    let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    writer.await.unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn onboarding_cli_and_mcp_share_discovery_selection_and_creation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("web")).unwrap();
    std::fs::write(
        root.join("web/package.json"),
        r#"{"dependencies":{"vue":"1"}}"#,
    )
    .unwrap();
    let preview = Command::new(env!("CARGO_BIN_EXE_testrun"))
        .args(["init", "--dry-run", "--json", "--root"])
        .arg(&root)
        .output()
        .await
        .unwrap();
    assert!(preview.status.success(), "{preview:?}");
    let preview: Value = serde_json::from_slice(&preview.stdout).unwrap();
    let selected = Command::new(env!("CARGO_BIN_EXE_testrun"))
        .args([
            "init",
            "--dry-run",
            "--json",
            "--path",
            "web",
            "--kind",
            "react",
            "--platform",
            "web",
            "--root",
        ])
        .arg(&root)
        .output()
        .await
        .unwrap();
    assert!(selected.status.success(), "{selected:?}");
    let selected: Value = serde_json::from_slice(&selected.stdout).unwrap();
    let replies = exchange([
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme onboarding","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"project_discover","arguments":{"root":root}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"project_init_preview","arguments":{"root":root,"projects":[{"path":"web","kind":"react","platforms":["web"]}]}}}),
    ].into_iter().map(|request| serde_json::to_vec(&request).unwrap()).collect()).await;
    for (id, expected) in [(2, &preview), (3, &selected)] {
        let response = replies
            .iter()
            .find(|response| response["id"] == id)
            .unwrap();
        let mut actual = response["result"]["structuredContent"]["data"].clone();
        if id == 2 {
            assert!(actual["nextCursor"].is_null());
            assert_eq!(actual["scanDepth"], 6);
            assert_eq!(actual["warnings_total"], 0);
            assert_eq!(actual["deeperProjects"]["tool"], "project_discover");
            for field in [
                "nextCursor",
                "scanDepth",
                "warnings_total",
                "deeperProjects",
            ] {
                actual.as_object_mut().unwrap().remove(field);
            }
        }
        assert_eq!(actual, *expected, "{response}");
    }
    assert!(!root.join("testrun.yml").exists());
    assert!(!root.join(".gitignore").exists());
    let created = Command::new(env!("CARGO_BIN_EXE_testrun"))
        .args([
            "init",
            "--yes",
            "--json",
            "--path",
            "web",
            "--kind",
            "react",
            "--platform",
            "web",
            "--root",
        ])
        .arg(&root)
        .output()
        .await
        .unwrap();
    assert!(created.status.success(), "{created:?}");
    let configuration: Value =
        serde_json::from_slice(&std::fs::read(root.join("testrun.yml")).unwrap()).unwrap();
    assert_eq!(configuration, selected["configuration"]);
}

#[tokio::test]
async fn initialize_is_strict_and_ready_tools_list_uses_the_shared_catalog() {
    let mut frames = Vec::new();
    for (id, client) in [
        (1, json!({})),
        (2, json!({"name":"Acme"})),
        (3, json!({"name":"Acme","version":1})),
        (4, json!({"name":"Acme","version":"1"})),
    ] {
        frames.push(serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":client}})).unwrap());
    }
    frames.push(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_vec());
    frames.push(br#"{"jsonrpc":"2.0","id":5,"method":"tools/list"}"#.to_vec());
    let replies = exchange(frames).await;
    assert_eq!(replies.len(), 5);
    for reply in &replies[..3] {
        assert_eq!(reply["error"]["code"], -32602);
    }
    assert_eq!(
        replies[3]["result"]["serverInfo"]["name"],
        "canonic-testrun"
    );
    assert_eq!(
        replies[4]["result"],
        testrun::protocol::tools()
            .list_page(&json!({}), "2025-11-25")
            .unwrap()
    );
}

#[tokio::test]
async fn duplicate_json_keys_are_parse_errors_and_the_next_frame_survives() {
    let replies = exchange(vec![
        br#"{"jsonrpc":"2.0","id":1,"method":"ping","method":"tools/list"}"#.to_vec(),
        br#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#.to_vec(),
    ])
    .await;
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["error"]["code"], -32700);
    assert_eq!(replies[1]["id"], 2);
    assert_eq!(replies[1]["result"], json!({}));
}

#[tokio::test]
async fn an_oversized_frame_gets_a_fatal_parse_reply() {
    let replies = exchange(vec![
        vec![b' '; 6 * 1024 * 1024 + 1],
        br#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#.to_vec(),
    ])
    .await;
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["error"]["code"], -32700);
}

#[cfg(unix)]
#[derive(Default)]
struct HeldCallRelease {
    done: Mutex<bool>,
    changed: Condvar,
}

#[cfg(unix)]
struct HeldCalls {
    started: tokio::sync::mpsc::UnboundedSender<String>,
    release: Arc<HeldCallRelease>,
}

#[cfg(unix)]
impl Handler for HeldCalls {
    fn call(&self, _: &Request, _: &Context<'_>) -> Result<Value, ProductError> {
        Err(ProductError(json!({"code":"UNEXPECTED_CALL"})))
    }

    fn extensions(&self) -> &[&str] {
        &["task_end", "inspect"]
    }

    fn control(&self, request: &Control, _: &Context<'_>) -> Result<Value, ProductError> {
        let _ = self.started.send(request.action.clone());
        let _released = self
            .release
            .changed
            .wait_timeout_while(
                self.release.done.lock().unwrap(),
                Duration::from_secs(10),
                |done| !*done,
            )
            .unwrap();
        Ok(json!({}))
    }
}

#[cfg(unix)]
#[tokio::test]
async fn stdio_eof_refuses_buffered_work_and_bounds_the_responsive_drain() {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let workspace = crate::Workspace::new();
    let root = Root::explicit(workspace.data_root()).unwrap();
    let lease = Lease::acquire(&root, Duration::ZERO).unwrap();
    // The fixture serves only the version-independent manager controls.
    let identity = Identity::new("acme-eof".into(), 1, vec![]).unwrap();
    let gate = Admission::new();
    let hold = gate.hold().unwrap();
    let release = Arc::new(HeldCallRelease::default());
    let (started, mut calls) = tokio::sync::mpsc::unbounded_channel();
    let handler = Arc::new(HeldCalls {
        started,
        release: release.clone(),
    });
    let serving = gate.clone();
    let manager = tokio::task::spawn_blocking(move || {
        server::serve(
            lease,
            identity,
            serving,
            handler,
            &IdleGrace(Duration::from_secs(1)),
            server::Options {
                control_workers: 2,
                ..server::Options::default()
            },
        )
    });

    let scenario = async {
        while !root.socket(true).exists() && !manager.is_finished() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_testrun"))
            .args(["mcp", "serve"])
            .current_dir(workspace.path())
            .env("TESTRUN_DATA_ROOT", workspace.data_root())
            .env("TESTRUN_TASK_ID", "acme-eof")
            .env_remove("CODEX_THREAD_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
        for request in [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme EOF fixture","version":"1"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"testrun/task_end","params":{}}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"project_status","arguments":{"project":"acme","profile":"dev"}}}),
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"browser_list","arguments":{}}}),
            json!({"jsonrpc":"2.0","id":5,"method":"ping"}),
        ] {
            input.write_all(format!("{request}\n").as_bytes()).await?;
        }
        let initialized = output.next_line().await?;
        let ping = output.next_line().await?;
        let mut started = [calls.recv().await.unwrap(), calls.recv().await.unwrap()];
        started.sort();

        drop(input);
        tokio::time::timeout(Duration::from_secs(3), async {
            let mut replies = Vec::new();
            while let Some(line) = output.next_line().await? {
                replies.push(line);
            }
            Ok::<_, std::io::Error>((
                initialized,
                ping,
                started,
                replies,
                child.wait_with_output().await?,
            ))
        })
        .await
        .map_err(std::io::Error::other)?
    };
    let outcome = tokio::time::timeout(Duration::from_secs(10), scenario).await;
    *release.done.lock().unwrap() = true;
    release.changed.notify_all();
    let _ = gate.drain();
    drop(hold);
    manager.await.unwrap().unwrap();

    let (initialized, ping, started, replies, output) = outcome.unwrap().unwrap();
    assert_eq!(started, ["inspect", "task_end"]);
    for (line, id) in [(initialized, 1), (ping, 5)] {
        let response: Value = serde_json::from_str(&line.unwrap()).unwrap();
        assert_eq!(response["id"], id, "{response}");
    }
    assert!(output.status.success(), "{output:?}");
    let replies: Vec<Value> = replies
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        replies,
        [
            json!({"jsonrpc":"2.0","id":2,"error":{"code":-32800,"message":"Connection closed; dispatched mutations may have an unknown outcome"}}),
            json!({"jsonrpc":"2.0","id":4,"error":{"code":-32800,"message":"Connection closed before dispatch"}}),
        ]
    );
}
