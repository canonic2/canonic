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
        .map(|object| projection::list(object, file, root))
        .map_err(|message| format!("{}: {message}", file.display()));
    Some(Projection { entry, sources })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn validation_errors_keep_the_definition_path() {
        let root = Path::new("/acme");
        let file = root.join("page.workbench.ts");
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
                    .starts_with(&format!("{}: {message}", file.display()))
            );
        }
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
            format!(
                "{}: source must exist inside the project: missing.html",
                file.display()
            )
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
        let raw = json!({"id":"page","adapter":"html","source":{"entry":"page.html"},"states":{"a--b":{},"-ready":{},"empty":{}},"links":{"/next":{"state":"empty"}},"sizes":["mobile"]});
        let entry = project(&raw, &file, root, |_| true).unwrap().entry.unwrap();
        assert_eq!(entry["states"][0]["label"], "A- B");
        assert_eq!(entry["states"][1]["label"], " Ready");
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
