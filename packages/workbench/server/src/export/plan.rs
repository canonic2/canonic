//! The reference screenshots an export plans: every declared scenario of every
//! page at each size it supports, a preview's scenarios or their artboards,
//! each declared iteration beside its artboard, and for a docs page the whole
//! page and each example a lens renders. The capture
//! helper loads these addresses itself, so exporting never drives the
//! visible canvas. Pure, except that a missing design file is checked.
use super::paths::page_url;
use super::sizes::{EXPORT_FILL, default_sizes, export_sizes, supported};
use super::types::Reference;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

/// A docs page's capture width: its 960-pixel layout and the page's margins.
pub(crate) const DOCS_EXPORT_WIDTH: u32 = 960 + 2 * 48;

/// References to capture, and why some pages have none.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Plan {
    pub(crate) captures: Vec<Reference>,
    pub(crate) warnings: Vec<String>,
}

/// Every page item of `collections`, groups flattened, in order.
pub(crate) fn page_items(collections: &Value) -> Vec<&Value> {
    fn visit<'a>(entry: &'a Value, out: &mut Vec<&'a Value>) {
        if entry["group"]
            .as_str()
            .is_some_and(|group| !group.is_empty())
        {
            for item in entry["items"].as_array().map_or(&[][..], Vec::as_slice) {
                visit(item, out);
            }
        } else if entry["src"].as_str().is_some_and(|src| !src.is_empty()) {
            out.push(entry);
        }
    }
    let mut out = Vec::new();
    for collection in collections.as_array().map_or(&[][..], Vec::as_slice) {
        for entry in collection["items"]
            .as_array()
            .map_or(&[][..], Vec::as_slice)
        {
            visit(entry, &mut out);
        }
    }
    out
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

fn list(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

fn dimension(value: &Value) -> u32 {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_default()
}

fn is_markdown_page(item: &Value) -> bool {
    item["markdown"]
        .as_str()
        .is_some_and(|markdown| !markdown.is_empty() && item["src"] == markdown)
}

/// The capture plan for a space's pages; docs pages are planned separately.
pub(crate) fn capture_plan(view: &Value, base_url: &str) -> Plan {
    let mut items = page_items(&view["collections"]);
    items.extend(page_items(&view["catalogCollections"]));
    let space_sizes = view["sizes"]
        .as_array()
        .cloned()
        .unwrap_or_else(default_sizes);
    let mut plan = Plan::default();
    let mut seen = BTreeSet::new();
    for item in items {
        let src = text(&item["src"]);
        if !seen.insert(src.to_owned()) || is_markdown_page(item) {
            continue;
        }
        let known = &view["pages"][src];
        let keys = known["sizes"].as_array().or(item["sizes"].as_array());
        let own = known["ownSizes"].as_array().or(item["ownSizes"].as_array());
        let sizes = export_sizes(&supported(
            &space_sizes,
            keys.map(Vec::as_slice),
            own.map_or(&[], Vec::as_slice),
        ));
        let preview = &known["preview"];
        if preview.is_object() {
            plan_preview(&mut plan, src, preview, &sizes, base_url);
            continue;
        }
        if let Some(key) = item["implementationOnly"]
            .as_str()
            .filter(|key| !key.is_empty())
        {
            let kind = view["implementations"][key]["kind"]
                .as_str()
                .filter(|kind| !kind.is_empty());
            plan.warnings.push(format!(
                "{}: reference screenshots are not supported for {}.",
                text(&item["label"]),
                kind.unwrap_or("this implementation")
            ));
            continue;
        }
        if let Some(design) = known["design"].as_str().filter(|design| !design.is_empty())
            && !Path::new(design).exists()
        {
            plan.warnings.push(format!(
                "{}: the design file is missing, so no reference screenshot was captured.",
                text(&item["label"])
            ));
            continue;
        }
        let default_scenario = [serde_json::json!({ "id": "default", "label": "Default" })];
        let scenarios = if list(&item["scenarios"]).is_empty() {
            &default_scenario[..]
        } else {
            list(&item["scenarios"])
        };
        for (index, scenario) in scenarios.iter().enumerate() {
            let id = text(&scenario["id"]);
            let mut params = vec![("actions", "off")];
            if index > 0 {
                params.push(("scenario", id));
            }
            let url = page_url(src, base_url, &params);
            plan_sizes(&mut plan, src, scenario, None, &url, &sizes);
            for iteration in list(&scenario["iterations"]) {
                // Like the canvas, the first scenario loads without naming itself,
                // so the page marks it `default`.
                let mut iteration_params = params.clone();
                iteration_params.push(("iteration", text(&iteration["id"])));
                let url = page_url(src, base_url, &iteration_params);
                plan_sizes(&mut plan, src, scenario, Some(iteration), &url, &sizes);
            }
        }
    }
    plan
}

/// One reference per size for a scenario's implicit artboard, or one of its
/// iterations, loaded from `url`.
fn plan_sizes(
    plan: &mut Plan,
    src: &str,
    scenario: &Value,
    iteration: Option<&Value>,
    url: &str,
    sizes: &[super::sizes::ExportSize],
) {
    let id = text(&scenario["id"]);
    let (variant, label) = match iteration {
        Some(iteration) => (
            format!("{id}+{}", text(&iteration["id"])),
            format!(
                "{} · {}",
                text(&scenario["label"]),
                text(&iteration["label"])
            ),
        ),
        None => (id.to_owned(), text(&scenario["label"]).to_owned()),
    };
    for size in sizes {
        plan.captures.push(Reference {
            page: src.to_owned(),
            scenario: id.to_owned(),
            iteration: iteration.map(|iteration| text(&iteration["id"]).to_owned()),
            variant: format!("{variant}-{}", size.key),
            label: label.clone(),
            size: size.key.clone(),
            size_label: size.label.clone(),
            url: url.to_owned(),
            external: false,
            width: size.width,
            height: size.height,
            ..Reference::default()
        });
    }
}

/// Per scenario: each declared artboard at its authored size, or the implicit
/// artboard at every size; then each of their iterations the same way.
fn plan_preview(
    plan: &mut Plan,
    src: &str,
    preview: &Value,
    sizes: &[super::sizes::ExportSize],
    base_url: &str,
) {
    let file = text(&preview["file"]);
    for scenario in list(&preview["scenarios"]) {
        let id = text(&scenario["id"]);
        let artboards = list(&scenario["artboards"]);
        if artboards.is_empty() {
            let url = page_url(file, base_url, &[("scenario", id)]);
            plan_sizes(plan, src, scenario, None, &url, sizes);
            for iteration in list(&scenario["iterations"]) {
                let url = page_url(
                    file,
                    base_url,
                    &[("scenario", id), ("iteration", text(&iteration["id"]))],
                );
                plan_sizes(plan, src, scenario, Some(iteration), &url, sizes);
            }
            continue;
        }
        for board in artboards {
            let board_id = text(&board["id"]);
            let board_label = text(&board["label"]);
            // Different scenarios can declare the same artboard, so labels name both.
            let scenario_label = text(&scenario["label"]);
            let base = Reference {
                page: src.to_owned(),
                scenario: id.to_owned(),
                artboard: Some(board_id.to_owned()),
                variant: format!("{id}-{board_id}"),
                label: format!("{scenario_label} · {board_label}"),
                size: board_id.to_owned(),
                size_label: board_label.to_owned(),
                url: page_url(file, base_url, &[("scenario", id), ("artboard", board_id)]),
                external: false,
                width: dimension(&board["width"]),
                height: dimension(&board["height"]),
                ..Reference::default()
            };
            plan.captures.push(base.clone());
            for iteration in list(&board["iterations"]) {
                let iteration_id = text(&iteration["id"]);
                plan.captures.push(Reference {
                    iteration: Some(iteration_id.to_owned()),
                    variant: format!("{id}-{board_id}+{iteration_id}"),
                    label: format!(
                        "{scenario_label} · {board_label} · {}",
                        text(&iteration["label"])
                    ),
                    url: page_url(
                        file,
                        base_url,
                        &[
                            ("scenario", id),
                            ("artboard", board_id),
                            ("iteration", iteration_id),
                        ],
                    ),
                    ..base.clone()
                });
            }
        }
    }
}

/// One docs lens of a page, with the examples it can render or why it can't list them.
#[derive(Debug, Clone)]
pub(crate) struct DocsLensPlan {
    pub(crate) key: String,
    pub(crate) label: String,
    /// The example IDs the lens renders; empty where examples can't run here.
    pub(crate) examples: Result<Vec<String>, String>,
}

/// One docs page as its Markdown places examples.
#[derive(Debug, Clone)]
pub(crate) struct DocsPagePlan {
    /// The page the docs belong to.
    pub(crate) page: String,
    /// The Markdown file, relative to the project root.
    pub(crate) src: String,
    pub(crate) label: String,
    /// The examples the Markdown places, `(id, label)`; `None` when the file is missing.
    pub(crate) placed: Option<Vec<(String, String)>>,
    pub(crate) lenses: Vec<DocsLensPlan>,
}

/// Each docs page whole at its layout width, and each example a lens renders,
/// cropped to its panel, per lens.
pub(crate) fn docs_capture_plan(pages: &[DocsPagePlan], base_url: &str) -> Plan {
    let mut plan = Plan::default();
    for page in pages {
        let Some(placed) = &page.placed else {
            plan.warnings
                .push(format!("{}: the Markdown file doesn’t exist.", page.label));
            continue;
        };
        let lenses: Vec<Option<&DocsLensPlan>> = if page.lenses.is_empty() {
            vec![None]
        } else {
            page.lenses.iter().map(Some).collect()
        };
        for lens in lenses {
            let available = match lens.map(|lens| &lens.examples) {
                None => Vec::new(),
                Some(Ok(ids)) => ids.clone(),
                Some(Err(error)) => {
                    let label = lens.map_or("", |lens| lens.label.as_str());
                    plan.warnings
                        .push(format!("{} — {label}: {error}", page.label));
                    Vec::new()
                }
            };
            let url = match lens {
                Some(lens) => page_url(&page.src, base_url, &[("lens", &lens.key)]),
                None => page_url(&page.src, base_url, &[]),
            };
            let prefix = lens.map_or(String::new(), |lens| format!("{}-", lens.key));
            let named = |label: &str| {
                lens.map_or(label.to_owned(), |lens| format!("{} · {label}", lens.label))
            };
            let base = Reference {
                page: page.page.clone(),
                scenario: "default".into(),
                size: "docs".into(),
                size_label: "Docs page".into(),
                url,
                external: false,
                docs_page: true,
                width: DOCS_EXPORT_WIDTH,
                height: EXPORT_FILL.1,
                ..Reference::default()
            };
            plan.captures.push(Reference {
                variant: format!("{prefix}page"),
                label: named("Whole page"),
                ..base.clone()
            });
            for (id, label) in placed {
                if available.contains(id) {
                    plan.captures.push(Reference {
                        variant: format!("{prefix}{id}"),
                        label: named(label),
                        selector: Some(format!("#example-{id} [data-wb-example-stage]")),
                        ..base.clone()
                    });
                }
            }
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Variant, artboard, iteration, label and URL of one capture.
    type Row<'a> = (&'a str, Option<&'a str>, Option<&'a str>, &'a str, &'a str);

    #[test]
    fn plans_every_local_scenario_for_export_capture_and_warns_for_a_catalogued_simulator_page() {
        let view = json!({
            "implementations": { "ios": { "kind": "ios-simulator", "device": "booted", "catalog": true } },
            "collections": [{ "items": [{
                "label": "Sign in", "src": "pages/sign-in.html",
                "sizes": ["laptop", "mobile", "resizable", "fit"],
                "scenarios": [{ "id": "default", "label": "Default" }, { "id": "error", "label": "Error" }],
            }] }],
            "catalogCollections": [{ "items": [{ "label": "iPhone 17", "src": "__ios-simulator/ios/SIM-1.html", "implementationOnly": "ios" }] }],
            "pages": {},
        });
        let plan = capture_plan(&view, "http://127.0.0.1:3579/");
        let shown: Vec<(&str, &str, &str, u32, u32, bool)> = plan
            .captures
            .iter()
            .map(|capture| {
                (
                    capture.page.as_str(),
                    capture.scenario.as_str(),
                    capture.size.as_str(),
                    capture.width,
                    capture.height,
                    capture.external,
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                ("pages/sign-in.html", "default", "laptop", 1512, 982, false),
                ("pages/sign-in.html", "default", "mobile", 393, 852, false),
                ("pages/sign-in.html", "default", "fit", 1440, 900, false),
                ("pages/sign-in.html", "error", "laptop", 1512, 982, false),
                ("pages/sign-in.html", "error", "mobile", 393, 852, false),
                ("pages/sign-in.html", "error", "fit", 1440, 900, false),
            ]
        );
        assert_eq!(
            plan.captures[0].url,
            "http://127.0.0.1:3579/pages/sign-in.html?actions=off"
        );
        assert!(plan.captures[3].url.contains("scenario=error"));
        assert_eq!(
            plan.warnings,
            ["iPhone 17: reference screenshots are not supported for ios-simulator."]
        );
    }

    #[test]
    fn an_authored_page_plans_each_scenario_iteration_at_every_size() {
        let view = json!({
            "sizes": [{ "key": "mobile", "label": "Mobile", "kind": "fixed", "width": 393, "height": 852 }],
            "collections": [{ "items": [{
                "label": "Jobs", "src": "pages/jobs.html",
                "scenarios": [
                    { "id": "list", "label": "List",
                      "iterations": [{ "id": "compact", "label": "Compact" }] },
                    { "id": "empty", "label": "Empty",
                      "iterations": [{ "id": "illustrated", "label": "Illustrated" }] },
                ],
            }] }],
            "pages": {},
        });
        let plan = capture_plan(&view, "http://127.0.0.1:3579/");
        let shown: Vec<(&str, Option<&str>, &str, &str)> = plan
            .captures
            .iter()
            .map(|capture| {
                (
                    capture.variant.as_str(),
                    capture.iteration.as_deref(),
                    capture.label.as_str(),
                    capture.url.as_str(),
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                (
                    "list-mobile",
                    None,
                    "List",
                    "http://127.0.0.1:3579/pages/jobs.html?actions=off"
                ),
                // The first scenario loads unnamed, as on the canvas.
                (
                    "list+compact-mobile",
                    Some("compact"),
                    "List · Compact",
                    "http://127.0.0.1:3579/pages/jobs.html?actions=off&iteration=compact"
                ),
                (
                    "empty-mobile",
                    None,
                    "Empty",
                    "http://127.0.0.1:3579/pages/jobs.html?actions=off&scenario=empty"
                ),
                (
                    "empty+illustrated-mobile",
                    Some("illustrated"),
                    "Empty · Illustrated",
                    "http://127.0.0.1:3579/pages/jobs.html?actions=off&scenario=empty&iteration=illustrated"
                ),
            ]
        );
    }

    #[test]
    fn previews_plan_their_scenarios_or_their_artboards_with_each_iteration_and_markdown_pages_none()
     {
        let view = json!({
            "sizes": [{ "key": "mobile", "label": "Mobile", "kind": "fixed", "width": 393, "height": 852 }],
            "collections": [{ "items": [
                { "label": "Button", "src": "button.workbench.ts" },
                { "label": "Toast", "src": "toast.workbench.ts" },
                { "label": "Guide", "src": "docs/guide.md", "markdown": "docs/guide.md" },
                { "label": "Missing", "src": "missing.html" },
            ] }],
            "pages": {
                "button.workbench.ts": { "preview": { "file": "button.workbench.ts", "scenarios": [{ "id": "primary", "label": "Primary",
                    "iterations": [{ "id": "outlined", "label": "Outlined" }] }] } },
                "toast.workbench.ts": { "preview": { "file": "toast.workbench.ts", "scenarios": [
                    { "id": "default", "label": "Default", "artboards": [
                        { "id": "info", "label": "Info", "width": 560, "height": 79 },
                        { "id": "warning", "label": "Warning", "width": 480, "height": 120,
                          "iterations": [{ "id": "bold", "label": "Bold" }] },
                    ] },
                    { "id": "empty", "label": "Empty" },
                ] } },
                "missing.html": { "design": "/nonexistent/acme/missing.html" },
            },
        });
        let plan = capture_plan(&view, "http://127.0.0.1:3579/");
        let variants: Vec<Row<'_>> = plan
            .captures
            .iter()
            .map(|capture| {
                (
                    capture.variant.as_str(),
                    capture.artboard.as_deref(),
                    capture.iteration.as_deref(),
                    capture.label.as_str(),
                    capture.url.as_str(),
                )
            })
            .collect();
        assert_eq!(
            variants,
            [
                (
                    "primary-mobile",
                    None,
                    None,
                    "Primary",
                    "http://127.0.0.1:3579/button.workbench.ts?scenario=primary"
                ),
                (
                    "primary+outlined-mobile",
                    None,
                    Some("outlined"),
                    "Primary · Outlined",
                    "http://127.0.0.1:3579/button.workbench.ts?scenario=primary&iteration=outlined"
                ),
                (
                    "default-info",
                    Some("info"),
                    None,
                    "Default · Info",
                    "http://127.0.0.1:3579/toast.workbench.ts?scenario=default&artboard=info"
                ),
                (
                    "default-warning",
                    Some("warning"),
                    None,
                    "Default · Warning",
                    "http://127.0.0.1:3579/toast.workbench.ts?scenario=default&artboard=warning"
                ),
                (
                    "default-warning+bold",
                    Some("warning"),
                    Some("bold"),
                    "Default · Warning · Bold",
                    "http://127.0.0.1:3579/toast.workbench.ts?scenario=default&artboard=warning&iteration=bold"
                ),
                (
                    "empty-mobile",
                    None,
                    None,
                    "Empty",
                    "http://127.0.0.1:3579/toast.workbench.ts?scenario=empty"
                ),
            ]
        );
        assert_eq!((plan.captures[2].width, plan.captures[2].height), (560, 79));
        assert_eq!(
            (plan.captures[4].width, plan.captures[4].height),
            (480, 120)
        );
        assert_eq!(plan.captures[4].size, "warning");
        assert_eq!(
            plan.warnings,
            ["Missing: the design file is missing, so no reference screenshot was captured."]
        );
    }

    #[test]
    fn a_docs_page_is_captured_whole_and_per_rendered_example_in_each_lens() {
        let page = DocsPagePlan {
            page: "docs/button.md".into(),
            src: "docs/button.md".into(),
            label: "Button".into(),
            placed: Some(vec![
                ("primary".into(), "Primary".into()),
                ("secondary".into(), "Secondary".into()),
            ]),
            lenses: vec![
                DocsLensPlan {
                    key: "web".into(),
                    label: "Web".into(),
                    examples: Ok(vec!["primary".into(), "secondary".into()]),
                },
                DocsLensPlan {
                    key: "native".into(),
                    label: "Native".into(),
                    examples: Ok(vec!["primary".into()]),
                },
                DocsLensPlan {
                    key: "broken".into(),
                    label: "Broken".into(),
                    examples: Err("Build failed".into()),
                },
            ],
        };
        let missing = DocsPagePlan {
            page: "a.md".into(),
            src: "a.md".into(),
            label: "Gone".into(),
            placed: None,
            lenses: vec![],
        };
        let plan = docs_capture_plan(&[page, missing], "http://127.0.0.1:3579/");
        let shown: Vec<(&str, &str, Option<&str>)> = plan
            .captures
            .iter()
            .map(|capture| {
                (
                    capture.variant.as_str(),
                    capture.label.as_str(),
                    capture.selector.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                ("web-page", "Web · Whole page", None),
                (
                    "web-primary",
                    "Web · Primary",
                    Some("#example-primary [data-wb-example-stage]")
                ),
                (
                    "web-secondary",
                    "Web · Secondary",
                    Some("#example-secondary [data-wb-example-stage]")
                ),
                ("native-page", "Native · Whole page", None),
                (
                    "native-primary",
                    "Native · Primary",
                    Some("#example-primary [data-wb-example-stage]")
                ),
                ("broken-page", "Broken · Whole page", None),
            ]
        );
        assert!(
            plan.captures
                .iter()
                .all(|capture| capture.docs_page && capture.width == 1056 && capture.height == 900)
        );
        assert_eq!(
            plan.captures[0].url,
            "http://127.0.0.1:3579/docs/button.md?lens=web"
        );
        assert_eq!(
            plan.warnings,
            [
                "Button — Broken: Build failed",
                "Gone: the Markdown file doesn’t exist."
            ]
        );
    }
}
