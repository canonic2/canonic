//! What Custom size… and Edit sizes… write, worked out on the committed
//! file's parsed values as `contracts/src/sizes/edit.ts` does: the space's
//! `sizes` map, and its `collections` when pages change too. Writing them is
//! the parent module's job; this decides what to write and refuses what
//! can't be written. Pure.
use super::super::js::{self, label_of};
use super::super::sizes::{is_size_key, page_sizes, space_sizes};
use super::truthy;
use serde_json::{Map, Value};

/// What a size dialog writes: the space's sizes and collections, each when it changed.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct SizeEdit {
    pub(crate) sizes: Option<Map<String, Value>>,
    pub(crate) collections: Option<Value>,
    /// The key a new size was given.
    pub(crate) key: Option<String>,
}

const DEFAULT_KEYS: [&str; 4] = ["fit", "laptop", "mobile", "resizable"];

/// The committed sizes, or the defaults spelled out when the space declares none.
fn committed(raw: Option<&Value>) -> Map<String, Value> {
    if let Some(Value::Object(map)) = raw {
        return map.clone();
    }
    DEFAULT_KEYS
        .iter()
        .map(|key| ((*key).to_owned(), Value::Bool(true)))
        .collect()
}

fn collect_pages<'a>(items: &'a mut Value, out: &mut Vec<&'a mut Map<String, Value>>) {
    let Some(items) = items.as_array_mut() else {
        return;
    };
    for item in items {
        let Some(entry) = item.as_object_mut() else {
            continue;
        };
        if entry.get("group").is_some_and(Value::is_string) {
            if let Some(inner) = entry.get_mut("items") {
                collect_pages(inner, out);
            }
        } else {
            out.push(entry);
        }
    }
}

/// Every page entry in a collections list, groups included, in order.
fn page_entries(collections: &mut Value) -> Vec<&mut Map<String, Value>> {
    let mut out = Vec::new();
    if let Some(list) = collections.as_array_mut() {
        for collection in list.iter_mut().filter_map(Value::as_object_mut) {
            if let Some(items) = collection.get_mut("items") {
                collect_pages(items, &mut out);
            }
        }
    }
    out
}

fn list_of(value: Option<&Value>) -> Option<Vec<Value>> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text.is_empty() => None,
        Some(Value::Array(items)) => Some(items.clone()),
        Some(other) => Some(vec![other.clone()]),
    }
}

/// Keys of the sizes pages define for themselves, so a new key avoids them.
fn own_keys(collections: &mut Value) -> Vec<String> {
    let mut keys = Vec::new();
    for page in page_entries(collections) {
        for entry in list_of(page.get("sizes")).unwrap_or_default() {
            if let Value::Object(map) = entry {
                keys.extend(map.keys().cloned());
            }
        }
    }
    keys
}

/// A key for a size named `name` that none of `taken` uses.
pub(super) fn key_for_name(name: &str, taken: &[String]) -> String {
    let lower = name.to_lowercase();
    let decomposed = icu_normalizer::DecomposingNormalizerBorrowed::new_nfkd().normalize(&lower);
    let mut key = String::new();
    let mut gap = false;
    for c in decomposed
        .chars()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
    {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if gap && !key.is_empty() {
                key.push('-');
            }
            gap = false;
            key.push(c);
        } else {
            gap = true;
        }
    }
    if !is_size_key(&key) {
        key = if key.is_empty() {
            "size".to_owned()
        } else {
            format!("size-{key}")
        };
    }
    let mut candidate = key.clone();
    let mut n = 2;
    while taken.contains(&candidate) {
        candidate = format!("{key}-{n}");
        n += 1;
    }
    candidate
}

/// The first problem the reader finds with a size, without where it was.
fn check(key: &str, value: &Value, page: bool) -> Result<(), String> {
    let mut problems = Vec::new();
    let mut entry = Map::new();
    entry.insert(key.to_owned(), value.clone());
    if page {
        page_sizes(
            Some(&Value::Array(vec![Value::Object(entry)])),
            &[],
            "Page",
            &mut problems,
        );
    } else {
        space_sizes(Some(&Value::Object(entry)), &mut problems, &[]);
    }
    match problems.first() {
        None => Ok(()),
        Some(problem) => Err(match problem.find(':') {
            Some(at) if problem[at + 1..].starts_with(' ') => problem[at + 2..].to_owned(),
            _ => problem.clone(),
        }),
    }
}

fn fields(input: &Map<String, Value>, name: &str, key: &str, page: bool) -> Map<String, Value> {
    let mut out = Map::new();
    let name = js::trim(name);
    if !name.is_empty() && name != label_of(key) {
        out.insert("label".to_owned(), Value::String(name.to_owned()));
    }
    for length in ["width", "height"] {
        if let Some(value) = input.get(length) {
            out.insert(length.to_owned(), value.clone());
        }
    }
    if let Some(icon) = input
        .get("icon")
        .and_then(Value::as_str)
        .map(js::trim)
        .filter(|icon| !icon.is_empty())
    {
        out.insert("icon".to_owned(), Value::String(icon.to_owned()));
    }
    if truthy(input.get("button")) && !page {
        out.insert("button".to_owned(), Value::Bool(true));
    }
    out
}

/// Custom size…: a new size `{ name, width, height, icon?, button?, page?,
/// pageOnly? }`, for the space or for one page.
pub(crate) fn add_size(
    raw_sizes: Option<&Value>,
    raw_collections: &Value,
    input: &Value,
) -> Result<SizeEdit, String> {
    let empty = Map::new();
    let input = input.as_object().unwrap_or(&empty);
    let name = input.get("name").and_then(Value::as_str).unwrap_or("");
    if js::trim(name).is_empty() {
        return Err("Give the size a name.".to_owned());
    }
    let mut sizes = committed(raw_sizes);
    let mut collections = if raw_collections.is_array() {
        raw_collections.clone()
    } else {
        Value::Array(Vec::new())
    };
    let mut taken: Vec<String> = sizes.keys().cloned().collect();
    taken.extend(own_keys(&mut collections));
    let key = key_for_name(name, &taken);
    let page_only = truthy(input.get("pageOnly"));
    let value = Value::Object(fields(input, name, &key, page_only));
    check(&key, &value, page_only)?;
    let wanted = input.get("page").filter(|page| truthy(Some(page)));
    let mut entries = page_entries(&mut collections);
    let page = wanted.and_then(|wanted| {
        entries
            .iter_mut()
            .find(|entry| entry.get("src") == Some(wanted))
    });
    if page_only {
        let Some(page) = page else {
            return Err(
                "Only a page listed in workbench.yaml can have sizes of its own.".to_owned(),
            );
        };
        let mut listed = list_of(page.get("sizes")).unwrap_or_default();
        let mut own = Map::new();
        own.insert(key.clone(), value);
        listed.push(Value::Object(own));
        page.insert("sizes".to_owned(), Value::Array(listed));
        drop(entries);
        return Ok(SizeEdit {
            sizes: None,
            collections: Some(collections),
            key: Some(key),
        });
    }
    sizes.insert(key.clone(), value);
    if let Some(page) = page
        && let Some(mut listed) = list_of(page.get("sizes"))
    {
        listed.push(Value::String(key.clone()));
        page.insert("sizes".to_owned(), Value::Array(listed));
        drop(entries);
        return Ok(SizeEdit {
            sizes: Some(sizes),
            collections: Some(collections),
            key: Some(key),
        });
    }
    Ok(SizeEdit {
        sizes: Some(sizes),
        collections: None,
        key: Some(key),
    })
}

/// Edit sizes…: the space's sizes as the dialog left them, `[{ key, value }]`
/// in order. A size left out is removed, from the space and from every page
/// that lists it. Sizes `local` (workbench.local.yaml) defines or changes
/// can't be changed here: a committed one keeps its committed value, and one
/// only the local file defines isn't written.
pub(crate) fn update_sizes(
    raw_sizes: Option<&Value>,
    raw_collections: &Value,
    rows: &Value,
    local: &[String],
) -> Result<SizeEdit, String> {
    let before = committed(raw_sizes);
    let rows: Vec<(String, Value)> = rows
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    (
                        row.get("key")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        row.get("value").cloned().unwrap_or(Value::Null),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if !rows
        .iter()
        .any(|(key, _)| !local.contains(key) || before.contains_key(key))
    {
        return Err("A space keeps at least one size.".to_owned());
    }
    let mut sizes = Map::new();
    for (key, value) in &rows {
        if sizes.contains_key(key) {
            return Err(format!("Size “{key}” is listed twice."));
        }
        let was = before.get(key);
        if local.contains(key) {
            if let Some(was) = was {
                sizes.insert(key.clone(), was.clone());
            }
            continue;
        }
        if was.is_none() {
            return Err(format!(
                "There is no size “{key}” to change; add it with Custom size…."
            ));
        }
        check(key, value, false)?;
        sizes.insert(key.clone(), value.clone());
    }
    let removed: Vec<&String> = before
        .keys()
        .filter(|key| !sizes.contains_key(*key))
        .collect();
    if let Some(blocked) = removed.iter().find(|key| local.contains(key)) {
        return Err(format!(
            "Size “{blocked}” comes from workbench.local.yaml; remove it there."
        ));
    }
    if removed.is_empty() {
        return Ok(SizeEdit {
            sizes: Some(sizes),
            ..SizeEdit::default()
        });
    }
    let mut collections = if raw_collections.is_array() {
        raw_collections.clone()
    } else {
        Value::Array(Vec::new())
    };
    let mut changed = false;
    for page in page_entries(&mut collections) {
        let Some(listed) = list_of(page.get("sizes")) else {
            continue;
        };
        let kept: Vec<Value> = listed
            .iter()
            .filter(|entry| {
                !entry
                    .as_str()
                    .is_some_and(|key| removed.iter().any(|gone| *gone == js::trim(key)))
            })
            .cloned()
            .collect();
        if kept.len() == listed.len() {
            continue;
        }
        changed = true;
        if kept.is_empty() {
            page.remove("sizes");
        } else {
            page.insert("sizes".to_owned(), Value::Array(kept));
        }
    }
    Ok(SizeEdit {
        sizes: Some(sizes),
        collections: changed.then_some(collections),
        key: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn collections() -> Value {
        json!([{
            "name": "Interface",
            "items": [
                { "label": "Sidebar", "src": "design/sidebar.html", "sizes": ["sidebar", "resizable"] },
                { "group": "Menus", "items": [
                    { "label": "Account menu", "src": "design/account-menu.html", "sizes": ["sidebar", { "popover": { "width": 280, "height": 360 } }] },
                ] },
                { "label": "Dashboard", "src": "pages/dashboard.html" },
            ],
        }])
    }

    fn keys(map: &Map<String, Value>) -> Vec<&str> {
        map.keys().map(String::as_str).collect()
    }

    #[test]
    fn a_size_added_to_a_space_without_sizes_keeps_the_defaults_ahead_of_it() {
        let edit = add_size(
            None,
            &collections(),
            &json!({ "name": "Tablet", "width": 1024, "height": 1366 }),
        )
        .expect("added");
        assert_eq!(edit.key.as_deref(), Some("tablet"));
        assert_eq!(
            Value::Object(edit.sizes.expect("sizes")),
            json!({ "fit": true, "laptop": true, "mobile": true, "resizable": true, "tablet": { "width": 1024, "height": 1366 } })
        );
        assert_eq!(
            edit.collections, None,
            "no page listed sizes, so none changes"
        );
    }

    #[test]
    fn a_size_added_from_a_page_joins_its_list_and_its_name_becomes_a_label() {
        let edit = add_size(
            Some(&json!({ "fit": true, "sidebar": { "width": 340, "height": "fill" } })),
            &collections(),
            &json!({ "name": "Wide panel!", "width": "fill", "height": 200, "icon": "panel-top", "button": true, "page": "design/sidebar.html" }),
        )
        .expect("added");
        assert_eq!(edit.key.as_deref(), Some("wide-panel"));
        assert_eq!(
            edit.sizes.expect("sizes")["wide-panel"],
            json!({ "label": "Wide panel!", "width": "fill", "height": 200, "icon": "panel-top", "button": true })
        );
        assert_eq!(
            edit.collections.expect("collections")[0]["items"][0]["sizes"],
            json!(["sidebar", "resizable", "wide-panel"])
        );
    }

    #[test]
    fn a_size_only_for_one_page_is_written_to_that_page_with_an_unused_key() {
        let edit = add_size(
            Some(&json!({ "fit": true, "popover": { "width": 1, "height": 1 } })),
            &collections(),
            &json!({ "name": "Popover", "width": 300, "height": 200, "button": true, "page": "design/account-menu.html", "pageOnly": true }),
        )
        .expect("added");
        assert_eq!(edit.key.as_deref(), Some("popover-2"));
        assert_eq!(edit.sizes, None, "the space’s sizes are not written");
        assert_eq!(
            edit.collections.expect("collections")[0]["items"][1]["items"][0]["sizes"],
            json!(["sidebar", { "popover": { "width": 280, "height": 360 } }, { "popover-2": { "label": "Popover", "width": 300, "height": 200 } }])
        );
    }

    #[test]
    fn adding_refuses_what_cannot_be_written() {
        let refused = |sizes: Option<&Value>, collections: &Value, input: Value| {
            add_size(sizes, collections, &input).expect_err("refused")
        };
        assert!(
            refused(
                None,
                &json!([]),
                json!({ "name": " ", "width": 10, "height": 10 })
            )
            .contains("name")
        );
        assert!(
            refused(
                None,
                &json!([]),
                json!({ "name": "Wide", "width": "fill", "height": "fill" })
            )
            .contains("both be fill")
        );
        assert!(
            refused(
                None,
                &json!([]),
                json!({ "name": "Big", "width": 9000, "height": 10 })
            )
            .contains("1 to 8192")
        );
        assert!(
            refused(None, &collections(), json!({ "name": "Mine", "width": 10, "height": 10, "page": "pages/missing.html", "pageOnly": true }))
                .contains("listed in workbench.yaml")
        );
    }

    #[test]
    fn keys_come_from_names_without_accents_and_avoid_taken_keys() {
        assert_eq!(key_for_name("Écran Large", &[]), "ecran-large");
        assert_eq!(key_for_name("1024", &[]), "size-1024");
        assert_eq!(key_for_name("!!!", &[]), "size");
        assert_eq!(
            key_for_name("Tablet", &["tablet".to_owned(), "tablet-2".to_owned()]),
            "tablet-3"
        );
    }

    #[test]
    fn updating_writes_the_dialog_order_and_removes_a_dropped_size_from_every_page() {
        let edit = update_sizes(
            Some(&json!({ "fit": true, "sidebar": { "width": 340, "height": "fill" }, "resizable": true })),
            &collections(),
            &json!([{ "key": "resizable", "value": { "label": "Free" } }, { "key": "fit", "value": true }]),
            &[],
        )
        .expect("updated");
        assert_eq!(
            keys(edit.sizes.as_ref().expect("sizes")),
            ["resizable", "fit"]
        );
        let collections = edit.collections.expect("collections");
        let items = &collections[0]["items"];
        assert_eq!(items[0]["sizes"], json!(["resizable"]));
        assert_eq!(
            items[1]["items"][0]["sizes"],
            json!([{ "popover": { "width": 280, "height": 360 } }])
        );
        assert!(items[2].get("sizes").is_none());
    }

    #[test]
    fn a_page_left_with_no_sizes_loses_its_list() {
        let edit = update_sizes(
            Some(&json!({ "fit": true, "sidebar": { "width": 340, "height": "fill" } })),
            &json!([{ "name": "A", "items": [{ "label": "S", "src": "s.html", "sizes": ["sidebar"] }] }]),
            &json!([{ "key": "fit", "value": true }]),
            &[],
        )
        .expect("updated");
        assert!(
            edit.collections.expect("collections")[0]["items"][0]
                .get("sizes")
                .is_none()
        );
    }

    #[test]
    fn updating_refuses_bad_rows_and_changes_to_local_sizes() {
        let sizes = json!({ "fit": true, "sidebar": { "width": 340, "height": "fill" } });
        let refused = |rows: Value, local: &[&str]| {
            let local: Vec<String> = local.iter().map(|key| (*key).to_owned()).collect();
            update_sizes(Some(&sizes), &json!([]), &rows, &local).expect_err("refused")
        };
        assert!(refused(json!([]), &[]).contains("at least one size"));
        assert!(
            refused(
                json!([{ "key": "tablet", "value": { "width": 1, "height": 1 } }]),
                &[]
            )
            .contains("no size “tablet”")
        );
        assert!(
            refused(
                json!([{ "key": "fit", "value": true }, { "key": "fit", "value": true }]),
                &[]
            )
            .contains("twice")
        );
        assert!(
            refused(
                json!([{ "key": "sidebar", "value": { "width": "fill", "height": "fill" } }]),
                &[]
            )
            .contains("both be fill")
        );
        assert!(
            refused(json!([{ "key": "fit", "value": true }]), &["sidebar"])
                .contains("workbench.local.yaml; remove it there")
        );
    }

    #[test]
    fn a_local_size_keeps_its_committed_value_and_one_only_local_is_not_written() {
        let edit = update_sizes(
            Some(&json!({ "fit": true, "sidebar": { "width": 340, "height": "fill" } })),
            &json!([]),
            &json!([
                { "key": "sidebar", "value": { "width": 360, "height": "fill", "label": "Local" } },
                { "key": "phone", "value": { "width": 1, "height": 1 } },
                { "key": "fit", "value": true },
            ]),
            &["sidebar".to_owned(), "phone".to_owned()],
        )
        .expect("updated");
        assert_eq!(
            Value::Object(edit.sizes.expect("sizes")),
            json!({ "sidebar": { "width": 340, "height": "fill" }, "fit": true })
        );
    }
}
