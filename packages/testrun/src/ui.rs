//! Bounded accessibility queries and geometry checks, independent of native I/O.
use crate::error::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
pub(crate) mod semantic;

/// Shared role vocabulary; unknown native roles remain available for exact queries.
pub fn canonical_role(role: &str) -> &str {
    match role {
        "AXApplication" | "application" => "application",
        "AXButton" | "Button" | "button" => "button",
        "AXStaticText" | "StaticText" | "text" => "text",
        "AXTextField" | "TextField" | "text_field" => "text_field",
        "AXSecureTextField" | "SecureTextField" | "secure_text_field" => "secure_text_field",
        "AXTextArea" | "TextArea" | "text_area" => "text_area",
        // iOS reports UISwitch as AXCheckBox; Android check boxes keep `checkbox`.
        "AXCheckBox" | "AXSwitch" | "switch" => "switch",
        "checkbox" => "checkbox",
        "radio_button" => "radio_button",
        "AXSlider" | "slider" => "slider",
        "AXImage" | "image" => "image",
        "AXScrollArea" | "scroll_view" => "scroll_view",
        "AXAlert" | "alert" => "alert",
        _ => role,
    }
}

/// Gives every platform's nodes the shared shape: a canonical `role` with the
/// platform's own in `nativeRole`, an `elementId` where native producers write
/// `elementRef`, and no value for secure fields.
pub(crate) fn normalize_roles(node: &mut Value) {
    let Some(fields) = node.as_object_mut() else {
        return;
    };
    if fields.get("secure") == Some(&json!(true)) {
        fields.remove("value");
    }
    if let Some(id) = fields.remove("elementRef") {
        fields.insert("elementId".into(), id);
    }
    if let Some(role) = fields
        .get("role")
        .and_then(Value::as_str)
        .map(str::to_owned)
    {
        fields
            .entry("nativeRole")
            .or_insert_with(|| json!(role.clone()));
        fields.insert("role".into(), json!(canonical_role(&role)));
    }
    if let Some(children) = node["children"].as_array_mut() {
        for child in children {
            normalize_roles(child);
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
/// Shared millisecond bounds for complete mobile gestures at every adapter boundary.
pub(crate) fn validate_gesture_duration(duration: u64) -> Result<()> {
    if duration > 10_000 {
        return Err(Error::invalid(
            "durationMs must be an integer between 0 and 10000 ms",
        ));
    }
    Ok(())
}
impl Point {
    pub fn validate(&self) -> Result<[f64; 2]> {
        if !self.x.is_finite()
            || !self.y.is_finite()
            || !(0.0..=1.0).contains(&self.x)
            || !(0.0..=1.0).contains(&self.y)
        {
            return Err(Error::invalid(
                "Points must be finite normalized coordinates in 0..1",
            ));
        }
        Ok([self.x, self.y])
    }
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Display {
    pub display_id: String,
    pub geometry_version: String,
}
impl Display {
    pub fn validate(&self, width: u32, height: u32) -> Result<()> {
        if self.display_id != "main" || self.geometry_version != format!("{width}x{height}") {
            return Err(Error::new(
                "STALE_GEOMETRY",
                "Display geometry changed; capture a fresh screenshot",
            ));
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Query {
    pub identifier: Option<String>,
    pub role: Option<String>,
    pub label: Option<TextMatch>,
    pub value: Option<TextMatch>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextMatch {
    pub text: String,
    pub mode: String,
    #[serde(default)]
    pub case_sensitive: bool,
}
impl TextMatch {
    fn matches(&self, value: &str) -> Result<bool> {
        if self.text.len() > 8192 {
            return Err(Error::invalid("Query text exceeds 8192 bytes"));
        }
        let (needle, haystack) = if self.case_sensitive {
            (self.text.clone(), value.to_owned())
        } else {
            (self.text.to_lowercase(), value.to_lowercase())
        };
        match self.mode.as_str() {
            "exact" => Ok(haystack == needle),
            "substring" => Ok(haystack.contains(&needle)),
            "regex" => {
                if self.text.len() > 256 {
                    return Err(Error::invalid("Regex patterns have a 256-byte limit"));
                }
                let regex = regex::RegexBuilder::new(&self.text)
                    .case_insensitive(!self.case_sensitive)
                    .size_limit(262144)
                    .dfa_size_limit(1048576)
                    .build()
                    .map_err(|_| Error::invalid("Invalid or oversized regex pattern"))?;
                Ok(regex.is_match(value))
            }
            _ => Err(Error::invalid(
                "Supported text modes are exact, substring and regex",
            )),
        }
    }
    /// Matches text that may be only the start of a longer value. `None`
    /// means the missing end decides: a substring found in the kept start
    /// matches, and an exact match fails once the needle fits in that start.
    pub(crate) fn matches_observed(&self, value: &str, truncated: bool) -> Result<Option<bool>> {
        let matched = self.matches(value)?;
        if !truncated {
            return Ok(Some(matched));
        }
        Ok(match self.mode.as_str() {
            "substring" if matched => Some(true),
            "exact" if self.text.chars().count() <= value.chars().count() => Some(false),
            _ => None,
        })
    }
}
impl Query {
    pub(crate) fn matches_node(&self, node: &Value) -> Result<bool> {
        let text = |field: &str| node[field].as_str().unwrap_or_default();
        let structural = self
            .identifier
            .as_ref()
            .is_none_or(|id| text("identifier") == id)
            && self.role.as_ref().is_none_or(|role| {
                canonical_role(text("role")) == canonical_role(role)
                    || text("nativeRole") == role
                    || text("nativeRole").strip_prefix("AX") == Some(role)
            });
        let label = self.label.as_ref().map_or(Ok(Some(true)), |matcher| {
            matcher.matches_observed(text("label"), node["labelTruncated"] == true)
        })?;
        let value = self.value.as_ref().map_or(Ok(Some(true)), |matcher| {
            if node["secure"] == true {
                Ok(Some(false))
            } else {
                matcher.matches_observed(text("value"), node["valueTruncated"] == true)
            }
        })?;
        Ok(structural && label == Some(true) && value == Some(true))
    }
    /// Rejects an empty query and invalid text matchers before any observation.
    pub(crate) fn validate(&self) -> Result<()> {
        if self.identifier.is_none()
            && self.role.is_none()
            && self.label.is_none()
            && self.value.is_none()
        {
            return Err(Error::invalid("Provide at least one element criterion"));
        }
        for matcher in [&self.label, &self.value].into_iter().flatten() {
            matcher.matches("")?;
        }
        Ok(())
    }
    pub fn find(&self, root: &Value, limit: usize) -> Result<Value> {
        self.validate()?;
        if !(1..=200).contains(&limit)
            || self.identifier.is_none()
                && self.role.is_none()
                && self.label.is_none()
                && self.value.is_none()
        {
            return Err(Error::invalid("Provide a query criterion and limit 1..200"));
        }
        let mut nodes = vec![root];
        let mut matches = Vec::new();
        let mut truncated = false;
        let mut summary_bytes = 0;
        while let Some(node) = nodes.pop() {
            let text = |name: &str| node.get(name).and_then(Value::as_str).unwrap_or("");
            let label = self.label.as_ref().map_or(Ok(Some(true)), |m| {
                m.matches_observed(text("label"), node["labelTruncated"] == true)
            })?;
            let value = self.value.as_ref().map_or(Ok(Some(true)), |m| {
                if node["secure"] == true {
                    Ok(Some(false))
                } else {
                    m.matches_observed(text("value"), node["valueTruncated"] == true)
                }
            })?;
            let structural = self
                .identifier
                .as_ref()
                .is_none_or(|v| text("identifier") == v)
                && self.role.as_ref().is_none_or(|v| {
                    canonical_role(text("role")) == canonical_role(v)
                        || text("nativeRole") == v
                        || text("nativeRole").strip_prefix("AX") == Some(v.as_str())
                        || text("role").strip_prefix("AX") == Some(v.as_str())
                });
            // A node whose cut text decides the match leaves the result unproven.
            if structural
                && label != Some(false)
                && value != Some(false)
                && (label.is_none() || value.is_none())
            {
                truncated = true;
            }
            let found = structural && label == Some(true) && value == Some(true);
            if found {
                if matches.len() == limit {
                    truncated = true;
                    break;
                }
                let result = Value::Object(
                    node.as_object()
                        .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "UI node is not an object"))?
                        .iter()
                        .filter(|(key, _)| {
                            key.as_str() != "children"
                                && !(node["secure"] == true && key.as_str() == "value")
                        })
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                );
                let size = serde_json::to_vec(&result)
                    .map_err(|_| Error::new("INTERNAL", "Cannot encode query summary"))?
                    .len();
                if summary_bytes + size > 60000 {
                    truncated = true;
                    break;
                }
                summary_bytes += size;
                matches.push(result);
            }
            if let Some(children) = node.get("children").and_then(Value::as_array) {
                nodes.extend(children.iter().rev());
            }
        }
        Ok(json!({"matches":matches,"truncated":truncated}))
    }
}
/// Adds each match's frame center as the display fraction that taps take.
/// A center outside the display, or a frame or display without usable
/// numbers, gets no point rather than a guess.
pub(crate) fn add_centers(matches: &mut Value, display: &Value) {
    let number = |value: &Value, name: &str| value[name].as_f64();
    let (Some(width), Some(height)) = (number(display, "width"), number(display, "height")) else {
        return;
    };
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let (left, top) = (
        number(display, "x").unwrap_or(0.0),
        number(display, "y").unwrap_or(0.0),
    );
    for element in matches.as_array_mut().into_iter().flatten() {
        let frame = &element["frame"];
        let (Some(x), Some(y), Some(w), Some(h)) = (
            number(frame, "x"),
            number(frame, "y"),
            number(frame, "width"),
            number(frame, "height"),
        ) else {
            continue;
        };
        let point = Point {
            x: (x + w / 2.0 - left) / width,
            y: (y + h / 2.0 - top) / height,
        };
        if w > 0.0 && h > 0.0 && point.validate().is_ok() {
            element["center"] = json!({"x":point.x,"y":point.y});
        }
    }
}
pub fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "Native capture returned invalid PNG",
        ));
    }
    let width = u32::from_be_bytes(
        bytes[16..20]
            .try_into()
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid PNG width"))?,
    );
    let height = u32::from_be_bytes(
        bytes[20..24]
            .try_into()
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid PNG height"))?,
    );
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "Invalid capture dimensions",
        ));
    }
    Ok((width, height))
}
pub(crate) fn focused_editable(node: &Value) -> bool {
    node["states"]["focused"].as_bool() == Some(true)
        && matches!(
            node["role"].as_str(),
            Some(
                "AXTextField"
                    | "AXTextArea"
                    | "AXSecureTextField"
                    | "text_field"
                    | "text_area"
                    | "secure_text_field"
            )
        )
        || node["children"]
            .as_array()
            .is_some_and(|nodes| nodes.iter().any(focused_editable))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalization_preserves_native_role_query_aliases() {
        for native in [
            "AXApplication",
            "AXButton",
            "AXStaticText",
            "AXTextField",
            "AXSecureTextField",
            "AXTextArea",
            "AXCheckBox",
            "AXSwitch",
            "AXSlider",
            "AXImage",
            "AXScrollArea",
            "AXAlert",
            "AXUnknownRole",
        ] {
            let original = json!({"role":native});
            let mut normalized = original.clone();
            normalize_roles(&mut normalized);
            for alias in [
                native,
                native.strip_prefix("AX").unwrap(),
                canonical_role(native),
            ] {
                let query: Query = serde_json::from_value(json!({"role":alias})).unwrap();
                for root in [&original, &normalized] {
                    assert_eq!(
                        query.find(root, 10).unwrap()["matches"]
                            .as_array()
                            .unwrap()
                            .len(),
                        1,
                        "{alias} must match {root}"
                    );
                }
            }
        }
    }
    #[test]
    fn role_queries_match_both_backends_and_preserve_native_evidence() {
        let mut simulator = json!({"role":"AXApplication","children":[{"role":"AXButton","label":"Continue"},{"role":"AXSecureTextField","secure":true}]});
        normalize_roles(&mut simulator);
        assert_eq!(simulator["children"][0]["role"], "button");
        assert_eq!(simulator["children"][0]["nativeRole"], "AXButton");
        let physical =
            json!({"role":"application","children":[{"role":"button","label":"Continue"}]});
        for role in ["button", "AXButton", "Button"] {
            let query: Query = serde_json::from_value(json!({"role":role})).unwrap();
            for root in [&simulator, &physical] {
                assert_eq!(
                    query.find(root, 10).unwrap()["matches"]
                        .as_array()
                        .unwrap()
                        .len(),
                    1
                );
            }
        }
        let before = simulator.clone();
        normalize_roles(&mut simulator);
        assert_eq!(simulator, before);
    }
    #[test]
    fn mobile_focus_uses_editable_roles_across_both_platforms() {
        for role in [
            "AXTextField",
            "AXTextArea",
            "AXSecureTextField",
            "text_field",
            "text_area",
            "secure_text_field",
        ] {
            let field = json!({"role":role,"states":{"focused":true}});
            assert!(focused_editable(&field));
            assert!(focused_editable(
                &json!({"role":"application","children":[field]})
            ));
            assert!(!focused_editable(
                &json!({"role":role,"states":{"focused":false}})
            ));
        }
        assert!(!focused_editable(
            &json!({"role":"button","states":{"focused":true}})
        ));
        assert!(!focused_editable(&json!({"role":"text_field"})));
    }
    #[test]
    fn match_centers_follow_the_reported_display_not_a_model_size() {
        // A zoomed 390x844 phone reports a 320x693 display.
        let mut matches = json!([
            {"frame":{"x":0.0,"y":470.0,"width":320.0,"height":30.0}},
            {"frame":{"x":0.0,"y":900.0,"width":320.0,"height":30.0}},
            {"frame":{"available":false}},
            {"frame":{"x":10.0,"y":10.0,"width":0.0,"height":10.0}}
        ]);
        add_centers(&mut matches, &json!({"x":0,"y":0,"width":320,"height":693}));
        let center = &matches[0]["center"];
        assert_eq!(center["x"], 0.5);
        assert!((center["y"].as_f64().unwrap() - 485.0 / 693.0).abs() < 1e-9);
        for index in 1..4 {
            assert!(matches[index].get("center").is_none());
        }
        let mut offset = json!([{"frame":{"x":100,"y":100,"width":100,"height":100}}]);
        add_centers(
            &mut offset,
            &json!({"x":100,"y":100,"width":200,"height":200}),
        );
        assert_eq!(offset[0]["center"], json!({"x":0.25,"y":0.25}));
        let mut unknown = json!([{"frame":{"x":0,"y":0,"width":10,"height":10}}]);
        add_centers(&mut unknown, &Value::Null);
        assert!(unknown[0].get("center").is_none());
    }
    #[test]
    fn gesture_duration_includes_zero_and_rejects_overflow_at_every_boundary() {
        for duration in [0, 1, 50, 500, 10_000] {
            validate_gesture_duration(duration).unwrap();
        }
        for duration in [10_001, u64::MAX] {
            assert_eq!(
                validate_gesture_duration(duration).unwrap_err().code,
                "INVALID_ARGUMENT"
            );
        }
    }
    #[test]
    fn queries_preserve_ambiguity_and_geometry_rejects_stale_input() {
        let query: Query =
            serde_json::from_value(json!({"label":{"text":"acme","mode":"substring"}})).unwrap();
        let result = query
            .find(
                &json!({"children":[{"label":"Acme one"},{"label":"ACME two"}]}),
                20,
            )
            .unwrap();
        assert_eq!(result["matches"].as_array().unwrap().len(), 2);
        assert_eq!(
            query
                .find(
                    &json!({"children":[{"label":"Acme one"},{"label":"Acme two"}]}),
                    1
                )
                .unwrap()["truncated"],
            true
        );
        let display = Display {
            display_id: "main".into(),
            geometry_version: "100x200".into(),
        };
        assert_eq!(
            display.validate(200, 100).unwrap_err().code,
            "STALE_GEOMETRY"
        );
        assert!(
            Point {
                x: f64::NAN,
                y: 0.5
            }
            .validate()
            .is_err()
        );
    }
}
