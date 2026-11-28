//! Network WebSocket observation; no interception or HTTP correlation guessing.
use super::*;
use crate::browser::diagnostics::{self, DiagnosticKind};
use std::collections::VecDeque;
struct Socket {
    id: Uuid,
    context: Reference,
    url: String,
    missing: bool,
}
#[derive(Default)]
pub(super) struct Sockets {
    items: BTreeMap<(String, String), Socket>,
    order: VecDeque<(String, String)>,
}
impl Sockets {
    pub(super) fn close_context(&mut self, context: Reference) {
        self.items.retain(|_, socket| socket.context != context);
        self.order.retain(|key| self.items.contains_key(key));
    }
    pub(super) fn clear(&mut self) -> usize {
        let count = self.items.len();
        self.items.clear();
        self.order.clear();
        count
    }
}
impl Worker {
    #[expect(
        clippy::too_many_lines,
        reason = "one match over every WebSocket event phase"
    )]
    pub(super) fn websocket_event(&mut self, event: &Event) {
        let phase = match event.method.as_str() {
            "Network.webSocketCreated" => "created",
            "Network.webSocketHandshakeResponseReceived" => "opened",
            "Network.webSocketFrameSent" | "Network.webSocketFrameReceived" => "frame",
            "Network.webSocketClosed" => "closed",
            "Network.webSocketFrameError" => "error",
            _ => return,
        };
        let Some(session) = event.session_id.as_deref() else {
            return;
        };
        let Some((context, page, worker)) = self.index.diagnostic_scope(session) else {
            return;
        };
        if let Some(native) = event.params["requestId"].as_str() {
            // HAR capture records sockets independently of diagnostic history.
            let key = format!("{session}\u{0}{native}");
            let p = &event.params;
            match phase {
                "created" => self.har_archives.capture_socket_start(
                    context,
                    key,
                    page.filter(|_| worker.is_none()),
                    p["url"].as_str().unwrap_or(""),
                ),
                "opened" => self.har_archives.capture_socket_event(
                    &key,
                    &crate::browser::har::SocketEvent::Open {
                        status: p["response"]["status"].as_u64(),
                    },
                ),
                "frame" => self.har_archives.capture_socket_event(
                    &key,
                    &crate::browser::har::SocketEvent::Frame {
                        sent: event.method == "Network.webSocketFrameSent",
                        opcode: p["response"]["opcode"].as_u64().unwrap_or(1),
                        data: p["response"]["payloadData"].as_str().unwrap_or(""),
                    },
                ),
                "error" => self
                    .har_archives
                    .capture_socket_event(&key, &crate::browser::har::SocketEvent::Error),
                _ => self
                    .har_archives
                    .capture_socket_event(&key, &crate::browser::har::SocketEvent::Closed),
            }
        }
        if !self.diagnostics.history.active(context) {
            return;
        }
        let Some(native) = event.params["requestId"].as_str() else {
            return;
        };
        let key = (session.to_owned(), native.to_owned());
        if !self.websockets.items.contains_key(&key) {
            if self.websockets.order.len() == 512 {
                let oldest = self.websockets.order.pop_front().expect("Socket capacity");
                self.websockets.items.remove(&oldest);
                self.diagnostics.history.forget_sockets(1);
            }
            self.websockets.order.push_back(key.clone());
            self.websockets.items.insert(
                key.clone(),
                Socket {
                    id: Uuid::new_v4(),
                    context,
                    url: if phase == "created" {
                        diagnostics::url(event.params["url"].as_str().unwrap_or(""))
                    } else {
                        "[unavailable URL]".into()
                    },
                    missing: phase != "created",
                },
            );
        }
        let socket = &self.websockets.items[&key];
        if socket.context != context {
            return;
        }
        let p = &event.params;
        let mut data = json!({"webSocketId":socket.id,"url":socket.url,"initialEventMissing":socket.missing,"phase":phase,"worker":worker,"sourceTimestamp":p["timestamp"],"interception":false});
        if phase == "frame" {
            let payload = p["response"]["payloadData"].as_str().unwrap_or("");
            let (text, truncated) = diagnostics::text(payload);
            data["direction"] = json!(if event.method == "Network.webSocketFrameSent" {
                "sent"
            } else {
                "received"
            });
            data["opcode"] = p["response"]["opcode"].clone();
            data["payloadEncoding"] = json!(if p["response"]["opcode"] == 1 {
                "utf8"
            } else {
                "base64"
            });
            data["payloadEncodedBytes"] = json!(payload.len());
            data["text"] = json!(text);
            data["textTruncated"] = json!(truncated);
        }
        if phase == "opened" {
            data["status"] = p["response"]["status"].clone();
        }
        if phase == "error" {
            let (text, truncated) =
                diagnostics::text(p["errorMessage"].as_str().unwrap_or("WebSocket failure"));
            data["text"] = json!(text);
            data["textTruncated"] = json!(truncated);
        }
        self.diagnostics.history.push(
            context,
            page,
            DiagnosticKind::WebSocket,
            data,
            crate::browser::capture::now_ms().into(),
        );
        if phase == "closed" {
            self.websockets.items.remove(&key);
            self.websockets.order.retain(|entry| entry != &key);
        }
    }
}
