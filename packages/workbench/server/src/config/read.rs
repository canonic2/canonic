//! Reading a space's workbench.yaml and workbench.local.yaml, and resolving
//! its paths for this machine, as `server/node/config.js` does with `read`
//! and `resolve`. The handwritten page tree keeps its malformed entries out
//! without reporting them; implementation, size, lens and code problems are
//! kept.
use super::{js, manifest, sizes, yaml};
use crate::paths;
use serde_json::{Map, Value, json};
use std::path::Path;

const FILE: &str = "workbench.yaml";
const LOCAL: &str = "workbench.local.yaml";

type Problems = Vec<String>;

/// `text` of config.js: a trimmed string, else empty. Numbers are not text here.
fn text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(|text| js::trim(text).to_owned())
        .unwrap_or_default()
}

fn list(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// A page's states, when there are at least two to choose between.
fn states(raw: Option<&Value>) -> Option<Value> {
    let out: Vec<Value> = list(raw)
        .iter()
        .filter_map(|state| {
            let id = text(state.get("id"));
            let label = text(state.get("label"));
            let valid = !id.is_empty() && !label.is_empty() && manifest::is_key(&id);
            valid.then(|| json!({ "id": id, "label": label }))
        })
        .collect();
    (out.len() > 1).then_some(Value::Array(out))
}

struct Space<'a> {
    implementations: &'a Map<String, Value>,
    sizes: &'a [Value],
}

fn page(raw: &Value, where_: &str, space: &Space<'_>, problems: &mut Problems) -> Option<Value> {
    let label = text(raw.get("label"));
    let src = text(raw.get("src"));
    if label.is_empty()
        || src.is_empty()
        || src.starts_with('/')
        || src.contains("..")
        || manifest::src_problem(&src).is_some()
    {
        return None;
    }
    let at = format!("{where_} › {label}");
    let mut item = Map::new();
    item.insert("label".to_owned(), json!(label));
    item.insert("src".to_owned(), json!(src));
    manifest::page_lens_label(raw.get("lensLabel"), &mut item, &at, problems);
    if !manifest::is_docs(&src)
        && let Some(sized) = sizes::page_sizes(raw.get("sizes"), space.sizes, &at, problems)
    {
        for field in ["sizes", "ownSizes"] {
            if let Some(value) = sized.get(field) {
                item.insert(field.to_owned(), value.clone());
            }
        }
    }
    let icon = text(raw.get("icon"));
    if !icon.is_empty() {
        item.insert("icon".to_owned(), json!(icon));
    }
    if let Some(found) = states(raw.get("states")) {
        item.insert("states".to_owned(), found);
    }
    manifest::page_markdown(raw, &mut item, &at, problems);
    if let Some(lenses) = manifest::page_lenses(
        raw.get("implementations"),
        &item,
        space.implementations,
        &at,
        problems,
    ) {
        item.insert("implementations".to_owned(), lenses);
    }
    manifest::docs_page_entry(raw, &mut item, space.implementations, &at, problems);
    if let Some(code) = manifest::page_code(raw.get("code"), space.implementations, &at, problems) {
        item.insert("code".to_owned(), code);
    }
    Some(Value::Object(item))
}

fn entries(
    raw: Option<&Value>,
    where_: &str,
    space: &Space<'_>,
    problems: &mut Problems,
    in_group: bool,
) -> Vec<Value> {
    list(raw)
        .iter()
        .filter_map(|entry| {
            let group = text(entry.get("group"));
            if group.is_empty() {
                return page(entry, where_, space, problems);
            }
            if in_group {
                return None;
            }
            let items = entries(
                entry.get("items"),
                &format!("{where_} › {group}"),
                space,
                problems,
                true,
            );
            (!items.is_empty()).then(|| json!({ "group": group, "items": items }))
        })
        .collect()
}

fn collections(raw: Option<&Value>, space: &Space<'_>, problems: &mut Problems) -> Vec<Value> {
    list(raw)
        .iter()
        .filter_map(|collection| {
            let name = text(collection.get("name"));
            if name.is_empty() {
                return None;
            }
            let items = entries(collection.get("items"), &name, space, problems, false);
            let icon = text(collection.get("icon"));
            (!items.is_empty() || !icon.is_empty())
                .then(|| json!({ "name": name, "icon": icon, "items": items }))
        })
        .collect()
}

/// The file's text, or `None` when it isn't there.
fn read_file(dir: &Path, name: &str) -> Result<Option<String>, String> {
    match std::fs::read_to_string(dir.join(name)) {
        Ok(body) => Ok(Some(body)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Couldn’t read {name}: {error}")),
    }
}

fn parse_file(body: &str, name: &str) -> Result<Value, String> {
    yaml::parse(body).map_err(|error| format!("{name}, {error}"))
}

struct Found {
    raw: Value,
    local: bool,
    local_raw: Option<Value>,
}

fn read_raw(dir: &Path) -> Result<Option<Found>, String> {
    let Some(main) = read_file(dir, FILE)? else {
        return Ok(None);
    };
    let local = read_file(dir, LOCAL)?;
    let mut raw = parse_file(&main, FILE)?;
    let local_raw = local
        .as_deref()
        .map(|body| parse_file(body, LOCAL))
        .transpose()?;
    if let Some(local_raw) = &local_raw {
        raw = manifest::merge(&raw, local_raw);
    }
    Ok(Some(Found {
        raw,
        local: local.is_some(),
        local_raw,
    }))
}

/// The size keys workbench.local.yaml sets for the space `key`.
fn local_size_keys(local_raw: Option<&Value>, key: Option<&str>) -> Vec<String> {
    let Some(local) = local_raw.filter(|raw| raw.is_object()) else {
        return Vec::new();
    };
    let holder = match key {
        Some(key) => local.get("spaces").and_then(|spaces| spaces.get(key)),
        None => Some(local),
    };
    holder
        .and_then(|holder| holder.get("sizes"))
        .and_then(Value::as_object)
        .map(|sizes| sizes.keys().cloned().collect())
        .unwrap_or_default()
}

/// The config of the space `key` of the workbench.yaml in `dir`, resolved
/// for this machine, or `None` without a workbench.yaml. The answer of
/// `config.resolve(dir, config.read({ dir, key }))` in the Node reader.
pub(crate) fn resolved(dir: &Path, key: Option<&str>) -> Result<Option<Value>, String> {
    let Some(found) = read_raw(dir)? else {
        return Ok(None);
    };
    let mut problems = Vec::new();
    let (raw, picked_key, picked_root) = manifest::select_space(&found.raw, key, &mut problems);
    let root = picked_root
        .as_deref()
        .map_or_else(|| dir.to_path_buf(), |root| paths::resolve(dir, root));
    let implementations = manifest::implementations(raw.get("implementations"), &mut problems);
    if picked_key.is_some()
        && found
            .raw
            .as_object()
            .is_some_and(|raw| raw.contains_key("sizes"))
    {
        problems.push(
            "Sizes: sizes go in each space, under spaces.<key>, when the file lists spaces."
                .to_owned(),
        );
    }
    let local_keys = local_size_keys(found.local_raw.as_ref(), picked_key.as_deref());
    let space_sizes = sizes::space_sizes(raw.get("sizes"), &mut problems, &local_keys);
    let mark = manifest::space_mark(&raw, &mut problems);
    let space = Space {
        implementations: &implementations,
        sizes: &space_sizes,
    };
    let collections = collections(raw.get("collections"), &space, &mut problems);
    let previews = manifest::previews(raw.get("previews"), &mut problems);
    let mut name = text(raw.get("name"));
    if name.is_empty() {
        name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    Ok(Some(resolve(
        &root,
        Read {
            name,
            mark,
            sizes: space_sizes,
            collections,
            implementations,
            previews,
            problems,
            local: found.local,
        },
    )))
}

struct Read {
    name: String,
    mark: Value,
    sizes: Vec<Value>,
    collections: Vec<Value>,
    implementations: Map<String, Value>,
    previews: Value,
    problems: Problems,
    local: bool,
}

fn pages_in(items: &[Value], out: &mut Vec<Value>) {
    for entry in items {
        if entry.get("group").is_some() {
            pages_in(list(entry.get("items")), out);
        } else {
            out.push(entry.clone());
        }
    }
}

fn absolute(path: &Path) -> Value {
    Value::String(path.to_string_lossy().into_owned())
}

/// Every path made absolute for this machine, with the problems only this
/// side can see: a missing icon file, code without a root.
fn resolve(root: &Path, read: Read) -> Value {
    let mut problems = read.problems;
    if let Some(image) = read.mark["image"].as_str()
        && !paths::resolve(root, image).exists()
    {
        problems.push(format!("icon: {image} isn’t in the project."));
    }
    let mut implementations = read.implementations;
    for implementation in implementations.values_mut() {
        let resolved = implementation["root"]
            .as_str()
            .map(|own| absolute(&paths::resolve(root, own)));
        implementation["root"] = resolved.unwrap_or(Value::Null);
    }
    let mut pages = Map::new();
    for collection in &read.collections {
        let mut found = Vec::new();
        pages_in(list(collection.get("items")), &mut found);
        for item in found {
            let label = item["label"].as_str().unwrap_or_default();
            let src = item["src"].as_str().unwrap_or_default();
            let where_ = format!(
                "{} › {label}",
                collection["name"].as_str().unwrap_or_default()
            );
            let mut reported = Vec::new();
            let mut code = Vec::new();
            for entry in list(item.get("code")) {
                let implementation = entry["implementation"].as_str().unwrap_or_default();
                let relative = entry["path"].as_str().unwrap_or_default();
                match implementations
                    .get(implementation)
                    .and_then(|found| found["root"].as_str())
                {
                    None => {
                        if !reported.contains(&implementation) {
                            problems.push(format!(
                                "{where_}: code for “{implementation}” can’t resolve — implementation “{implementation}” has no root."
                            ));
                            reported.push(implementation);
                        }
                        code.push(json!({ "implementation": implementation, "path": null, "relative": relative, "exists": false }));
                    }
                    Some(own_root) => {
                        let file = paths::resolve(Path::new(own_root), relative);
                        let exists = file.exists();
                        code.push(json!({ "implementation": implementation, "path": absolute(&file), "relative": relative, "exists": exists }));
                    }
                }
            }
            let mut resolved = json!({ "label": label, "design": absolute(&paths::resolve(root, src)), "code": code });
            for field in ["sizes", "ownSizes"] {
                if let Some(value) = item.get(field) {
                    resolved[field] = value.clone();
                }
            }
            let windows: Map<String, Value> = item
                .get("implementations")
                .and_then(Value::as_object)
                .map(|lenses| {
                    lenses
                        .iter()
                        .filter_map(|(key, lens)| {
                            lens.get("window")
                                .map(|window| (key.clone(), window.clone()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !windows.is_empty() {
                resolved["windows"] = Value::Object(windows);
            }
            pages.insert(src.to_owned(), resolved);
        }
    }
    json!({
        "name": read.name,
        "mark": read.mark,
        "sizes": read.sizes,
        "collections": read.collections,
        "implementations": implementations,
        "pages": js::ordered(pages),
        "files": { "main": true, "local": read.local },
        "problems": problems,
        "previews": read.previews,
    })
}
