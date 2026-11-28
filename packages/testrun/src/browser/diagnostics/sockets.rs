//! Playwright WebSocket identities are independent of HTTP request identities.
use super::*;
#[derive(Clone)]
struct Socket {
    context: Reference,
    page: Reference,
    id: Uuid,
    url: String,
}
#[derive(Default)]
pub(super) struct Sockets {
    items: HashMap<ObjectRef, Socket>,
    order: VecDeque<ObjectRef>,
}
impl Sockets {
    pub(super) fn close_context(&mut self, context: Reference) {
        self.items.retain(|_, socket| socket.context != context);
        self.order.retain(|wire| self.items.contains_key(wire));
    }
    pub(super) fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
    }
    pub(super) fn forget(&mut self, wire: &ObjectRef) {
        self.items.remove(wire);
        self.order.retain(|entry| entry != wire);
    }
}
impl Diagnostics {
    pub(crate) fn socket_start(
        &mut self,
        wire: ObjectRef,
        context: Reference,
        page: Reference,
        source_url: &str,
        now: u128,
    ) {
        if !self.active(context) || self.sockets.items.contains_key(&wire) {
            return;
        }
        if self.sockets.order.len() == MAX_RECORDS {
            let oldest = self.sockets.order.pop_front().expect("Socket capacity");
            self.sockets.items.remove(&oldest);
            self.forget_sockets(1);
        }
        let socket = Socket {
            context,
            page,
            id: Uuid::new_v4(),
            url: url(source_url),
        };
        self.push(context,Some(page),DiagnosticKind::WebSocket,json!({"webSocketId":socket.id,"url":socket.url,"phase":"created","initialEventMissing":false,"interception":false}),now);
        self.sockets.order.push_back(wire.clone());
        self.sockets.items.insert(wire, socket);
    }
    pub(crate) fn socket_event(
        &mut self,
        wire: &ObjectRef,
        method: &str,
        params: &Value,
        now: u128,
    ) -> bool {
        let Some(socket) = self.sockets.items.get(wire).cloned() else {
            return false;
        };
        if !self.active(socket.context) {
            return true;
        }
        let (phase, direction) = match method {
            "open" => ("opened", None),
            "frameSent" => ("frame", Some("sent")),
            "frameReceived" => ("frame", Some("received")),
            "socketError" => ("error", None),
            "close" => ("closed", None),
            _ => return false,
        };
        let mut data = json!({"webSocketId":socket.id,"url":socket.url,"phase":phase,"direction":direction,"interception":false});
        if phase == "frame" {
            let payload = params["data"].as_str().unwrap_or("");
            let binary = params["opcode"] != 1;
            let (text, truncated) = text(payload);
            data["opcode"] = params["opcode"].clone();
            data["payloadEncoding"] = json!(if binary { "base64" } else { "utf8" });
            data["payloadEncodedBytes"] = json!(payload.len());
            data["text"] = json!(text);
            data["textTruncated"] = json!(truncated);
        }
        if phase == "error" {
            let (text, truncated) = text(params["error"].as_str().unwrap_or("WebSocket failure"));
            data["text"] = json!(text);
            data["textTruncated"] = json!(truncated);
        }
        self.push(
            socket.context,
            Some(socket.page),
            DiagnosticKind::WebSocket,
            data,
            now,
        );
        if phase == "closed" {
            self.sockets.forget(wire);
        }
        true
    }
}
