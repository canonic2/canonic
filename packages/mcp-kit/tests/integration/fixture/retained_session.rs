//! Deterministic Acme data shared by retained-session measurements and checks.

use canonic_mcp_kit::{
    Annotations, Catalog, Client, RpcError, ServerInfo, Tool, ToolResult, modern, rpc,
    stdio::{Context, Handler, Outcome, Request},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

pub(crate) const SMALL_BYTES: usize = 32;
pub(crate) const LARGE_BYTES: usize = 256 * 1024;

pub(crate) struct Prepared {
    pub(crate) catalog: Arc<Catalog>,
    pub(crate) handler: Arc<Acme>,
    pub(crate) catalog_time: Duration,
    pub(crate) result_time: Duration,
}

pub(crate) struct Acme {
    small: ToolResult,
    large: ToolResult,
    calls: AtomicUsize,
}

impl Acme {
    pub(crate) fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", "1"))
    }

    fn call(&self, request: Request, _: &Context) -> Outcome {
        self.calls.fetch_add(1, Ordering::Relaxed);
        match request {
            Request::Tool(call) if call.name == "acme_small" => Outcome::Tool(self.small.clone()),
            Request::Tool(call) if call.name == "acme_large" => Outcome::Tool(self.large.clone()),
            _ => Outcome::Method(Err(RpcError::new(
                rpc::METHOD_NOT_FOUND,
                "Unknown Acme operation",
            ))),
        }
    }
}

pub(crate) fn prepare(count: usize) -> Prepared {
    assert!(
        count == 0 || count >= 2,
        "fixture needs both callable tools"
    );
    let result_start = Instant::now();
    let handler = Arc::new(Acme {
        small: ToolResult::structured(json!({"value":"a".repeat(SMALL_BYTES)}), false),
        large: ToolResult::structured(json!({"value":"a".repeat(LARGE_BYTES)}), false),
        calls: AtomicUsize::new(0),
    });
    let result_time = result_start.elapsed();
    let input = json!({"type":"object","properties":{"value":{"type":"string","maxLength":64}},
        "required":["value"],"additionalProperties":false});
    let output = json!({"type":"object","properties":{"value":{"type":"string"}},
        "required":["value"],"additionalProperties":false});
    let tools = (0..count)
        .map(|index| {
            let name = if index + 2 == count {
                "acme_small".into()
            } else if index + 1 == count {
                "acme_large".into()
            } else {
                format!("acme_unused_{index:04}")
            };
            Tool::new(name, "Return prepared Acme data.", input.clone())
                .with_output_schema(output.clone())
                .with_annotations(Annotations {
                    read_only_hint: Some(true),
                    ..Annotations::default()
                })
        })
        .collect();
    let catalog_start = Instant::now();
    let catalog = Arc::new(Catalog::new(tools).expect("valid Acme catalog"));
    Prepared {
        catalog,
        handler,
        catalog_time: catalog_start.elapsed(),
        result_time,
    }
}

pub(crate) fn request(id: Value, method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION,
        modern::CAPABILITIES_KEY:{}});
    rpc::request(id, method, params)
}

pub(crate) fn check_result(reply: &Value, size: usize) {
    assert!(reply.get("error").is_none(), "unexpected RPC failure");
    let result = &reply["result"];
    assert_eq!(result["isError"], false);
    let structured = &result["structuredContent"];
    assert_eq!(
        structured["value"].as_str().expect("string result").len(),
        size
    );
    let text: Value = serde_json::from_str(
        result["content"][0]["text"]
            .as_str()
            .expect("text fallback"),
    )
    .expect("JSON text fallback");
    assert_eq!(&text, structured, "text and structured results agree");
}
