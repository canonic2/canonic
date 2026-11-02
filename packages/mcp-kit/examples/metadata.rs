//! An independent runtime retains request trace context and publishes custom metadata.
use canonic_mcp_kit::{
    Catalog, Content, Icon, Inbound, Metadata, ModernOptions, ServerInfo, Session, SessionOptions,
    Tool, ToolResult, modern, rpc,
};
use serde_json::json;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let icon = Icon::new(
        "https://example.com/icon.png",
        Some("image/png".into()),
        vec!["any".into()],
        None,
    )?;
    let mut tool = Tool::new("acme_status", "Read status", json!({"type":"object"}));
    tool.icons = vec![icon.clone()];
    tool.meta = Some(Metadata::new(json!({"com.acme/category":"reports"}))?);
    let mut server = ServerInfo::new("acme", "1");
    server.description = Some("Acme reporting service".into());
    server.website_url = Some("https://example.com".into());
    server.icons = vec![icon];
    let mut session = Session::try_new(
        Arc::new(Catalog::new(vec![tool])?),
        SessionOptions {
            modern: Some(ModernOptions::new(server)),
            ..SessionOptions::default()
        },
    )?;

    let id = json!(1);
    let request = rpc::request(
        id.clone(),
        "tools/call",
        json!({"name":"acme_status","_meta":{
            modern::VERSION_KEY:modern::PROTOCOL_VERSION,modern::CAPABILITIES_KEY:{},
            "traceparent":"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "baggage":"region=example"
        }}),
    );
    if let Inbound::Tool { .. } = session.receive_value(request) {
        let context = session
            .request_context(&id)
            .expect("admitted request context");
        // Feed these strings to your observability system. They grant no authority.
        let trace = context.trace_context();
        assert!(trace.traceparent().is_some());
        assert_eq!(trace.baggage(), Some("region=example"));
        let result = ToolResult::success(vec![Content::text("running")])
            .with_metadata(Metadata::new(json!({"com.acme/reportId":"example-1"}))?);
        let bytes = context.encode_success(&id, &result, 4096, &|| Ok(()))?;
        // An actual runtime checks cancellation/deadline before publication too.
        if session.finish(&id) {
            println!("{}", std::str::from_utf8(&bytes)?);
        }
    }
    Ok(())
}
