//! The page index's previews and docs definitions placed in the page tree:
//! each where its title says, with its page entry, its source in the source
//! menu, and the links and Workbench lens mappings it declares checked.
//! Pure: the answer and the view come in, the additions go out.
use super::answer::IndexAnswer;
use crate::config::page_sizes;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;

/// The page tree's additions, built on the configuration's own.
#[derive(Debug, Default)]
pub(crate) struct Assembly {
    /// Collections the index adds: the catalog collections.
    pub(crate) collections: Vec<Value>,
    /// The configuration's own collections, with resolved lens mappings.
    pub(crate) view_collections: Vec<Value>,
    pub(crate) pages: Map<String, Value>,
    pub(crate) problems: Vec<String>,
    pub(crate) implementations: Map<String, Value>,
    /// Docs pages definitions declare, as the docs service lists pages.
    pub(crate) discovered_docs: Vec<Value>,
}

/// Every page item of a collection list, inside groups too.
pub(crate) fn page_items(collections: &[Value]) -> Vec<&Value> {
    fn visit<'a>(entry: &'a Value, out: &mut Vec<&'a Value>) {
        if entry.get("group").is_some() {
            for item in entry["items"].as_array().into_iter().flatten() {
                visit(item, out);
            }
        } else if entry["src"].as_str().is_some_and(|src| !src.is_empty()) {
            out.push(entry);
        }
    }
    let mut out = Vec::new();
    for collection in collections {
        for entry in collection["items"].as_array().into_iter().flatten() {
            visit(entry, &mut out);
        }
    }
    out
}

/// `manifest.titleIcon`: the icon the longest matching title prefix maps to,
/// else the settings' icon, else the fallback.
fn title_icon(settings: &Value, title: &str, fallback: &str) -> (String, bool) {
    let icons = settings["icons"].as_object();
    let prefix = icons
        .into_iter()
        .flatten()
        .map(|(prefix, _)| prefix)
        .filter(|prefix| title == prefix.as_str() || title.starts_with(&format!("{prefix}/")))
        .max_by_key(|prefix| prefix.len());
    match prefix {
        Some(prefix) => (
            icons
                .and_then(|icons| icons[prefix].as_str())
                .unwrap_or(fallback)
                .to_owned(),
            true,
        ),
        None => (
            settings["icon"]
                .as_str()
                .filter(|icon| !icon.is_empty())
                .unwrap_or(fallback)
                .to_owned(),
            false,
        ),
    }
}

/// A title's parts: its collection (or `fallback` when it has one part),
/// the group the middle parts make, and its label.
fn placed(title: &str, id: &str, fallback: &str) -> (String, String, String) {
    let mut parts: Vec<&str> = title.split('/').filter(|part| !part.is_empty()).collect();
    let collection = if parts.len() > 1 {
        parts.remove(0).to_owned()
    } else {
        fallback.to_owned()
    };
    let label = parts.pop().map_or_else(|| id.to_owned(), str::to_owned);
    (collection, parts.join(" / "), label)
}

/// Adds `item` to the named collection, in its group when it has one.
fn place(
    collections: &mut Vec<Value>,
    name: &str,
    group: &str,
    item: Value,
    new: impl FnOnce() -> Value,
) {
    let at = if let Some(at) = collections
        .iter()
        .position(|candidate| candidate["name"] == name)
    {
        at
    } else {
        collections.push(new());
        collections.len() - 1
    };
    let items = collections[at]["items"]
        .as_array_mut()
        .expect("collections the catalog makes have items");
    if group.is_empty() {
        items.push(item);
        return;
    }
    let found = items
        .iter()
        .position(|candidate| candidate["group"] == group);
    let at = found.unwrap_or_else(|| {
        items.push(json!({ "group": group, "items": [] }));
        items.len() - 1
    });
    items[at]["items"]
        .as_array_mut()
        .expect("groups the catalog makes have items")
        .push(item);
}

fn joined(root: &Path, relative: &str) -> Value {
    Value::String(root.join(relative).to_string_lossy().into_owned())
}

/// Merges `fields` over a copy of `pages[key]`, as `Object.assign` does.
fn assign(pages: &mut Map<String, Value>, key: &str, fields: Map<String, Value>) {
    let mut page = pages
        .get(key)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    page.extend(fields);
    pages.insert(key.to_owned(), Value::Object(page));
}

fn docs_pages(view: &Value, index: &IndexAnswer, root: &Path, into: &mut Assembly) {
    let collections = view["collections"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let authored: HashSet<&str> = page_items(collections)
        .into_iter()
        .filter_map(|item| item["markdown"].as_str().or_else(|| item["src"].as_str()))
        .collect();
    for page in &index.docs {
        let src = page["src"].as_str().unwrap_or_default();
        if authored.contains(src) {
            continue;
        }
        let (collection, group, label) = placed(
            page["title"].as_str().unwrap_or_default(),
            page["id"].as_str().unwrap_or_default(),
            "Docs",
        );
        let lenses = page["lenses"].as_array().cloned().unwrap_or_default();
        into.discovered_docs.push(json!({
            "src": src, "page": src, "label": label, "lens": page.get("lens").cloned().unwrap_or(Value::Null), "lenses": lenses,
        }));
        let mut item = json!({
            "src": src, "label": label, "markdown": src,
            "icon": page["icon"].as_str().filter(|icon| !icon.is_empty()).unwrap_or("book-open"),
            "docsLenses": lenses.iter().map(|lens| json!({ "key": lens["key"], "label": lens["label"] })).collect::<Vec<_>>(),
        });
        if page["lens"].as_str().is_some_and(|lens| !lens.is_empty()) {
            item["lens"] = page["lens"].clone();
        }
        if page["states"]
            .as_array()
            .is_some_and(|states| states.len() > 1)
        {
            item["states"] = page["states"].clone();
        }
        if !lenses.is_empty() {
            let mapped: Map<String, Value> = lenses
                .iter()
                .map(|lens| {
                    (
                        lens["key"].as_str().unwrap_or_default().to_owned(),
                        json!({ "examples": lens["examples"] }),
                    )
                })
                .collect();
            item["implementations"] = Value::Object(mapped);
        }
        place(
            &mut into.collections,
            &collection,
            &group,
            item,
            || json!({ "name": collection, "icon": "book-open", "items": [] }),
        );
        let mut fields = Map::new();
        fields.insert("label".into(), json!(label));
        fields.insert("design".into(), joined(root, src));
        fields.insert("docs".into(), page.clone());
        assign(&mut into.pages, src, fields);
    }
}

fn previews(view: &Value, index: &IndexAnswer, root: &Path, into: &mut Assembly) {
    let settings = if view["previews"].is_object() {
        view["previews"].clone()
    } else {
        json!({})
    };
    let space_sizes = view["sizes"].as_array().map_or(&[][..], Vec::as_slice);
    for preview in &index.previews {
        let title = preview["title"].as_str().unwrap_or_default();
        let file = preview["file"].as_str().unwrap_or_default();
        let (collection, group, label) = placed(
            title,
            preview["id"].as_str().unwrap_or_default(),
            "Previews",
        );
        let icon = preview["icon"]
            .as_str()
            .filter(|icon| !icon.is_empty())
            .map_or_else(
                || title_icon(&settings, title, "component").0,
                str::to_owned,
            );
        let mut item = json!({ "src": file, "label": label, "states": preview["states"], "workbench": true, "icon": icon });
        if settings["lensLabel"]
            .as_str()
            .is_some_and(|label| !label.is_empty())
        {
            item["lensLabel"] = settings["lensLabel"].clone();
        }
        let sizes = page_sizes(preview.get("sizes"), space_sizes, file, &mut into.problems);
        if let Some(sizes) = &sizes {
            item["sizes"] = sizes["sizes"].clone();
        }
        place(&mut into.collections, &collection, &group, item, || {
            let (icon, mapped) = title_icon(&settings, &collection, "component");
            let priority = if mapped {
                6
            } else if settings["icon"]
                .as_str()
                .is_some_and(|icon| !icon.is_empty())
            {
                4
            } else {
                1
            };
            json!({ "name": collection, "icon": icon, "iconPriority": priority, "items": [] })
        });
        let previous = into.pages.get(file).cloned().unwrap_or(Value::Null);
        let mut fields = Map::new();
        // A page that lists the preview keeps its own sizes; without any, the definition's apply.
        if let Some(sizes) = &sizes
            && previous["sizes"].is_null()
        {
            fields.insert("sizes".into(), sizes["sizes"].clone());
        }
        let own_label = previous["label"].as_str().filter(|label| !label.is_empty());
        fields.insert("label".into(), json!(own_label.unwrap_or(&label)));
        fields.insert("design".into(), joined(root, file));
        fields.insert("preview".into(), preview.clone());
        let mut code = previous["code"].as_array().cloned().unwrap_or_default();
        let source = preview["source"].as_str().unwrap_or_default();
        code.push(json!({ "implementation": "workbench", "path": joined(root, source), "relative": source, "exists": true }));
        fields.insert("code".into(), Value::Array(code));
        assign(&mut into.pages, file, fields);
    }
}

fn links(index: &IndexAnswer, problems: &mut Vec<String>) {
    for preview in &index.previews {
        let file = preview["file"].as_str().unwrap_or_default();
        for (href, to) in preview["links"].as_object().into_iter().flatten() {
            let target_id = to["preview"].as_str().unwrap_or_default();
            let target = index
                .previews
                .iter()
                .find(|candidate| candidate["id"] == target_id);
            match target {
                None => problems.push(format!(
                    "{file}: link {href} names unknown Workbench preview “{target_id}”."
                )),
                Some(target) => {
                    if let Some(state) = to["state"].as_str().filter(|state| !state.is_empty())
                        && !target["states"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|candidate| candidate["id"] == state)
                    {
                        problems.push(format!(
                            "{file}: link {href} names unknown state “{state}” of {target_id}."
                        ));
                    }
                }
            }
        }
    }
}

fn workbench_lenses(view: &Value, index: &IndexAnswer, root: &Path, into: &mut Assembly) {
    let collections = view["collections"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let keys: Vec<String> = into
        .implementations
        .iter()
        .filter(|(_, implementation)| implementation["kind"] == "workbench")
        .map(|(key, _)| key.clone())
        .collect();
    for key in keys {
        let implementation = &mut into.implementations[&key];
        implementation["base"] = json!("");
        if implementation["root"].as_str() != Some(root.to_string_lossy().as_ref()) {
            into.problems.push("Workbench implementations use this project root; use previews.config to configure adapters.".into());
        }
        for item in page_items(collections) {
            let Some(reference) = item["implementations"].get(&key) else {
                continue;
            };
            let wanted = reference["preview"].as_str().unwrap_or_default();
            let Some(preview) = index
                .previews
                .iter()
                .find(|candidate| candidate["id"] == wanted)
            else {
                into.problems.push(format!(
                    "{}: unknown Workbench preview “{wanted}”.",
                    item["label"].as_str().unwrap_or_default()
                ));
                continue;
            };
            let file = preview["file"].as_str().unwrap_or_default();
            let mut mapped = reference.clone();
            mapped["path"] = json!(format!("/{file}"));
            let states: Map<String, Value> = preview["states"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|state| state["id"].as_str())
                .map(|id| {
                    (
                        id.to_owned(),
                        json!(format!("/{file}?state={}", encode_component(id))),
                    )
                })
                .collect();
            mapped["states"] = Value::Object(states);
            let src = item["src"].as_str().unwrap_or_default();
            for collection in &mut into.view_collections {
                set_reference(collection, src, &key, &mapped);
            }
            let source = preview["source"].as_str().unwrap_or_default();
            if let Some(code) = into
                .pages
                .get_mut(src)
                .and_then(|page| page["code"].as_array_mut())
            {
                code.push(json!({ "implementation": key, "path": joined(root, source), "relative": source, "exists": true }));
            }
        }
    }
}

fn set_reference(entry: &mut Value, src: &str, key: &str, mapped: &Value) {
    if let Some(items) = entry.get_mut("items").and_then(Value::as_array_mut) {
        for item in items {
            set_reference(item, src, key, mapped);
        }
    }
    if entry["src"] == src
        && let Some(references) = entry
            .get_mut("implementations")
            .and_then(Value::as_object_mut)
        && references.contains_key(key)
    {
        references.insert(key.to_owned(), mapped.clone());
    }
}

/// `encodeURIComponent`.
pub(crate) fn encode_component(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _written = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Adds the index's docs pages and previews to `into`, which starts as the
/// configuration's own collections, pages and implementations.
pub(crate) fn import(view: &Value, index: &IndexAnswer, root: &Path, into: &mut Assembly) {
    docs_pages(view, index, root, into);
    previews(view, index, root, into);
    links(index, &mut into.problems);
    workbench_lenses(view, index, root, into);
}
