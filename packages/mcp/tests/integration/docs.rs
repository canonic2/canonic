//! Bundled documentation tools through the stdio driver and an independent runtime.

use crate::fixture::{Host, Script, result_text};
use canonic_mcp::{
    Catalog, Inbound, ServerInfo, Session, SessionOptions,
    docs::{Bundle, Documentation, build},
    rpc,
    stdio::{Options, Outcome, Request},
};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const GUIDE: &str = "# Acme guide\n\nInstall Acme.\r\nRun `acme start`.\n";

fn documentation() -> Documentation {
    documentation_of(GUIDE)
}

/// Bundles `guides/start.md` from a temporary folder and leaks it, as a `static` would hold it.
fn documentation_of(page: &str) -> Documentation {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let folder = std::env::temp_dir().join(format!(
        "acme-docs-driver-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let folder = TempFolder(folder);
    std::fs::create_dir_all(folder.0.join("guides")).expect("create the guide folder");
    std::fs::write(folder.0.join("guides/start.md"), page).expect("write the guide");
    let bytes = build::bundle(&folder.0).expect("bundle the guide");
    let bundle: &'static Bundle =
        Box::leak(Box::new(Bundle::new(Box::leak(bytes.into_boxed_slice()))));
    Documentation::new(bundle, "acme", "Acme", "2.0.0")
}

struct TempFolder(PathBuf);

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An initialized driver that answers only `docs` calls, with default options.
fn serve(docs: Documentation) -> (Host, Arc<Catalog>) {
    let catalog = Arc::new(Catalog::new(docs.tools()).expect("valid documentation tools"));
    let handler = Script::new(move |request, context| match request {
        Request::Tool(call) => {
            let outcome = docs
                .call(&call.name, &call.arguments, &|| context.check())
                .expect("only documentation tools are published");
            Outcome::Tool(Documentation::result(outcome))
        }
        _ => Outcome::Done,
    });
    let mut host = Host::start_with(Arc::new(handler), catalog.clone(), Options::default());
    host.initialize(json!({}));
    (host, catalog)
}

#[test]
fn docs_tools_list_validate_and_answer_through_the_driver() {
    let (host, catalog) = serve(documentation());

    host.send(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}));
    assert_eq!(host.recv()["result"], catalog.listing());

    host.call(2, "acme_docs_search", json!({"query":"ACME START"}));
    let reply = host.recv();
    assert_eq!(reply["result"]["isError"], false);
    assert_eq!(
        reply["result"]["structuredContent"],
        json!({"version":"2.0.0","total":1,"next_offset":null,
            "matches":[{"path":"guides/start.md","line":4,"text":"Run `acme start`."}]})
    );

    host.call(
        3,
        "acme_docs_read",
        json!({"path":"guides/start.md","limit":400}),
    );
    assert_eq!(host.recv()["result"]["structuredContent"]["content"], GUIDE);

    host.call(
        4,
        "acme_docs_read",
        json!({"path":"guides/start.md","project":"acme"}),
    );
    let refused = host.recv();
    assert_eq!(refused["result"]["isError"], true);
    assert!(result_text(&refused).contains("project"));

    host.call(5, "acme_docs_read", json!({"path":"../start.md"}));
    let missing = host.recv();
    assert_eq!(missing["result"]["isError"], true);
    assert_eq!(
        result_text(&missing),
        "Unknown documentation page; call acme_docs_list."
    );
    host.assert_quiet();
}

#[test]
fn escaping_heavy_pages_paginate_within_the_default_output_frame() {
    // Each control character encodes to 13 bytes across the structured
    // content and its JSON text copy, so these 480 KiB would exceed 4 MiB.
    let line = format!("acme {}\n", "\u{1}".repeat(60 * 1024));
    let page = line.repeat(8);
    let (host, _) = serve(documentation_of(&page));

    let mut content = String::new();
    let mut start = 1;
    for id in 1.. {
        host.call(
            id,
            "acme_docs_read",
            json!({"path":"guides/start.md","start_line":start,"limit":400}),
        );
        let reply = host.recv();
        assert_eq!(reply["result"]["isError"], false, "read {id}");
        let result = &reply["result"]["structuredContent"];
        content.push_str(result["content"].as_str().unwrap());
        match result["next_start_line"].as_u64() {
            Some(next) => start = next,
            None => break,
        }
    }
    assert_eq!(content, page);

    host.call(100, "acme_docs_search", json!({"query":"acme","limit":100}));
    let reply = host.recv();
    assert_eq!(reply["result"]["isError"], false);
    let result = &reply["result"]["structuredContent"];
    let rows = result["matches"].as_array().unwrap().len();
    assert!((1..8).contains(&rows));
    assert_eq!(
        (result["total"].clone(), result["next_offset"].clone()),
        (json!(8), json!(rows))
    );
    host.assert_quiet();
}

#[test]
fn an_independent_runtime_answers_docs_calls_through_the_core() {
    let docs = documentation();
    let mut session = Session::new(
        Arc::new(Catalog::new(docs.tools()).unwrap()),
        SessionOptions::default(),
    );
    let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"acme-host","version":"1"}}});
    let Inbound::Initialize { id, .. } = session.receive_value(initialize) else {
        panic!("initialize");
    };
    session.accept_initialize(id, &ServerInfo::new("acme", "2.0.0"));
    let _ = session.receive_value(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));

    let call = rpc::request(
        json!(7),
        "tools/call",
        json!({"name":"acme_docs_list","arguments":{}}),
    );
    let Inbound::Tool { id, call } = session.receive_value(call) else {
        panic!("tool call");
    };
    let outcome = docs.call(&call.name, &call.arguments, &|| Ok(())).unwrap();
    assert!(session.finish(&id));
    let reply = rpc::success(
        id,
        serde_json::to_value(Documentation::result(outcome)).unwrap(),
    );
    assert_eq!(
        reply["result"]["structuredContent"]["documents"],
        json!([{"path":"guides/start.md","title":"Acme guide","total_lines":4}])
    );
}
