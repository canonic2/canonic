//! Bounded Codex browsing coverage. No reference-to-URL inference or policy decisions.
use crate::agents::text;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    q: String,
    recency: Option<u64>,
    domains: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    ref_id: String,
    lineno: Option<u64>,
}

pub(super) fn operation(input: &Value) -> Option<Value> {
    let unsupported = || json!({"kind":"unsupported","name":"webrun"});
    if input
        .as_object()?
        .keys()
        .any(|k| !["search_query", "open", "response_length"].contains(&k.as_str()))
    {
        return Some(unsupported());
    }
    if let Some(length) = input.get("response_length") {
        ["short", "medium", "long"]
            .contains(&length.as_str()?)
            .then_some(())?;
    }
    let mut operations = Vec::new();
    let mut unmodeled = false;
    for action in ["search_query", "open"] {
        let Some(items) = input.get(action) else {
            continue;
        };
        let items = items.as_array()?;
        if items.is_empty() || operations.len() + items.len() > 16 {
            return None;
        }
        for item in items {
            let operation = if action == "search_query" {
                let search: Search = serde_json::from_value(item.clone()).ok()?;
                let query = text(&item["q"], 8192)?;
                // These fields narrow search results; the core decides the query.
                let _ = (search.q, search.recency, search.domains);
                json!({"kind":"web","action":"search","query":query,"url":null})
            } else {
                let open: Open = serde_json::from_value(item.clone()).ok()?;
                let target = text(&item["ref_id"], 8192)?;
                // A line number changes the viewport, not the fetched resource.
                let _ = (open.ref_id, open.lineno);
                if target.to_ascii_lowercase().starts_with("http:")
                    || target.to_ascii_lowercase().starts_with("https:")
                {
                    // Require a direct URL without URL-parser repair. Preserve its
                    // exact spelling for core URL predicates and original-input rules.
                    if target.chars().any(char::is_whitespace)
                        || target.chars().any(char::is_control)
                        || target.contains('\\')
                        || !target.contains("://")
                    {
                        return None;
                    }
                    let (_, authority) = target.split_once("://")?;
                    if authority.starts_with('/') {
                        return None;
                    }
                    let url = url::Url::parse(target).ok()?;
                    url.host_str()?;
                } else {
                    unmodeled = true;
                }
                json!({"kind":"web","action":"fetch","query":null,"url":target})
            };
            operations.push(operation);
        }
    }
    if operations.is_empty() {
        return None;
    }
    // Coverage must be complete before any part reaches evaluation. Never drop
    // an opaque reference or unsupported action from an otherwise allowed batch.
    Some(if unmodeled {
        unsupported()
    } else {
        json!({"kind":"batch","operations":operations})
    })
}
