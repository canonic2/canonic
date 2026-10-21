//! Docs pages: Markdown with live examples, read in a docs lens.
//!
//! This module owns what a docs page shows: which Markdown files are docs
//! pages ([`docs_pages`]), how a page's Markdown reads ([`markdown`]), the
//! HTML it is served as ([`render`], [`page`]), and the page service the
//! routes call ([`service`]). Listing and building a lens's examples belong to
//! the preview compiler and are passed in. The browser half is
//! `ui/src/modules/docs/page/`, served unchanged.

pub(crate) mod export_source;
pub(crate) mod highlight;
pub(crate) mod markdown;
pub(crate) mod page;
pub(crate) mod portable;
pub(crate) mod render;
pub(crate) mod service;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Where a docs page's script asks for an example's code.
pub(crate) const SOURCE_PATH: &str = "/_workbench/docs/source";
/// Colours live code fences after the readable document is shown.
pub(crate) const HIGHLIGHT_PATH: &str = "/_workbench/docs/highlight";
/// Where an open docs page polls for its revision.
pub(crate) const REVISION_PATH: &str = "/_workbench/docs/revision";
/// Where a deferred docs page asks what to mount.
pub(crate) const BUNDLE_PATH: &str = "/_workbench/docs/bundle";

/// One docs lens of a page: a docs implementation and the example source the
/// page maps to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocsLens {
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) adapter: String,
    #[serde(default)]
    pub(crate) styles: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) environment: Option<String>,
    /// The example folder (ending in `/`) or file, relative to the project root.
    pub(crate) examples: String,
}

/// A docs page: one Markdown file and the lenses that render its examples.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocsPageEntry {
    /// The Markdown file, relative to the project root, with `/` separators.
    /// Docs are served at it.
    pub(crate) src: String,
    /// The page whose docs these are: the Markdown file itself for a Markdown page.
    pub(crate) page: String,
    pub(crate) label: String,
    /// The docs lens a Markdown page opens with.
    #[serde(default)]
    pub(crate) lens: Option<String>,
    #[serde(default)]
    pub(crate) lenses: Vec<DocsLens>,
}

/// An example as the compiler lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ListedExample {
    pub(crate) id: String,
    /// The example's file, relative to the project root.
    pub(crate) file: String,
    /// The export that renders it: `default` for a file of its own.
    pub(crate) export: String,
}

/// A lens's listed examples and the problems listing them found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Listing {
    pub(crate) examples: Vec<ListedExample>,
    pub(crate) problems: Vec<String>,
}

/// A lens's built examples bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocsBundle {
    pub(crate) module: String,
    pub(crate) stylesheet: Option<String>,
    pub(crate) revision: String,
    pub(crate) examples: Vec<ListedExample>,
    pub(crate) problems: Vec<String>,
}

/// What a deferred page's script mounts, answered after the page itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct BundleInfo {
    pub(crate) module: Option<String>,
    pub(crate) stylesheet: Option<String>,
    /// The IDs the lens exports; a placed example that isn't one says it's missing.
    pub(crate) examples: Vec<String>,
    /// Why nothing mounts: the build failed, or examples can't run here.
    pub(crate) error: Option<String>,
    /// The page's revision, which the page then polls against.
    pub(crate) revision: String,
}

/// The compiler request for a lens's examples.
pub(crate) fn lens_request(lens: &DocsLens) -> Value {
    let mut request =
        json!({ "adapter": lens.adapter, "examples": lens.examples, "styles": lens.styles });
    if let Some(environment) = &lens.environment {
        request["environment"] = json!(environment);
    }
    request
}

/// The lens a request shows: the one it names, else the page's own, else the first.
pub(crate) fn choose_lens<'entry>(
    entry: &'entry DocsPageEntry,
    requested: Option<&str>,
) -> Option<&'entry DocsLens> {
    entry
        .lenses
        .iter()
        .find(|lens| Some(lens.key.as_str()) == requested)
        .or_else(|| {
            entry
                .lenses
                .iter()
                .find(|lens| Some(lens.key.as_str()) == entry.lens.as_deref())
        })
        .or_else(|| entry.lenses.first())
}

/// Every page item of a config view's collections, groups flattened.
fn page_items(collections: Option<&Value>) -> Vec<&Value> {
    let mut out = vec![];
    for collection in collections.and_then(Value::as_array).into_iter().flatten() {
        for entry in collection["items"].as_array().into_iter().flatten() {
            if entry.get("group").is_some() {
                out.extend(entry["items"].as_array().into_iter().flatten());
            } else if entry["src"].as_str().is_some_and(|src| !src.is_empty()) {
                out.push(entry);
            }
        }
    }
    out
}

/// The docs a space has: every page the config gives Markdown, with its docs
/// lenses, then the discovered pages whose Markdown the config doesn't
/// already list. One entry per Markdown file; the first page to claim a file
/// has it.
pub(crate) fn docs_pages(config: &Value, discovered: &[DocsPageEntry]) -> Vec<DocsPageEntry> {
    let mut out: Vec<DocsPageEntry> = vec![];
    let implementations = &config["implementations"];
    for item in page_items(config.get("collections")) {
        let Some(markdown) = item["markdown"].as_str().filter(|text| !text.is_empty()) else {
            continue;
        };
        if out.iter().any(|entry| entry.src == markdown) {
            continue;
        }
        let lenses = item["implementations"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(key, mapping)| {
                let implementation = &implementations[key.as_str()];
                let adapter = implementation["adapter"]
                    .as_str()
                    .filter(|text| !text.is_empty())?;
                let examples = mapping["examples"]
                    .as_str()
                    .filter(|text| !text.is_empty())?;
                if implementation["kind"] != "docs" {
                    return None;
                }
                Some(DocsLens {
                    key: key.clone(),
                    label: implementation["label"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    adapter: adapter.to_owned(),
                    styles: implementation["styles"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|style| style.as_str().map(str::to_owned))
                        .collect(),
                    environment: implementation["environment"]
                        .as_str()
                        .filter(|text| !text.is_empty())
                        .map(str::to_owned),
                    examples: examples.to_owned(),
                })
            })
            .collect();
        out.push(DocsPageEntry {
            src: markdown.to_owned(),
            page: item["src"].as_str().unwrap_or_default().to_owned(),
            label: item["label"].as_str().unwrap_or_default().to_owned(),
            lens: item["lens"].as_str().map(str::to_owned),
            lenses,
        });
    }
    let claimed: Vec<String> = out.iter().map(|entry| entry.src.clone()).collect();
    out.extend(
        discovered
            .iter()
            .filter(|entry| !claimed.contains(&entry.src))
            .cloned(),
    );
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn lens(key: &str, label: &str, examples: &str) -> DocsLens {
        DocsLens {
            key: key.into(),
            label: label.into(),
            adapter: "react".into(),
            styles: vec![],
            environment: None,
            examples: examples.into(),
        }
    }

    #[test]
    fn configured_pages_come_first_with_their_docs_lenses_and_claim_their_markdown() {
        let config = json!({
            "implementations": {
                "web": { "kind": "docs", "label": "Web", "adapter": "react", "styles": ["docs/web.css"] },
                "dev": { "kind": "url", "label": "Dev" },
            },
            "collections": [{ "items": [
                { "src": "docs/card.md", "label": "Card", "markdown": "docs/card.md", "lens": "web",
                  "implementations": { "web": { "examples": "docs/card.examples.tsx" }, "dev": { "path": "/card" } } },
                { "group": "More", "items": [
                    { "src": "pages/button.html", "label": "Button", "markdown": "docs/button.md" },
                    { "src": "pages/again.html", "label": "Again", "markdown": "docs/card.md" },
                ] },
                { "src": "pages/plain.html", "label": "Plain" },
            ] }],
        });
        let discovered = [
            DocsPageEntry {
                src: "docs/button.md".into(),
                page: "docs/button.md".into(),
                label: "Found".into(),
                lens: None,
                lenses: vec![],
            },
            DocsPageEntry {
                src: "docs/new.md".into(),
                page: "docs/new.md".into(),
                label: "New".into(),
                lens: None,
                lenses: vec![],
            },
        ];
        let pages = docs_pages(&config, &discovered);
        let found: Vec<(&str, &str, &str)> = pages
            .iter()
            .map(|entry| {
                (
                    entry.src.as_str(),
                    entry.page.as_str(),
                    entry.label.as_str(),
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                ("docs/card.md", "docs/card.md", "Card"),
                ("docs/button.md", "pages/button.html", "Button"),
                ("docs/new.md", "docs/new.md", "New"),
            ]
        );
        assert_eq!(pages[0].lens.as_deref(), Some("web"));
        assert_eq!(pages[0].lenses.len(), 1);
        assert_eq!(pages[0].lenses[0].styles, ["docs/web.css"]);
        assert_eq!(
            lens_request(&pages[0].lenses[0]),
            json!({ "adapter": "react", "examples": "docs/card.examples.tsx", "styles": ["docs/web.css"] })
        );
    }

    #[test]
    fn the_lens_shown_is_the_named_one_else_the_pages_own_else_the_first() {
        let entry = DocsPageEntry {
            src: "docs/card.md".into(),
            page: "docs/card.md".into(),
            label: "Card".into(),
            lens: Some("native".into()),
            lenses: vec![lens("web", "Web", "a/"), lens("native", "Native", "b/")],
        };
        assert_eq!(
            choose_lens(&entry, Some("web")).map(|lens| lens.key.as_str()),
            Some("web")
        );
        assert_eq!(
            choose_lens(&entry, Some("sepia")).map(|lens| lens.key.as_str()),
            Some("native")
        );
        let without = DocsPageEntry {
            lens: None,
            ..entry.clone()
        };
        assert_eq!(
            choose_lens(&without, None).map(|lens| lens.key.as_str()),
            Some("web")
        );
        let none = DocsPageEntry {
            lenses: vec![],
            ..entry
        };
        assert!(choose_lens(&none, None).is_none());
    }
}
