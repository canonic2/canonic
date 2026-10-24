//! What an export asks for, read from the canvas's request body, and the part
//! of the space it selects. Pure; errors are messages for a 400 answer.
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// The view a current-view export captures: the page and size on screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CurrentView {
    pub(crate) page: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) scenario: Option<String>,
    pub(crate) lens: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) artboard: Option<String>,
}

/// One export request. Serialized, it is the key identical requests share.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportRequest {
    pub(crate) format: String,
    pub(crate) scope: String,
    pub(crate) pages: Vec<String>,
    pub(crate) variants: String,
    pub(crate) scenarios: Vec<String>,
    pub(crate) sizes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) collection: Option<String>,
    pub(crate) lens: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) current: Option<CurrentView>,
    pub(crate) paper: String,
    pub(crate) image_format: String,
}

impl ExportRequest {
    pub(crate) fn is(&self, format: &str) -> bool {
        self.format == format
    }
}

fn length(text: &str) -> usize {
    text.encode_utf16().count()
}

fn record<'a>(value: &'a Value, message: &str) -> Result<&'a Map<String, Value>, String> {
    value.as_object().ok_or_else(|| message.to_owned())
}

fn strings(value: Option<&Value>, label: &str) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let invalid = || format!("Invalid export {label}.");
    let list = value.as_array().ok_or_else(invalid)?;
    if list.len() > 10_000 {
        return Err(invalid());
    }
    let mut out = BTreeSet::new();
    for item in list {
        let text = item
            .as_str()
            .filter(|text| !text.is_empty() && length(text) <= 2000);
        out.insert(text.ok_or_else(invalid)?.to_owned());
    }
    Ok(out.into_iter().collect())
}

fn choice(value: Option<&Value>, choices: &[&str], fallback: &str) -> Result<String, String> {
    let Some(value) = value else {
        return Ok(fallback.to_owned());
    };
    match value.as_str() {
        Some(text) if choices.contains(&text) => Ok(text.to_owned()),
        _ => Err(format!(
            "Invalid export option: {}",
            value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned)
        )),
    }
}

fn dimension(value: &Value) -> Option<u32> {
    let number = value.as_f64()?;
    (number.fract() == 0.0 && (1.0..=8192.0).contains(&number)).then(|| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a whole number from 1 to 8192"
        )]
        let whole = number as u32;
        whole
    })
}

fn current_view(value: &Value) -> Result<CurrentView, String> {
    let view = record(value, "Export options must be an object.")?;
    let invalid = || "Invalid current export view.".to_owned();
    let page = view
        .get("page")
        .and_then(Value::as_str)
        .filter(|page| !page.is_empty() && length(page) <= 2000)
        .ok_or_else(invalid)?;
    let width = view.get("width").and_then(dimension).ok_or_else(invalid)?;
    let height = view.get("height").and_then(dimension).ok_or_else(invalid)?;
    for key in ["scenario", "lens", "size", "artboard"] {
        match view.get(key) {
            None | Some(Value::Null) => {}
            Some(Value::String(text)) if length(text) <= 2000 => {}
            Some(_) => return Err(format!("Invalid current export {key}.")),
        }
    }
    let text = |key: &str| view.get(key).and_then(Value::as_str).map(str::to_owned);
    Ok(CurrentView {
        page: page.to_owned(),
        width,
        height,
        scenario: text("scenario"),
        lens: text("lens"),
        size: text("size"),
        artboard: text("artboard"),
    })
}

/// Reads an export request; `null` asks for the defaults.
///
/// # Errors
/// A message for the canvas when an option is invalid or inconsistent.
pub(crate) fn read_request(value: &Value) -> Result<ExportRequest, String> {
    let empty = Map::new();
    let input = if value.is_null() {
        &empty
    } else {
        record(value, "Export options must be an object.")?
    };
    let format = choice(
        input.get("format"),
        &["zip", "pdf", "images", "browser"],
        "zip",
    )?;
    let scope = choice(
        input.get("scope"),
        &["space", "page", "pages", "collection"],
        "space",
    )?;
    let visual = scope == "page" && (format == "pdf" || format == "images");
    let variants = choice(
        input.get("variants"),
        &["all", "current", "custom"],
        if visual { "current" } else { "all" },
    )?;
    if format == "browser" && variants == "current" {
        return Err("Portable browser export uses declared variants.".into());
    }
    let current = input.get("current").map(current_view).transpose()?;
    let mut pages = strings(input.get("pages"), "pages")?;
    if scope == "page"
        && pages.is_empty()
        && let Some(current) = &current
    {
        pages.push(current.page.clone());
    }
    if (scope == "page" && pages.len() != 1) || (scope == "pages" && pages.is_empty()) {
        return Err("Choose the pages to export.".into());
    }
    if variants == "current"
        && (scope != "page"
            || current
                .as_ref()
                .is_none_or(|current| current.page != pages[0]))
    {
        return Err("Current view export requires its selected page.".into());
    }
    let collection = input
        .get("collection")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if scope == "collection" && collection.as_deref().is_none_or(str::is_empty) {
        return Err("Choose a collection to export.".into());
    }
    let lens = match input.get("lens") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if length(text) <= 2000 => {
            Some(text.clone()).filter(|text| !text.is_empty())
        }
        Some(_) => return Err("Invalid export lens.".into()),
    };
    Ok(ExportRequest {
        format,
        scope,
        pages,
        variants,
        scenarios: strings(input.get("scenarios"), "scenarios")?,
        sizes: strings(input.get("sizes"), "sizes")?,
        collection,
        lens,
        current,
        paper: choice(input.get("paper"), &["A4", "Letter"], "A4")?,
        image_format: choice(input.get("imageFormat"), &["png", "jpeg"], "png")?,
    })
}

/// Only identical requests share a background job.
pub(crate) fn request_key(request: &ExportRequest) -> String {
    serde_json::to_string(request).unwrap_or_default()
}

fn page_ids(items: &[Value], out: &mut Vec<String>) {
    for item in items {
        match item["src"].as_str().filter(|src| !src.is_empty()) {
            Some(src) => out.push(src.to_owned()),
            None => page_ids(item["items"].as_array().map_or(&[], Vec::as_slice), out),
        }
    }
}

fn filter_items(items: &[Value], ids: &BTreeSet<String>) -> Vec<Value> {
    items
        .iter()
        .filter_map(|item| {
            if let Some(src) = item["src"].as_str().filter(|src| !src.is_empty()) {
                return ids.contains(src).then(|| item.clone());
            }
            let children = filter_items(item["items"].as_array().map_or(&[], Vec::as_slice), ids);
            (!children.is_empty()).then(|| {
                let mut group = item.clone();
                group["items"] = Value::Array(children);
                group
            })
        })
        .collect()
}

fn filter_collections(collections: &Value, ids: &BTreeSet<String>) -> Value {
    Value::Array(
        collections
            .as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter_map(|collection| {
                let items = filter_items(
                    collection["items"].as_array().map_or(&[], Vec::as_slice),
                    ids,
                );
                (!items.is_empty()).then(|| {
                    let mut kept = collection.clone();
                    kept["items"] = Value::Array(items);
                    kept
                })
            })
            .collect(),
    )
}

/// The space narrowed to the request's pages, keeping its collections' groups.
///
/// # Errors
/// A collection or page that is not this space's, or nothing to export.
pub(crate) fn select_view(view: &Value, request: &ExportRequest) -> Result<Value, String> {
    let empty = Map::new();
    let all = view["pages"].as_object().unwrap_or(&empty);
    let mut selected: Vec<String> = all.keys().cloned().collect();
    if request.scope == "page" || request.scope == "pages" {
        selected.clone_from(&request.pages);
    }
    if request.scope == "collection" {
        let wanted = request.collection.as_deref().unwrap_or_default();
        let collections: Vec<&Value> = ["collections", "catalogCollections"]
            .iter()
            .flat_map(|key| view[*key].as_array().map_or(&[][..], Vec::as_slice))
            .filter(|collection| collection["name"] == wanted)
            .collect();
        if collections.is_empty() {
            return Err("That export collection does not exist.".into());
        }
        selected.clear();
        for collection in collections {
            page_ids(
                collection["items"].as_array().map_or(&[], Vec::as_slice),
                &mut selected,
            );
        }
    }
    if selected.iter().any(|id| !all.contains_key(id)) {
        return Err("An export page does not belong to this space.".into());
    }
    let ids: BTreeSet<String> = selected.into_iter().collect();
    if ids.is_empty() {
        return Err("There are no pages to export.".into());
    }
    let mut out = view.clone();
    out["pages"] = Value::Object(
        all.iter()
            .filter(|(id, _)| ids.contains(*id))
            .map(|(id, page)| (id.clone(), page.clone()))
            .collect(),
    );
    out["collections"] = filter_collections(&view["collections"], &ids);
    out["catalogCollections"] = filter_collections(&view["catalogCollections"], &ids);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_export_the_whole_space_as_a_zip() {
        let request = read_request(&Value::Null).expect("defaults");
        assert_eq!(
            request_key(&request),
            r#"{"format":"zip","scope":"space","pages":[],"variants":"all","scenarios":[],"sizes":[],"lens":null,"paper":"A4","imageFormat":"png"}"#
        );
        assert_eq!(read_request(&json!({})).expect("empty"), request);
    }

    #[test]
    fn a_visual_page_export_defaults_to_the_current_view() {
        let request = read_request(&json!({ "format": "pdf", "scope": "page",
            "current": { "page": "a", "scenario": null, "width": 777, "height": 600 } }))
        .expect("current");
        assert_eq!(request.variants, "current");
        assert_eq!(request.pages, ["a"]);
        assert_eq!(
            read_request(&json!({ "scope": "page", "pages": ["a"] }))
                .expect("page")
                .variants,
            "all"
        );
    }

    #[test]
    fn invalid_or_inconsistent_options_are_refused() {
        let refused = |value: Value| read_request(&value).expect_err("refused");
        assert_eq!(refused(json!([])), "Export options must be an object.");
        assert_eq!(
            refused(json!({ "format": "docx" })),
            "Invalid export option: docx"
        );
        assert_eq!(
            refused(json!({ "format": null })),
            "Invalid export option: null"
        );
        assert_eq!(
            refused(json!({ "scope": "pages" })),
            "Choose the pages to export."
        );
        assert_eq!(
            refused(json!({ "scope": "collection" })),
            "Choose a collection to export."
        );
        assert_eq!(refused(json!({ "pages": [""] })), "Invalid export pages.");
        assert_eq!(refused(json!({ "lens": 3 })), "Invalid export lens.");
        assert_eq!(
            refused(json!({ "format": "browser", "variants": "current" })),
            "Portable browser export uses declared variants."
        );
        assert_eq!(
            refused(json!({ "variants": "current" })),
            "Current view export requires its selected page."
        );
        assert_eq!(
            refused(
                json!({ "scope": "page", "current": { "page": "a", "width": 0, "height": 1 } })
            ),
            "Invalid current export view."
        );
        assert_eq!(
            refused(
                json!({ "scope": "page", "current": { "page": "a", "width": 1, "height": 1, "size": 4 } })
            ),
            "Invalid current export size."
        );
    }

    #[test]
    fn selected_pages_keep_their_collection_hierarchy_and_reject_foreign_pages() {
        let view = json!({ "pages": { "a": { "label": "A" }, "b": { "label": "B" } },
            "collections": [{ "name": "Pages", "items": [{ "label": "Group", "items": [{ "src": "a" }, { "src": "b" }] }] }] });
        let selected = select_view(
            &view,
            &read_request(&json!({ "scope": "pages", "pages": ["b"] })).expect("pages"),
        )
        .expect("selected");
        assert_eq!(
            selected["pages"]
                .as_object()
                .map(|pages| pages.keys().cloned().collect::<Vec<_>>()),
            Some(vec!["b".to_owned()])
        );
        assert_eq!(
            selected["collections"][0]["items"][0]["items"],
            json!([{ "src": "b" }])
        );
        assert_eq!(selected["catalogCollections"], json!([]));
        let foreign =
            read_request(&json!({ "scope": "page", "pages": ["foreign"] })).expect("page");
        assert_eq!(
            select_view(&view, &foreign).expect_err("foreign"),
            "An export page does not belong to this space."
        );
    }

    #[test]
    fn a_collection_export_takes_every_page_under_that_name() {
        let view = json!({ "pages": { "a": {}, "b": {}, "c": {} },
            "collections": [{ "name": "Pages", "items": [{ "src": "a" }] }, { "name": "Other", "items": [{ "src": "b" }] }],
            "catalogCollections": [{ "name": "Pages", "items": [{ "src": "c" }] }] });
        let request = read_request(&json!({ "scope": "collection", "collection": "Pages" }))
            .expect("collection");
        let selected = select_view(&view, &request).expect("selected");
        assert_eq!(selected["pages"], json!({ "a": {}, "c": {} }));
        assert_eq!(
            selected["collections"],
            json!([{ "name": "Pages", "items": [{ "src": "a" }] }])
        );
        let missing = read_request(&json!({ "scope": "collection", "collection": "Missing" }))
            .expect("collection");
        assert_eq!(
            select_view(&view, &missing).expect_err("missing"),
            "That export collection does not exist."
        );
        assert_eq!(
            select_view(
                &json!({ "pages": {} }),
                &read_request(&Value::Null).expect("space")
            )
            .expect_err("empty"),
            "There are no pages to export."
        );
    }
}
