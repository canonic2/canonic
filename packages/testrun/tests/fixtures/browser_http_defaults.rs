//! Owned HTTP wire observations for request defaults and redirect transitions.
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct Site {
    pub url: String,
    records: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}
async fn respond(
    mut stream: tokio::net::TcpStream,
    cross: Option<String>,
    observed: Arc<Mutex<Vec<Value>>>,
) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    while bytes.len() < 16384 && !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).await.unwrap();
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let Some(end) = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .map(|offset| offset + 4)
    else {
        return;
    };
    let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let headers = head
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), json!(value.trim())))
        .collect::<serde_json::Map<_, _>>();
    let length = headers
        .get("content-length")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    assert!(length <= 65536);
    while bytes.len() < end + length {
        let count = stream.read(&mut buffer).await.unwrap();
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    assert_eq!(bytes.len(), end + length);
    let method = head.split_whitespace().next().unwrap();
    let path = head.split_whitespace().nth(1).unwrap();
    let record = json!({"method":method,"path":path,"headers":headers,"body":String::from_utf8_lossy(&bytes[end..])});
    {
        let mut records = observed.lock().unwrap();
        assert!(records.len() < 2048);
        records.push(record.clone());
    }
    if let Some(delay) = path
        .strip_prefix("/delay/")
        .and_then(|value| value.parse::<u64>().ok())
    {
        assert!(delay <= 5000);
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
    }
    let (status, extra) = response_headers(path, cross.as_deref(), &record);
    let html = path.starts_with("/header-page");
    let content_type = if html {
        "text/html"
    } else {
        "application/json"
    };
    let body = if html {
        "<!doctype html><title>Acme headers</title><p>Acme header fixture</p>".into()
    } else {
        record.to_string()
    };
    let keepalive = path == "/proxy-keepalive";
    let connection = if keepalive { "keep-alive" } else { "close" };
    let response = format!(
        "HTTP/1.1 {status} Acme\r\n{extra}Content-Type: {content_type}\r\nContent-Length: {}\r\nConnection: {connection}\r\n\r\n{}",
        body.len(),
        if method == "HEAD" { "" } else { &body }
    );
    let _ = stream.write_all(response.as_bytes()).await;
    if keepalive {
        let mut next = [0; 1];
        assert_eq!(
            stream.read(&mut next).await.unwrap(),
            0,
            "Pinned proxy requests must not reuse an idle tunnel"
        );
    }
}
impl Site {
    pub async fn start(cross: Option<String>) -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let records = Arc::new(Mutex::new(Vec::new()));
        let observed = records.clone();
        let task = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept(), if requests.len() < 64 => {
                        let (stream, _) = accepted.unwrap();
                        requests.spawn(respond(stream, cross.clone(), observed.clone()));
                    },
                    Some(completed) = requests.join_next(), if !requests.is_empty() => { completed.unwrap(); },
                }
            }
        });
        Self { url, records, task }
    }
    pub fn records(&self) -> Vec<Value> {
        self.records.lock().unwrap().clone()
    }
}
impl Drop for Site {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn response_headers(path: &str, cross: Option<&str>, record: &Value) -> (u16, String) {
    let mut status = 200;
    let mut location = None;
    if let Some(count) = path
        .strip_prefix("/chain/")
        .and_then(|value| value.parse::<usize>().ok())
    {
        if count > 0 {
            status = 302;
            location = Some(format!("/chain/{}", count - 1));
        }
    } else if path == "/empty" {
        status = 302;
        location = Some(String::new());
    } else if path == "/missing" {
        status = 302;
    } else if path == "/exhaust" {
        status = 302;
        location = Some("/missing".into());
    } else if let Some(code) = path.strip_prefix("/rewrite/") {
        status = code.parse().unwrap();
        location = Some("/echo".into());
    } else if path == "/cross" {
        status = 307;
        location = Some(format!("{}/echo", cross.unwrap()));
    }
    let mut extra = location
        .map(|value| format!("Location: {value}\r\n"))
        .unwrap_or_default();
    if path.starts_with("/auth/") {
        let expected_authorization = if path == "/auth/changed" {
            "Basic QWNtZTI6bmV3LXBhc3M="
        } else {
            "Basic QWNtZTpwYXNz"
        };
        if path == "/auth/cross" {
            status = 307;
            extra = format!("Location: {}/auth/basic\r\n", cross.unwrap());
        } else if path == "/auth/redirect" {
            status = 307;
            extra = "Location: /auth/basic\r\n".into();
        } else if record["headers"]["authorization"] != expected_authorization
            || path == "/auth/reject"
        {
            status = 401;
            if path != "/auth/absent" {
                let challenge = match path {
                    "/auth/quoted" => "Basic realm=\"Acme\"",
                    "/auth/lower" => "basic realm=Acme",
                    "/auth/digest" => "Digest realm=Acme",
                    "/auth/prefix" => "  BasicExtra realm=Acme",
                    _ => "Basic realm=Acme",
                };
                extra.push_str(&format!("WWW-Authenticate: {challenge}\r\n"));
            }
            extra.push_str("Set-Cookie: Acme-auth=ready; Path=/\r\n");
        }
    }
    (status, extra)
}
