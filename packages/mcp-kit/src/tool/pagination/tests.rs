use super::*;

fn tools(count: usize) -> Vec<Tool> {
    (0..count)
        .map(|i| {
            Tool::new(
                format!("acme_{i}"),
                format!("Return item {i}."),
                json!({"type":"object"}),
            )
        })
        .collect()
}

fn collect(catalog: &Catalog, version: &str) -> Vec<Value> {
    let mut params = json!({});
    let mut all = vec![];
    loop {
        let page = catalog.list_page(&params, version).expect("valid page");
        let entries = page["tools"].as_array().expect("tool array");
        assert!(entries.len() <= PAGE_SIZE);
        all.extend(entries.iter().cloned());
        let Some(cursor) = page.get("nextCursor") else {
            break;
        };
        assert_ne!(entries.as_slice(), &[] as &[Value]);
        params = json!({"cursor":cursor});
    }
    all
}

#[test]
fn pagination_tools_handle_empty_single_exact_and_multiple_pages() {
    for count in [
        0,
        1,
        PAGE_SIZE,
        PAGE_SIZE + 1,
        PAGE_SIZE * 2,
        PAGE_SIZE * 2 + 1,
    ] {
        let catalog = Catalog::new(tools(count)).unwrap();
        let expected = catalog.listing()["tools"].as_array().unwrap().clone();
        for version in crate::SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .copied()
            .chain([crate::modern::PROTOCOL_VERSION])
        {
            assert_eq!(collect(&catalog, version), expected);
            let first = catalog.list_page(&json!({}), version).unwrap();
            assert_eq!(first.get("nextCursor").is_some(), count > PAGE_SIZE);
            assert!(first.get("resultType").is_none());
        }
        assert_eq!(catalog.tools().len(), count);
        assert_eq!(catalog.listing()["tools"].as_array().unwrap().len(), count);
    }
}

#[test]
fn pagination_filters_legacy_before_paging_and_retains_modern_metadata() {
    let mut definitions = tools(150);
    for (i, tool) in definitions.iter_mut().enumerate() {
        tool.output_schema = Some(if i % 2 == 0 {
            json!({"type":"array"})
        } else {
            json!({"type":"object"})
        });
        tool.title = Some(format!("Item {i}"));
        tool.annotations.read_only_hint = Some(true);
    }
    let catalog = Catalog::new(definitions).unwrap();
    let modern = collect(&catalog, crate::modern::PROTOCOL_VERSION);
    assert_eq!(modern.len(), 150);
    assert_eq!(modern[64]["title"], "Item 64");
    assert_eq!(modern[64]["annotations"]["readOnlyHint"], true);
    for version in crate::SUPPORTED_PROTOCOL_VERSIONS {
        let legacy = collect(&catalog, version);
        assert_eq!(legacy.len(), 75);
        assert_eq!(legacy[64]["name"], "acme_129");
        assert_eq!(
            legacy,
            modern
                .iter()
                .enumerate()
                .filter(|(i, _)| i % 2 == 1)
                .map(|(_, tool)| tool.clone())
                .collect::<Vec<_>>()
        );
    }
    let hidden = Catalog::new(
        tools(80)
            .into_iter()
            .map(|tool| tool.with_output_schema(json!({"type":"array"})))
            .collect(),
    )
    .unwrap();
    assert_eq!(
        hidden
            .list_page(&json!({}), crate::LATEST_PROTOCOL_VERSION)
            .unwrap(),
        json!({"tools":[]})
    );
}

#[test]
fn pagination_refuses_bad_foreign_changed_and_cross_revision_cursors() {
    let definitions = tools(130);
    let catalog = Catalog::new(definitions.clone()).unwrap();
    let version = crate::modern::PROTOCOL_VERSION;
    let first = catalog.list_page(&json!({}), version).unwrap();
    let cursor = first["nextCursor"].clone();
    for params in [
        json!({"cursor":null}),
        json!({"cursor":64}),
        json!({"cursor":""}),
        json!({"cursor":"prompts:64"}),
        json!({"cursor":"x".repeat(MAX_CURSOR_BYTES+1)}),
        json!({"requestState":"retry"}),
        json!({"inputResponses":{}}),
        json!({"limit":1}),
    ] {
        assert_eq!(
            catalog.list_page(&params, version).unwrap_err().code,
            rpc::INVALID_PARAMS
        );
    }
    let prefix = format!("tools:{}:{version}:", catalog.pages.digest);
    for offset in [
        "0",
        "1",
        "63",
        "064",
        "+64",
        "192",
        "-64",
        "18446744073709551616",
    ] {
        assert!(
            catalog
                .list_page(&json!({"cursor":format!("{prefix}{offset}")}), version)
                .is_err(),
            "{offset}"
        );
    }
    for other in crate::SUPPORTED_PROTOCOL_VERSIONS {
        assert!(catalog.list_page(&json!({"cursor":cursor}), other).is_err());
    }
    let legacy_cursor = catalog
        .list_page(&json!({}), crate::SUPPORTED_PROTOCOL_VERSIONS[0])
        .unwrap()["nextCursor"]
        .clone();
    assert!(
        catalog
            .list_page(
                &json!({"cursor":legacy_cursor}),
                crate::LATEST_PROTOCOL_VERSION
            )
            .is_err()
    );
    assert!(catalog.list_page(&json!({}), "future").is_err());
    let same = Catalog::new(definitions.clone()).unwrap();
    assert_eq!(
        same.list_page(&json!({"cursor":cursor}), version),
        catalog.list_page(&json!({"cursor":cursor}), version)
    );
    let mut changed = definitions.clone();
    changed[0].description.push('!');
    assert!(
        Catalog::new(changed)
            .unwrap()
            .list_page(&json!({"cursor":cursor}), version)
            .is_err()
    );
    let mut reordered = definitions;
    reordered.swap(0, 1);
    assert!(
        Catalog::new(reordered)
            .unwrap()
            .list_page(&json!({"cursor":cursor}), version)
            .is_err()
    );
}
