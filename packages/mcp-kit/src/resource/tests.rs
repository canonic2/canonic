//! Resource boundaries and direct-runtime protocol fixtures.

use super::*;
use crate::{Catalog, Inbound, ServerInfo, Session, SessionOptions};
use std::sync::Arc;

fn catalog(count: usize) -> ResourceCatalog {
    ResourceCatalog::new(
        (0..count)
            .map(|i| Resource::new(format!("acme://reports/{i}"), "Report"))
            .collect(),
        vec![ResourceTemplate::new(
            "acme://reports/{id}{?format}",
            "Reports",
        )],
    )
    .unwrap()
}

fn session(resources: Option<ResourceCatalog>, version: &str) -> Session {
    let enabled = resources.is_some();
    let mut session = Session::new(
        Arc::new(Catalog::new(vec![]).unwrap()),
        SessionOptions {
            resources: resources.map(Arc::new),
            // Reserved names cannot bypass capability checks.
            extension_methods: vec!["resources/read".into(), "resources/list".into()],
            ..SessionOptions::default()
        },
    );
    assert!(matches!(session.receive_value(rpc::request(json!(0), "initialize", json!({"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"acme","version":"1"}}))), Inbound::Initialize { .. }));
    let reply = session.accept_initialize(json!(0), &ServerInfo::new("acme", "1"));
    assert_eq!(reply["result"]["protocolVersion"], version);
    assert_eq!(
        reply["result"]["capabilities"].get("resources").is_some(),
        enabled
    );
    session.receive_value(rpc::notification("notifications/initialized", json!({})));
    session
}

fn reply(session: &mut Session, method: &str, params: Value) -> Value {
    let Inbound::Reply(reply) = session.receive_value(rpc::request(json!(1), method, params))
    else {
        panic!("expected immediate reply")
    };
    reply
}

#[test]
fn resources_opt_in_empty_and_both_versions() {
    for version in crate::SUPPORTED_PROTOCOL_VERSIONS {
        let mut disabled = session(None, version);
        for method in [
            "resources/list",
            "resources/templates/list",
            "resources/read",
        ] {
            assert_eq!(
                reply(&mut disabled, method, json!({"uri":"acme://guide"}))["error"]["code"],
                rpc::METHOD_NOT_FOUND
            );
        }
        let mut enabled = session(Some(ResourceCatalog::new(vec![], vec![]).unwrap()), version);
        assert_eq!(
            reply(&mut enabled, "resources/list", json!({}))["result"],
            json!({"resources":[]})
        );
        assert_eq!(
            reply(&mut enabled, "resources/templates/list", json!({}))["result"],
            json!({"resourceTemplates":[]})
        );
    }
}

#[test]
fn resources_paginate_each_catalog_and_refuse_bad_cursors() {
    let mut s = session(Some(catalog(130)), "2025-11-25");
    let mut uris = Vec::new();
    let mut params = json!({});
    loop {
        let page = reply(&mut s, "resources/list", params)["result"].clone();
        uris.extend(
            page["resources"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["uri"].as_str().unwrap().to_owned()),
        );
        let Some(cursor) = page.get("nextCursor") else {
            break;
        };
        params = json!({"cursor":cursor});
    }
    assert_eq!(
        uris,
        (0..130)
            .map(|i| format!("acme://reports/{i}"))
            .collect::<Vec<_>>()
    );
    for cursor in [
        json!(null),
        json!(1),
        json!(""),
        json!("resources:0"),
        json!("resources:65"),
        json!("resources:192"),
        json!("resources:064"),
        json!("templates:64"),
    ] {
        assert_eq!(
            reply(&mut s, "resources/list", json!({"cursor":cursor}))["error"]["code"],
            rpc::INVALID_PARAMS
        );
    }
    let templates = ResourceCatalog::new(
        vec![],
        (0..130)
            .map(|i| ResourceTemplate::new(format!("acme://reports/{i}/{{id}}"), "Report"))
            .collect(),
    )
    .unwrap();
    assert_eq!(
        templates
            .listing(&json!({"cursor":"templates:64"}), true)
            .unwrap()["resourceTemplates"]
            .as_array()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(
        templates
            .listing(&json!({"cursor":"templates:128"}), true)
            .unwrap()["resourceTemplates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        templates
            .listing(&json!({"cursor":"resources:64"}), true)
            .is_err()
    );
}

#[test]
fn resources_validate_addresses_metadata_duplicates_and_limits() {
    for uri in [
        "",
        "relative/file",
        "acme://bad space",
        "acme://bad%xy",
        "acme://é",
        "1bad:uri",
        "acme://x\0",
    ] {
        assert!(validate_uri(uri).is_err(), "{uri}");
    }
    for uri in [
        "acme://reports/a%2Fb",
        "file:///guide.md",
        "urn:acme:report",
        "acme:opaque#part",
    ] {
        validate_uri(uri).unwrap();
    }
    for template in [
        "acme://{",
        "acme://{bad name}",
        "acme://{id:00000}",
        "acme://{id:0}",
        "acme://{id:01}",
        "acme://{id:10000}",
        "acme://bad%xy/{id}",
        "acme://{id!}",
    ] {
        assert!(
            ResourceCatalog::new(vec![], vec![ResourceTemplate::new(template, "Report")]).is_err(),
            "{template}"
        );
    }
    for template in [
        "{+path}",
        "acme://reports/{id}{?format}",
        "{#fragment}",
        "{/path*}",
        "{id:4}",
        "{id:9999}",
    ] {
        ResourceCatalog::new(vec![], vec![ResourceTemplate::new(template, "Report")]).unwrap();
    }
    let r = Resource::new("acme://guide", "Guide");
    assert!(matches!(
        ResourceCatalog::new(vec![r.clone(), r], vec![]),
        Err(ResourceError::Duplicate(_))
    ));
    let t = ResourceTemplate::new("{id}", "Report");
    assert!(matches!(
        ResourceCatalog::new(vec![], vec![t.clone(), t]),
        Err(ResourceError::Duplicate(_))
    ));
    assert_eq!(catalog(MAX_ENTRIES - 1).resources.len(), MAX_ENTRIES - 1);
    assert!(
        ResourceCatalog::new(
            (0..=MAX_ENTRIES)
                .map(|i| Resource::new(format!("acme:{i}"), "x"))
                .collect(),
            vec![]
        )
        .is_err()
    );
    let mut r = Resource::new("acme://guide", "Guide");
    r.description = Some("x".repeat(16385));
    assert!(ResourceCatalog::new(vec![r], vec![]).is_err());
    let mut entries = catalog(100).resources;
    for entry in &mut entries {
        entry.description = Some("x".repeat(16384));
    }
    assert!(matches!(
        ResourceCatalog::new(entries, vec![]),
        Err(ResourceError::Limit("catalog bytes"))
    ));
    assert!(validate_uri(&format!("acme:{}", "x".repeat(MAX_URI_BYTES))).is_err());
}

#[test]
fn resources_read_validation_and_direct_runtime_cancellation() {
    for version in crate::SUPPORTED_PROTOCOL_VERSIONS {
        let mut s = session(Some(catalog(1)), version);
        for params in [
            json!({}),
            json!({"uri":null}),
            json!({"uri":"relative"}),
            json!({"uri":"acme://x","extra":1}),
            json!({"uri":"acme://x","_meta":null}),
            json!({"uri":"acme://x","_meta":{"progressToken":1.5}}),
        ] {
            assert_eq!(
                reply(&mut s, "resources/read", params)["error"]["code"],
                rpc::INVALID_PARAMS
            );
            assert_eq!(s.outstanding(), 0);
        }
        let params = json!({"uri":"acme://unlisted/a%2Fb","_meta":{"progressToken":7}});
        assert!(
            matches!(s.receive_value(rpc::request(json!(1), "resources/read", params.clone())), Inbound::Method { params: p, .. } if p == params)
        );
        assert_eq!(s.outstanding(), 1);
        let result = ReadResourceResult::new(vec![
            ResourceContents::text("acme://unlisted/a%2Fb", None, "report").unwrap(),
        ])
        .unwrap();
        let frame = rpc::encode_success(&json!(1), &result, 1024, &|| Ok(())).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&frame).unwrap()["result"]["contents"][0]["text"],
            "report"
        );
        assert!(s.finish(&json!(1)));
        s.receive_value(rpc::request(json!(1), "resources/read", params));
        assert!(matches!(
            s.receive_value(rpc::notification(
                "notifications/cancelled",
                json!({"requestId":1})
            )),
            Inbound::Cancel { .. }
        ));
        assert!(!s.finish(&json!(1)));
    }
}

#[test]
fn resources_encode_contents_and_bound_raw_data() {
    let result = ReadResourceResult::new(vec![
        ResourceContents::text("acme://guide", Some("text/markdown".into()), "# Guide").unwrap(),
        ResourceContents::blob("acme://capture", None, &[0, 255, 42]).unwrap(),
    ])
    .unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        json!({"contents":[{"uri":"acme://guide","mimeType":"text/markdown","text":"# Guide"},{"uri":"acme://capture","blob":"AP8q"}]})
    );
    assert!(ReadResourceResult::new(vec![]).is_err());
    let empty = ResourceContents::text("acme:x", None, "").unwrap();
    assert!(ReadResourceResult::new(vec![empty.clone(); MAX_CONTENTS]).is_ok());
    assert!(ReadResourceResult::new(vec![empty; MAX_CONTENTS + 1]).is_err());
    let full = ResourceContents::blob("acme:x", None, &vec![0; MAX_BYTES]).unwrap();
    assert!(ReadResourceResult::new(vec![full.clone()]).is_ok());
    assert!(
        ReadResourceResult::new(vec![
            full,
            ResourceContents::text("acme:x", None, "x").unwrap()
        ])
        .is_err()
    );
    assert!(ResourceContents::text("acme:x", None, "x".repeat(MAX_BYTES + 1)).is_err());
    assert!(ResourceContents::blob("acme:x", None, &vec![0; MAX_BYTES + 1]).is_err());
    assert!(ResourceContents::text("relative", None, "x").is_err());
    assert!(ResourceContents::text("acme:x", Some("x".repeat(257)), "x").is_err());
    assert_eq!(
        not_found("acme:missing").data,
        Some(json!({"uri":"acme:missing"}))
    );
}
