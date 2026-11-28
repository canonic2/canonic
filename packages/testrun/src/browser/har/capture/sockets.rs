//! Full-mode WebSocket entries with bounded `_webSocketMessages`, as the pinned tracer records them.
use super::*;

/// Messages kept per socket; later messages are counted as dropped.
const MAX_MESSAGES: usize = 1000;

/// One observed socket event, translated from either backend's protocol.
pub(crate) enum SocketEvent<'a> {
    Open {
        status: Option<u64>,
    },
    Frame {
        sent: bool,
        opcode: u64,
        data: &'a str,
    },
    Error,
    Closed,
}

/// Socket URLs use the HTTP(S) matcher with the scheme mapped as `ws:`→`http:`.
fn http_url(url: &str) -> Option<String> {
    url.strip_prefix("wss://")
        .map(|rest| format!("https://{rest}"))
        .or_else(|| {
            url.strip_prefix("ws://")
                .map(|rest| format!("http://{rest}"))
        })
}
fn now_ms() -> f64 {
    time::OffsetDateTime::now_utc().unix_timestamp_nanos() as f64 / 1_000_000.0
}

impl Capture {
    /// Start a socket entry; only full-mode captures that select its URL and page record it.
    pub fn socket_start(&mut self, key: String, page: Option<Reference>, url: &str) {
        if self.mode != CaptureMode::Full
            || self.sockets.contains_key(&key)
            || url.len() > 8192
            || !http_url(url).is_some_and(|http| self.accepts(&http, page))
        {
            return;
        }
        if self.order.len() >= MAX_ENTRIES || self.metadata_bytes + url.len() > MAX_BYTES {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        let started = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .expect("Current UTC timestamp");
        let id = Uuid::new_v4();
        let value = json!({"startedDateTime":started,"time":-1,"request":{"method":"GET","url":url,"httpVersion":"HTTP/1.1","cookies":[],"headers":[],"queryString":[],"headersSize":-1,"bodySize":0},"response":{"status":0,"statusText":"","httpVersion":"HTTP/1.1","cookies":[],"headers":[],"content":{"size":-1,"mimeType":"x-unknown"},"redirectURL":"","headersSize":-1,"bodySize":-1},"cache":{},"timings":{"send":-1,"wait":-1,"receive":-1},"_resourceType":"websocket","_webSocketMessages":[],"_testrunRequestId":id,"_testrunCaptureComplete":false});
        self.metadata_bytes += url.len();
        self.order.push(id);
        self.entries.insert(
            id,
            Record {
                value,
                complete: false,
                page,
            },
        );
        self.sockets.insert(key, (id, None, None));
    }
    /// Apply an event to a recorded socket; unknown sockets are ignored.
    pub fn socket_event(&mut self, key: &str, event: &SocketEvent<'_>) -> bool {
        let Some((id, first, last)) = self.sockets.get_mut(key) else {
            return false;
        };
        let Some(record) = self.entries.get_mut(id) else {
            return true;
        };
        // A frame proves the handshake succeeded, which RFC 6455 defines as 101,
        // when the backend reported no open event (pinned Firefox).
        if matches!(event, SocketEvent::Frame { .. }) && record.value["response"]["status"] == 0 {
            record.value["response"]["status"] = json!(101);
            record.value["response"]["statusText"] = json!("Switching Protocols");
        }
        match event {
            SocketEvent::Open { status } => {
                record.value["response"]["status"] = json!(status.unwrap_or(101));
                record.value["response"]["statusText"] = json!("Switching Protocols");
            }
            // Pinned: `content: "omit"` keeps the entry and its timing but no messages.
            SocketEvent::Frame { .. } if self.content == CaptureContent::Omit => {
                let time = now_ms();
                *first = Some(first.unwrap_or(time));
                record.value["time"] = json!(time - first.unwrap_or(time));
            }
            SocketEvent::Frame { sent, opcode, data } => {
                let messages = record.value["_webSocketMessages"]
                    .as_array()
                    .map_or(0, Vec::len);
                if messages >= MAX_MESSAGES
                    || data.len() > MAX_BODY
                    || self.metadata_bytes + self.payload_bytes + data.len() > MAX_BYTES
                {
                    let dropped = record.value["_testrunDroppedMessages"]
                        .as_u64()
                        .unwrap_or(0);
                    record.value["_testrunDroppedMessages"] = json!(dropped + 1);
                    return true;
                }
                let time = now_ms();
                *first = Some(first.unwrap_or(time));
                *last = Some(time);
                record.value["time"] = json!(time - first.unwrap_or(time));
                self.payload_bytes += data.len();
                record.value["_webSocketMessages"]
                    .as_array_mut()
                    .expect("Socket messages")
                    .push(json!({"type":if *sent {"send"} else {"receive"},"time":time,"opcode":opcode,"data":data}));
            }
            SocketEvent::Error | SocketEvent::Closed => {
                if !record.complete {
                    self.completed += 1;
                }
                record.complete = true;
                record.value["_testrunCaptureComplete"] = json!(true);
                if matches!(event, SocketEvent::Error) {
                    record.value["response"]["_failureText"] = json!("WebSocket error");
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn capture(mode: CaptureMode) -> Capture {
        let mut capture = Capture::new(
            Reference {
                connection_id: Uuid::new_v4(),
                id: Uuid::new_v4(),
                generation: Uuid::new_v4(),
            },
            CaptureContent::Omit,
        );
        capture.mode = mode;
        capture
    }
    #[test]
    fn full_captures_record_selected_socket_messages_with_bounds() {
        let mut minimal = capture(CaptureMode::Minimal);
        minimal.socket_start("a".into(), None, "ws://example.com/acme");
        assert!(minimal.sockets.is_empty());

        let mut omitted = capture(CaptureMode::Full);
        omitted.socket_start("a".into(), None, "ws://example.com/acme");
        omitted.socket_event(
            "a",
            &SocketEvent::Frame {
                sent: true,
                opcode: 1,
                data: "secret",
            },
        );
        let entry = &omitted.entries.values().next().unwrap().value;
        assert!(entry["_webSocketMessages"].as_array().unwrap().is_empty());
        assert_eq!(
            entry["response"]["status"], 101,
            "a frame proves the handshake"
        );

        let mut full = capture(CaptureMode::Full);
        full.content = CaptureContent::Embed;
        full.filter = Some(
            crate::browser::routing::Matcher::Glob {
                pattern: "**/acme".into(),
            }
            .compile()
            .unwrap(),
        );
        full.socket_start("other".into(), None, "wss://example.com/other");
        full.socket_start("a".into(), None, "wss://example.com/acme");
        assert_eq!(full.sockets.len(), 1);
        assert!(full.socket_event("a", &SocketEvent::Open { status: None }));
        full.socket_event(
            "a",
            &SocketEvent::Frame {
                sent: true,
                opcode: 1,
                data: "Acme hello",
            },
        );
        full.socket_event(
            "a",
            &SocketEvent::Frame {
                sent: false,
                opcode: 2,
                data: "AP8=",
            },
        );
        for _ in 0..MAX_MESSAGES {
            full.socket_event(
                "a",
                &SocketEvent::Frame {
                    sent: false,
                    opcode: 1,
                    data: "x",
                },
            );
        }
        assert!(!full.socket_event("unknown", &SocketEvent::Closed));
        assert_eq!(full.summary()["completedEntries"], 0);
        full.socket_event("a", &SocketEvent::Closed);
        assert_eq!(full.summary()["completedEntries"], 1);
        let entry = &full.entries.values().next().unwrap().value;
        assert_eq!(entry["_resourceType"], "websocket");
        assert_eq!(entry["response"]["status"], 101);
        let messages = entry["_webSocketMessages"].as_array().unwrap();
        assert_eq!(messages.len(), MAX_MESSAGES);
        assert_eq!(messages[0]["type"], "send");
        assert_eq!(messages[0]["data"], "Acme hello");
        assert_eq!(messages[1]["type"], "receive");
        assert_eq!(messages[1]["opcode"], 2);
        assert_eq!(entry["_testrunDroppedMessages"], 2);
        let archive: Value =
            serde_json::from_slice(&full.archive().unwrap().encode().unwrap()).unwrap();
        let entries = archive["log"]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["request"]["url"], "wss://example.com/acme");
        assert_eq!(
            entries[0]["_webSocketMessages"].as_array().unwrap().len(),
            MAX_MESSAGES
        );
    }
}
