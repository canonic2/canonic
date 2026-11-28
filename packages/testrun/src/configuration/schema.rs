//! One schema owner for strict file, overlay and inline-profile validation.
use crate::error::{Error, Result};
use serde_json::{Value, json};

pub(super) fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
pub(super) fn text() -> Value {
    json!({"type":"string","minLength":1,"maxLength":16384})
}
pub(super) fn strings() -> Value {
    json!({"type":"array","maxItems":128,"items":{"type":"string","maxLength":16384}})
}
pub(super) fn env(local: bool) -> Value {
    json!({"type":"object","maxProperties":128,"additionalProperties":if local { json!({"anyOf":[{"type":"string"},{"type":"null"}]}) } else {json!({"type":"string"})}})
}
pub(crate) fn profile(local: bool) -> Value {
    let launch = object(
        json!({"args":strings(),"env":env(local),"inherit_env":strings()}),
        &[],
    );
    let mut artifact = object(json!({"path":text(),"glob":text()}), &[]);
    artifact["minProperties"] = json!(1);
    artifact["maxProperties"] = json!(1);
    let mut result = object(
        json!({
            "builder":{"enum":["xcode","gradle","react-native","expo","flutter","custom"]},
            "platform":{"enum":["ios","android"]},"target_kind":{"enum":["simulator","emulator","device"]},
            "cwd":text(),"env":env(local),"inherit_env":strings(),"timeout_ms":{"type":"integer","minimum":1000,"maximum":3600000},
            "artifact":artifact,"launch":launch,"development":crate::project::schema::development(local),
            "diagnostics": object(json!({
                "symbols":{"enum":["none","embedded","external","unknown"]},
                "profileable":{"type":"boolean"},"coverage":{"type":"boolean"},
                "sanitizers":{"type":"array","maxItems":3,"uniqueItems":true,"items":{"enum":["address","thread","undefined"]}},
                "probes":{"type":"array","maxItems":32,"uniqueItems":true,"items":{"type":"string","minLength":1,"maxLength":256}},
                "timing":{"enum":["production_like","instrumented","unknown"]}
            }), if local { &[] } else { &["symbols","profileable","sanitizers","coverage","probes","timing"] }),
            "xcode":object(json!({"workspace":text(),"project":text(),"scheme":text(),"configuration":text(),"sdk":text(),"build_settings":env(false),"development_team":text()}), if local { &[] } else { &["scheme","configuration"] }),
            "gradle":object(json!({"tasks":{"type":"array","minItems":1,"maxItems":32,"items":text()},"properties":env(false)}), if local { &[] } else { &["tasks"] }),
            "react-native":object(json!({"configuration":text(),"variant":text(),"scheme":text()}), &[]),
            "expo":object(json!({"profile":text()}), if local { &[] } else { &["profile"] }),
            "flutter":object(json!({"mode":{"enum":["debug","profile","release"]},"flavor":text(),"entry_point":text(),"dart_defines":env(false)}), if local { &[] } else { &["mode"] }),
            "custom":object(json!({"command":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"string","maxLength":16384}}}), if local { &[] } else { &["command"] })
        }),
        if local {
            &[]
        } else {
            &["builder", "platform", "target_kind"]
        },
    );
    if !local {
        let branches: Vec<_> = [
            "xcode",
            "gradle",
            "react-native",
            "expo",
            "flutter",
            "custom",
        ]
        .iter()
        .map(|builder| json!({"properties":{"builder":{"const":builder}},"required":[builder]}))
        .collect();
        result["anyOf"] = json!(branches);
    }
    result
}
pub(crate) fn document(local: bool) -> Value {
    object(
        json!({"version":{"const":1},"builds":{"type":"object","maxProperties":128,"additionalProperties":profile(local)}}),
        &["version"],
    )
}
pub(crate) fn validate_document(value: &Value, local: bool) -> Result<()> {
    if value["version"] == 2 {
        return super::projects::schema::validate_root(value, local);
    }
    canonic_mcp_kit::schema::validate(&document(local), value)
        .map_err(|error| super::provenance::argument(error, value))?;
    for (name, profile) in value["builds"].as_object().into_iter().flatten() {
        if name.is_empty()
            || name.len() > 64
            || !name.as_bytes()[0].is_ascii_lowercase()
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(Error::new("CONFIG_INVALID", "Invalid build profile name")
                .details(json!({"pointer":format!("/builds/{name}")})));
        }
        if !local {
            validate_profile(profile).map_err(|mut error| {
                let mut details = error.details.take().unwrap_or(json!({}));
                let pointer = details["pointer"].as_str().unwrap_or("");
                details["pointer"] = json!(format!("/builds/{name}{pointer}"));
                error.details(details)
            })?;
        }
    }
    Ok(())
}
pub(crate) fn validate_profile(value: &Value) -> Result<()> {
    canonic_mcp_kit::schema::validate(&profile(false), value)
        .map_err(|error| super::provenance::argument(error, value))?;
    let builder = value["builder"].as_str().unwrap_or_default();
    for key in [
        "xcode",
        "gradle",
        "react-native",
        "expo",
        "flutter",
        "custom",
    ] {
        if value.get(key).is_some() && key != builder {
            return Err(Error::invalid("Only the selected builder block is allowed"));
        }
    }
    if builder == "custom" && value.get("artifact").is_none() {
        return Err(Error::invalid("Custom builds require artifact"));
    }
    if value["platform"] == "ios" && value["target_kind"] == "emulator"
        || value["platform"] == "android"
            && (value["target_kind"] == "simulator" || builder == "xcode")
        || builder == "gradle" && value["platform"] != "android"
    {
        return Err(Error::invalid("Builder/platform/target_kind mismatch"));
    }
    Ok(())
}
