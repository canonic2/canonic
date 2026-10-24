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

/// A new collection an import makes: a `previews.icons` prefix of its name
/// ranks 6, `previews.icon` 4, and `fallback` 1 (see `merge_collection_icon`).
fn imported_collection(settings: &Value, name: &str, fallback: &str) -> Value {
    let (icon, mapped) = title_icon(settings, name, fallback);
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
    json!({ "name": name, "icon": icon, "iconPriority": priority, "items": [] })
}

/// The `previews` settings, which govern discovered previews and docs.
fn preview_settings(view: &Value) -> Value {
    if view["previews"].is_object() {
        view["previews"].clone()
    } else {
        json!({})
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

/// `manifest.mergeCollectionIcon`: the collection icon ladder, highest wins
/// (7 authored, 6 a configured icon for the collection — a `previews.icons`
/// prefix or a catalog's `catalogIcon` — 4 `previews.icon`, 2 an authored
/// default, 1 an import's built-in default), equal imported ranks taking the
/// lexically first icon. A collection without a rank is authored.
fn merge_collection_icon(target: &mut Value, incoming: &Value) {
    let rank = |collection: &Value| collection["iconPriority"].as_u64().unwrap_or(7);
    let (current, next) = (rank(target), rank(incoming));
    let icon = incoming["icon"].as_str().unwrap_or_default();
    let first = next == current && next < 7 && icon < target["icon"].as_str().unwrap_or_default();
    if next > current || first {
        target["icon"] = json!(icon);
        target["iconPriority"] = json!(next);
    }
}

/// Adds `item` to the named collection, in its group when it has one. A
/// collection another import made first takes `new`'s icon when it ranks
/// higher.
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
        merge_collection_icon(&mut collections[at], &new());
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
    let settings = preview_settings(view);
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
        if page["scenarios"]
            .as_array()
            .is_some_and(|scenarios| scenarios.len() > 1)
        {
            item["scenarios"] = page["scenarios"].clone();
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
        // Docs are discovered with previews, so their collections take the
        // same configured icons; page icons keep the docs default.
        place(&mut into.collections, &collection, &group, item, || {
            imported_collection(&settings, &collection, "book-open")
        });
        let mut fields = Map::new();
        fields.insert("label".into(), json!(label));
        fields.insert("design".into(), joined(root, src));
        fields.insert("docs".into(), page.clone());
        assign(&mut into.pages, src, fields);
    }
}

fn previews(view: &Value, index: &IndexAnswer, root: &Path, into: &mut Assembly) {
    let settings = preview_settings(view);
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
        let mut item = json!({ "src": file, "label": label, "scenarios": preview["scenarios"], "workbench": true, "icon": icon });
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
            imported_collection(&settings, &collection, "component")
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
                    if let Some(scenario) = to["scenario"]
                        .as_str()
                        .filter(|scenario| !scenario.is_empty())
                        && !target["scenarios"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|candidate| candidate["id"] == scenario)
                    {
                        problems.push(format!(
                            "{file}: link {href} names unknown scenario “{scenario}” of {target_id}."
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
            let scenarios: Map<String, Value> = preview["scenarios"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|scenario| scenario["id"].as_str())
                .map(|id| {
                    (
                        id.to_owned(),
                        json!(format!("/{file}?scenario={}", encode_component(id))),
                    )
                })
                .collect();
            mapped["scenarios"] = Value::Object(scenarios);
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

fn scenario_ids(scenarios: &Value) -> Vec<String> {
    scenarios
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|scenario| scenario["id"].as_str().map(str::to_owned))
        .collect()
}

/// Checks the pending lens path maps of a page that lists a preview
/// definition against the definition's scenarios, which the page takes
/// (`mergeCollections`), or its own when no definition matches. `where_`
/// names its collection and group, as the config reader does.
fn settle_entry(entry: &mut Value, where_: &str, index: &IndexAnswer, problems: &mut Vec<String>) {
    if let Some(group) = entry["group"].as_str().map(str::to_owned) {
        let at = format!("{where_} › {group}");
        for item in entry
            .get_mut("items")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            settle_entry(item, &at, index, problems);
        }
        return;
    }
    let src = entry["src"].as_str().unwrap_or_default();
    let ids = index
        .previews
        .iter()
        .find(|preview| preview["file"] == src)
        .map_or_else(
            || scenario_ids(&entry["scenarios"]),
            |preview| scenario_ids(&preview["scenarios"]),
        );
    let at = format!("{where_} › {}", entry["label"].as_str().unwrap_or_default());
    crate::config::settle_lenses(entry, &ids, &at, problems);
}

fn settle_lenses(index: &IndexAnswer, into: &mut Assembly) {
    for collection in &mut into.view_collections {
        let name = collection["name"].as_str().unwrap_or_default().to_owned();
        for entry in collection
            .get_mut("items")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            settle_entry(entry, &name, index, &mut into.problems);
        }
    }
}

/// Adds the index's docs pages and previews to `into`, which starts as the
/// configuration's own collections, pages and implementations.
pub(crate) fn import(view: &Value, index: &IndexAnswer, root: &Path, into: &mut Assembly) {
    docs_pages(view, index, root, into);
    previews(view, index, root, into);
    links(index, &mut into.problems);
    workbench_lenses(view, index, root, into);
    settle_lenses(index, into);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(file: &str, title: &str, scenarios: &[&str]) -> Value {
        let scenarios: Vec<Value> = scenarios
            .iter()
            .map(|id| json!({ "id": id, "label": id }))
            .collect();
        json!({ "id": file.trim_end_matches(".workbench.ts"), "file": file, "title": title, "source": file, "scenarios": scenarios })
    }

    #[test]
    fn a_configured_preview_icon_outranks_the_icon_of_a_collection_docs_made_first() {
        let view = json!({ "collections": [], "previews": { "icons": { "Web App": "monitor" } } });
        let index = IndexAnswer {
            docs: vec![json!({ "src": "docs/guide.md", "id": "guide", "title": "Web App/Guide" })],
            previews: vec![
                preview("jobs.workbench.ts", "Web App/Jobs", &["default"]),
                preview("button.workbench.ts", "Kit/Button", &["default"]),
            ],
            ..IndexAnswer::default()
        };
        let mut into = Assembly::default();
        import(&view, &index, Path::new("/acme"), &mut into);
        let names: Vec<_> = into
            .collections
            .iter()
            .map(|collection| {
                (
                    collection["name"].clone(),
                    collection["icon"].clone(),
                    collection["iconPriority"].clone(),
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                (json!("Web App"), json!("monitor"), json!(6)),
                (json!("Kit"), json!("component"), json!(1)),
            ]
        );
    }

    #[test]
    fn docs_collections_take_the_configured_preview_icons() {
        let docs = IndexAnswer {
            docs: vec![
                json!({ "src": "docs/guide.md", "id": "guide", "title": "Web App/Guide" }),
                json!({ "src": "docs/colors.md", "id": "colors", "title": "Kit/Colors", "icon": "palette" }),
                json!({ "src": "docs/intro.md", "id": "intro", "title": "Intro" }),
            ],
            ..IndexAnswer::default()
        };
        let icons = |view: &Value| {
            let mut into = Assembly::default();
            import(view, &docs, Path::new("/acme"), &mut into);
            let collections: Vec<_> = into
                .collections
                .iter()
                .map(|collection| {
                    (
                        collection["name"].clone(),
                        collection["icon"].clone(),
                        collection["iconPriority"].clone(),
                    )
                })
                .collect();
            let pages: Vec<_> = page_items(&into.collections)
                .into_iter()
                .map(|item| item["icon"].clone())
                .collect();
            (collections, pages)
        };
        // A prefix ranks as a configured icon, `previews.icon` below it.
        let (collections, pages) = icons(&json!({
            "collections": [],
            "previews": { "icon": "boxes", "icons": { "Web App": "monitor" } },
        }));
        assert_eq!(
            collections,
            [
                (json!("Web App"), json!("monitor"), json!(6)),
                (json!("Kit"), json!("boxes"), json!(4)),
                (json!("Docs"), json!("boxes"), json!(4)),
            ]
        );
        // Page icons keep the docs default.
        assert_eq!(
            pages,
            [json!("book-open"), json!("palette"), json!("book-open")]
        );
        // Without settings a docs collection ranks as a built-in default.
        let (collections, _) = icons(&json!({ "collections": [] }));
        assert_eq!(
            collections,
            [
                (json!("Web App"), json!("book-open"), json!(1)),
                (json!("Kit"), json!("book-open"), json!(1)),
                (json!("Docs"), json!("book-open"), json!(1)),
            ]
        );
    }

    #[test]
    fn a_page_listing_a_preview_checks_its_lens_paths_against_the_definitions_scenarios() {
        let page = json!({
            "label": "Jobs", "src": "jobs.workbench.ts",
            "implementations": {
                "dev": { "pending": { "default": "/jobs", "empty": "/jobs?empty", "ghost": "/ghost" } },
                "live": { "pending": { "empty": "/x" } },
                "sb": { "title": "Jobs" },
            },
        });
        let other = json!({
            "label": "Sign in", "src": "sign-in.html",
            "scenarios": [{ "id": "default", "label": "Default" }, { "id": "error", "label": "Error" }],
            "implementations": { "dev": { "pending": { "default": "/", "error": "/?error" } } },
        });
        let view = json!({ "collections": [{ "name": "App", "items": [{ "group": "Pages", "items": [page] }, other] }] });
        let index = IndexAnswer {
            previews: vec![preview(
                "jobs.workbench.ts",
                "App/Pages/Jobs",
                &["default", "empty"],
            )],
            ..IndexAnswer::default()
        };
        let mut into = Assembly {
            view_collections: view["collections"].as_array().cloned().unwrap_or_default(),
            ..Assembly::default()
        };
        import(&view, &index, Path::new("/acme"), &mut into);
        let items = &into.view_collections[0]["items"];
        assert_eq!(
            items[0]["items"][0]["implementations"],
            json!({
                "dev": { "path": "/jobs", "scenarios": { "empty": "/jobs?empty" } },
                "sb": { "title": "Jobs" },
            })
        );
        assert_eq!(
            items[1]["implementations"],
            json!({ "dev": { "path": "/", "scenarios": { "error": "/?error" } } })
        );
        assert_eq!(
            into.problems,
            [
                "App › Pages › Jobs: implementation “dev” maps scenario “ghost”, which this page doesn’t declare.",
                "App › Pages › Jobs: implementation “live” needs a path for the default scenario (“default”).",
            ]
        );
    }
}
