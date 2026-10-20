//! Which planned references an export request captures: its states, sizes
//! and docs lens, or exactly the current view once. Pure.
use super::paths::query_param;
use super::request::ExportRequest;
use super::types::Reference;
use std::collections::BTreeSet;

fn present(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !text.is_empty())
}

/// The references `request` asks for, each page and variant once.
pub(crate) fn select_captures(captures: Vec<Reference>, request: &ExportRequest) -> Vec<Reference> {
    let current = if request.variants == "current" {
        request.current.as_ref()
    } else {
        None
    };
    let first_state = captures
        .iter()
        .find(|capture| !capture.docs_page)
        .map(|capture| capture.state.clone());
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
            let state = present(current.state.as_deref()).or(first_state.as_deref());
            if Some(reference.state.as_str()) != state {
                continue;
            }
            if let Some(artboard) = present(current.artboard.as_deref())
                && reference.artboard.as_deref() != Some(artboard)
            {
                continue;
            }
        } else {
            if !request.states.is_empty() && !request.states.contains(&reference.state) {
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
            selected.variant = format!("{}-current", selected.state);
        }
        if seen.insert(format!("{}:{}", selected.page, selected.variant)) {
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

    fn reference(page: &str, state: &str, size: &str, url: &str) -> Reference {
        Reference {
            page: page.into(),
            state: state.into(),
            size: size.into(),
            variant: format!("{state}{size}"),
            url: url.into(),
            width: 100,
            height: 100,
            ..Reference::default()
        }
    }

    #[test]
    fn visual_page_defaults_use_the_current_state_and_actual_dimensions_exactly_once() {
        let request = read_request(&json!({ "format": "pdf", "scope": "page",
            "current": { "page": "a", "state": null, "width": 777, "height": 600 } }))
        .expect("request");
        let captures = ["wide", "mobile"]
            .iter()
            .flat_map(|size| {
                ["first", "error"].map(|state| reference("a", state, size, "http://localhost/a"))
            })
            .collect();
        let selected = select_captures(captures, &request);
        assert_eq!(selected.len(), 1);
        assert_eq!(
            (
                selected[0].state.as_str(),
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
            state: "default".into(),
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
    fn docs_references_follow_the_lens_and_pdfs_skip_example_crops() {
        let docs = |lens: &str, selector: Option<&str>| Reference {
            page: "docs/button.md".into(),
            state: "default".into(),
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
    fn custom_variants_keep_the_asked_states_and_sizes() {
        let request = read_request(
            &json!({ "variants": "custom", "states": ["error"], "sizes": ["mobile"] }),
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
