//! Bundled docs are discoverable, searchable and readable without project file access.
use super::*;

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
        let source = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("docs")
                .join(&path),
        )
        .unwrap();
        let mut actual = Vec::new();
        let mut start = 1;
        loop {
            let read = invoke(
                &context,
                &connection,
                "shield_docs_read",
                json!({"path":path,"start_line":start,"limit":10}),
            );
            assert!(read["error"].is_null(), "{read}");
            if read["data"]["total_lines"].as_u64().unwrap() >= start {
                actual.extend(
                    read["data"]["content"]
                        .as_str()
                        .unwrap()
                        .split('\n')
                        .map(str::to_owned),
                );
            }
            let Some(next) = read["data"]["next_start_line"].as_u64() else {
                break;
            };
            start = next;
        }
        assert_eq!(
            actual.join("\n"),
            source.lines().collect::<Vec<_>>().join("\n")
        );
    }
    let after = Store::open(root.path()).unwrap().read().unwrap();
    assert_eq!(after.policy_revision, state.policy_revision);
    assert!(after.proposals.is_empty());
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
    assert_eq!(read["data"]["content"], row["text"]);
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
    let heading = read["data"]["content"].as_str().unwrap();
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
    for path in ["../README.md", "/etc/passwd", "missing.md"] {
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
        ("shield_docs_search", json!({"query":"rule","limit":0})),
        ("shield_docs_list", json!({"project":"other"})),
    ] {
        assert_eq!(
            invoke(&context, &connection, name, args)["error"]["code"],
            "invalid_input"
        );
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
}
