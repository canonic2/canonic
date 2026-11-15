//! Pack discovery arguments and results pass the published transport schemas.
use super::*;

#[test]
fn pack_discovery_invalid_references_explain_catalog_syntax_and_installed_tool() {
    let (server, _project) = fixture(Some("codex"));
    let mut messages = initialize(&json!({}), "codex").to_vec();
    for (id, reference) in [
        (2, "pack:00000000-0000-0000-0000-000000000001"),
        (3, "main:acme/runtime"),
    ] {
        messages.push(tool(
            json!(id),
            "pack_info",
            &json!({"reference":reference,"view":"summary"}),
        ));
    }
    let outputs = exchange(server, &messages);
    for id in [2, 3] {
        let response = reply(&outputs, &json!(id));
        assert!(response["error"].is_null(), "{response}");
        let result = &response["result"];
        assert_eq!(result["isError"], true, "{result}");
        let error = &result["structuredContent"]["error"];
        assert_eq!(error["code"], "invalid_input");
        let message = error["message"].as_str().unwrap();
        assert!(message.contains("pack_show"), "{message}");
        if id == 3 {
            assert!(message.contains("alias/publisher/name"), "{message}");
        }
    }
}

#[test]
fn pack_discovery_search_accepts_maximum_limit_and_empty_results() {
    let (server, project) = fixture(Some("codex"));
    let source = assert_fs::TempDir::new().unwrap();
    let directory = source.path().canonicalize().unwrap();
    std::fs::write(
        directory.join("catalog.json"),
        json!({"schema_version":1,"packs":[{
            "id":"acme/runtime","version":"1.0.0","path":"runtime.json","sha256":"0".repeat(64)
        }]})
        .to_string(),
    )
    .unwrap();
    crate::review::update_operator_fixture(&project.review, |operator| {
        operator["sources"]["tools"] =
            json!(canonic_registry_kit::source::Source::local(&directory).unwrap());
        operator["default"] = json!("tools");
    })
    .unwrap();
    let before = project.revision();
    let mut messages = initialize(&json!({}), "codex").to_vec();
    for (index, query) in [
        "runtime",
        "acme-missing-build",
        "acme-missing-test",
        "acme-missing-compile",
        "acme-missing-graph",
        "acme-missing-copy",
        "acme-missing-parallel",
    ]
    .into_iter()
    .enumerate()
    {
        messages.push(tool(
            json!(index + 2),
            "pack_search",
            &json!({"query":query,"limit":100}),
        ));
    }
    let outputs = exchange(server, &messages);
    for index in 0..7 {
        let response = reply(&outputs, &json!(index + 2));
        assert!(response["error"].is_null(), "{response}");
        let result = &response["result"];
        assert_eq!(result["isError"], false, "{result}");
        let data = &result["structuredContent"]["data"];
        assert_eq!(
            data["packs"].as_array().unwrap().len(),
            usize::from(index == 0)
        );
        assert!(data["next_cursor"].is_null(), "{data}");
    }
    assert_eq!(project.revision(), before);
}
