//! A canvas review: one screenshot of several artboards, each a labelled
//! region with its own page prompt, its annotations local to it.

use crate::js;
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::HashSet;

const MAX_ARTBOARDS: usize = 32;

fn valid_region(region: Option<&Value>) -> Option<[f64; 4]> {
    let region = region?.as_object()?;
    let mut sides = [0.0; 4];
    for (side, key) in sides.iter_mut().zip(["x", "y", "width", "height"]) {
        *side = region.get(key)?.as_f64().filter(|n| *n >= 0.0)?;
    }
    (sides[2] != 0.0 && sides[3] != 0.0).then_some(sides)
}

/// The review's prompt, with `single` writing each artboard's page prompt.
///
/// # Errors
/// Returns why the review or one of its artboards is malformed.
pub(crate) fn canvas_prompt(
    value: &Value,
    single: impl Fn(&Value) -> Result<String>,
) -> Result<String> {
    let review = match value {
        Value::Object(review) => review,
        Value::Array(_) => bail!("A canvas handoff needs its image and artboards."),
        _ => bail!("Invalid canvas review."),
    };
    let file = review.get("file").and_then(Value::as_str).unwrap_or("");
    let artboards = review
        .get("artboards")
        .and_then(Value::as_array)
        .filter(|boards| !boards.is_empty() && boards.len() <= MAX_ARTBOARDS);
    let (false, Some(artboards)) = (file.is_empty(), artboards) else {
        bail!("A canvas handoff needs its image and artboards.");
    };
    let selected = review.get("selected");
    let mut lines = vec![
        "Here is a Workbench canvas review.".to_owned(),
        String::new(),
        format!("- Screenshot: `{file}`"),
        format!(
            "- Selected artboard: {}",
            if js::truthy(selected) {
                js::text(selected)
            } else {
                "none".into()
            }
        ),
        "Each labelled image region is an artboard. Annotation coordinates below are local to \
         that artboard, in CSS pixels."
            .into(),
    ];
    let mut ids = HashSet::new();
    for board in artboards {
        let id = board.get("id").and_then(Value::as_str).unwrap_or("");
        let space = board.get("space");
        let space_id = space
            .and_then(|space| space.get("id"))
            .and_then(Value::as_str);
        let space_name = space
            .and_then(|space| space.get("name"))
            .and_then(Value::as_str);
        let region = valid_region(board.get("region"));
        let (false, false, true, Some(space_id), Some(space_name), Some(region)) = (
            id.is_empty(),
            ids.contains(id),
            board.get("src").is_some_and(Value::is_string),
            space_id,
            space_name,
            region,
        ) else {
            bail!("Invalid artboard in canvas review.");
        };
        ids.insert(id.to_owned());
        let [x, y, width, height] = region.map(js::number_text);
        let mut local = board.clone();
        local["file"] = json!(file);
        if !board.get("size").is_some_and(Value::is_string) {
            local["size"] = json!(format!("{width} × {height}"));
        }
        let status = board.get("status");
        lines.extend([
            String::new(),
            format!("Artboard {id} — Space: {space_name} ({space_id})"),
            format!("- Image region: ({x}, {y}), {width} × {height}"),
            format!(
                "- Status: {}",
                if js::truthy(status) {
                    js::text(status)
                } else {
                    "ready".into()
                }
            ),
            format!("- Captured at: {}", js::text(board.get("capturedAt"))),
            single(&local)?,
        ]);
    }
    match selected {
        Some(Value::Null) => {}
        Some(Value::String(selected)) if ids.contains(selected) => {}
        _ => bail!("Invalid selected artboard in canvas review."),
    }
    Ok(lines.join("\n"))
}
