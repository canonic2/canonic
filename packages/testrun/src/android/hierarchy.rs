//! Pure bounded Android accessibility XML translation with secure-value omission.
use crate::error::{Error, Result};
use quick_xml::{Reader, events::Event};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[expect(
    clippy::too_many_lines,
    reason = "The bounded streaming XML parser owns one traversal stack and truncation state"
)]
pub fn hierarchy(bytes: &[u8]) -> Result<Value> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Android UI hierarchy is not UTF-8"))?;
    let start = text.find("<hierarchy").ok_or_else(|| {
        Error::new(
            "UI_UNAVAILABLE",
            "Android UI automation did not produce a hierarchy",
        )
    })?;
    let mut reader = Reader::from_str(&text[start..]);
    let mut stack = vec![json!({"role":"application","children":[]})];
    let mut count = 0;
    let mut truncated = false;
    let mut active_window = false;
    loop {
        let event = reader
            .read_event()
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Android hierarchy XML is malformed"))?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(ref root) if root.name().as_ref() == "hierarchy" => {
                for attribute in root.attributes() {
                    let attribute = attribute.map_err(|_| {
                        Error::new("PLATFORM_PROTOCOL", "Invalid hierarchy coverage")
                    })?;
                    if attribute.key.as_ref() == "truncated" && attribute.value.as_ref() == "true" {
                        truncated = true;
                    }
                    if attribute.key.as_ref() == "scope"
                        && attribute.value.as_ref() == "active_window"
                    {
                        active_window = true;
                    }
                    // Some targets renumber the same window on every helper
                    // connection, so the number only marks a missing window
                    // and stays out of the tree that references compare.
                    if attribute.key.as_ref() == "window-id" {
                        let window = attribute.value.parse::<i32>().map_err(|_| {
                            Error::new("PLATFORM_PROTOCOL", "Invalid active window identity")
                        })?;
                        truncated |= window < 0;
                    }
                }
            }
            Event::Start(node) | Event::Empty(node) if node.name().as_ref() == "node" => {
                if count >= 3000 || stack.len() > 60 {
                    truncated = true;
                    break;
                }
                count += 1;
                let mut attrs = BTreeMap::new();
                for attribute in node.attributes() {
                    let attribute = attribute
                        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid UI attribute"))?;
                    attrs.insert(
                        attribute.key.as_ref().to_owned(),
                        attribute
                            .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                            .map_err(|_| {
                                Error::new("PLATFORM_PROTOCOL", "Invalid UI text encoding")
                            })?
                            .into_owned(),
                    );
                }
                let get = |key: &str| attrs.get(key).map(String::as_str).unwrap_or("");
                let secure = get("password") == "true";
                let bounds = bounds(get("bounds"))?;
                let class = get("class");
                let editable = is_editable(class);
                let role = role(class, secure, get("scrollable") == "true");
                let mut states = serde_json::Map::new();
                for key in [
                    "enabled",
                    "focused",
                    "clickable",
                    "selected",
                    "checked",
                    "checkable",
                    "scrollable",
                ] {
                    if matches!(get(key), "true" | "false") {
                        states.insert(key.into(), json!(get(key) == "true"));
                    }
                }
                let native = class.rsplit(['.', '$']).next().unwrap_or(class);
                let mut value = json!({"elementId":format!("node-{count}"),"role":role,"nativeRole":native,"class":class,"identifier":get("resource-id"),"label":if !get("content-desc").is_empty(){get("content-desc")}else if secure{""}else{get("text")},"secure":secure,"frame":bounds,"states":states,"children":[]});
                if !secure && editable {
                    value["value"] = json!(get("text"));
                }
                // The helper caps long text per node and marks what it cut.
                let text_cut = get("text-truncated") == "true";
                let label_cut = if get("content-desc").is_empty() {
                    text_cut && !secure
                } else {
                    get("content-desc-truncated") == "true"
                };
                if label_cut {
                    value["labelTruncated"] = json!(true);
                }
                if text_cut && !secure && editable {
                    value["valueTruncated"] = json!(true);
                }
                stack.push(value);
                if empty {
                    let child = stack
                        .pop()
                        .ok_or_else(|| Error::new("INTERNAL", "UI parser stack is empty"))?;
                    stack
                        .last_mut()
                        .and_then(|n| n["children"].as_array_mut())
                        .ok_or_else(|| Error::new("INTERNAL", "UI parser parent unavailable"))?
                        .push(child);
                }
            }
            Event::End(node) if node.name().as_ref() == "node" => {
                if stack.len() < 2 {
                    return Err(Error::new(
                        "PLATFORM_PROTOCOL",
                        "Unbalanced Android UI hierarchy",
                    ));
                }
                let child = stack
                    .pop()
                    .ok_or_else(|| Error::new("INTERNAL", "UI parser stack is empty"))?;
                stack
                    .last_mut()
                    .and_then(|n| n["children"].as_array_mut())
                    .ok_or_else(|| Error::new("INTERNAL", "UI parser parent is unavailable"))?
                    .push(child);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !truncated && stack.len() != 1 {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "Android hierarchy ended with unclosed elements",
        ));
    }
    while stack.len() > 1 {
        let child = stack
            .pop()
            .ok_or_else(|| Error::new("INTERNAL", "UI parser stack is empty"))?;
        stack
            .last_mut()
            .and_then(|n| n["children"].as_array_mut())
            .ok_or_else(|| Error::new("INTERNAL", "UI parser parent is unavailable"))?
            .push(child);
    }
    Ok(
        json!({"quality":if truncated{"partial"}else{"complete"},"coverage":if active_window{"active_window_exposed_tree"}else{"uiautomator_exposed_tree"},"truncated":truncated,"nodeCount":count,"root":stack.remove(0)}),
    )
}
fn is_editable(class: &str) -> bool {
    class.ends_with("EditText") || class.ends_with("AutoCompleteTextView")
}

/// Maps widget classes to the shared role vocabulary. The `nativeRole` and
/// `class` fields keep the native evidence; unknown classes stay `element`.
fn role(class: &str, secure: bool, scrollable: bool) -> &'static str {
    let ends = |suffixes: &[&str]| suffixes.iter().any(|suffix| class.ends_with(suffix));
    if is_editable(class) {
        if secure {
            "secure_text_field"
        } else {
            "text_field"
        }
    } else if ends(&["CheckBox"]) {
        "checkbox"
    } else if ends(&["RadioButton"]) {
        "radio_button"
    } else if ends(&["Switch", "SwitchCompat", "SwitchMaterial", "ToggleButton"]) {
        "switch"
    } else if ends(&["Button"]) {
        "button"
    } else if ends(&["SeekBar", "Slider", "RatingBar"]) {
        "slider"
    } else if ends(&["ImageView"]) {
        "image"
    } else if ends(&["TextView"]) {
        "text"
    } else if scrollable || ends(&["ScrollView", "RecyclerView", "ListView", "GridView"]) {
        "scroll_view"
    } else {
        "element"
    }
}

fn bounds(text: &str) -> Result<Value> {
    let numbers: Vec<_> = text
        .split(['[', ']', ','])
        .filter(|s| !s.is_empty())
        .map(str::parse::<i32>)
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid Android element bounds"))?;
    if numbers.len() != 4 {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "Invalid Android element bounds",
        ));
    }
    // UI Automator can emit inverted clipped rectangles for off-screen children.
    // Preserve the observation but never advertise an actionable rectangle.
    if numbers[2] < numbers[0] || numbers[3] < numbers[1] {
        return Ok(json!({"available":false,"reason":"offscreen_inverted_bounds"}));
    }
    Ok(
        json!({"x":numbers[0],"y":numbers[1],"width":i64::from(numbers[2])-i64::from(numbers[0]),"height":i64::from(numbers[3])-i64::from(numbers[1])}),
    )
}
