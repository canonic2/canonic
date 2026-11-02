use super::*;
use crate::{Content, Resource, ResourceContents, ToolResult};
use serde_json::json;

const PARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

#[test]
fn metadata_preserves_values_and_shares_validated_storage() {
    let value = json!({"com.acme/hints":{"nested":[null,true,3]},"":null});
    let meta = Metadata::new(value.clone()).unwrap();
    let copy = meta.clone();
    assert!(Arc::ptr_eq(&meta.0, &copy.0));
    assert_eq!(serde_json::to_value(meta).unwrap(), value);
    for value in [
        json!(null),
        json!([]),
        json!({"bad/key/again":0}),
        json!({"-bad/x":0}),
        json!({"a/ends-":0}),
        json!({"x":"a".repeat(MAX_METADATA_BYTES)}),
    ] {
        assert!(Metadata::new(value).is_err());
    }
    let mut deep = json!(0);
    for _ in 0..18 {
        deep = json!({"x":deep});
    }
    assert!(Metadata::new(deep).is_err());
    assert!(serde_json::from_value::<ToolResult>(json!({"content":[],"_meta":null})).is_err());
    let absent: ToolResult = serde_json::from_value(json!({"content":[]})).unwrap();
    assert!(absent.meta.is_none());
    let empty: ToolResult = serde_json::from_value(json!({"content":[],"_meta":{}})).unwrap();
    assert_eq!(serde_json::to_value(empty).unwrap()["_meta"], json!({}));
}

#[test]
fn metadata_trace_context_validates_and_retains_exact_propagation() {
    let state = "tenant@vendor=value, other=some value";
    let baggage = "user=one%20two;property=yes, user=other;flag";
    let trace = TraceContext::from_metadata(
        Metadata::new(json!({
            "traceparent":PARENT,"tracestate":state,"baggage":baggage
        }))
        .unwrap(),
    );
    assert_eq!(trace.traceparent(), Some(PARENT));
    assert_eq!(trace.tracestate(), Some(state));
    assert_eq!(trace.baggage(), Some(baggage));
    let orphan = TraceContext::from_metadata(Metadata::new(json!({"tracestate":state})).unwrap());
    assert_eq!(orphan.tracestate(), None);
    assert_eq!(orphan.0.as_map()["tracestate"], state);
    assert!(
        Metadata::new(json!({"traceparent":PARENT.replacen("00-", "01-", 1) + "-extra"})).is_ok()
    );
    for value in [
        json!({"traceparent":null}),
        json!({"traceparent":PARENT.to_uppercase()}),
        json!({"traceparent":PARENT.to_string()+"-extra"}),
        json!({"traceparent":"00-00000000000000000000000000000000-00f067aa0ba902b7-01"}),
        json!({"traceparent":"ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"}),
        json!({"tracestate":"a=x,a=y"}),
        json!({"tracestate":"A=x"}),
        json!({"tracestate":"a=x=y"}),
        json!({"tracestate":"a="}),
        json!({"tracestate":(0..33).map(|n|format!("a{n}=x")).collect::<Vec<_>>().join(",")}),
        json!({"baggage":"x=%zz"}),
        json!({"baggage":"x=unescaped space"}),
        json!({"baggage":"x=\"quoted\""}),
        json!({"baggage":"x"}),
        json!({"baggage":"x=".to_owned()+&"a".repeat(8192)}),
        json!({"baggage":vec!["a=b";181].join(",")}),
    ] {
        assert!(Metadata::new(value.clone()).is_err(), "{value}");
    }
    assert!(Metadata::new(json!({"tracestate":" \t, a=x ,","baggage":""})).is_ok());
    assert!(
        Metadata::new(
            json!({"tracestate":(0..32).map(|n|format!("a{n}=x")).collect::<Vec<_>>().join(",")})
        )
        .is_ok()
    );
}

#[test]
fn metadata_icons_check_sources_hints_and_catalog_count() {
    let icon = Icon::new(
        "https://example.com/icon.png",
        Some("image/png".into()),
        vec!["24x24".into(), "any".into()],
        Some(IconTheme::Dark),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_value::<Icon>(serde_json::to_value(&icon).unwrap()).unwrap(),
        icon
    );
    assert!(Icon::new("data:image/png;base64,AA==", None, vec![], None).is_ok());
    for src in [
        "http://example.com/icon.png",
        "file:///icon.png",
        "https:///icon.png",
        "https://:443/icon.png",
        "https://user@example.com/icon.png",
        "data:text/plain;base64,AA==",
        "data:image/png;base64,bad",
    ] {
        assert!(Icon::new(src, None, vec![], None).is_err(), "{src}");
    }
    for size in ["0x1", "1x0", "65536x1", "1X1", "anymore"] {
        assert!(Icon::new("https://example.com/i", None, vec![size.into()], None).is_err());
    }
    assert!(validate_icons(&vec![icon.clone(); 8]).is_ok());
    let mut tool = crate::Tool::new("acme", "Acme", json!({"type":"object"}));
    tool.icons = vec![icon; 9];
    assert!(crate::Catalog::new(vec![tool]).is_err());
}

#[test]
fn metadata_content_and_task_result_round_trips_preserve_all_locations() {
    let meta = Metadata::new(json!({"com.acme/item":{"value":null}})).unwrap();
    let contents = ResourceContents::text("acme://summary", None, "Summary")
        .unwrap()
        .with_metadata(meta.clone());
    let mut link = Resource::new("acme://report", "Report");
    link.meta = Some(meta.clone());
    link.icons = vec![Icon::new("https://example.com/i", None, vec![], None).unwrap()];
    let result = ToolResult::success(vec![
        Content::text("hello").with_metadata(meta.clone()),
        Content::image(&[0], "image/png").with_metadata(meta.clone()),
        Content::audio(&[0], "audio/wav")
            .unwrap()
            .with_metadata(meta.clone()),
        Content::resource(contents).with_metadata(meta.clone()),
        Content::resource_link(link).unwrap(),
    ])
    .with_metadata(meta.clone());
    let wire = serde_json::to_value(&result).unwrap();
    let restored: ToolResult = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap(), wire);
    for block in &result.content {
        assert_eq!(block.metadata(), Some(&meta));
    }
    assert_eq!(
        wire["content"][3]["resource"]["_meta"],
        serde_json::to_value(meta).unwrap()
    );
}

#[test]
fn metadata_exact_byte_boundaries_refuse_the_next_byte() {
    assert!(Metadata::new(json!({"x":"a".repeat(MAX_METADATA_BYTES-8)})).is_ok());
    assert!(Metadata::new(json!({"x":"a".repeat(MAX_METADATA_BYTES-7)})).is_err());
    let parent = PARENT.replacen("00-", "01-", 1) + "-" + &"a".repeat(200);
    assert_eq!(parent.len(), 256);
    assert!(Metadata::new(json!({"traceparent":parent})).is_ok());
    assert!(Metadata::new(json!({"traceparent":parent+"a"})).is_err());
    let state = format!("a={},b={}", "a".repeat(256), "b".repeat(251));
    assert_eq!(state.len(), 512);
    assert!(Metadata::new(json!({"tracestate":state})).is_ok());
    assert!(Metadata::new(json!({"tracestate":state+"b"})).is_err());
    let baggage = "x=".to_owned() + &"a".repeat(8190);
    assert!(Metadata::new(json!({"baggage":baggage})).is_ok());
    assert!(Metadata::new(json!({"baggage":baggage+"a"})).is_err());
}
