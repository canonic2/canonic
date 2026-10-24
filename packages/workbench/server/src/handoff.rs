//! The handoff prompt: what the canvas knows, in the words an agent is handed.
//!
//! A screenshot alone is a poor brief, so the prompt says in words what the
//! workbench already knows exactly. It establishes, in order, that the red
//! annotations are not part of the design; which page, scenario, lens and width
//! were being looked at; and which element each annotation is attached to.
//! Payloads are the canvas's JSON, read with JavaScript's value rules so the
//! wording matches what the browser composed. A canvas review (version 2)
//! lists each artboard's region and its own prompt; see [`canvas`].

mod canvas;

use crate::js;
use anyhow::Result;
use serde_json::Value;

fn type_name(kind: &str) -> &str {
    match kind {
        "arrow" => "Arrow",
        "line" => "Line",
        "rect" => "Rectangle",
        "ellipse" => "Circle",
        "draw" => "Scribble",
        "text" => "Note",
        "comment" => "Comment",
        other => other,
    }
}

fn at(point: Option<&Value>) -> String {
    let coordinate = |key| js::number_text(js::round(point.and_then(|point| point.get(key))));
    format!("({}, {})", coordinate("x"), coordinate("y"))
}

fn size(rect: Option<&Value>) -> String {
    let side = |key| js::number_text(js::round(rect.and_then(|rect| rect.get(key))));
    format!("{} × {}", side("w"), side("h"))
}

/// `value` as text after `prefix`, or nothing when it is falsy.
fn suffix(prefix: &str, value: Option<&Value>) -> String {
    if js::truthy(value) {
        format!("{prefix}{}", js::text(value))
    } else {
        String::new()
    }
}

/// One line per annotation, numbered the way they were drawn, with the
/// element under it last, where it reads as the subject.
pub(crate) fn annotation_line(annotation: &Value, index: usize) -> String {
    let kind = js::text(annotation.get("type"));
    let rect = annotation.get("box");
    let words = || js::text(annotation.get("text"));
    let detail = match kind.as_str() {
        "text" => return format!("{}. Note at {}: “{}”", index + 1, at(rect), words()),
        "comment" => format!(" at {}, {}: “{}”", at(rect), size(rect), words()),
        "arrow" => format!(" pointing at {}", at(annotation.get("to"))),
        "line" => format!(
            " from {} to {}",
            at(annotation.get("from")),
            at(annotation.get("to"))
        ),
        _ => format!(" at {}, {}", at(rect), size(rect)),
    };
    format!(
        "{}. {}{detail}{}",
        index + 1,
        type_name(&kind),
        suffix(" — on ", annotation.get("target"))
    )
}

fn kind(annotation: &Value) -> Option<&str> {
    annotation.get("type").and_then(Value::as_str)
}

fn items(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| js::truthy(Some(value)))
}

/// The prompt for a canvas payload: one page, or a canvas review.
///
/// # Errors
/// Returns why a canvas review (version 2) is malformed.
pub(crate) fn prompt(payload: &Value) -> Result<String> {
    if payload.get("version").and_then(Value::as_f64) == Some(2.0) {
        return canvas::canvas_prompt(payload, prompt);
    }
    Ok(page_prompt(payload))
}

fn page_prompt(payload: &Value) -> String {
    let docs = present(payload.get("docs"));
    let lens = present(payload.get("lens"));
    let annotations = items(payload.get("annotations"));
    let mut out = facts(payload, docs, lens);
    if docs.is_some() {
        out.push(
            "The screenshot is the part of the docs in view, with their examples rendered live."
                .into(),
        );
        out.push(String::new());
    } else if lens.is_some() {
        out.push(format!(
            "The screenshot is the implementation at that address, seen through the workbench — \
             the design it should match is `{}`.",
            js::text(payload.get("src"))
        ));
        out.push(String::new());
    }
    if annotations.is_empty() {
        out.push(
            if docs.is_some() {
                "Nothing is annotated — the screenshot is the docs as they stand."
            } else if lens.is_some() {
                "Nothing is annotated — the screenshot is the implementation as it stands."
            } else {
                "Nothing is annotated — the screenshot is the design as it stands."
            }
            .into(),
        );
    } else {
        out.extend(annotated(payload, annotations));
    }
    out.join("\n")
}

/// Where the page came from and how it was seen, ending with a blank line.
fn facts(payload: &Value, docs: Option<&Value>, lens: Option<&Value>) -> Vec<String> {
    let field = |key| js::text(payload.get(key));
    let mut out = vec![
        "Here is a page from Workbench.".to_owned(),
        String::new(),
        format!("- Page: {} — `{}`", field("label"), field("src")),
    ];
    // Only when it isn't the default one.
    if js::truthy(payload.get("scenario")) {
        out.push(format!("- Scenario: {}", field("scenario")));
    }
    // Only when the artboard shows one; its source is among the code pointers.
    let iteration = payload.get("iteration");
    if let Some(id) = present(iteration.and_then(|iteration| iteration.get("id"))) {
        let id = js::text(Some(id));
        let label = present(iteration.and_then(|iteration| iteration.get("label")))
            .map_or_else(|| id.clone(), |label| js::text(Some(label)));
        out.push(format!("- Iteration: {label} ({id})"));
    }
    if let Some(story) = present(payload.get("story")) {
        out.push(format!(
            "- Story: {} — `{}`",
            js::text(story.get("name")),
            js::text(story.get("id"))
        ));
    }
    if let Some(docs) = docs {
        let markdown =
            present(docs.get("markdown")).map_or_else(|| field("src"), |m| js::text(Some(m)));
        let rendered = suffix(", examples rendered by ", docs.get("lensLabel"));
        out.push(format!("- Docs: `{markdown}`{rendered}"));
        let shown: Vec<String> = items(docs.get("examples"))
            .iter()
            .filter(|example| js::truthy(example.get("id")))
            .map(|example| {
                let file = present(example.get("file"))
                    .map(|file| format!(" — `{}`", js::text(Some(file))))
                    .unwrap_or_default();
                let status = present(example.get("status"))
                    .filter(|status| status.as_str() != Some("ready"))
                    .map(|status| format!(" ({})", js::text(Some(status))))
                    .unwrap_or_default();
                format!("{}{file}{status}", js::text(example.get("id")))
            })
            .collect();
        if !shown.is_empty() {
            out.push(format!("- Examples in view: {}", shown.join("; ")));
        }
    }
    match (lens, docs) {
        (Some(lens), None) => out.push(format!(
            "- Implementation: {} — `{}`",
            js::text(lens.get("label")),
            js::text(lens.get("url"))
        )),
        (None, None) if js::truthy(payload.get("lensLabel")) => {
            out.push(format!("- Lens: {}", field("lensLabel")));
        }
        _ => {}
    }
    let code = items(payload.get("code"));
    if !code.is_empty() {
        let files: Vec<String> = code
            .iter()
            .map(|file| format!("`{}`", js::text(Some(file))))
            .collect();
        out.push(format!("- Source: {}", files.join(", ")));
    }
    let frame = payload.get("frame");
    let side = |key| js::text(frame.and_then(|frame| frame.get(key)));
    let (width, height) = (side("w"), side("h"));
    out.push(if docs.is_some() {
        format!("- View: the docs fill the canvas; the part in view is {width} × {height} CSS px")
    } else {
        format!(
            "- Size: {} (artboard is {width} × {height} CSS px)",
            field("size")
        )
    });
    out.push(format!("- Screenshot: `{}`", field("file")));
    out.push(String::new());
    out
}

/// The annotations in words: the warning that red is not the design, one
/// line each, what landed on nothing, and what the notes ask for.
fn annotated(payload: &Value, annotations: &[Value]) -> Vec<String> {
    let mut out = vec![
        format!(
            "The screenshot has {} {} drawn over it. **Everything red in the image is an \
             annotation, not the design** — arrows, boxes, circles, scribbles and the red text \
             are notes about the page, drawn on top of it. Nothing red is something to build.",
            annotations.len(),
            if annotations.len() == 1 {
                "annotation"
            } else {
                "annotations"
            }
        ),
        String::new(),
        "Rather than reading them off the picture, here they are. Coordinates are CSS pixels \
         from the top-left of the artboard at the size above, and the element named on each \
         line is the one under that annotation in the live DOM:"
            .into(),
        String::new(),
    ];
    out.extend(
        annotations
            .iter()
            .enumerate()
            .map(|(index, annotation)| annotation_line(annotation, index)),
    );

    // Annotations over nothing are worth admitting to.
    let unplaced = annotations
        .iter()
        .filter(|annotation| {
            kind(annotation) != Some("text") && !js::truthy(annotation.get("target"))
        })
        .count();
    if unplaced > 0 && payload.get("inspected") == Some(&Value::Bool(false)) {
        // A page served from elsewhere, whose DOM the workbench can't read.
        out.push(String::new());
        out.push(
            "(The elements under the annotations couldn’t be read — the page is served from \
             elsewhere — so go by the coordinates and the screenshot.)"
                .into(),
        );
    } else if unplaced > 0 {
        let one = unplaced == 1;
        out.push(String::new());
        out.push(format!(
            "({unplaced} of them {} over empty space rather than an element — check the \
             screenshot for what {}.)",
            if one { "is" } else { "are" },
            if one { "it means" } else { "they mean" }
        ));
    }

    // Notes and comments carry words, and those words are the request.
    let asks: Vec<String> = annotations
        .iter()
        .filter(|annotation| {
            matches!(kind(annotation), Some("text" | "comment"))
                && js::truthy(annotation.get("text"))
        })
        .map(|ask| format!("- “{}”", js::text(ask.get("text"))))
        .collect();
    if !asks.is_empty() {
        out.push(String::new());
        out.push("In words, the notes say:".into());
        out.extend(asks);
    }
    out
}

#[cfg(test)]
mod tests;
