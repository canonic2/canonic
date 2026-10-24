//! Literal-definition domain facade; compatibility, validation and presentation are private roles.
mod compatibility;
mod projection;
mod validation;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) struct Projection {
    pub(crate) entry: Result<Value, String>,
    pub(crate) sources: Vec<PathBuf>,
}

/// Return `None` when JavaScript must decide; retain watched sources on validation errors.
/// A validation error is the problem alone: the page index names the file.
pub(crate) fn project(
    raw: &Value,
    file: &Path,
    root: &Path,
    source_exists: impl FnMut(&Path) -> bool,
) -> Option<Projection> {
    if !compatibility::supported(raw, file, root) {
        return None;
    }
    let mut sources = vec![];
    let entry = validation::check(raw, file, root, source_exists, &mut sources)
        .map(|object| projection::list(object, file, root));
    Some(Projection { entry, sources })
}

/// A literal definition's validated value, with the default scenario filled in
/// as the compiler's validation does. `None` when JavaScript must decide. An
/// error names the definition by its path from the project root, as the Node
/// evaluator's validation does.
pub(crate) fn validated(
    raw: &Value,
    file: &Path,
    root: &Path,
    source_exists: impl FnMut(&Path) -> bool,
) -> Option<Result<Value, String>> {
    if !compatibility::supported(raw, file, root) {
        return None;
    }
    let mut sources = vec![];
    Some(
        validation::check(raw, file, root, source_exists, &mut sources)
            .map(|object| {
                let mut object = object.clone();
                let empty = object
                    .get("scenarios")
                    .and_then(Value::as_object)
                    .is_none_or(serde_json::Map::is_empty);
                if empty {
                    object.insert("scenarios".into(), serde_json::json!({ "default": {} }));
                }
                Value::Object(object)
            })
            .map_err(|message| format!("{}: {message}", crate::paths::relative(root, file))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn page_artboards_use_the_shared_definition_evaluator() {
        let source = json!({"entry":"page.html"});
        for raw in [
            json!({"id":"page","adapter":"html","source":source,"scenarios":{"default":{"artboards":{"info":{"width":560,"height":79}}}}}),
            json!({"id":"page","adapter":"html","source":source,"scenarios":{"empty":{"iterations":{"illustrated":{}}}}}),
            json!({"id":"page","adapter":"html","source":source,"artboards":{"info":{"width":560,"height":79}}}),
            json!({"id":"page","adapter":"html","source":source,"iterations":{"bold":{}}}),
        ] {
            assert!(
                project(
                    &raw,
                    Path::new("/acme/page.workbench.ts"),
                    Path::new("/acme"),
                    |_| true
                )
                .is_none()
            );
        }
    }
    #[test]
    fn index_errors_leave_the_file_to_the_index_and_validated_names_it_once() {
        let root = Path::new("/acme");
        let file = root.join("pages/page.workbench.ts");
        for (raw, message) in [
            (json!(null), "default export"),
            (json!({}), "id must"),
            (json!({"id":"page"}), "adapter must"),
            (
                json!({"id":"page","adapter":"html","icon":"BAD"}),
                "icon must",
            ),
        ] {
            assert!(
                project(&raw, &file, root, |_| true)
                    .unwrap()
                    .entry
                    .unwrap_err()
                    .starts_with(message)
            );
            let named = validated(&raw, &file, root, |_| true).unwrap().unwrap_err();
            assert!(
                named.starts_with(&format!("pages/page.workbench.ts: {message}")),
                "{named}"
            );
        }
    }

    #[test]
    fn scenario_ids_follow_the_artboard_and_iteration_id_rule() {
        let root = Path::new("/acme");
        let file = root.join("page.workbench.ts");
        let source = json!({"entry":"page.html"});
        for name in ["2-up", "a--b", "-ready", "ready-", "Ready"] {
            let raw = json!({"id":"page","adapter":"html","source":source,"scenarios":{name:{}}});
            assert_eq!(
                project(&raw, &file, root, |_| true)
                    .unwrap()
                    .entry
                    .unwrap_err(),
                "scenarios need kebab-case IDs and object definitions.",
                "{name}"
            );
        }
        let link = json!({"id":"page","adapter":"html","source":source,"links":{"/next":{"scenario":"2-up"}}});
        assert!(
            project(&link, &file, root, |_| true)
                .unwrap()
                .entry
                .unwrap_err()
                .starts_with("link /next must")
        );
    }

    #[test]
    fn a_missing_source_stays_watched_and_precedes_later_field_errors() {
        let root = Path::new("/acme");
        let file = root.join("page.workbench.ts");
        let raw = json!({
            "id": "page", "adapter": "html", "source": {"entry": "missing.html"},
            "sizes": [], "controls": {"tone": {"type": "invalid"}}
        });
        let missing = project(&raw, &file, root, |_| false).unwrap();
        assert_eq!(missing.sources, vec![root.join("missing.html")]);
        assert_eq!(
            missing.entry.unwrap_err(),
            "source must exist inside the project: missing.html"
        );
        let available = project(&raw, &file, root, |_| true).unwrap();
        assert!(
            available
                .entry
                .unwrap_err()
                .ends_with("sizes must be a list of size keys, such as laptop or mobile.")
        );
    }

    #[test]
    fn projection_and_validation_match_list_fields() {
        let root = Path::new("/acme");
        let file = root.join("page.workbench.ts");
        let raw = json!({"id":"page","adapter":"html","source":{"entry":"page.html"},"scenarios":{"with-filters":{},"step2":{},"empty":{}},"links":{"/next":{"scenario":"empty"}},"sizes":["mobile"]});
        let entry = project(&raw, &file, root, |_| true).unwrap().entry.unwrap();
        // Default labels are the ID in sentence case.
        assert_eq!(entry["scenarios"][0]["label"], "With filters");
        assert_eq!(entry["scenarios"][1]["label"], "Step2");
        assert_eq!(entry["links"]["/next"]["preview"], "page");
        assert_eq!(entry["title"], "page");
        insta::assert_json_snapshot!("page_list_entry", entry);
        for (key, value, message) in [
            ("sizes", json!([]), "sizes must"),
            (
                "controls",
                json!({"tone":{"type":"select","options":[]}}),
                "select controls",
            ),
            ("links", json!({"/bad":null}), "link /bad must"),
        ] {
            let mut invalid = raw.clone();
            invalid[key] = value;
            assert!(
                project(&invalid, &file, root, |_| true)
                    .unwrap()
                    .entry
                    .unwrap_err()
                    .contains(message)
            );
        }
        assert!(
            project(&raw, &file, root, |_| false)
                .unwrap()
                .entry
                .unwrap_err()
                .contains("source must exist")
        );
    }
}
