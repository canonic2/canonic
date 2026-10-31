//! Audio and annotation validation, wire shapes and boundary behavior.

use super::*;
use crate::{Content, Resource, ResourceCatalog, ResourceContents, ResourceTemplate, ToolResult};
use serde_json::json;

#[test]
fn content_audio_encodes_and_restores_exact_bytes() {
    for bytes in [
        vec![],
        vec![0],
        vec![0, 255],
        vec![0, 255, 42],
        vec![255; MAX_AUDIO_BYTES],
    ] {
        let audio = AudioContent::new(&bytes, "audio/wav").unwrap();
        assert_eq!(audio.data(), base64(&bytes));
        assert_eq!(audio.mime_type(), "audio/wav");
        let block = Content::audio(&bytes, "audio/wav").unwrap();
        let wire = json!(&block);
        assert_eq!(
            wire,
            json!({"type":"audio","data":base64(&bytes),"mimeType":"audio/wav"})
        );
        assert_eq!(serde_json::from_value::<Content>(wire).unwrap(), block);
    }
    assert!(Content::audio(&vec![0; MAX_AUDIO_BYTES + 1], "audio/wav").is_err());
    assert!(Content::audio(&[], "x".repeat(MAX_MIME_BYTES)).is_ok());
    for mime in [
        String::new(),
        "audio/\0wav".into(),
        "x".repeat(MAX_MIME_BYTES + 1),
    ] {
        assert!(Content::audio(&[], mime.clone()).is_err());
        assert!(
            serde_json::from_value::<Content>(json!({"type":"audio","data":"","mimeType":mime}))
                .is_err()
        );
    }
}

#[test]
fn content_audio_deserialization_refuses_invalid_data_and_bounds() {
    for value in [
        json!({"type":"audio","mimeType":"audio/wav"}),
        json!({"type":"audio","data":null,"mimeType":"audio/wav"}),
        json!({"type":"audio","data":""}),
        json!({"type":"audio","data":"","mimeType":42}),
        json!({"type":"audio","data":"","mimeType":"audio/wav","annotations":null}),
    ] {
        assert!(serde_json::from_value::<Content>(value).is_err());
    }
    for data in [
        "A".into(),
        "AA=A".into(),
        "A===".into(),
        "AB==".into(),
        "AAB=".into(),
        "AA-_".into(),
        "AA\n=".into(),
        base64(&vec![0; MAX_AUDIO_BYTES + 1]),
        "A".repeat(MAX_AUDIO_BYTES.div_ceil(3) * 4 + 4),
    ] {
        assert!(
            serde_json::from_value::<Content>(
                json!({"type":"audio","data":data,"mimeType":"audio/wav"})
            )
            .is_err()
        );
    }
}

#[test]
fn content_annotations_apply_to_every_block_at_the_correct_level() {
    let hints = ContentAnnotations::new(
        Some(vec![Role::User, Role::Assistant]),
        Some(0.7),
        Some("2026-10-31T12:30:00.120+02:00".into()),
    )
    .unwrap();
    assert_eq!(
        hints.audience(),
        Some([Role::User, Role::Assistant].as_slice())
    );
    assert_eq!(hints.priority(), Some(0.7));
    assert_eq!(hints.last_modified(), Some("2026-10-31T12:30:00.120+02:00"));
    let blocks = vec![
        Content::text("Acme"),
        Content::image(&[0, 255], "image/png"),
        Content::audio(&[0, 255], "audio/wav").unwrap(),
        Content::resource_link(Resource::new("acme://report", "Report")).unwrap(),
        Content::resource(ResourceContents::text("acme://summary", None, "Passed").unwrap()),
        Content::resource(ResourceContents::blob("acme://capture", None, &[0, 255]).unwrap()),
    ];
    for block in blocks {
        assert!(block.annotations().is_none());
        assert!(json!(&block).get("annotations").is_none());
        let annotated = block.with_annotations(hints.clone());
        let wire = json!(&annotated);
        assert_eq!(
            wire["annotations"],
            json!({"audience":["user","assistant"],"priority":0.7,"lastModified":"2026-10-31T12:30:00.120+02:00"})
        );
        assert!(wire["resource"].get("annotations").is_none());
        assert_eq!(annotated.annotations(), Some(&hints));
        assert_eq!(serde_json::from_value::<Content>(wire).unwrap(), annotated);
        let replaced = annotated.with_annotations(ContentAnnotations::default());
        assert_eq!(json!(&replaced)["annotations"], json!({}));
    }
}

#[test]
fn content_annotations_validate_values_without_normalizing_payloads() {
    for priority in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1, 1.1] {
        assert!(ContentAnnotations::new(None, Some(priority), None).is_err());
    }
    assert!(ContentAnnotations::new(Some(vec![Role::User; 3]), None, None).is_err());
    assert!(ContentAnnotations::new(None, None, Some("x".repeat(MAX_MODIFIED_BYTES))).is_ok());
    assert!(ContentAnnotations::new(None, None, Some("x".repeat(MAX_MODIFIED_BYTES + 1))).is_err());
    for value in [
        json!({}),
        json!({"audience":[]}),
        json!({"priority":0}),
        json!({"priority":1}),
        json!({"priority":0.0}),
        json!({"priority":1.0}),
        json!({"lastModified":"2026-10-31T12:30:00.000Z"}),
    ] {
        let hints: ContentAnnotations = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(json!(hints), value);
    }
    for value in [
        json!(null),
        json!([]),
        json!({"audience":["system"]}),
        json!({"audience":["user","user","assistant"]}),
        json!({"audience":null}),
        json!({"priority":null}),
        json!({"priority":"0.7"}),
        json!({"priority":-1}),
        json!({"priority":2}),
        json!({"lastModified":null}),
        json!({"lastModified":42}),
        json!({"lastModified":"bad\0date"}),
        json!({"lastModified":"x".repeat(MAX_MODIFIED_BYTES + 1)}),
    ] {
        assert!(serde_json::from_value::<ContentAnnotations>(value).is_err());
    }
}

#[test]
fn content_annotations_resource_discovery_retains_hints() {
    let hints = ContentAnnotations::new(Some(vec![Role::Assistant]), Some(1.0), None).unwrap();
    let mut resource = Resource::new("acme://report", "Report");
    resource.annotations = Some(hints.clone());
    let mut template = ResourceTemplate::new("acme://reports/{id}", "Reports");
    template.annotations = Some(hints.clone());
    let catalog = ResourceCatalog::new(vec![resource.clone()], vec![template]).unwrap();
    assert_eq!(
        catalog.listing(&json!({}), false).unwrap()["resources"][0]["annotations"],
        json!(&hints)
    );
    assert_eq!(
        catalog.listing(&json!({}), true).unwrap()["resourceTemplates"][0]["annotations"],
        json!(&hints)
    );
    let link = Content::resource_link(resource).unwrap();
    assert_eq!(link.annotations(), Some(&hints));
    assert_eq!(
        serde_json::from_value::<Content>(json!(&link)).unwrap(),
        link
    );
    let result = ToolResult::structured(json!({"passed":true}), false).with_content(link);
    assert_eq!(result.content[0], Content::text("{\"passed\":true}"));
}
