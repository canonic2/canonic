//! Acme status view: a static Apps document, text fallback and app-only refresh.
use canonic_mcp_kit::{
    AppsCatalog, Catalog, Client, Content, ModernOptions, RpcError, ServerInfo, SessionOptions,
    Tool, ToolResult, ToolUi, UiResource, UiVisibility,
    stdio::{self, Context, Handler, Options, Outcome, Request},
};
use serde_json::json;
use std::{io, sync::Arc};

const URI: &str = "ui://acme/status-dashboard";
const HTML: &[u8] = include_bytes!("apps/status.html");

fn catalog() -> Catalog {
    let linked = ToolUi::new(URI);
    let mut refresh = linked.clone();
    refresh.visibility = Some(vec![UiVisibility::App]);
    Catalog::new(vec![
        Tool::new(
            "acme_show_status",
            "Show the Acme service status.",
            json!({"type":"object"}),
        )
        .with_ui(linked)
        .with_annotations(canonic_mcp_kit::tool::Annotations {
            read_only_hint: Some(true),
            ..canonic_mcp_kit::tool::Annotations::default()
        }),
        Tool::new(
            "acme_refresh_status",
            "Refresh the Acme service status.",
            json!({"type":"object"}),
        )
        .with_ui(refresh)
        .with_annotations(canonic_mcp_kit::tool::Annotations {
            read_only_hint: Some(true),
            ..canonic_mcp_kit::tool::Annotations::default()
        }),
    ])
    .expect("valid Acme tools")
}

fn options() -> Options {
    Options {
        session: SessionOptions {
            apps: Some(Arc::new(
                AppsCatalog::new(vec![UiResource::new(URI, "Acme status", HTML)])
                    .expect("valid Acme HTML"),
            )),
            modern: Some(ModernOptions::new(ServerInfo::new("acme-apps", "1"))),
            ..SessionOptions::default()
        },
        ..Options::default()
    }
}

struct Acme;
impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme-apps", "1"))
    }
    fn call(&self, request: Request, context: &Context) -> Outcome {
        let Request::Tool(call) = request else {
            return Outcome::Done;
        };
        if !["acme_show_status", "acme_refresh_status"].contains(&call.name.as_str()) {
            return Outcome::Done;
        }
        let summary = if context.supports_apps() {
            "Acme is healthy. The status view is available."
        } else {
            "Acme is healthy."
        };
        let mut result = ToolResult::success(vec![Content::text(summary)]);
        result.structured_content = Some(json!({"service":"Acme","status":"healthy"}));
        Outcome::Tool(result)
    }
}

fn main() -> Result<(), stdio::ServeError> {
    stdio::serve(
        Arc::new(Acme),
        Arc::new(catalog()),
        io::BufReader::new(io::stdin()),
        io::stdout(),
        &options(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use canonic_mcp_kit::{Inbound, Session, apps, modern, rpc};
    use serde_json::Value;

    fn reply(event: Inbound) -> Value {
        let Inbound::Reply(value) = event else {
            panic!("reply")
        };
        value
    }

    #[test]
    fn apps_example_independent_runtime_negotiates_lists_reads_and_checks_results() {
        for version in ["2025-06-18", "2025-11-25", modern::PROTOCOL_VERSION] {
            for supported in [false, true] {
                let mut session = Session::try_new(Arc::new(catalog()), options().session).unwrap();
                let capabilities = if supported {
                    json!({"extensions":{apps::EXTENSION:{"mimeTypes":[apps::MIME_TYPE]}}})
                } else {
                    json!({})
                };
                let metadata = json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:capabilities});
                let params = |mut value: Value| {
                    if version == modern::PROTOCOL_VERSION {
                        value["_meta"] = metadata.clone();
                    }
                    value
                };
                if version == modern::PROTOCOL_VERSION {
                    let discovery = reply(session.receive_value(rpc::request(
                        json!(0),
                        "server/discover",
                        params(json!({})),
                    )));
                    assert_eq!(
                        discovery["result"]["capabilities"]["extensions"][apps::EXTENSION],
                        json!({})
                    );
                } else {
                    let Inbound::Initialize { id, .. } = session.receive_value(rpc::request(json!(0), "initialize", json!({"protocolVersion":version,"capabilities":capabilities,"clientInfo":{"name":"acme-host","version":"1"}}))) else { panic!("initialize") };
                    session.accept_initialize(id, &ServerInfo::new("acme-apps", "1"));
                    session
                        .receive_value(rpc::notification("notifications/initialized", json!({})));
                }
                let apps = supported && version != "2025-06-18";
                let listing = reply(session.receive_value(rpc::request(
                    json!(1),
                    "tools/list",
                    params(json!({})),
                )));
                assert_eq!(
                    listing["result"]["tools"].as_array().unwrap().len(),
                    if apps { 2 } else { 1 }
                );
                let Inbound::BundledResource { id, read, .. } = session.receive_value(
                    rpc::request(json!(2), "resources/read", params(json!({"uri":URI}))),
                ) else {
                    panic!("prepared read")
                };
                let bytes = if version == modern::PROTOCOL_VERSION {
                    session
                        .request_context(&id)
                        .unwrap()
                        .encode_success(&id, read.result(), 65536, &|| Ok(()))
                        .unwrap()
                } else {
                    rpc::encode_success(&id, read.result(), 65536, &|| Ok(())).unwrap()
                };
                let value: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(
                    value["result"]["contents"][0]["text"]
                        .as_str()
                        .unwrap()
                        .as_bytes(),
                    HTML
                );
                assert!(session.finish(&id));
                for name in ["acme_show_status", "acme_refresh_status"] {
                    let event = session.receive_value(rpc::request(
                        json!(3),
                        "tools/call",
                        params(json!({"name":name})),
                    ));
                    if name == "acme_refresh_status" && !apps {
                        assert_eq!(reply(event)["error"]["code"], rpc::INVALID_PARAMS);
                        continue;
                    }
                    let Inbound::Tool { id, call } = event else {
                        panic!("tool")
                    };
                    let contract = session.catalog().result_schema(&call.name).unwrap();
                    assert!(contract.validate(&ToolResult::success(vec![])).is_err());
                    let result =
                        ToolResult::structured(json!({"service":"Acme","status":"healthy"}), false)
                            .with_content(Content::text("Acme is healthy."));
                    assert!(contract.validate(&result).is_ok());
                    assert_eq!(session.supports_apps(Some(&id)), apps);
                    assert!(session.finish(&id));
                }
            }
        }
    }

    #[test]
    fn apps_example_real_handler_serves_embedded_html_and_both_tools_over_stdio() {
        let mut input = Vec::new();
        for (id, method, mut params) in [
            (1, "resources/read", json!({"uri":URI})),
            (2, "tools/call", json!({"name":"acme_show_status"})),
            (3, "tools/call", json!({"name":"acme_refresh_status"})),
        ] {
            params["_meta"] = json!({modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:{"extensions":{apps::EXTENSION:{"mimeTypes":[apps::MIME_TYPE]}}}});
            serde_json::to_writer(&mut input, &rpc::request(json!(id), method, params)).unwrap();
            input.push(b'\n');
        }
        let mut output = Vec::new();
        stdio::serve(
            Arc::new(Acme),
            Arc::new(catalog()),
            io::Cursor::new(input),
            &mut output,
            &options(),
        )
        .unwrap();
        let replies: Vec<Value> = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(replies.len(), 3);
        let read = replies.iter().find(|reply| reply["id"] == 1).unwrap();
        assert_eq!(
            read["result"]["contents"][0]["text"]
                .as_str()
                .unwrap()
                .as_bytes(),
            HTML
        );
        for id in [2, 3] {
            let reply = replies.iter().find(|reply| reply["id"] == id).unwrap();
            assert_eq!(
                reply["result"]["structuredContent"],
                json!({"service":"Acme","status":"healthy"})
            );
            assert_eq!(
                reply["result"]["content"][0]["text"],
                "Acme is healthy. The status view is available."
            );
        }
    }
}
