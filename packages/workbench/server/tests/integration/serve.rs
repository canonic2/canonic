//! The Workbench server through the built binary: its own routes, a
//! question to its host, its standard-stream messages, the resident index
//! and its watcher.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(10);

/// Workbench's own folder, which the server serves its files from.
fn product() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// A running `workbench-server serve` and the messages it writes, without
/// the log notices every request may add.
struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<Value>,
    port: u16,
}

impl Server {
    fn start(root: &Path) -> Self {
        let mut child = Command::new(assert_cmd::cargo::cargo_bin!("workbench-server"))
            .arg("serve")
            .arg("--root")
            .arg(root)
            .arg("--product")
            .arg(product())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the server starts");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("server output");
        let (send, lines) = mpsc::channel::<Value>();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                let message: Value =
                    serde_json::from_str(&line).expect("one JSON message per line");
                if matches!(message["type"].as_str(), Some("log" | "diagnostic")) {
                    continue;
                }
                if send.send(message).is_err() {
                    return;
                }
            }
        });
        let listening = lines.recv_timeout(WAIT).expect("listening message");
        assert_eq!(listening["type"], "listening");
        let port =
            u16::try_from(listening["port"].as_u64().expect("listening port")).expect("a port");
        Self {
            child,
            stdin,
            lines,
            port,
        }
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("open input");
        writeln!(stdin, "{message}").expect("message sent");
    }

    fn next(&self) -> Value {
        self.lines
            .recv_timeout(WAIT)
            .expect("a message from the server")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stdin.take();
        let _killed = self.child.kill();
        let _waited = self.child.wait();
    }
}

/// Sends a raw request and reads the whole answer.
fn exchange(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("the server accepts");
    stream.set_read_timeout(Some(WAIT)).expect("timeout");
    stream.write_all(request.as_bytes()).expect("request sent");
    let mut answer = String::new();
    let _read = stream.read_to_string(&mut answer);
    answer
}

fn get(port: u16, path: &str) -> String {
    exchange(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
    )
}

#[test]
fn serves_the_canvas_the_project_and_its_configuration() {
    let (_temp, root) = crate::fixture();
    let server = Server::start(&root);
    let canvas = get(server.port, "/_workbench/");
    assert!(canvas.starts_with("HTTP/1.1 200"), "{canvas}");
    let page = get(server.port, "/page.html");
    assert!(
        page.contains("<script src=\"/_workbench/preview-compat.js\"></script><p>Acme</p>"),
        "{page}"
    );
    let missing = get(server.port, "/missing.html");
    assert!(
        missing.starts_with("HTTP/1.1 404") && missing.contains("Not found: /missing.html"),
        "{missing}"
    );
    let none = get(server.port, "/_workbench/config");
    assert!(
        none.starts_with("HTTP/1.1 404") && none.contains("There is no workbench.yaml"),
        "{none}"
    );
    std::fs::write(root.join("workbench.yaml"), "name: Acme\ncollections:\n  - name: Pages\n    items:\n      - label: Page\n        src: page.html\n").expect("manifest");
    let config = get(server.port, "/_workbench/config");
    assert!(config.starts_with("HTTP/1.1 200"), "{config}");
    let body: Value =
        serde_json::from_str(config.split("\r\n\r\n").nth(1).expect("a body")).expect("JSON");
    assert_eq!(body["name"], "Acme");
    assert!(body["pages"]["page.html"].is_object(), "{body}");
    assert_eq!(
        body["catalogCollections"][0]["items"][0]["src"],
        "a.workbench.ts"
    );
    let refused = exchange(
        server.port,
        "PUT /page.html HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(refused.starts_with("HTTP/1.1 405"), "{refused}");
}

#[test]
fn asks_its_host_to_open_a_source_and_answers_with_the_reply() {
    let (_temp, root) = crate::fixture();
    std::fs::write(root.join("workbench.yaml"), "name: Acme\ncollections:\n  - name: Pages\n    items:\n      - label: Page\n        src: page.html\n").expect("manifest");
    let mut server = Server::start(&root);
    let port = server.port;
    let opened = thread::spawn(move || {
        let body = r#"{"src":"page.html"}"#;
        exchange(
            port,
            &format!(
                "POST /_workbench/open HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ),
        )
    });
    let question = loop {
        let message = server.next();
        if message["type"] == "host" {
            break message;
        }
    };
    assert_eq!(question["question"], "open");
    assert_eq!(
        question["file"],
        root.join("page.html").to_string_lossy().as_ref()
    );
    server.send(&json!({ "type": "reply", "id": question["id"], "ok": true, "value": null }));
    let answer = opened.join().expect("the request finishes");
    assert!(
        answer.starts_with("HTTP/1.1 200") && answer.contains("\"opened\""),
        "{answer}"
    );
}

#[test]
fn exits_when_its_input_closes() {
    let (_temp, root) = crate::fixture();
    let mut front = Server::start(&root);
    front.stdin.take();
    let started = Instant::now();
    while started.elapsed() < WAIT {
        if front.child.try_wait().expect("status").is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("the server outlived its input");
}

#[test]
fn answers_index_requests_resident_and_reports_page_changes() {
    let (_temp, root) = crate::fixture();
    let mut front = Server::start(&root);
    let mut request = crate::request(&root);
    front.send(&json!({ "type": "index", "id": 1, "request": request }));
    let first = front.next();
    assert_eq!(first["type"], "answer");
    assert_eq!(first["id"], 1);
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(first["response"]["records"][0]["file"], "a.workbench.ts");
    let generation = first["response"]["generation"].clone();

    request["cached_generation"] = generation.clone();
    front.send(&json!({ "type": "index", "id": 2, "request": request }));
    let second = front.next();
    assert_eq!(second["response"]["reuse_answer"], true, "{second}");
    assert_eq!(second["response"]["generation"], generation);

    // A component edit leaves the page tree alone: no report.
    std::fs::write(root.join("page.html"), "<p>Acme, edited</p>").expect("edit");
    assert!(
        front.lines.recv_timeout(Duration::from_secs(2)).is_err(),
        "a component edit is not a page change"
    );
    // A definition added is reported once its background index finishes.
    crate::page(&root, "b.workbench.ts", "Added");
    let changed = front.next();
    assert_eq!(changed["type"], "changed", "{changed}");

    front.send(&json!({ "type": "index", "id": 3, "request": request }));
    let third = front.next();
    assert_eq!(third["id"], 3);
    assert_ne!(third["response"]["generation"], generation, "{third}");

    front.send(&json!({ "type": "nonsense" }));
    assert_eq!(front.next()["type"], "error");
}

#[test]
fn a_change_fetched_before_it_settles_is_still_reported() {
    let (_temp, root) = crate::fixture();
    let mut front = Server::start(&root);
    let request = crate::request(&root);
    front.send(&json!({ "type": "index", "id": 1, "request": request }));
    assert_eq!(front.next()["ok"], true);
    crate::page(&root, "b.workbench.ts", "Added");
    // Within the watcher's settle time: this answer already lists the page.
    thread::sleep(Duration::from_millis(30));
    front.send(&json!({ "type": "index", "id": 2, "request": request }));
    let mut seen = vec![];
    while !seen
        .iter()
        .any(|message: &Value| message["type"] == "changed")
    {
        seen.push(front.next());
    }
    let answer = seen
        .iter()
        .find(|message| message["id"] == 2)
        .expect("the second answer");
    assert_eq!(
        answer["response"]["records"].as_array().map(Vec::len),
        Some(2),
        "{answer}"
    );
}
