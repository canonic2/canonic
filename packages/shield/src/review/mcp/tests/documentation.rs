//! Bundled docs are discoverable, searchable and readable without project file access.
use super::*;
use crate::review::mcp::documentation as bundled;
use canonic_mcp_kit::docs::{Bundle as DocsBundle, DocsError, Documentation};

/// Follows `next_start_line` from line 1 and joins every returned `content`.
fn read_page(read: &dyn Fn(u64, u64) -> Value, limit: u64) -> String {
    let mut content = String::new();
    let mut start = 1;
    loop {
        let data = read(start, limit);
        assert_eq!(data["start_line"], start, "{data}");
        content.push_str(data["content"].as_str().unwrap());
        let Some(next) = data["next_start_line"].as_u64() else {
            return content;
        };
        assert!(next > start, "{data}");
        start = next;
    }
}

#[test]
fn bundled_documentation_pages_round_trip_through_list_and_read() {
    let (root, context, connection) = fixture();
    // Project files cannot replace the embedded guides.
    std::fs::create_dir(root.path().join("docs")).unwrap();
    std::fs::write(
        root.path().join("docs/project-mcp.md"),
        "project replacement",
    )
    .unwrap();
    let state = Store::open(root.path()).unwrap().read().unwrap();
    let mut paths = Vec::new();
    let mut offset = 0;
    loop {
        let result = invoke(
            &context,
            &connection,
            "shield_docs_list",
            json!({"offset":offset,"limit":2}),
        );
        assert!(result["error"].is_null(), "{result}");
        assert_eq!(result["data"]["version"], env!("CARGO_PKG_VERSION"));
        paths.extend(
            result["data"]["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["path"].as_str().unwrap().to_owned()),
        );
        let Some(next) = result["data"]["next_offset"].as_u64() else {
            break;
        };
        offset = next;
    }
    assert!(paths.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(paths.contains(&"policy-rules.md".to_owned()));
    for path in paths {
        let source = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("docs")
                .join(&path),
        )
        .unwrap();
        let read = |start: u64, limit: u64| {
            let result = invoke(
                &context,
                &connection,
                "shield_docs_read",
                json!({"path":path,"start_line":start,"limit":limit}),
            );
            assert!(result["error"].is_null(), "{result}");
            assert_eq!(result["redacted"], false);
            result["data"].clone()
        };
        // Line limits that end chunks at different lines, including one line
        // per read and a single whole-page read.
        for limit in [1, 7, 400] {
            assert_eq!(
                read_page(&read, limit).as_bytes(),
                source,
                "{path} at {limit}"
            );
        }
    }
    let after = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(after.policy_revision, state.policy_revision);
    assert!(after.proposals.is_empty());
}

#[test]
fn paginated_reads_reproduce_crlf_final_newlines_and_chunk_boundaries() {
    let guides = assert_fs::TempDir::new().unwrap();
    let pages = [
        (
            "crlf.md",
            "# Acme rules\r\n\r\nAllow `acme --watch`.\r\nDeny café.\r\n",
        ),
        ("mixed.md", "# Mixed\nfirst\r\n\nlast without newline"),
        ("trailing.md", "# Trailing\n\n\n"),
    ];
    for (path, text) in pages {
        std::fs::write(guides.path().join(path), text).unwrap();
    }
    let bytes = canonic_mcp_kit::docs::build::bundle(guides.path()).unwrap();
    let bundle: &'static DocsBundle = Box::leak(Box::new(DocsBundle::new(bytes.leak())));
    let docs = Documentation::new(bundle, "shield", "Shield", env!("CARGO_PKG_VERSION"));
    let open = || Ok(());
    for (path, text) in pages {
        let read = |start: u64, limit: u64| {
            bundled::call_with(
                &docs,
                "shield_docs_read",
                &json!({"path":path,"start_line":start,"limit":limit}),
                &open,
            )
            .unwrap()
        };
        let lines = text.split_inclusive('\n').count() as u64;
        assert_eq!(read(1, 1)["total_lines"], lines);
        for limit in [1, 2, 3, lines] {
            assert_eq!(read_page(&read, limit), text, "{path} at {limit}");
        }
    }
    let search = bundled::call_with(
        &docs,
        "shield_docs_search",
        &json!({"query":"CAFÉ","path":"crlf.md"}),
        &open,
    )
    .unwrap();
    // Search rows identify a line without its terminator.
    assert_eq!(
        search["matches"],
        json!([{"path":"crlf.md","line":4,"text":"Deny café."}])
    );
}

#[test]
fn documentation_failures_map_to_shield_codes_with_recovery() {
    static CORRUPT: DocsBundle = DocsBundle::new(b"not an Acme bundle");
    let not_found = DocsError::NotFound {
        list_tool: "shield_docs_list".into(),
    };
    assert_eq!(
        bundled::error_fields(&not_found),
        (
            "not_found",
            "Unknown documentation page; call shield_docs_list.".into()
        )
    );
    let (code, message) =
        bundled::error_fields(&DocsError::InvalidArguments("`limit` is too large.".into()));
    assert_eq!(
        (code, message.as_str()),
        ("invalid_input", "`limit` is too large.")
    );
    assert_eq!(
        bundled::error_fields(&DocsError::Cancelled),
        error_fields(&Error::Cancelled)
    );
    // A corrupt bundle fails every documentation call the same way.
    let docs = Documentation::new(&CORRUPT, "shield", "Shield", env!("CARGO_PKG_VERSION"));
    for (name, args) in [
        ("shield_docs_list", json!({})),
        ("shield_docs_read", json!({"path":"policy-rules.md"})),
    ] {
        let error = bundled::call_with(&docs, name, &args, &|| Ok(())).unwrap_err();
        assert_eq!(bundled::error_fields(&error).0, "documentation_unavailable");
    }
}

#[test]
fn catalog_publishes_the_shared_documentation_tools_in_shields_envelope() {
    static NONE: DocsBundle = DocsBundle::new(b"");
    let shared = Documentation::new(&NONE, "shield", "Shield", env!("CARGO_PKG_VERSION")).tools();
    let listing = tools();
    let listed = listing["tools"].as_array().unwrap();
    let envelope = &listed[0]["outputSchema"];
    for tool in &shared {
        let mut entry = listed
            .iter()
            .find(|entry| entry["name"] == tool.name.as_str())
            .unwrap()
            .clone();
        assert_eq!(&entry["outputSchema"], envelope);
        entry.as_object_mut().unwrap().remove("outputSchema");
        assert_eq!(entry, serde_json::to_value(tool).unwrap());
    }
    // The published catalog validates the shared schemas.
    let catalog = crate::mcp::catalog(None).unwrap();
    assert!(catalog.get("shield_docs_read").unwrap().read_only());
}

#[test]
fn documentation_search_returns_page_line_references_and_continuation() {
    let (_root, context, connection) = fixture();
    let search = invoke(
        &context,
        &connection,
        "shield_docs_search",
        json!({"query":"--WATCH","path":"project-mcp.md","limit":1}),
    );
    assert!(search["error"].is_null(), "{search}");
    let row = &search["data"]["matches"][0];
    assert_eq!(row["path"], "project-mcp.md");
    let read = invoke(
        &context,
        &connection,
        "shield_docs_read",
        json!({"path":row["path"],"start_line":row["line"],"limit":1}),
    );
    assert_eq!(
        read["data"]["content"]
            .as_str()
            .unwrap()
            .trim_end_matches(['\r', '\n']),
        row["text"]
    );
    assert!(search["data"]["next_offset"].is_number());
    let next = invoke(
        &context,
        &connection,
        "shield_docs_search",
        json!({"query":"--WATCH","path":"project-mcp.md","limit":1,"offset":search["data"]["next_offset"]}),
    );
    assert_ne!(next["data"]["matches"][0]["line"], row["line"]);
}

#[test]
fn documentation_search_preserves_surrounding_spaces() {
    let (_root, context, connection) = fixture();
    let read = invoke(
        &context,
        &connection,
        "shield_docs_read",
        json!({"path":"project-mcp.md","limit":1}),
    );
    let heading = read["data"]["content"]
        .as_str()
        .unwrap()
        .trim_end_matches(['\r', '\n']);
    let exact = invoke(
        &context,
        &connection,
        "shield_docs_search",
        json!({"path":"project-mcp.md","query":heading}),
    );
    assert!(exact["error"].is_null(), "{exact}");
    assert_eq!(exact["data"]["matches"][0]["line"], 1);
    for query in [format!(" {heading}"), format!("{heading} ")] {
        let result = invoke(
            &context,
            &connection,
            "shield_docs_search",
            json!({"path":"project-mcp.md","query":query}),
        );
        assert!(result["error"].is_null(), "{result}");
        assert_eq!(result["data"]["matches"], json!([]), "{result}");
    }
}

#[test]
fn documentation_refuses_invalid_pages_bounds_and_cancellation() {
    let (_root, context, connection) = fixture();
    for path in [
        "../README.md",
        "/etc/passwd",
        "missing.md",
        "POLICY-RULES.md",
    ] {
        for name in ["shield_docs_read", "shield_docs_search"] {
            let args = if name == "shield_docs_read" {
                json!({"path":path})
            } else {
                json!({"path":path,"query":"rule"})
            };
            let result = invoke(&context, &connection, name, args);
            assert_eq!(result["error"]["code"], "not_found");
            assert_eq!(
                result["error"]["message"],
                "Unknown documentation page; call shield_docs_list."
            );
        }
    }
    for (name, args) in [
        ("shield_docs_list", json!({"offset":-1})),
        ("shield_docs_list", json!({"limit":101})),
        (
            "shield_docs_read",
            json!({"path":"policy-rules.md","start_line":0}),
        ),
        (
            "shield_docs_read",
            json!({"path":"policy-rules.md","limit":401}),
        ),
        ("shield_docs_search", json!({"query":"  "})),
        ("shield_docs_search", json!({"query":"rule\nline"})),
        ("shield_docs_search", json!({"query":"rule","limit":0})),
        ("shield_docs_list", json!({"project":"other"})),
    ] {
        let result = invoke(&context, &connection, name, args);
        assert_eq!(result["error"]["code"], "invalid_input", "{result}");
        assert!(result["error"]["operator_route"].is_null());
    }
    let empty = invoke(
        &context,
        &connection,
        "shield_docs_search",
        json!({"query":"acme-no-such-documentation-term"}),
    );
    assert_eq!(empty["data"]["matches"], json!([]));
    let beyond = invoke(
        &context,
        &connection,
        "shield_docs_read",
        json!({"path":"policy-rules.md","start_line":1_000_000}),
    );
    assert_eq!(beyond["data"]["content"], "");
    assert!(beyond["data"]["next_start_line"].is_null());
    let ticks = std::cell::Cell::new(0);
    let cancelled = call(
        &context,
        &connection,
        "shield_docs_search",
        &json!({"query":"rule"}),
        &|| {
            ticks.set(ticks.get() + 1);
            if ticks.get() > 3 {
                Err(crate::policy::Error::Interrupted)
            } else {
                Ok(())
            }
        },
    );
    assert_eq!(cancelled["error"]["code"], "cancelled");
    assert!(cancelled["data"].is_null());
    // The interrupted call leaves the prepared guides usable.
    let after = invoke(
        &context,
        &connection,
        "shield_docs_search",
        json!({"query":"rule","limit":1}),
    );
    assert!(after["error"].is_null(), "{after}");
}
