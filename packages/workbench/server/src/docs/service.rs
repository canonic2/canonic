//! Docs pages on the server: the page document for a lens, the bundle its
//! script mounts, the problems the shared list shows, an example's code for
//! Show code, and the revision that reloads an open page.
//!
//! Reading files and building examples come from a [`DocsSource`], so this
//! module decides what a page shows without owning any I/O.

use super::export_source::export_source;
use super::highlight::{escape_html, highlight, language_of};
use super::markdown::{ExamplePlacement, match_lens, read_docs_markdown};
use super::page::{BundleRef, DocsPageOptions, PageDocument, page_document};
use super::render::{PanelStatus, PanelView, RenderInput, encode_uri_component, render_docs};
use super::{
    BUNDLE_PATH, BundleInfo, DocsBundle, DocsLens, DocsPageEntry, HIGHLIGHT_PATH, Listing,
    REVISION_PATH, SOURCE_PATH, choose_lens,
};
use crate::refused::refused;
use anyhow::Result;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::future::Future;

/// Where a page's files and examples come from.
pub(crate) trait DocsSource {
    /// A project file's text, or `None` when it doesn't exist.
    ///
    /// # Errors
    /// Fails when the file exists but can't be read.
    fn read_file(&self, file: &str) -> Result<Option<String>>;

    /// Why examples can't run here (they run project code), or `None` when they can.
    fn blocked(&self) -> Option<String>;

    /// Lists a lens's examples, or fails saying why it can't.
    fn list_examples(&self, lens: &DocsLens) -> impl Future<Output = Result<Listing>> + Send;

    /// Builds a lens's examples bundle, or fails saying why it can't.
    fn bundle(&self, lens: &DocsLens) -> impl Future<Output = Result<DocsBundle>> + Send;
}

/// What a page request asks for.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DocsRequest<'request> {
    pub(crate) lens: Option<&'request str>,
    pub(crate) state: Option<&'request str>,
}

/// Where a page finds its script, stylesheet and example code: the server's
/// routes unless given.
#[derive(Debug, Clone, Default)]
pub(crate) struct PageLocations {
    pub(crate) script: Option<String>,
    pub(crate) stylesheet: Option<String>,
    /// Show code fetches this followed by the example's ID.
    pub(crate) source: Option<String>,
    /// Polled for edits; empty for a page that never changes.
    pub(crate) revision: Option<String>,
}

/// An example's code for Show code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ExampleCode {
    pub(crate) file: String,
    pub(crate) text: String,
    pub(crate) html: String,
}

/// An example a page places, for the export's references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Outlined {
    pub(crate) id: String,
    pub(crate) label: String,
}

/// What a page's panels show in one lens.
struct Shows<'page> {
    /// Why no example shows: no lens, no trust, or a failed build.
    unavailable: Option<&'page str>,
    deferred: bool,
    available: &'page HashSet<&'page str>,
    lens_label: &'page str,
}

impl Shows<'_> {
    fn panel(&self, placement: &ExamplePlacement) -> PanelView {
        if placement.duplicate {
            return PanelView::noted(
                PanelStatus::Invalid,
                if placement.id.is_empty() {
                    "This example block names no example."
                } else {
                    "This example is placed more than once on the page."
                },
            );
        }
        if let Some(note) = self.unavailable {
            return PanelView::noted(PanelStatus::Unavailable, note);
        }
        if self.deferred {
            return PanelView::shown(PanelStatus::Pending);
        }
        if !self.available.contains(placement.id.as_str()) {
            return PanelView::noted(
                PanelStatus::Missing,
                format!("Not available in {}", self.lens_label),
            );
        }
        PanelView::shown(PanelStatus::Ready)
    }
}

/// The docs page service over one source.
pub(crate) struct DocsService<S> {
    source: S,
}

fn message(error: &anyhow::Error) -> String {
    format!("{error:#}")
}

impl<S: DocsSource + Sync> DocsService<S> {
    pub(crate) fn new(source: S) -> Self {
        Self { source }
    }

    pub(crate) fn source(&self) -> &S {
        &self.source
    }

    fn markdown(&self, entry: &DocsPageEntry) -> Result<String> {
        self.source
            .read_file(&entry.src)?
            .ok_or_else(|| refused(404, format!("{} doesn’t exist.", entry.src)))
    }

    /// The page document. The Markdown, rendered without waiting for anything
    /// else, is the page; a deferred page's examples follow once its script
    /// has the bundle.
    ///
    /// # Errors
    /// Refuses with 404 when the Markdown file doesn't exist.
    pub(crate) async fn page(
        &self,
        entry: &DocsPageEntry,
        request: DocsRequest<'_>,
        docs_pages: &HashSet<String>,
        locations: &PageLocations,
        defer: bool,
    ) -> Result<String> {
        let source = self.markdown(entry)?;
        let lens = choose_lens(entry, request.lens);
        let mut bundle: Option<DocsBundle> = None;
        let mut unavailable: Option<String> = lens
            .is_none()
            .then(|| "This page has no docs lens that renders examples.".to_owned());
        let blocked = self.source.blocked();
        let deferred = lens.is_some() && blocked.is_none() && defer;
        if let Some(lens) = lens {
            if let Some(blocked) = &blocked {
                unavailable = Some(blocked.clone());
            } else if !deferred {
                match self.source.bundle(lens).await {
                    Ok(built) => bundle = Some(built),
                    Err(error) => {
                        unavailable = Some(format!("{}: {}", lens.label, message(&error)));
                    }
                }
            }
        }
        let available: HashSet<&str> = bundle
            .as_ref()
            .map(|bundle| {
                bundle
                    .examples
                    .iter()
                    .map(|example| example.id.as_str())
                    .collect()
            })
            .unwrap_or_default();
        let shows = Shows {
            unavailable: unavailable.as_deref(),
            deferred,
            available: &available,
            lens_label: lens.map_or("", |lens| lens.label.as_str()),
        };
        let rendering = crate::trace::phase("markdown");
        let rendered = render_docs(
            &RenderInput {
                defer_highlight: defer,
                source: &source,
                file: &entry.src,
                docs_pages,
            },
            |placement| shows.panel(placement),
        );
        let query = query(entry, lens);
        drop(rendering);
        let bundle_ref = if deferred {
            Some(BundleRef::Info {
                info: format!("{BUNDLE_PATH}{query}"),
            })
        } else {
            bundle
                .as_ref()
                .filter(|_| unavailable.is_none())
                .map(|bundle| BundleRef::Module {
                    module: bundle.module.clone(),
                })
        };
        let options = DocsPageOptions {
            highlight_url: defer.then(|| format!("{HIGHLIGHT_PATH}{query}")),
            page: entry.src.clone(),
            lens: lens.map(|lens| lens.key.clone()),
            lens_label: lens.map(|lens| lens.label.clone()),
            state: request.state.map(str::to_owned),
            bundle: bundle_ref,
            source_url: locations
                .source
                .clone()
                .unwrap_or_else(|| format!("{SOURCE_PATH}{query}&example=")),
            revision_url: locations
                .revision
                .clone()
                .unwrap_or_else(|| format!("{REVISION_PATH}{query}")),
            // A deferred page learns its revision with its bundle.
            revision: if deferred {
                String::new()
            } else {
                revision_of(&source, bundle.as_ref())
            },
        };
        let stylesheets: Vec<String> = bundle
            .iter()
            .filter_map(|bundle| bundle.stylesheet.clone())
            .collect();
        Ok(page_document(&PageDocument {
            title: rendered.title.as_deref().unwrap_or(&entry.label),
            script: locations.script.as_deref(),
            stylesheet: locations.stylesheet.as_deref(),
            body: &rendered.html,
            stylesheets: &stylesheets,
            options: &options,
        }))
    }

    /// What a deferred page mounts in a lens, with the page's revision.
    ///
    /// # Errors
    /// Refuses with 404 when the Markdown file doesn't exist.
    pub(crate) async fn bundle_info(
        &self,
        entry: &DocsPageEntry,
        lens_key: Option<&str>,
    ) -> Result<BundleInfo> {
        let source = self.markdown(entry)?;
        let none = |error: String| BundleInfo {
            module: None,
            stylesheet: None,
            examples: vec![],
            error: Some(error),
            revision: revision_of(&source, None),
        };
        let Some(lens) = choose_lens(entry, lens_key) else {
            return Ok(none(
                "This page has no docs lens that renders examples.".into(),
            ));
        };
        if let Some(blocked) = self.source.blocked() {
            return Ok(none(blocked));
        }
        match self.source.bundle(lens).await {
            Ok(bundle) => Ok(BundleInfo {
                module: Some(bundle.module.clone()),
                stylesheet: bundle.stylesheet.clone(),
                examples: bundle
                    .examples
                    .iter()
                    .map(|example| example.id.clone())
                    .collect(),
                error: None,
                revision: revision_of(&source, Some(&bundle)),
            }),
            Err(error) => Ok(none(format!("{}: {}", lens.label, message(&error)))),
        }
    }

    /// The current revision of a page in a lens; it changes when the Markdown
    /// or the examples do.
    ///
    /// # Errors
    /// Refuses with 404 when the Markdown file doesn't exist.
    pub(crate) async fn revision(
        &self,
        entry: &DocsPageEntry,
        lens_key: Option<&str>,
    ) -> Result<String> {
        let source = self.markdown(entry)?;
        let mut bundle = None;
        if let Some(lens) = choose_lens(entry, lens_key)
            && self.source.blocked().is_none()
        {
            // A failing build shows on the page; its revision is the Markdown's.
            bundle = self.source.bundle(lens).await.ok();
        }
        Ok(revision_of(&source, bundle.as_ref()))
    }

    /// One example's code: a whole file for a folder source, the export's
    /// statement for a file source.
    ///
    /// # Errors
    /// Refuses with 404 for a page without a lens, an unknown example or a
    /// missing file or export, and with 403 when examples can't run here.
    pub(crate) async fn example_source(
        &self,
        entry: &DocsPageEntry,
        lens_key: Option<&str>,
        id: &str,
        plain: bool,
    ) -> Result<ExampleCode> {
        let Some(lens) = choose_lens(entry, lens_key) else {
            return Err(refused(
                404,
                format!("{} has no docs lens that renders examples.", entry.src),
            ));
        };
        if let Some(blocked) = self.source.blocked() {
            return Err(refused(403, blocked));
        }
        let listed = self.source.list_examples(lens).await?;
        let Some(example) = listed.examples.iter().find(|candidate| candidate.id == id) else {
            return Err(refused(
                404,
                format!("{} has no example “{id}”.", lens.label),
            ));
        };
        let Some(file) = self.source.read_file(&example.file)? else {
            return Err(refused(404, format!("{} doesn’t exist.", example.file)));
        };
        let text = if example.export == "default" {
            Some(file)
        } else {
            export_source(&file, &example.export)
        };
        let Some(text) = text else {
            return Err(refused(
                404,
                format!(
                    "Couldn’t find the export {} in {}.",
                    example.export, example.file
                ),
            ));
        };
        let html = if plain {
            escape_html(&text)
        } else {
            highlight(&text, Some(&language_of(&example.file)))
        };
        Ok(ExampleCode {
            file: example.file.clone(),
            text,
            html,
        })
    }

    /// Highlight the current Markdown off the request executor; examples are never built here.
    pub(crate) fn highlight_page(
        &self,
        entry: &DocsPageEntry,
    ) -> Result<Vec<super::render::HighlightedCode>> {
        let source = self.markdown(entry)?;
        Ok(render_docs(
            &RenderInput {
                source: &source,
                file: &entry.src,
                docs_pages: &HashSet::new(),
                defer_highlight: false,
            },
            |_| PanelView::shown(PanelStatus::Pending),
        )
        .highlights)
    }

    /// The examples a page places, for the export's references; `None` when
    /// its Markdown is missing.
    ///
    /// # Errors
    /// Fails when the Markdown exists but can't be read.
    pub(crate) fn outline(&self, entry: &DocsPageEntry) -> Result<Option<Vec<Outlined>>> {
        let Some(text) = self.source.read_file(&entry.src)? else {
            return Ok(None);
        };
        Ok(Some(
            read_docs_markdown(&text)
                .placements
                .into_iter()
                .filter(|placement| !placement.id.is_empty() && !placement.duplicate)
                .map(|placement| Outlined {
                    id: placement.id,
                    label: placement.label,
                })
                .collect(),
        ))
    }

    /// Problems for the shared list, named by page, listing each lens.
    ///
    /// # Errors
    /// Fails when a Markdown file exists but can't be read.
    #[cfg(test)]
    pub(crate) async fn problems(&self, entries: &[DocsPageEntry]) -> Result<Vec<String>> {
        let mut known = vec![];
        for entry in entries {
            for lens in &entry.lenses {
                if self.source.blocked().is_none() {
                    known.push((
                        lens.clone(),
                        self.source
                            .list_examples(lens)
                            .await
                            .map_err(|error| message(&error)),
                    ));
                }
            }
        }
        self.problems_with(entries, |lens| {
            known
                .iter()
                .find(|(listed, _)| listed == lens)
                .map(|(_, result)| result.clone())
        })
    }

    /// Problems for the shared list, named by page. `listing` answers a
    /// lens's examples as last listed; a lens it answers `None` for isn't
    /// listed yet and is left out.
    ///
    /// # Errors
    /// Fails when a Markdown file exists but can't be read.
    pub(crate) fn problems_with(
        &self,
        entries: &[DocsPageEntry],
        listing: impl Fn(&DocsLens) -> Option<Result<Listing, String>>,
    ) -> Result<Vec<String>> {
        let mut out = vec![];
        let blocked = self.source.blocked();
        let mut skipped = false;
        for entry in entries {
            let place = format!("{} ({})", entry.label, entry.src);
            let Some(text) = self.source.read_file(&entry.src)? else {
                out.push(format!("{place}: the Markdown file doesn’t exist."));
                continue;
            };
            let doc = read_docs_markdown(&text);
            for problem in &doc.problems {
                out.push(format!(
                    "{place}, line {}: {}",
                    problem.line, problem.message
                ));
            }
            for lens in &entry.lenses {
                if blocked.is_some() {
                    skipped = true;
                    continue;
                }
                match listing(lens) {
                    None => {}
                    Some(Ok(listed)) => {
                        for problem in &listed.problems {
                            out.push(format!("{place}: {}: {problem}", lens.label));
                        }
                        let exported: Vec<String> = listed
                            .examples
                            .iter()
                            .map(|example| example.id.clone())
                            .collect();
                        for id in match_lens(&doc.placements, &exported).unplaced {
                            out.push(format!(
                                "{place}: example “{id}” in {} is not placed in {}.",
                                lens.label, entry.src
                            ));
                        }
                    }
                    Some(Err(error)) => out.push(format!("{place}: {}: {error}", lens.label)),
                }
            }
        }
        if skipped && let Some(blocked) = blocked {
            out.push(format!("Docs page examples: {blocked}"));
        }
        Ok(out)
    }
}

/// `?page=…&lens=…` for a page's routes.
fn query(entry: &DocsPageEntry, lens: Option<&DocsLens>) -> String {
    let mut query = format!("?page={}", encode_uri_component(&entry.src));
    if let Some(lens) = lens {
        query.push_str("&lens=");
        query.push_str(&encode_uri_component(&lens.key));
    }
    query
}

fn revision_of(markdown: &str, bundle: Option<&DocsBundle>) -> String {
    let mut hash = Sha256::new();
    hash.update(markdown.as_bytes());
    hash.update(b"\0");
    hash.update(
        bundle
            .map_or("", |bundle| bundle.revision.as_str())
            .as_bytes(),
    );
    let mut out = String::with_capacity(16);
    for byte in hash.finalize().iter().take(8) {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::ListedExample;
    use crate::docs::tests::lens;
    use crate::refused::status_of;
    use futures::executor::block_on;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct Fixture {
        files: Mutex<HashMap<String, String>>,
        trusted: bool,
        failing: Option<&'static str>,
        built: Mutex<Vec<String>>,
    }

    fn listing(key: &str) -> Vec<ListedExample> {
        let example = |id: &str, file: &str, export: &str| ListedExample {
            id: id.into(),
            file: file.into(),
            export: export.into(),
        };
        if key == "web" {
            vec![
                example("basic", "docs/card.examples.ts", "basic"),
                example("nested", "docs/card.examples.ts", "nested"),
                example("large", "docs/card.examples.ts", "large"),
            ]
        } else {
            vec![example("basic", "docs/native/basic.tsx", "default")]
        }
    }

    impl DocsSource for Fixture {
        fn read_file(&self, file: &str) -> Result<Option<String>> {
            Ok(self.files.lock().expect("files").get(file).cloned())
        }

        fn blocked(&self) -> Option<String> {
            (!self.trusted).then(|| {
                "Examples run project code, so they appear in a trusted workspace only.".into()
            })
        }

        fn list_examples(&self, lens: &DocsLens) -> impl Future<Output = Result<Listing>> + Send {
            let listed = if self.failing == Some(lens.key.as_str()) {
                Err(anyhow::anyhow!("Build failed: missing ./card.tsx"))
            } else {
                let problems = if lens.key == "native" {
                    vec!["Shadow.tsx: example file names must be kebab-case.".into()]
                } else {
                    vec![]
                };
                Ok(Listing {
                    examples: listing(&lens.key),
                    problems,
                })
            };
            std::future::ready(listed)
        }

        fn bundle(&self, lens: &DocsLens) -> impl Future<Output = Result<DocsBundle>> + Send {
            self.built.lock().expect("built").push(lens.key.clone());
            let built = if self.failing == Some(lens.key.as_str()) {
                Err(anyhow::anyhow!("Build failed: missing ./card.tsx"))
            } else {
                Ok(DocsBundle {
                    module: format!("/_workbench/previews/docs/{}/examples.js", lens.key),
                    stylesheet: (lens.key == "native")
                        .then(|| "/_workbench/previews/docs/native/examples.css".into()),
                    revision: format!("r-{}", lens.key),
                    examples: listing(&lens.key),
                    problems: vec![],
                })
            };
            std::future::ready(built)
        }
    }

    fn files() -> HashMap<String, String> {
        HashMap::from([
            ("docs/card.md".into(), "# Card\n\n## Basic\n```example basic\ncaption: children\n```\n## Nested\n```example nested\n```".into()),
            (
                "docs/card.examples.ts".into(),
                "import { Card } from \"./card.tsx\";\n\nexport const basic = () => <Card>Basic</Card>;\n\nexport const nested = () => (\n  <Card>Nested</Card>\n);".into(),
            ),
            ("docs/native/basic.tsx".into(), "export default () => null;\n".into()),
        ])
    }

    fn service(trusted: bool, failing: Option<&'static str>) -> DocsService<Fixture> {
        DocsService::new(Fixture {
            files: Mutex::new(files()),
            trusted,
            failing,
            built: Mutex::default(),
        })
    }

    fn card() -> DocsPageEntry {
        let native = DocsLens {
            styles: vec!["docs/native.css".into()],
            ..lens("native", "Native", "docs/native/")
        };
        DocsPageEntry {
            src: "docs/card.md".into(),
            page: "docs/card.md".into(),
            label: "Card".into(),
            lens: Some("web".into()),
            lenses: vec![lens("web", "Web", "docs/card.examples.ts"), native],
        }
    }

    fn page(
        service: &DocsService<Fixture>,
        entry: &DocsPageEntry,
        request: DocsRequest<'_>,
        defer: bool,
    ) -> Result<String> {
        block_on(service.page(
            entry,
            request,
            &HashSet::new(),
            &PageLocations::default(),
            defer,
        ))
    }

    fn options_of(html: &str) -> serde_json::Value {
        let start = html.find("window.__workbenchDocs=").expect("options")
            + "window.__workbenchDocs=".len();
        let end = start + html[start..].find("</script>").expect("end");
        serde_json::from_str(&html[start..end]).expect("json")
    }

    fn built(service: &DocsService<Fixture>) -> Vec<String> {
        service.source().built.lock().expect("built").clone()
    }

    #[test]
    fn a_page_opens_in_its_own_lens_and_mounts_the_examples_that_lens_has() {
        let docs = service(true, None);
        let html = page(&docs, &card(), DocsRequest::default(), false).expect("page");
        assert_eq!(built(&docs), ["web"]);
        assert!(html.contains("<title>Card</title>"));
        assert!(html.contains(r#"data-wb-example="basic" data-status="ready""#));
        assert!(html.contains(r#"data-wb-example="nested" data-status="ready""#));
        let options = options_of(&html);
        assert_eq!(options["lens"], "web");
        assert_eq!(
            options["bundle"],
            serde_json::json!({ "module": "/_workbench/previews/docs/web/examples.js" })
        );
        assert_eq!(
            options["sourceUrl"],
            "/_workbench/docs/source?page=docs%2Fcard.md&lens=web&example="
        );
    }

    #[test]
    fn another_lens_keeps_every_panel_and_what_it_lacks_says_so() {
        let docs = service(true, None);
        let html = page(
            &docs,
            &card(),
            DocsRequest {
                lens: Some("native"),
                state: Some("loading"),
            },
            false,
        )
        .expect("page");
        assert!(html.contains(r#"data-wb-example="basic" data-status="ready""#));
        assert!(html.contains(r#"data-wb-example="nested" data-status="missing"><p class="wb-docs-example-note">Not available in Native</p>"#));
        assert!(html.contains(
            r#"<link rel="stylesheet" href="/_workbench/previews/docs/native/examples.css">"#
        ));
        assert_eq!(options_of(&html)["state"], "loading");
    }

    #[test]
    fn without_trust_or_when_a_lens_fails_to_build_the_markdown_still_renders() {
        let untrusted = service(false, None);
        let html = page(&untrusted, &card(), DocsRequest::default(), false).expect("page");
        assert_eq!(built(&untrusted), Vec::<String>::new());
        assert!(html.contains(r#"data-status="unavailable"><p class="wb-docs-example-note">Examples run project code, so they appear in a trusted workspace only.</p>"#));
        assert_eq!(options_of(&html)["bundle"], serde_json::Value::Null);
        assert!(html.contains(r#"<h2 id="basic">Basic</h2>"#));

        let failing = service(true, Some("web"));
        let html = page(&failing, &card(), DocsRequest::default(), false).expect("page");
        assert!(html.contains(r#"data-status="unavailable"><p class="wb-docs-example-note">Web: Build failed: missing ./card.tsx</p>"#));
        let gone = DocsPageEntry {
            src: "docs/gone.md".into(),
            ..card()
        };
        let error = page(&failing, &gone, DocsRequest::default(), false).expect_err("missing");
        assert_eq!(status_of(&error, 500), 404);
    }

    #[test]
    fn show_code_gets_a_named_exports_statement_or_a_folder_examples_whole_file() {
        let docs = service(true, None);
        let nested =
            block_on(docs.example_source(&card(), Some("web"), "nested", false)).expect("nested");
        assert_eq!(
            nested.text,
            "export const nested = () => (\n  <Card>Nested</Card>\n);"
        );
        assert!(nested.html.contains("hljs-keyword"));
        let whole =
            block_on(docs.example_source(&card(), Some("native"), "basic", false)).expect("basic");
        assert_eq!(whole.text, "export default () => null;\n");
        let large =
            block_on(docs.example_source(&card(), Some("web"), "large", false)).expect_err("large");
        assert_eq!(status_of(&large, 500), 404);
        assert!(large.to_string().contains("Couldn’t find the export large"));
        let unknown = block_on(docs.example_source(&card(), Some("native"), "nested", false))
            .expect_err("nested");
        assert_eq!(
            (status_of(&unknown, 500), unknown.to_string()),
            (404, "Native has no example “nested”.".into())
        );
        let untrusted =
            block_on(service(false, None).example_source(&card(), Some("web"), "basic", false))
                .expect_err("blocked");
        assert_eq!(status_of(&untrusted, 500), 403);
    }

    #[test]
    fn problems_name_the_page_the_line_and_the_lens() {
        let docs = service(true, Some("native"));
        docs.source().files.lock().expect("files").insert(
            "docs/broken.md".into(),
            "```example basic\ntitle: x\n```".into(),
        );
        let other = |src: &str, label: &str| DocsPageEntry {
            src: src.into(),
            page: src.into(),
            label: label.into(),
            lens: None,
            lenses: vec![],
        };
        let problems = block_on(docs.problems(&[
            card(),
            other("docs/broken.md", "Broken"),
            other("docs/gone.md", "Gone"),
        ]))
        .expect("problems");
        assert_eq!(
            problems,
            [
                "Card (docs/card.md): example “large” in Web is not placed in docs/card.md.",
                "Card (docs/card.md): Native: Build failed: missing ./card.tsx",
                "Broken (docs/broken.md), line 2: example “basic”: unknown key “title”",
                "Gone (docs/gone.md): the Markdown file doesn’t exist.",
            ]
        );
        assert_eq!(
            block_on(service(false, None).problems(&[card()])).expect("problems"),
            [
                "Docs page examples: Examples run project code, so they appear in a trusted workspace only."
            ]
        );
    }

    #[test]
    fn problems_leave_out_a_lens_that_isnt_listed_yet() {
        let docs = service(true, None);
        let listed = docs
            .problems_with(&[card()], |lens| {
                (lens.key != "web").then(|| {
                    Ok(Listing {
                        examples: listing("native"),
                        problems: vec!["Shadow.tsx: example file names must be kebab-case.".into()],
                    })
                })
            })
            .expect("problems");
        assert_eq!(
            listed,
            ["Card (docs/card.md): Native: Shadow.tsx: example file names must be kebab-case."]
        );
    }

    #[test]
    fn the_revision_follows_the_markdown_and_the_lenss_examples() {
        let docs = service(true, None);
        let first = block_on(docs.revision(&card(), Some("web"))).expect("revision");
        assert_eq!(first.len(), 16);
        assert_eq!(
            block_on(docs.revision(&card(), Some("web"))).expect("again"),
            first
        );
        assert_ne!(
            block_on(docs.revision(&card(), Some("native"))).expect("native"),
            first
        );
        docs.source()
            .files
            .lock()
            .expect("files")
            .get_mut("docs/card.md")
            .expect("card")
            .push_str("\nMore.");
        assert_ne!(
            block_on(docs.revision(&card(), Some("web"))).expect("edited"),
            first
        );
    }

    #[test]
    fn a_deferred_page_answers_with_its_markdown_and_its_bundle_says_what_each_panel_shows() {
        let docs = service(true, None);
        let html = page(&docs, &card(), DocsRequest::default(), true).expect("page");
        assert!(
            built(&docs).is_empty(),
            "nothing is built for the page itself"
        );
        assert!(html.contains(
            r#"data-wb-example="basic" data-status="pending"><div class="wb-docs-example-stage""#
        ));
        let options = options_of(&html);
        assert_eq!(
            options["bundle"],
            serde_json::json!({ "info": "/_workbench/docs/bundle?page=docs%2Fcard.md&lens=web" })
        );
        assert_eq!(options["lensLabel"], "Web");
        assert_eq!(options["revision"], "");

        let info = block_on(docs.bundle_info(&card(), Some("native"))).expect("info");
        assert_eq!(
            info,
            BundleInfo {
                module: Some("/_workbench/previews/docs/native/examples.js".into()),
                stylesheet: Some("/_workbench/previews/docs/native/examples.css".into()),
                examples: vec!["basic".into()],
                error: None,
                revision: block_on(docs.revision(&card(), Some("native"))).expect("revision"),
            }
        );

        let failing = service(true, Some("web"));
        let info = block_on(failing.bundle_info(&card(), Some("web"))).expect("info");
        assert_eq!(info.module, None);
        assert_eq!(
            info.error.as_deref(),
            Some("Web: Build failed: missing ./card.tsx")
        );
        assert_eq!(
            info.revision,
            block_on(failing.revision(&card(), Some("web"))).expect("revision")
        );

        let untrusted = service(false, None);
        assert!(
            page(&untrusted, &card(), DocsRequest::default(), true)
                .expect("page")
                .contains(r#"data-status="unavailable""#)
        );
        assert_eq!(
            block_on(untrusted.bundle_info(&card(), Some("web")))
                .expect("info")
                .error
                .as_deref(),
            Some("Examples run project code, so they appear in a trusted workspace only.")
        );
    }

    #[test]
    fn the_outline_lists_placed_examples_and_none_without_markdown() {
        let docs = service(true, None);
        let outline = docs.outline(&card()).expect("outline").expect("markdown");
        assert_eq!(
            outline,
            [
                Outlined {
                    id: "basic".into(),
                    label: "Basic".into()
                },
                Outlined {
                    id: "nested".into(),
                    label: "Nested".into()
                }
            ]
        );
        assert_eq!(
            docs.outline(&DocsPageEntry {
                src: "docs/gone.md".into(),
                ..card()
            })
            .expect("outline"),
            None
        );
    }
}
