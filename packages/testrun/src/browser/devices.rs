//! Pinned Playwright 1.64.1 device descriptors behind the `device` context option.
//!
//! `node tools/browser-device-descriptors.mjs` extracts `devices.json` from the pinned
//! package without executing it; `tools/browser-provenance/device-descriptors.json` records
//! its provenance.

use serde::Deserialize as _;
use serde_json::{Map, Value};
use std::sync::OnceLock;

/// Descriptor fields that become context options; `defaultBrowserType` only
/// selects a browser in Playwright Test and is not a context option.
const FIELDS: [&str; 6] = [
    "userAgent",
    "viewport",
    "screen",
    "deviceScaleFactor",
    "isMobile",
    "hasTouch",
];

fn table() -> &'static Map<String, Value> {
    static TABLE: OnceLock<Map<String, Value>> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(include_str!("devices.json")).expect("Pinned device descriptors")
    })
}

/// Descriptor names in upstream order.
pub(crate) fn names() -> Vec<&'static str> {
    table().keys().map(String::as_str).collect()
}

/// Fills the fields the caller left out from the named descriptor, as the
/// pinned SDK's `{...devices[name], ...options}` spread does.
pub(crate) fn expand(options: &mut Map<String, Value>) -> Result<(), &'static str> {
    let Some(device) = options.remove("device") else {
        return Ok(());
    };
    let descriptor = device
        .as_str()
        .and_then(|name| table().get(name))
        .and_then(Value::as_object)
        .ok_or("UNKNOWN_DEVICE")?;
    for field in FIELDS {
        if let Some(value) = descriptor.get(field) {
            options.entry(field).or_insert_with(|| value.clone());
        }
    }
    Ok(())
}

/// Serde adapter for context options that accept `device`.
pub(crate) fn context_options<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<super::contract::ContextOptions, D::Error> {
    let mut options = Map::deserialize(deserializer)?;
    expand(&mut options).map_err(serde::de::Error::custom)?;
    serde_json::from_value(Value::Object(options)).map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn descriptors_fill_only_omitted_options() {
        assert!(names().len() > 100);
        assert!(names().contains(&"iPhone 15"));
        let mut options = json!({"device":"Pixel 7","viewport":{"width":500,"height":600}})
            .as_object()
            .unwrap()
            .clone();
        expand(&mut options).unwrap();
        let pixel = table()["Pixel 7"].as_object().unwrap();
        assert_eq!(options["viewport"], json!({"width":500,"height":600}));
        assert_eq!(options["userAgent"], pixel["userAgent"]);
        assert_eq!(options["isMobile"], true);
        assert_eq!(options["hasTouch"], true);
        assert!(!options.contains_key("device"));
        assert!(!options.contains_key("defaultBrowserType"));
        let mut unknown = json!({"device":"Acme Phone"}).as_object().unwrap().clone();
        assert_eq!(expand(&mut unknown), Err("UNKNOWN_DEVICE"));
    }

    #[test]
    fn device_options_deserialize_through_the_typed_context() {
        #[derive(serde::Deserialize)]
        struct Input {
            #[serde(deserialize_with = "context_options")]
            context: super::super::contract::ContextOptions,
        }
        let input: Input = serde_json::from_value(
            json!({"context":{"device":"Desktop Firefox","deviceScaleFactor":2}}),
        )
        .unwrap();
        assert_eq!(input.context.viewport.width, 1280);
        assert!((input.context.device_scale_factor - 2.0).abs() < f64::EPSILON);
        assert_eq!(input.context.is_mobile, Some(false));
        assert!(
            serde_json::from_value::<Input>(json!({"context":{"device":"Acme Phone"}})).is_err()
        );
    }
}
