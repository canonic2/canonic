//! The HTML document a docs page is served as: Workbench's docs stylesheet,
//! the lens's example styles, the rendered Markdown, and the page script that
//! mounts the examples. Everything the script needs travels in
//! `window.__workbenchDocs`.

use super::highlight::escape_html;
use serde::Serialize;
use std::fmt::Write as _;

/// The docs page stylesheet, served from Workbench's browser files.
pub(crate) const PAGE_STYLESHEET: &str = "/_workbench/ui/src/modules/docs/page/docs-page.css";
/// The docs page script, served from Workbench's browser files.
pub(crate) const PAGE_SCRIPT: &str = "/_workbench/ui/src/modules/docs/page/docs-page.js";

/// The lens's examples bundle, or where a deferred page asks for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub(crate) enum BundleRef {
    Module { module: String },
    Info { info: String },
}

/// What the docs page script reads from `window.__workbenchDocs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocsPageOptions {
    /// Local endpoint for live highlighting; absent in already-coloured portable documents.
    pub(crate) highlight_url: Option<String>,
    /// The docs page's src, relative to the project root.
    pub(crate) page: String,
    /// The lens rendering the examples, and its label, or null when the page has none.
    pub(crate) lens: Option<String>,
    pub(crate) lens_label: Option<String>,
    pub(crate) state: Option<String>,
    /// Null when nothing is mounted.
    pub(crate) bundle: Option<BundleRef>,
    /// Where Show code asks for an example's source; the example ID is appended.
    pub(crate) source_url: String,
    /// Answers the page's current revision, so an edit reloads it; empty where nothing changes.
    pub(crate) revision_url: String,
    /// Markdown identity and examples revision; the examples part is empty
    /// until a deferred bundle answers.
    pub(crate) revision: String,
}

/// One docs page document.
pub(crate) struct PageDocument<'input> {
    pub(crate) title: &'input str,
    /// The page's script: the server's by default, beside the page in a portable export.
    pub(crate) script: Option<&'input str>,
    pub(crate) stylesheet: Option<&'input str>,
    pub(crate) body: &'input str,
    pub(crate) stylesheets: &'input [String],
    pub(crate) options: &'input DocsPageOptions,
}

pub(crate) fn page_document(input: &PageDocument<'_>) -> String {
    let options = serde_json::to_string(input.options)
        .unwrap_or_else(|_| "null".into())
        .replace('<', "\\u003c");
    let mut out = String::from(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">",
    );
    let _ = write!(out, "<title>{}</title>", escape_html(input.title));
    let _ = write!(
        out,
        "<link rel=\"stylesheet\" href=\"{}\">",
        escape_html(input.stylesheet.unwrap_or(PAGE_STYLESHEET))
    );
    for href in input.stylesheets {
        let _ = write!(
            out,
            "<link rel=\"stylesheet\" href=\"{}\">",
            escape_html(href)
        );
    }
    out.push_str("</head><body class=\"wb-docs-body\">");
    let _ = write!(out, "<main class=\"wb-docs\">{}</main>", input.body);
    let _ = write!(out, "<script>window.__workbenchDocs={options}</script>");
    let _ = write!(
        out,
        "<script type=\"module\">import {{ start }} from {}; start(window.__workbenchDocs);</script>",
        serde_json::to_string(input.script.unwrap_or(PAGE_SCRIPT))
            .expect("script URL")
            .replace('<', "\\u003c")
    );
    out.push_str("</body></html>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_options_cannot_close_their_script_and_defaults_are_the_servers_files() {
        let options = DocsPageOptions {
            highlight_url: None,
            page: "docs/card.md".into(),
            lens: None,
            lens_label: None,
            state: None,
            bundle: Some(BundleRef::Info {
                info: "/_workbench/docs/bundle?page=x".into(),
            }),
            source_url: "/s?".into(),
            revision_url: "/r".into(),
            revision: "</script>".into(),
        };
        let html = page_document(&PageDocument {
            title: "Card & co",
            script: None,
            stylesheet: None,
            body: "<h1>Card</h1>",
            stylesheets: &["/x.css".into()],
            options: &options,
        });
        assert!(html.contains("<title>Card &#38; co</title>"));
        assert!(html.contains(&format!(
            "href=\"{PAGE_STYLESHEET}\"><link rel=\"stylesheet\" href=\"/x.css\">"
        )));
        assert!(html.contains("\"revision\":\"\\u003c/script>\""));
        assert!(html.contains("\"bundle\":{\"info\":\"/_workbench/docs/bundle?page=x\"}"));
        assert!(html.contains("\"lensLabel\":null"));
        assert!(html.ends_with(&format!(
            "<script type=\"module\">import {{ start }} from \"{PAGE_SCRIPT}\"; start(window.__workbenchDocs);</script></body></html>"
        )));
    }
}
