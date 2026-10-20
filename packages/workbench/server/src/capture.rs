//! Screenshot requests as the canvas sends them, checked before the capture
//! helper sees them, and screenshots saved beside the project.
//!
//! A local capture loads a page of this workbench; a page capture loads a
//! page an implementation serves, and only at an origin the configuration
//! names, so this server fetches nothing else on anybody's say-so. A live
//! DOM snapshot is checked to hold no script, frame or event handler.
use crate::refused::refused;
use anyhow::{Result, anyhow, bail};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// Where screenshots are saved, under the project root.
pub(crate) const HANDOFFS_DIR: &str = ".canonic/.handoffs";

/// What the annotations are dressed in over a page the workbench doesn't
/// serve: the tokens they use, scoped to their layer, then their own styles.
pub(crate) fn overlay_css(annotations: &str) -> String {
    [
        "#__wb_annotations{--wb-font-sans:-apple-system,BlinkMacSystemFont,\"Segoe UI\",Roboto,\"Helvetica Neue\",Arial,sans-serif;",
        "--wb-space-2:2px;--wb-space-4:4px;--wb-space-8:8px;--wb-space-12:12px;--wb-space-16:16px;--wb-space-24:24px;",
        "--wb-radius-sm:4px;--wb-radius-md:8px;--wb-radius-pill:999px;--wb-fg:#fff;--wb-fg-3:#8c8c8c;--wb-accent:#00a1ff}",
        annotations,
        "#__wb_annotations .wb-hit,#__wb_annotations .wb-sel{display:none}",
    ]
    .join("\n")
}

/// `Number(value)` for the values a payload carries.
fn number(value: &Value) -> f64 {
    match value {
        Value::Number(number) => number.as_f64().unwrap_or(f64::NAN),
        Value::String(text) if text.trim().is_empty() => 0.0,
        Value::String(text) => text.trim().parse().unwrap_or(f64::NAN),
        Value::Bool(flag) => f64::from(u8::from(*flag)),
        Value::Null => 0.0,
        _ => f64::NAN,
    }
}

/// `Math.round(Number(value))`, or `None` outside 1–8192.
fn dimension(value: &Value) -> Option<u32> {
    let rounded = (number(value) + 0.5).floor();
    (rounded.is_finite() && (1.0..=8192.0).contains(&rounded)).then(|| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "checked to be within 1–8192"
        )]
        let whole = rounded as u32;
        whole
    })
}

fn or_zero(value: &Value) -> Value {
    let found = number(value);
    if found.is_finite() && found != 0.0 {
        json!(found)
    } else {
        json!(0)
    }
}

fn format_of(value: &Value) -> Result<&'static str> {
    match value {
        Value::Null => Ok("png"),
        Value::String(format) if format == "png" => Ok("png"),
        Value::String(format) if format == "jpeg" => Ok("jpeg"),
        _ => bail!("capture format must be png or jpeg"),
    }
}

fn revision(value: &Value) -> Value {
    json!(
        value
            .as_str()
            .map(|text| text.chars().take(128).collect::<String>())
            .unwrap_or_default()
    )
}

fn key(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|text| !text.is_empty() && text.chars().count() <= 160)
}

fn node_is_safe(node: &Value) -> bool {
    if !key(&node["id"]) {
        return false;
    }
    match node["type"].as_u64() {
        Some(3 | 8) => node["text"].is_string(),
        Some(1) => {
            let tag = node["tag"].as_str().unwrap_or_default();
            let executable = ["script", "iframe", "object", "embed"]
                .iter()
                .any(|blocked| tag.eq_ignore_ascii_case(blocked));
            let attributes = node["attrs"].as_array().is_some_and(|attrs| {
                attrs.iter().all(|attr| {
                    let name = attr[0].as_str();
                    name.is_some_and(|name| {
                        let lower = name.to_ascii_lowercase();
                        !lower.starts_with("on") && lower != "http-equiv"
                    }) && attr[1].is_string()
                        && (attr[2].is_null() || attr[2].is_string())
                        && attr.as_array().is_some_and(|parts| parts.len() >= 2)
                })
            });
            node["tag"].is_string()
                && node["ns"].is_string()
                && !executable
                && attributes
                && node["children"]
                    .as_array()
                    .is_some_and(|children| children.iter().all(key))
                && (node.get("shadow").is_none()
                    || node["shadow"]
                        .as_array()
                        .is_some_and(|shadow| shadow.iter().all(key)))
        }
        _ => false,
    }
}

/// A live DOM snapshot or patch the helper renders instead of loading the page.
fn mirror(value: &Value) -> Result<Option<Value>> {
    if value.is_null() {
        return Ok(None);
    }
    let shaped = value.is_object()
        && value["revision"]
            .as_str()
            .is_some_and(|text| !text.is_empty() && text.chars().count() <= 160)
        && value["states"].is_array()
        && value["defined"].is_array()
        && value["animations"].is_array();
    if !shaped {
        bail!("Invalid live DOM snapshot");
    }
    if value["version"] == 1 {
        let pointer = &value["pointer"];
        if !pointer.is_null() {
            let inside =
                |axis: &Value| axis.as_f64().is_some_and(|at| (0.0..=8192.0).contains(&at));
            if !inside(&pointer["x"]) || !inside(&pointer["y"]) {
                bail!("Invalid live DOM pointer");
            }
        }
        let list_of_keys = |field: &str| {
            value[field]
                .as_array()
                .is_some_and(|items| items.iter().all(key))
        };
        let valid = key(&value["root"])
            && (value["base"].is_null() || key(&value["base"]))
            && value["nodes"]
                .as_array()
                .is_some_and(|nodes| nodes.iter().all(node_is_safe))
            && list_of_keys("removed")
            && list_of_keys("cleared")
            && value["states"]
                .as_array()
                .is_some_and(|states| states.iter().all(|state| key(&state["id"])));
        if !valid {
            bail!("Invalid live DOM patch");
        }
        let mut out = json!({
            "version": 1, "revision": value["revision"], "base": value["base"], "root": value["root"], "nodes": value["nodes"],
            "removed": value["removed"], "states": value["states"], "cleared": value["cleared"], "defined": value["defined"], "animations": value["animations"],
        });
        if value.get("pointer").is_some() {
            out["pointer"] = if pointer.is_null() {
                Value::Null
            } else {
                json!({ "x": pointer["x"], "y": pointer["y"] })
            };
        }
        return Ok(Some(out));
    }
    if !value["html"].is_string() {
        bail!("Invalid live DOM snapshot");
    }
    Ok(Some(json!({
        "html": value["html"], "revision": value["revision"], "states": value["states"], "defined": value["defined"], "animations": value["animations"],
    })))
}

fn geometry(payload: &Value) -> Result<(u32, u32)> {
    let width = dimension(&payload["width"])
        .ok_or_else(|| anyhow!("capture width is outside the supported range"))?;
    let height = dimension(&payload["height"])
        .ok_or_else(|| anyhow!("capture height is outside the supported range"))?;
    Ok((width, height))
}

fn common(payload: &Value, url: &str, width: u32, height: u32) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("url".into(), json!(url));
    out.insert("width".into(), json!(width));
    out.insert("height".into(), json!(height));
    out.insert(
        "scroll".into(),
        json!({ "x": or_zero(&payload["scroll"]["x"]), "y": or_zero(&payload["scroll"]["y"]) }),
    );
    out.insert(
        "annotations".into(),
        json!(payload["annotations"].as_str().unwrap_or_default()),
    );
    out
}

fn finish(mut out: Map<String, Value>, payload: &Value) -> Result<Value> {
    out.insert(
        "name".into(),
        payload.get("name").cloned().unwrap_or(Value::Null),
    );
    out.insert("format".into(), json!(format_of(&payload["format"])?));
    if let Some(mirrored) = mirror(&payload["mirror"])? {
        out.insert("mirror".into(), mirrored);
    }
    out.insert("revision".into(), revision(&payload["revision"]));
    Ok(Value::Object(out))
}

/// A local capture request: a page of this workbench, at `base`'s origin.
///
/// # Errors
/// Explains a geometry, address, format or snapshot that can't be captured.
pub(crate) fn local(body: &[u8], base: &str) -> Result<Value> {
    let payload: Value = serde_json::from_slice(body)?;
    let (width, height) = geometry(&payload)?;
    let target = resolve(payload["url"].as_str().unwrap_or_default(), base);
    let origin = crate::space::origin_of(&target);
    if origin.is_none() || origin != crate::space::origin_of(base) {
        bail!("capture URL must belong to this workbench");
    }
    let out = common(&payload, &target, width, height);
    finish(out, &payload)
}

/// A page capture request: a page an implementation serves, at one of `origins`.
///
/// # Errors
/// 403 for an address outside them; otherwise as [`local`].
pub(crate) fn page(body: &[u8], origins: &[String], overlay: &str) -> Result<Value> {
    let payload: Value = serde_json::from_slice(body)?;
    let (width, height) = geometry(&payload)?;
    let address = payload["url"].as_str().unwrap_or_default();
    let allowed = crate::space::origin_of(address).is_some_and(|origin| origins.contains(&origin));
    if !allowed {
        return Err(refused(
            403,
            "that address isn’t one of this workbench’s implementations",
        ));
    }
    let mut out = common(&payload, address, width, height);
    out.insert("anchors".into(), anchors(&payload["anchors"]));
    out.insert("css".into(), json!(overlay));
    finish(out, &payload)
}

/// The points under the annotations, null where one names no element.
fn anchors(raw: &Value) -> Value {
    let Some(points) = raw.as_array() else {
        return json!([]);
    };
    Value::Array(
        points
            .iter()
            .map(|point| {
                let (x, y) = (number(&point["x"]), number(&point["y"]));
                if point.is_object() && x.is_finite() && y.is_finite() {
                    json!({ "x": x, "y": y })
                } else {
                    Value::Null
                }
            })
            .collect(),
    )
}

/// A capture address against the workbench's base, as `new URL(url, base)` reads it.
fn resolve(address: &str, base: &str) -> String {
    let base = base.trim_end_matches('/');
    if address.contains("://") {
        address.to_owned()
    } else if let Some(path) = address.strip_prefix('/') {
        format!("{base}/{path}")
    } else {
        format!("{base}/{address}")
    }
}

/// A bridged capture renders on the workbench's inert capture page, with
/// the snapshot's `<base>` still pointing at the implementation.
pub(crate) fn bridged(payload: &Value, base: &str) -> Value {
    let mut out = payload.clone();
    out["url"] = json!(format!(
        "{}/_workbench/bridged-preview",
        base.trim_end_matches('/')
    ));
    out
}

/// "#pages/sign-in.html" arrives as a name, not a path: anything like a
/// path is flattened, with the format's extension.
pub(crate) fn safe_name(name: &Value, format: &str) -> String {
    let text = match name {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    let base = text.rsplit(['/', '\\']).next().unwrap_or_default();
    let stem = match base.rfind('.') {
        Some(at) => &base[..at],
        None => base,
    };
    let mut flattened = String::new();
    let mut run = false;
    for character in stem.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
            flattened.push(character);
            run = false;
        } else if !run {
            flattened.push('-');
            run = true;
        }
    }
    let flattened = flattened.trim_start_matches(['-', '.']);
    let stem = if flattened.is_empty() {
        "canonic"
    } else {
        flattened
    };
    format!("{stem}{}", if format == "jpeg" { ".jpg" } else { ".png" })
}

/// Saves a screenshot under the project's handoffs folder, numbering a
/// second one of the same name, and answers its project-relative path.
///
/// # Errors
/// Returns the filesystem error when it can't be written.
pub(crate) fn save(root: &Path, name: &Value, body: &[u8], format: &str) -> Result<String> {
    let dir = root.join(HANDOFFS_DIR);
    std::fs::create_dir_all(&dir)?;
    let file_name = safe_name(name, format);
    let (stem, extension) = file_name
        .rsplit_once('.')
        .map_or((file_name.as_str(), ""), |(stem, extension)| {
            (stem, extension)
        });
    let mut file: PathBuf = dir.join(&file_name);
    let mut n = 1;
    while file.exists() {
        n += 1;
        file = dir.join(format!("{stem}-{n}.{extension}"));
    }
    std::fs::write(&file, body)?;
    Ok(file
        .strip_prefix(root)
        .unwrap_or(&file)
        .to_string_lossy()
        .into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "http://127.0.0.1:3579/";

    #[test]
    fn a_local_capture_is_normalized_and_kept_on_this_workbench() {
        let payload = local(br#"{"url":"/pages/a.html","width":"320.4","height":640.6,"scroll":{"x":"12"},"name":"a"}"#, BASE).unwrap();
        assert_eq!(payload["url"], "http://127.0.0.1:3579/pages/a.html");
        assert_eq!(
            (payload["width"].as_u64(), payload["height"].as_u64()),
            (Some(320), Some(641))
        );
        assert_eq!(payload["scroll"], json!({ "x": 12.0, "y": 0 }));
        assert_eq!(payload["format"], "png");
        assert_eq!(payload["revision"], "");
        let outside = local(
            br#"{"url":"http://example.com/","width":1,"height":1}"#,
            BASE,
        )
        .unwrap_err();
        assert_eq!(
            outside.to_string(),
            "capture URL must belong to this workbench"
        );
        for disguised in [
            r"http://evil.example\\@127.0.0.1:3579/",
            "http://evil.example?@127.0.0.1:3579/",
        ] {
            let body = format!(r#"{{"url":"{disguised}","width":1,"height":1}}"#);
            assert!(local(body.as_bytes(), BASE).is_err(), "{disguised}");
        }
        for size in ["0", "8193", "\"wide\""] {
            let body = format!(r#"{{"url":"/","width":{size},"height":10}}"#);
            assert_eq!(
                local(body.as_bytes(), BASE).unwrap_err().to_string(),
                "capture width is outside the supported range"
            );
        }
        assert!(local(br#"{"url":"/","width":1,"height":1,"format":"gif"}"#, BASE).is_err());
    }

    #[test]
    fn a_page_capture_needs_an_implementation_origin() {
        let origins = vec!["http://127.0.0.1:6006".to_owned()];
        let payload = page(br#"{"url":"http://127.0.0.1:6006/iframe.html","width":10,"height":10,"anchors":[{"x":1,"y":2},{"x":"no"}]}"#, &origins, "css").unwrap();
        assert_eq!(payload["anchors"], json!([{ "x": 1.0, "y": 2.0 }, null]));
        assert_eq!(payload["css"], "css");
        let refused = page(
            br#"{"url":"http://example.com/","width":10,"height":10}"#,
            &origins,
            "",
        )
        .unwrap_err();
        assert_eq!(crate::refused::status_of(&refused, 500), 403);
        let disguised = page(
            br#"{"url":"http://evil.example\\@127.0.0.1:6006/","width":10,"height":10}"#,
            &origins,
            "",
        )
        .unwrap_err();
        assert_eq!(crate::refused::status_of(&disguised, 500), 403);
    }

    #[test]
    fn live_dom_records_refuse_scripts_handlers_and_bad_keys() {
        let record = |nodes: &str| {
            format!(
                r#"{{"url":"/","width":1,"height":1,"mirror":{{"version":1,"revision":"r1","root":"n1","base":null,"nodes":{nodes},"removed":[],"cleared":[],"states":[],"defined":[],"animations":[]}}}}"#
            )
        };
        let fine = record(
            r#"[{"id":"n1","type":1,"tag":"div","ns":"html","attrs":[["class","a",null]],"children":["n2"]},{"id":"n2","type":3,"text":"Acme"}]"#,
        );
        assert_eq!(
            local(fine.as_bytes(), BASE).unwrap()["mirror"]["root"],
            "n1"
        );
        for bad in [
            r#"[{"id":"n1","type":1,"tag":"SCRIPT","ns":"html","attrs":[],"children":[]}]"#,
            r#"[{"id":"n1","type":1,"tag":"div","ns":"html","attrs":[["onclick","x",null]],"children":[]}]"#,
            r#"[{"id":"","type":3,"text":"x"}]"#,
        ] {
            assert_eq!(
                local(record(bad).as_bytes(), BASE).unwrap_err().to_string(),
                "Invalid live DOM patch"
            );
        }
        let pointed = record(r#"[{"id":"n1","type":3,"text":"Acme"}]"#).replace(
            r#""animations":[]}}"#,
            r#""animations":[],"pointer":{"x":32,"y":16}}}"#,
        );
        assert_eq!(
            local(pointed.as_bytes(), BASE).unwrap()["mirror"]["pointer"],
            json!({ "x": 32, "y": 16 })
        );
        for pointer in [r#"{"x":-1,"y":16}"#, r#"{"x":"far","y":16}"#] {
            let bad = record("[]").replace(
                r#""animations":[]}}"#,
                &format!(r#""animations":[],"pointer":{pointer}}}}}"#),
            );
            assert_eq!(
                local(bad.as_bytes(), BASE).unwrap_err().to_string(),
                "Invalid live DOM pointer"
            );
        }
        let based = record("[]").replace(r#""base":null"#, r#""base":12"#);
        assert_eq!(
            local(based.as_bytes(), BASE).unwrap_err().to_string(),
            "Invalid live DOM patch"
        );
        let snapshot = br#"{"url":"/","width":1,"height":1,"mirror":{"html":"<p>Acme</p>","revision":"r","states":[],"defined":[],"animations":[]}}"#;
        assert_eq!(
            local(snapshot, BASE).unwrap()["mirror"]["html"],
            "<p>Acme</p>"
        );
        assert!(local(br#"{"url":"/","width":1,"height":1,"mirror":{"revision":"r","states":[],"defined":[],"animations":[]}}"#, BASE).is_err());
    }

    #[test]
    fn names_are_flattened_and_a_second_shot_is_numbered() {
        assert_eq!(
            safe_name(&json!("#pages/sign in.html"), "png"),
            "sign-in.png"
        );
        assert_eq!(safe_name(&json!("../.hidden"), "jpeg"), "canonic.jpg");
        assert_eq!(safe_name(&Value::Null, "png"), "canonic.png");
        let root = std::env::temp_dir().join(format!("wb-capture-{}", std::process::id()));
        let _clean = std::fs::remove_dir_all(&root);
        assert_eq!(
            save(&root, &json!("a"), b"1", "png").unwrap(),
            ".canonic/.handoffs/a.png"
        );
        assert_eq!(
            save(&root, &json!("a"), b"2", "png").unwrap(),
            ".canonic/.handoffs/a-2.png"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
