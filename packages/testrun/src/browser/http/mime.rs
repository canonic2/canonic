//! Filename inference from the pinned SDK's MIME table; no content sniffing.
use std::{collections::BTreeMap, sync::OnceLock};

pub(super) fn infer(filename: &str) -> &'static str {
    static TYPES: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    let types = TYPES.get_or_init(|| {
        serde_json::from_str(include_str!("mime-types.json")).expect("Pinned MIME table")
    });
    let last = filename.rsplit(['/', '\\']).next().unwrap_or("");
    let extension = last.rsplit('.').next().unwrap_or("");
    let has_path = last.len() < filename.len();
    let has_dot = extension.len().saturating_add(1) < last.len();
    if has_path && !has_dot {
        return "application/octet-stream";
    }
    types
        .get(&extension.to_lowercase())
        .map(String::as_str)
        .unwrap_or("application/octet-stream")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_filename_rules_preserve_primary_types_case_paths_and_unknown_fallback() {
        for (name, expected) in [
            ("Acme.JSON", "application/json"),
            ("Acme.xml", "application/xml"),
            ("Acme.svg", "image/svg+xml"),
            ("Acme.Ktx", "image/ktx"),
            (
                "Acme.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            (".json", "application/json"),
            ("json", "application/json"),
            ("/Acme/json", "application/octet-stream"),
            ("/Acme/.json", "application/octet-stream"),
            ("C:\\Acme\\data.json", "application/json"),
            ("Acme.unknown-extension", "application/octet-stream"),
        ] {
            assert_eq!(infer(name), expected, "{name}");
        }
    }
}
