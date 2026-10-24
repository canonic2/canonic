use super::*;
use serde_json::json;

fn text_of(payload: &Value) -> String {
    prompt(payload).expect("a page prompt")
}

fn has_line(text: &str, line: &str) -> bool {
    text.lines().any(|candidate| candidate == line)
}

#[test]
fn an_authored_lens_label_appears_without_treating_it_as_an_implementation() {
    let text = text_of(
        &json!({ "label": "Button", "src": "button.workbench.ts", "lensLabel": "Reference",
        "size": "Fit", "frame": { "w": 800, "h": 600 }, "file": "button.jpg", "annotations": [] }),
    );
    assert!(has_line(&text, "- Lens: Reference"));
    assert!(!text.contains("- Implementation:"));
    assert!(!text.contains("the design it should match"));
}

#[test]
fn serializes_every_annotation_type_with_its_geometry_and_target() {
    let text = text_of(&json!({
        "label": "Example", "src": "preview/example.html", "size": "Fit", "frame": { "w": 800, "h": 600 },
        "file": ".canonic/.handoffs/example.png",
        "annotations": [
            { "type": "arrow", "from": { "x": 1, "y": 2 }, "to": { "x": 3, "y": 4 }, "target": "button.primary" },
            { "type": "line", "from": { "x": 5, "y": 6 }, "to": { "x": 7, "y": 8 }, "target": "div.rule" },
            { "type": "rect", "box": { "x": 9, "y": 10, "w": 11, "h": 12 }, "target": "section.card" },
            { "type": "ellipse", "box": { "x": 13, "y": 14, "w": 15, "h": 16 }, "target": "span.avatar" },
            { "type": "draw", "box": { "x": 17, "y": 18, "w": 19, "h": 20 }, "target": "nav.sidebar" },
            { "type": "text", "box": { "x": 21, "y": 22, "w": 23, "h": 24 }, "text": "Freeform note" },
            { "type": "comment", "box": { "x": 25, "y": 26, "w": 27, "h": 28 }, "text": "Written comment", "target": "button.company" },
        ],
    }));
    for line in [
        "1. Arrow pointing at (3, 4) — on button.primary",
        "2. Line from (5, 6) to (7, 8) — on div.rule",
        "3. Rectangle at (9, 10), 11 × 12 — on section.card",
        "4. Circle at (13, 14), 15 × 16 — on span.avatar",
        "5. Scribble at (17, 18), 19 × 20 — on nav.sidebar",
        "6. Note at (21, 22): “Freeform note”",
        "7. Comment at (25, 26), 27 × 28: “Written comment” — on button.company",
    ] {
        assert!(has_line(&text, line), "{line}\n{text}");
    }
}

#[test]
fn coordinates_round_as_the_canvas_rounds_them() {
    let line = annotation_line(
        &json!({ "type": "rect", "box": { "x": 9.5, "y": 10.4, "w": 11.49, "h": 12.5 } }),
        0,
    );
    assert_eq!(line, "1. Rectangle at (10, 10), 11 × 13");
}

#[test]
fn includes_both_freeform_notes_and_comments_in_the_written_request_summary() {
    let text = text_of(&json!({
        "label": "Example", "src": "preview/example.html", "size": "Fit", "frame": { "w": 800, "h": 600 },
        "file": ".canonic/.handoffs/example.png",
        "annotations": [
            { "type": "text", "box": { "x": 10, "y": 20, "w": 100, "h": 30 }, "text": "Freeform note" },
            { "type": "comment", "box": { "x": 30, "y": 40, "w": 200, "h": 80 }, "text": "Written comment", "target": null },
        ],
    }));
    assert!(has_line(&text, "In words, the notes say:"));
    assert!(has_line(&text, "- “Freeform note”"));
    assert!(has_line(&text, "- “Written comment”"));
    assert!(text.contains("(1 of them is over empty space"));
}

#[test]
fn reads_word_for_word_for_a_design_without_code_pointers() {
    let text = text_of(&json!({
        "label": "Sign in", "src": "pages/sign-in.html", "scenario": "Wrong password",
        "size": "Mobile, 393 × 852", "frame": { "w": 393, "h": 852 },
        "file": ".canonic/.handoffs/sign-in-error.png",
        "annotations": [
            { "type": "rect", "box": { "x": 9, "y": 10, "w": 11, "h": 12 }, "target": "section.card" },
            { "type": "comment", "box": { "x": 25, "y": 26, "w": 27, "h": 28 }, "text": "Tighter", "target": null },
        ],
    }));
    assert_eq!(text, [
        "Here is a page from Workbench.",
        "",
        "- Page: Sign in — `pages/sign-in.html`",
        "- Scenario: Wrong password",
        "- Size: Mobile, 393 × 852 (artboard is 393 × 852 CSS px)",
        "- Screenshot: `.canonic/.handoffs/sign-in-error.png`",
        "",
        "The screenshot has 2 annotations drawn over it. **Everything red in the image is an annotation, not the design** — arrows, boxes, circles, scribbles and the red text are notes about the page, drawn on top of it. Nothing red is something to build.",
        "",
        "Rather than reading them off the picture, here they are. Coordinates are CSS pixels from the top-left of the artboard at the size above, and the element named on each line is the one under that annotation in the live DOM:",
        "",
        "1. Rectangle at (9, 10), 11 × 12 — on section.card",
        "2. Comment at (25, 26), 27 × 28: “Tighter”",
        "",
        "(1 of them is over empty space rather than an element — check the screenshot for what it means.)",
        "",
        "In words, the notes say:",
        "- “Tighter”",
    ].join("\n"));
}

#[test]
fn says_which_implementation_story_and_source_a_lens_shot_is_of() {
    let text = text_of(&json!({
        "label": "Button", "src": "preview/components-button.html", "scenario": "Icon Only",
        "story": { "id": "components-button--icon-only", "name": "Icon Only" },
        "lens": { "key": "storybook", "label": "Storybook", "kind": "storybook", "url": "http://localhost:6006/iframe.html?id=components-button--icon-only&viewMode=story" },
        "code": ["/acme/packages/ui/src/button.tsx", "/acme/packages/ui/src/button.stories.tsx"],
        "inspected": false, "size": "Fit", "frame": { "w": 800, "h": 600 },
        "file": ".canonic/.handoffs/components-button-icon-only-storybook.png",
        "annotations": [{ "type": "rect", "box": { "x": 9, "y": 10, "w": 11, "h": 12 }, "target": null }],
    }));
    assert!(has_line(&text, "- Scenario: Icon Only"));
    assert!(has_line(
        &text,
        "- Story: Icon Only — `components-button--icon-only`"
    ));
    assert!(has_line(
        &text,
        "- Implementation: Storybook — `http://localhost:6006/iframe.html?id=components-button--icon-only&viewMode=story`"
    ));
    assert!(has_line(
        &text,
        "- Source: `/acme/packages/ui/src/button.tsx`, `/acme/packages/ui/src/button.stories.tsx`"
    ));
    assert!(text.contains("The screenshot is the implementation at that address, seen through the workbench — the design it should match is `preview/components-button.html`."));
    assert!(text.contains("(The elements under the annotations couldn’t be read — the page is served from elsewhere — so go by the coordinates and the screenshot.)"));
    assert!(!text.contains("over empty space"));
}

#[test]
fn names_the_source_on_a_design_that_has_one_and_nothing_else_new() {
    let text = text_of(&json!({
        "label": "Sign in", "src": "pages/sign-in.html", "code": ["/acme/src/pages/login"],
        "size": "Fit", "frame": { "w": 800, "h": 600 }, "file": ".canonic/.handoffs/sign-in.png", "annotations": [],
    }));
    assert!(has_line(&text, "- Source: `/acme/src/pages/login`"));
    assert!(!text.contains("Implementation:"));
    assert!(text.contains("Nothing is annotated — the screenshot is the design as it stands."));
}

#[test]
fn names_a_selected_iteration_after_its_scenario_with_its_source() {
    let text = text_of(&json!({
        "label": "Feedback", "src": "previews/feedback.workbench.ts", "scenario": "Empty",
        "iteration": { "id": "illustrated", "label": "Illustrated" },
        "code": ["previews/feedback.empty-illustrated.tsx"],
        "size": "Fit", "frame": { "w": 800, "h": 600 }, "file": ".canonic/.handoffs/feedback.png", "annotations": [],
    }));
    let lines: Vec<&str> = text.lines().collect();
    let scenario = lines
        .iter()
        .position(|line| *line == "- Scenario: Empty")
        .expect("a scenario line");
    assert_eq!(
        lines[scenario + 1],
        "- Iteration: Illustrated (illustrated)"
    );
    assert!(has_line(
        &text,
        "- Source: `previews/feedback.empty-illustrated.tsx`"
    ));
    let authored = text_of(&json!({
        "label": "Feedback", "src": "previews/feedback.workbench.ts", "iteration": {},
        "size": "Fit", "frame": { "w": 800, "h": 600 }, "file": ".canonic/.handoffs/feedback.png", "annotations": [],
    }));
    assert!(!authored.contains("Iteration"));
}

#[test]
fn a_canvas_review_names_each_artboards_own_iteration() {
    let mut value = review(&json!(null));
    value["artboards"][1]["iteration"] = json!({ "id": "bold", "label": "Bold" });
    let text = prompt(&value).expect("a review");
    assert_eq!(text.matches("- Iteration: Bold (bold)").count(), 1);
}

#[test]
fn an_unannotated_lens_shot_is_the_implementation_as_it_stands() {
    let text = text_of(&json!({
        "label": "Sign in", "src": "pages/sign-in.html",
        "lens": { "key": "dev", "label": "Dev", "kind": "url", "url": "http://localhost:3710/" },
        "size": "Fit", "frame": { "w": 800, "h": 600 }, "file": ".canonic/.handoffs/sign-in-dev.png", "annotations": [],
    }));
    assert!(has_line(
        &text,
        "- Implementation: Dev — `http://localhost:3710/`"
    ));
    assert!(
        text.contains("Nothing is annotated — the screenshot is the implementation as it stands.")
    );
}

#[test]
fn a_docs_lens_handoff_names_the_markdown_lens_and_examples_in_view() {
    let text = text_of(&json!({
        "file": ".canonic/.handoffs/card.jpg", "src": "pages/card.html", "label": "Card", "scenario": null,
        "size": "Docs, filling the canvas", "frame": { "w": 1200, "h": 800 }, "annotations": [],
        "lens": { "key": "dark", "label": "Dark", "kind": "docs", "url": "http://127.0.0.1:3579/docs/card.md?lens=dark" },
        "docs": { "lens": "dark", "markdown": "docs/card.md", "lensLabel": "Dark", "examples": [
            { "id": "basic", "status": "ready", "file": "docs/card.examples.ts (basic)" },
            { "id": "nested", "status": "missing" },
        ] },
    }));
    assert!(has_line(
        &text,
        "- Docs: `docs/card.md`, examples rendered by Dark"
    ));
    assert!(has_line(
        &text,
        "- Examples in view: basic — `docs/card.examples.ts (basic)`; nested (missing)"
    ));
    assert!(!text.contains("Implementation:") && !text.contains("the design it should match"));
    assert!(has_line(
        &text,
        "- View: the docs fill the canvas; the part in view is 1200 × 800 CSS px"
    ));
    assert!(!text.contains("artboard is"));
    assert!(text.contains("the screenshot is the docs as they stand"));
}

fn review(selected: &Value) -> Value {
    let artboards: Vec<Value> = ["a", "b"]
        .iter()
        .enumerate()
        .map(|(i, id)| json!({
            "id": id, "src": "same.html", "space": { "id": format!("space{i}"), "name": format!("Space {i}") },
            "region": { "x": i * 500, "y": 32, "width": 393, "height": 852 },
            "capturedAt": i, "status": if i == 0 { "ready" } else { "loading" },
            "label": "Same", "frame": { "w": 393, "h": 852 }, "annotations": [],
        }))
        .collect();
    json!({ "version": 2, "file": "/tmp/canvas.jpg", "selected": selected, "artboards": artboards })
}

#[test]
fn a_canvas_review_keeps_every_labelled_region_with_its_own_page_prompt() {
    let visited = std::cell::RefCell::new(Vec::new());
    let text = canvas::canvas_prompt(&review(&json!("b")), |board| {
        visited.borrow_mut().push(js::text(board.get("id")));
        Ok(format!(
            "Local annotations for {}",
            js::text(board.get("id"))
        ))
    })
    .expect("a review");
    assert_eq!(*visited.borrow(), ["a", "b"]);
    assert!(text.contains("Space: Space 1 (space1)"));
    assert!(has_line(&text, "- Status: loading"));
    assert!(has_line(&text, "- Image region: (500, 32), 393 × 852"));
    assert!(text.contains("coordinates below are local"));

    let whole = prompt(&review(&json!(null))).expect("a review");
    assert!(has_line(&whole, "- Selected artboard: none"));
    assert!(has_line(&whole, "- Screenshot: `/tmp/canvas.jpg`"));
    assert!(has_line(
        &whole,
        "- Size: 393 × 852 (artboard is 393 × 852 CSS px)"
    ));
}

#[test]
fn a_malformed_canvas_review_says_what_is_wrong() {
    let error = |value: &Value| prompt(value).expect_err("refused").to_string();
    assert!(
        error(&json!({ "version": 2, "file": "/tmp/canvas.jpg", "artboards": [] }))
            .contains("artboards")
    );
    assert_eq!(
        error(&review(&json!("missing"))),
        "Invalid selected artboard in canvas review."
    );
    let mut zero = review(&json!(null));
    zero["artboards"][0]["region"]["width"] = json!(0);
    assert_eq!(error(&zero), "Invalid artboard in canvas review.");
    let mut twice = review(&json!(null));
    twice["artboards"][1]["id"] = json!("a");
    assert_eq!(error(&twice), "Invalid artboard in canvas review.");
}
