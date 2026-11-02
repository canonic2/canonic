//! Correctness controls for the reusable retained-session workload.

#[path = "fixture/retained_session.rs"]
mod workload;

use crate::fixture::Host;
use canonic_mcp_kit::{ModernOptions, ServerInfo, rpc, stdio::Options};
use serde_json::json;

#[test]
fn retained_session_fixture_preserves_schema_bounds_text_agreement_and_reused_ids() {
    let prepared = workload::prepare(2);
    eprintln!(
        "fixture preparation: catalog={:?}, result={:?}",
        prepared.catalog_time, prepared.result_time
    );
    let handler = prepared.handler;
    let mut options = Options::default();
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    let host = Host::start_with(handler.clone(), prepared.catalog, options);
    for (name, size) in [
        ("acme_small", workload::SMALL_BYTES),
        ("acme_large", workload::LARGE_BYTES),
    ] {
        host.send(&workload::request(
            json!(1),
            "tools/call",
            json!({"name":name,"arguments":{"value":"Acme"}}),
        ));
        workload::check_result(&host.recv(), size);
    }
    assert_eq!(handler.calls(), 2);
    host.send(&workload::request(
        json!(1),
        "tools/call",
        json!({"name":"acme_small","arguments":{"value":false}}),
    ));
    assert_eq!(host.recv()["result"]["isError"], true);
    assert_eq!(
        handler.calls(),
        2,
        "invalid input never reaches the handler"
    );
    host.finish().0.unwrap();
    let prepared = workload::prepare(2);
    let mut options = Options {
        max_output_bytes: 1024,
        ..Options::default()
    };
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    let host = Host::start_with(prepared.handler, prepared.catalog, options);
    host.send(&workload::request(
        json!(1),
        "tools/call",
        json!({"name":"acme_large","arguments":{"value":"Acme"}}),
    ));
    assert_eq!(host.recv()["error"]["code"], rpc::INTERNAL_ERROR);
    host.finish().0.unwrap();
}
