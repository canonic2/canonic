//! Which planned references an export request captures: its scenarios, sizes
//! and docs lens with every declared iteration, or exactly the current view,
//! with its iteration, once. Pure.
use super::paths::query_param;
use super::request::{CurrentView, ExportRequest};
use super::types::Reference;
use std::collections::BTreeSet;

fn present(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !text.is_empty())
}

/// The iteration a current view shows; implementation lenses ignore it.
fn current_iteration(current: &CurrentView) -> Option<&str> {
    present(current.iteration.as_deref()).filter(|_| present(current.lens.as_deref()).is_none())
}

/// The references `request` asks for, each page and variant once.
pub(crate) fn select_captures(captures: Vec<Reference>, request: &ExportRequest) -> Vec<Reference> {
    let current = if request.variants == "current" {
        request.current.as_ref()
    } else {
        None
    };
    let first_scenario = captures
        .iter()
        .find(|capture| !capture.docs_page)
        .map(|capture| capture.scenario.clone());
    let first_docs_lens = captures
        .iter()
        .find(|capture| capture.docs_page)
        .and_then(|capture| query_param(&capture.url, "lens"));
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for reference in captures {
        if (request.is("pdf") || current.is_some()) && reference.selector.is_some() {
            continue;
        }
        if reference.docs_page {
            let lens = match current {
                Some(current) => {
                    present(current.lens.as_deref()).or(present(first_docs_lens.as_deref()))
                }
                None => present(request.lens.as_deref()),
            };
            if let Some(lens) = lens
                && query_param(&reference.url, "lens").as_deref() != Some(lens)
            {
                continue;
            }
        } else if let Some(current) = current {
            let scenario = present(current.scenario.as_deref()).or(first_scenario.as_deref());
            if Some(reference.scenario.as_str()) != scenario {
                continue;
            }
            if let Some(artboard) = present(current.artboard.as_deref())
                && reference.artboard.as_deref() != Some(artboard)
            {
                continue;
            }
            if reference.iteration.as_deref() != current_iteration(current) {
                continue;
            }
        } else {
            if !request.scenarios.is_empty() && !request.scenarios.contains(&reference.scenario) {
                continue;
            }
            if reference.artboard.is_none()
                && !request.sizes.is_empty()
                && !request.sizes.contains(&reference.size)
            {
                continue;
            }
        }
        let mut selected = reference;
        if let Some(current) = current
            && !selected.docs_page
            && (selected.artboard.is_none() || present(current.artboard.as_deref()).is_some())
        {
            selected.width = current.width;
            selected.height = current.height;
            present(current.size.as_deref())
                .unwrap_or("current")
                .clone_into(&mut selected.size);
            selected.variant = match &selected.iteration {
                Some(iteration) => format!("{}+{iteration}-current", selected.scenario),
                None => format!("{}-current", selected.scenario),
            };
        }
        // Identity fields as well as the variant name: `+compact-tablet-landscape`
        // could be either `compact` at `tablet-landscape` or `compact-tablet` at
        // `landscape`, while docs example crops differ only by their variant.
        if seen.insert((
            selected.page.clone(),
            selected.scenario.clone(),
            selected.artboard.clone(),
            selected.iteration.clone(),
            selected.size.clone(),
            selected.variant.clone(),
        )) {
            out.push(selected);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::request::read_request;
    use serde_json::json;

    fn reference(page: &str, scenario: &str, size: &str, url: &str) -> Reference {
        Reference {
            page: page.into(),
            scenario: scenario.into(),
            size: size.into(),
            variant: format!("{scenario}{size}"),
            url: url.into(),
            width: 100,
            height: 100,
            ..Reference::default()
        }
    }

    #[test]
    fn visual_page_defaults_use_the_current_scenario_and_actual_dimensions_exactly_once() {
        let request = read_request(&json!({ "format": "pdf", "scope": "page",
            "current": { "page": "a", "scenario": null, "width": 777, "height": 600 } }))
        .expect("request");
        let captures = ["wide", "mobile"]
            .iter()
            .flat_map(|size| {
                ["first", "error"]
                    .map(|scenario| reference("a", scenario, size, "http://localhost/a"))
            })
            .collect();
        let selected = select_captures(captures, &request);
        assert_eq!(selected.len(), 1);
        assert_eq!(
            (
                selected[0].scenario.as_str(),
                selected[0].width,
                selected[0].size.as_str()
            ),
            ("first", 777, "current")
        );
        assert_eq!(selected[0].variant, "first-current");
    }

    #[test]
    fn page_artboards_export_at_authored_sizes_and_current_export_identifies_the_selected_artboard()
    {
        let board = |id: &str, height: u32| Reference {
            page: "feedback".into(),
            scenario: "default".into(),
            artboard: Some(id.into()),
            size: id.into(),
            width: 560,
            height,
            variant: format!("default-{id}"),
            url: format!("http://localhost/feedback?artboard={id}"),
            ..Reference::default()
        };
        let captures = vec![board("info", 79), board("success", 119)];
        let sized =
            read_request(&json!({ "format": "images", "sizes": ["laptop"] })).expect("request");
        assert_eq!(select_captures(captures.clone(), &sized).len(), 2);
        let request = read_request(&json!({ "format": "images", "scope": "page",
            "current": { "page": "feedback", "artboard": "success", "width": 500, "height": 140 } })).expect("request");
        let selected = select_captures(captures, &request);
        assert_eq!(selected.len(), 1);
        assert_eq!(
            (selected[0].artboard.as_deref(), selected[0].width),
            (Some("success"), 500)
        );
        assert!(selected[0].url.contains("artboard=success"));
    }

    #[test]
    fn references_whose_variant_names_coincide_are_both_kept() {
        let shot = |iteration: &str, size: &str| Reference {
            iteration: Some(iteration.into()),
            variant: format!("default+{iteration}-{size}"),
            ..reference("jobs", "default", size, "http://localhost/jobs")
        };
        let captures = vec![
            shot("compact", "tablet-landscape"),
            shot("compact-tablet", "landscape"),
        ];
        assert_eq!(captures[0].variant, captures[1].variant);
        let all = read_request(&json!({ "format": "images" })).expect("request");
        assert_eq!(select_captures(captures, &all).len(), 2);
    }

    #[test]
    fn declared_variants_include_iterations_and_the_current_view_captures_its_own_iteration() {
        let board = |id: &str, iteration: Option<&str>| Reference {
            page: "feedback".into(),
            scenario: "default".into(),
            artboard: Some(id.into()),
            iteration: iteration.map(str::to_owned),
            size: id.into(),
            width: 480,
            height: 120,
            variant: match iteration {
                Some(iteration) => format!("default-{id}+{iteration}"),
                None => format!("default-{id}"),
            },
            url: match iteration {
                Some(iteration) => {
                    format!("http://localhost/feedback?artboard={id}&iteration={iteration}")
                }
                None => format!("http://localhost/feedback?artboard={id}"),
            },
            ..Reference::default()
        };
        let captures = vec![
            board("warning", None),
            board("warning", Some("bold")),
            board("warning", Some("calm")),
        ];
        let all = read_request(&json!({ "format": "images" })).expect("request");
        assert_eq!(select_captures(captures.clone(), &all).len(), 3);
        let current = |iteration: Option<&str>, lens: Option<&str>| {
            read_request(&json!({ "format": "images", "scope": "page",
                "current": { "page": "feedback", "artboard": "warning", "iteration": iteration,
                    "lens": lens, "width": 500, "height": 140 } }))
            .expect("request")
        };
        let selected = select_captures(captures.clone(), &current(Some("bold"), None));
        assert_eq!(selected.len(), 1);
        assert_eq!(
            (
                selected[0].iteration.as_deref(),
                selected[0].variant.as_str(),
                selected[0].width
            ),
            (Some("bold"), "default+bold-current", 500)
        );
        assert!(selected[0].url.contains("iteration=bold"));
        let design = select_captures(captures.clone(), &current(None, None));
        assert_eq!(design.len(), 1);
        assert_eq!(
            (design[0].iteration.as_deref(), design[0].variant.as_str()),
            (None, "default-current")
        );
        let shipped = select_captures(captures, &current(Some("bold"), Some("web")));
        assert_eq!(shipped.len(), 1);
        assert_eq!(shipped[0].iteration, None);
    }

    #[test]
    fn docs_references_follow_the_lens_and_pdfs_skip_example_crops() {
        let docs = |lens: &str, selector: Option<&str>| Reference {
            page: "docs/button.md".into(),
            scenario: "default".into(),
            size: "docs".into(),
            docs_page: true,
            variant: format!("{lens}-{}", selector.map_or("page", |_| "primary")),
            url: format!("http://localhost/docs/button.md?lens={lens}"),
            selector: selector.map(str::to_owned),
            width: 1056,
            height: 900,
            ..Reference::default()
        };
        let captures = vec![
            docs("web", None),
            docs("web", Some("#example-primary")),
            docs("native", None),
        ];
        let web = read_request(&json!({ "lens": "web" })).expect("request");
        assert_eq!(select_captures(captures.clone(), &web).len(), 2);
        let pdf = read_request(&json!({ "format": "pdf" })).expect("request");
        let printed: Vec<String> = select_captures(captures, &pdf)
            .into_iter()
            .map(|capture| capture.variant)
            .collect();
        assert_eq!(printed, ["web-page", "native-page"]);
    }

    #[test]
    fn custom_variants_keep_the_asked_scenarios_and_sizes() {
        let request = read_request(
            &json!({ "variants": "custom", "scenarios": ["error"], "sizes": ["mobile"] }),
        )
        .expect("request");
        let captures = vec![
            reference("a", "default", "mobile", "http://localhost/a"),
            reference("a", "error", "mobile", "http://localhost/a"),
            reference("a", "error", "laptop", "http://localhost/a"),
        ];
        let selected = select_captures(captures, &request);
        assert_eq!(
            selected
                .iter()
                .map(|capture| capture.variant.as_str())
                .collect::<Vec<_>>(),
            ["errormobile"]
        );
    }
}
