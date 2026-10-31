//! Resource content wire shapes and validated deserialization.

use super::*;
use crate::{Content, ToolResult};

#[test]
fn resource_content_mixed_result_round_trip_preserves_wire_shapes() {
    let mut link = Resource::new("acme://reports/a%2Fb", "Report");
    link.title = Some("Acme report".into());
    link.description = Some("Checks completed".into());
    link.mime_type = Some("text/plain".into());
    link.size = Some(12);
    let result = ToolResult::structured(json!({"passed":true}), false)
        .with_content(Content::image(&[0, 255], "image/png"))
        .with_content(Content::resource_link(link.clone()).unwrap())
        .with_content(Content::resource(
            ResourceContents::text("acme://summary", Some("text/plain".into()), "Passed").unwrap(),
        ))
        .with_content(Content::resource(
            ResourceContents::blob("acme://capture", Some("image/png".into()), &[0, 255, 42])
                .unwrap(),
        ));
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(
        value["content"],
        json!([
            {"type":"text","text":"{\"passed\":true}"},
            {"type":"image","data":"AP8=","mimeType":"image/png"},
            {"type":"resource_link","uri":"acme://reports/a%2Fb","name":"Report","title":"Acme report","description":"Checks completed","mimeType":"text/plain","size":12},
            {"type":"resource","resource":{"uri":"acme://summary","mimeType":"text/plain","text":"Passed"}},
            {"type":"resource","resource":{"uri":"acme://capture","mimeType":"image/png","blob":"AP8q"}}
        ])
    );
    assert_eq!(value["structuredContent"], json!({"passed":true}));
    assert_eq!(serde_json::from_value::<ToolResult>(value).unwrap(), result);
    assert_eq!(ResourceLink::new(link.clone()).unwrap().resource(), &link);
}

#[test]
fn resource_content_links_validate_at_construction_and_restore() {
    for mut resource in [
        Resource::new("relative", "Report"),
        Resource::new("acme://report", "x".repeat(4097)),
    ] {
        assert!(Content::resource_link(resource.clone()).is_err());
        assert!(serde_json::from_value::<ResourceLink>(json!(&resource)).is_err());
        resource.uri = "acme://report".into();
        resource.name = "Report".into();
        resource.description = Some("x".repeat(16385));
        assert!(Content::resource_link(resource).is_err());
    }
    for value in [
        json!({"type":"resource_link","uri":"relative","name":"Report"}),
        json!({"type":"resource_link","uri":"acme://report"}),
        json!({"type":"resource_link","uri":"acme://report","name":"Report","size":-1}),
    ] {
        assert!(serde_json::from_value::<Content>(value).is_err());
    }
}

#[test]
fn resource_content_restore_refuses_malformed_or_ambiguous_payloads() {
    for value in [
        json!(null),
        json!([]),
        json!("report"),
        json!({"text":"report"}),
        json!({"uri":42,"text":"report"}),
        json!({"uri":"relative","text":"report"}),
        json!({"uri":"acme://report"}),
        json!({"uri":"acme://report","text":"report","blob":""}),
        json!({"uri":"acme://report","text":null}),
        json!({"uri":"acme://report","text":42}),
        json!({"uri":"acme://report","blob":null}),
        json!({"uri":"acme://report","blob":42}),
        json!({"uri":"acme://report","text":"report","mimeType":null}),
        json!({"uri":"acme://report","text":"report","mimeType":"x".repeat(257)}),
        json!({"uri":"acme://report","text":"x".repeat(MAX_BYTES + 1)}),
    ] {
        assert!(serde_json::from_value::<ResourceContents>(value).is_err());
    }
    for blob in [
        "A", "AA", "AAA", "====", "AA=A", "A===", "AB==", "AAB=", "AA-_", "AA\n=", "é==",
    ] {
        assert!(
            serde_json::from_value::<ResourceContents>(json!({"uri":"acme://capture","blob":blob}))
                .is_err(),
            "{blob}"
        );
    }
}

#[test]
fn resource_content_restore_retains_raw_byte_accounting() {
    for bytes in [
        vec![],
        vec![0],
        vec![0, 255],
        vec![0, 255, 42],
        vec![255; MAX_BYTES],
    ] {
        let content = ResourceContents::blob("acme://capture", None, &bytes).unwrap();
        let restored: ResourceContents = serde_json::from_value(json!(&content)).unwrap();
        assert_eq!(restored, content);
        assert_eq!(restored.raw_bytes, bytes.len());
        if bytes.len() == MAX_BYTES {
            assert!(
                ReadResourceResult::new(vec![
                    restored,
                    ResourceContents::text("acme://extra", None, "x").unwrap()
                ])
                .is_err()
            );
        }
    }
    let oversized = crate::tool::base64(&vec![0; MAX_BYTES + 1]);
    assert!(
        serde_json::from_value::<ResourceContents>(
            json!({"uri":"acme://capture","blob":oversized})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ResourceContents>(
            json!({"uri":"acme://capture","blob":"A".repeat(MAX_BYTES.div_ceil(3) * 4 + 4)})
        )
        .is_err()
    );
    let text = ResourceContents::text("acme://report", None, "x".repeat(MAX_BYTES)).unwrap();
    assert_eq!(
        serde_json::from_value::<ResourceContents>(json!(&text)).unwrap(),
        text
    );
}
