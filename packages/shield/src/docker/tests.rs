use super::*;
use serde_json::{Value, json};

const ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn example() -> Value {
    json!({
        "Id": ID,
        "Image": format!("sha256:{ID}"),
        "Platform": "linux",
        "State": {"Running": true, "Paused": false, "Restarting": false},
        "Config": {"Env": ["PRIVATE=do-not-display"]},
        "HostConfig": {"Privileged": false, "NetworkMode": "bridge"},
        "Mounts": [
            {"Type": "bind", "Source": "/checkout", "Destination": "/app", "RW": true},
            {"Type": "bind", "Source": "/checkout/config.yml", "Destination": "/app/config.yml", "RW": false},
            {"Type": "volume", "Source": "/private/daemon/path", "Destination": "/app/cache", "RW": true},
            {"Type": "tmpfs", "Destination": "/tmp", "RW": true}
        ]
    })
}

#[test]
fn considers_the_most_specific_mount_without_revealing_daemon_paths_or_config() {
    let observed = review(
        "engine".into(),
        ID,
        serde_json::from_value(example()).unwrap(),
        &[
            "/app/config.yml".into(),
            "/app/cache/index".into(),
            "/application/output".into(),
        ],
    )
    .unwrap();
    let output = serde_json::to_value(observed).unwrap();
    assert_eq!(
        output["paths"][0]["candidate_host_path"],
        "/checkout/config.yml"
    );
    assert_eq!(output["paths"][0]["writable"], false);
    assert_eq!(output["paths"][1]["mount_target"], "/app/cache");
    assert!(output["paths"][1]["candidate_host_path"].is_null());
    assert!(output["paths"][2]["mount_target"].is_null());
    let encoded = output.to_string();
    assert!(!encoded.contains("do-not-display"));
    assert!(!encoded.contains("/private/daemon/path"));
}

#[test]
fn refuses_ambiguous_mounts_and_nonmatching_container_identity() {
    let observe = |response, selected: &str, paths: &[String]| {
        review(
            "engine".into(),
            selected,
            serde_json::from_value(response).unwrap(),
            paths,
        )
    };
    assert!(observe(example(), &"a".repeat(64), &[]).is_err());
    assert!(observe(example(), ID, &["/app/../config".into()]).is_err());
    let mut duplicate = example();
    duplicate["Mounts"][2]["Destination"] = json!("/./app//");
    assert!(observe(duplicate, ID, &[]).is_err());
}

#[cfg(unix)]
#[test]
fn previews_the_enrolled_projects_policy_for_the_mapped_host_file() {
    let directory = assert_fs::TempDir::new().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let target = root.join("config.yml");
    std::fs::write(&target, "fixture: true\n").unwrap();
    let rules = crate::policy::rules::document(&[
        json!({"id":"project-read", "decision":"allow", "reason":"Read workspace files.",
            "conditions":{"domain":"file","action":"read"},
            "requirements":{"targets":{"action":"read","all":{"within":{"root":"project"}}}},
            "on_unmet":{"decision":"deny"}}),
        json!({"id":"reject-write", "decision":"deny", "reason":"Preserve workspace files.",
            "conditions":{"domain":"file","action":"write"}}),
    ])
    .unwrap()
    .to_string();
    let bundle = crate::configuration::Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    crate::review::initialize(&root, BTreeMap::from([("project".into(), json!(bundle))])).unwrap();
    let mut response = example();
    response["Mounts"][1]["Source"] = json!(target);
    let mut observed = review(
        "engine".into(),
        ID,
        serde_json::from_value(response).unwrap(),
        &["/app/config.yml".into()],
    )
    .unwrap();
    observed.preview_policy(&root).unwrap();
    let policy = observed.paths[0].candidate_host_policy.as_ref().unwrap();
    assert_eq!(policy.read, Decision::Allow);
    assert_eq!(policy.write, Decision::Deny);
}

#[cfg(unix)]
#[test]
fn inspects_the_selected_engine_and_full_container_id_over_its_socket() {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixListener,
    };
    let directory = assert_fs::TempDir::new().unwrap();
    let socket = directory.path().join("docker.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let respond = std::thread::spawn(move || {
        for (path, response) in [
            (
                "/info".to_owned(),
                json!({"ID":"fixture-engine","OSType":"linux"}),
            ),
            (format!("/containers/{ID}/json"), example()),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = BufReader::new(&mut stream);
            let mut first = String::new();
            request.read_line(&mut first).unwrap();
            assert_eq!(first, format!("GET {path} HTTP/1.0\r\n"));
            for line in request.lines() {
                if line.unwrap().is_empty() {
                    break;
                }
            }
            let response = response.to_string();
            write!(
                stream,
                "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{response}",
                response.len()
            )
            .unwrap();
        }
    });
    let observed = inspect(&socket, ID, &["/app/readme.md".into()]).unwrap();
    respond.join().unwrap();
    assert_eq!(observed.engine_id, "fixture-engine");
    assert_eq!(
        observed.paths[0].candidate_host_path.as_deref(),
        Some("/checkout/readme.md")
    );
}
