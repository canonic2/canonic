//! The front door through the built binary: forwarding to a scripted upstream,
//! its standard-stream messages, the resident index and its watcher.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(10);

/// A running `workbench-server serve` and the lines it writes.
struct FrontDoor {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<Value>,
    port: u16,
}

impl FrontDoor {
    fn start(upstream: u16) -> Self {
        let mut child = Command::new(assert_cmd::cargo::cargo_bin!("workbench-server"))
            .args(["serve", "--upstream", &upstream.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("front door starts");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("front door output");
        let (send, lines) = mpsc::channel::<Value>();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if send
                    .send(serde_json::from_str(&line).expect("one JSON message per line"))
                    .is_err()
                {
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
            .expect("a message from the front door")
    }
}

impl Drop for FrontDoor {
    fn drop(&mut self) {
        self.stdin.take();
        let _killed = self.child.kill();
        let _waited = self.child.wait();
    }
}

/// An upstream that answers each connection with `answer`, given the request head and body.
fn upstream(answer: impl Fn(&str, &[u8], &mut TcpStream) + Send + Sync + 'static) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("upstream listens");
    let port = listener.local_addr().expect("upstream address").port();
    let answer = std::sync::Arc::new(answer);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let answer = answer.clone();
            thread::spawn(move || {
                let (head, body) = read_request(&mut stream);
                answer(&head, &body, &mut stream);
            });
        }
    });
    port
}

fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut reader = BufReader::new(stream.try_clone().expect("upstream stream"));
    let mut head = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).expect("request head") == 0 || line == "\r\n" {
            break;
        }
        head.push_str(&line);
    }
    let length = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|value| value.trim().parse::<usize>().expect("length"))
        })
        .unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).expect("request body");
    (head, body)
}

/// Sends a raw request to the front door and reads the whole answer.
fn exchange(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("front door accepts");
    stream.set_read_timeout(Some(WAIT)).expect("timeout");
    stream.write_all(request.as_bytes()).expect("request sent");
    let mut answer = String::new();
    let _read = stream.read_to_string(&mut answer);
    answer
}

#[test]
fn forwards_method_target_headers_and_body_unchanged() {
    let port = upstream(|head, body, stream| {
        let first = head.lines().next().unwrap_or_default().to_owned();
        let host = head
            .lines()
            .find(|line| line.to_ascii_lowercase().starts_with("host:"))
            .unwrap_or_default()
            .to_owned();
        let acme = head
            .lines()
            .find(|line| line.to_ascii_lowercase().starts_with("x-acme:"))
            .unwrap_or_default()
            .to_owned();
        let text = format!("{first}|{host}|{acme}|{}", String::from_utf8_lossy(body));
        write!(stream, "HTTP/1.1 201 Created\r\nContent-Length: {}\r\nX-Upstream: yes\r\nConnection: close\r\n\r\n{text}", text.len()).expect("answer");
    });
    let front = FrontDoor::start(port);
    let answer = exchange(
        front.port,
        &format!(
            "POST /_workbench/open?src=a%20b.html HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Acme: kept\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
            front.port
        ),
    );
    assert!(answer.starts_with("HTTP/1.1 201"), "{answer}");
    assert!(
        answer.to_ascii_lowercase().contains("x-upstream: yes"),
        "{answer}"
    );
    let expected = format!(
        "post /_workbench/open?src=a%20b.html http/1.1|host: 127.0.0.1:{}|x-acme: kept|hello",
        front.port
    );
    assert!(answer.to_ascii_lowercase().contains(&expected), "{answer}");
}

#[test]
fn streams_a_response_as_the_upstream_writes_it() {
    let (release, released) = mpsc::channel::<()>();
    let released = std::sync::Mutex::new(released);
    let port = upstream(move |_, _, stream| {
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").expect("head");
        stream
            .write_all(b"d\r\ndata: first\n\n\r\n")
            .expect("first event");
        stream.flush().expect("flush");
        let _waited = released.lock().expect("lock").recv_timeout(WAIT);
        stream
            .write_all(b"e\r\ndata: second\n\n\r\n0\r\n\r\n")
            .expect("second event");
    });
    let front = FrontDoor::start(port);
    let mut stream = TcpStream::connect(("127.0.0.1", front.port)).expect("front door accepts");
    stream.set_read_timeout(Some(WAIT)).expect("timeout");
    stream
        .write_all(b"GET /_workbench/previews/events HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
        .expect("request");
    let mut seen = String::new();
    let mut buffer = [0; 1024];
    while !seen.contains("data: first") {
        let read = stream
            .read(&mut buffer)
            .expect("first event arrives before the second is written");
        assert!(read > 0, "closed early: {seen}");
        seen.push_str(&String::from_utf8_lossy(&buffer[..read]));
    }
    assert!(!seen.contains("data: second"));
    release.send(()).expect("release");
    while !seen.contains("data: second") {
        let read = stream.read(&mut buffer).expect("second event");
        assert!(read > 0, "closed early: {seen}");
        seen.push_str(&String::from_utf8_lossy(&buffer[..read]));
    }
}

#[test]
fn answers_head_with_the_upstream_headers_and_no_body() {
    let port = upstream(|head, _, stream| {
        assert!(head.starts_with("HEAD /page.html "), "{head}");
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n")
            .expect("answer");
    });
    let front = FrontDoor::start(port);
    let answer = exchange(
        front.port,
        "HEAD /page.html HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(
        answer.to_ascii_lowercase().contains("content-length: 5"),
        "{answer}"
    );
    assert!(answer.ends_with("\r\n\r\n"), "{answer:?}");
}

#[test]
fn an_unreachable_upstream_is_a_bad_gateway() {
    let closed = TcpListener::bind("127.0.0.1:0")
        .expect("port")
        .local_addr()
        .expect("address")
        .port();
    let front = FrontDoor::start(closed);
    let answer = exchange(
        front.port,
        "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(answer.starts_with("HTTP/1.1 502"), "{answer}");
    assert!(answer.contains("did not answer"), "{answer}");
}

#[test]
fn bridges_an_upgraded_connection_both_ways() {
    let port = upstream(|head, _, stream| {
        assert!(
            head.to_ascii_lowercase().contains("upgrade: acme-stream"),
            "{head}"
        );
        stream.write_all(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: acme-stream\r\n\r\n").expect("switch");
        let mut buffer = [0; 4];
        stream.read_exact(&mut buffer).expect("client bytes");
        stream.write_all(&buffer).expect("echo");
    });
    let front = FrontDoor::start(port);
    let mut stream = TcpStream::connect(("127.0.0.1", front.port)).expect("front door accepts");
    stream.set_read_timeout(Some(WAIT)).expect("timeout");
    stream.write_all(b"GET /_workbench/window/stream HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: Upgrade\r\nUpgrade: acme-stream\r\n\r\n").expect("upgrade request");
    let mut head = Vec::new();
    let mut byte = [0; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).expect("switching head");
        head.push(byte[0]);
    }
    assert!(
        head.starts_with(b"HTTP/1.1 101"),
        "{}",
        String::from_utf8_lossy(&head)
    );
    stream.write_all(b"ping").expect("bytes up");
    let mut echo = [0; 4];
    stream.read_exact(&mut echo).expect("bytes down");
    assert_eq!(&echo, b"ping");
    let _closed = stream.shutdown(Shutdown::Both);
}

#[test]
fn exits_when_its_input_closes() {
    let mut front = FrontDoor::start(1);
    front.stdin.take();
    let started = Instant::now();
    while started.elapsed() < WAIT {
        if front.child.try_wait().expect("status").is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("the front door outlived its input");
}

#[test]
fn answers_index_requests_resident_and_reports_page_changes() {
    let (_temp, root) = crate::fixture();
    let mut front = FrontDoor::start(1);
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
    let mut front = FrontDoor::start(1);
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
