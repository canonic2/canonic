//! Docs pages for the portable export's `browser/` folder: each page in each
//! lens becomes a static page beside its production examples bundle, with the
//! page script copied unchanged, every example's code for Show code as a JSON
//! file, and the images its Markdown references. A static server opens them;
//! nothing is rebuilt or fetched from Workbench.

use super::export_source::export_source;
use super::highlight::{highlight, language_of};
use super::markdown::read_docs_markdown;
use super::render::{decode, encode_uri_component};
use super::service::{DocsRequest, DocsService, DocsSource, PageLocations};
use super::{DocsBundle, DocsLens, DocsPageEntry, ListedExample, Listing, lens_request};
use anyhow::{Context as _, Result, anyhow};
use regex::Regex;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

/// A lens's examples, built for production.
#[derive(Debug, Clone, Default)]
pub(crate) struct CompiledDocs {
    pub(crate) revision: String,
    /// Output files by name: `examples.js`, `examples.css` and their chunks.
    pub(crate) outputs: Vec<(String, Vec<u8>)>,
    pub(crate) stylesheet: bool,
    pub(crate) examples: Vec<ListedExample>,
    pub(crate) problems: Vec<String>,
}

/// Builds a lens's examples for the export.
pub(crate) trait PortableCompiler {
    /// Builds the examples a [`lens_request`] names.
    fn compile_docs(&self, request: &Value) -> impl Future<Output = Result<CompiledDocs>> + Send;
}

/// One page as the export's index lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PortableDocsPage {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) src: String,
    pub(crate) lens: Option<String>,
    pub(crate) lenses: Vec<PortableLens>,
    pub(crate) examples: Vec<PortableExample>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PortableLens {
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) directory: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PortableExample {
    pub(crate) id: String,
    pub(crate) label: String,
}

/// The exported pages and what couldn't be exported.
#[derive(Debug, Clone, Default)]
pub(crate) struct PortableDocs {
    pub(crate) pages: Vec<PortableDocsPage>,
    pub(crate) warnings: Vec<String>,
}

/// `docs/card.md` → `docs-card`: a folder name for one page's lenses.
pub(crate) fn page_slug(src: &str) -> String {
    let stem = src
        .len()
        .checked_sub(3)
        .filter(|&at| {
            src.get(at..)
                .is_some_and(|tail| tail.eq_ignore_ascii_case(".md"))
        })
        .map_or(src, |at| &src[..at]);
    let mut slug = String::new();
    let mut dash = false;
    for character in stem.chars() {
        if character.is_ascii_alphanumeric() {
            if dash && !slug.is_empty() {
                slug.push('-');
            }
            dash = false;
            slug.push(character.to_ascii_lowercase());
        } else {
            dash = true;
        }
    }
    if slug.is_empty() { "page".into() } else { slug }
}

/// `relative` inside `root`, with `.` and `..` resolved, or `None` outside it.
fn inside(root: &Path, relative: &str) -> Option<PathBuf> {
    let mut out = root.to_path_buf();
    for component in Path::new(relative).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (out.starts_with(root) && out != root).then_some(out)
}

fn read_text(root: &Path, file: &str) -> Option<String> {
    std::fs::read_to_string(inside(root, file)?).ok()
}

static IMAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(<img\b[^>]*\ssrc=")(/[^"]+)""#).expect("image pattern"));

/// Images the Markdown references by project path are copied beside the page.
fn copy_images(
    html: &str,
    root: &Path,
    directory: &str,
    add: &mut impl FnMut(String, Vec<u8>),
) -> String {
    IMAGE
        .replace_all(html, |found: &regex::Captures<'_>| {
            let whole = found[0].to_owned();
            let href = &found[2];
            let path = href[1..].split(['?', '#']).next().unwrap_or_default();
            let Some(relative) = decode(path) else {
                return whole;
            };
            let Some(file) = inside(root, &relative).filter(|file| file.is_file()) else {
                return whole;
            };
            let Ok(body) = std::fs::read(&file) else {
                return whole;
            };
            add(format!("{directory}/files/{relative}"), body);
            let encoded: Vec<String> = relative.split('/').map(encode_uri_component).collect();
            format!("{}./files/{}\"", &found[1], encoded.join("/"))
        })
        .into_owned()
}

/// One lens's built examples, as the page service sees them.
struct Built<'built> {
    root: &'built Path,
    compiled: Option<&'built CompiledDocs>,
}

impl DocsSource for Built<'_> {
    fn read_file(&self, file: &str) -> Result<Option<String>> {
        Ok(read_text(self.root, file))
    }

    fn blocked(&self) -> Option<String> {
        None
    }

    fn list_examples(&self, _lens: &DocsLens) -> impl Future<Output = Result<Listing>> + Send {
        std::future::ready(Ok(Listing {
            examples: self
                .compiled
                .map(|built| built.examples.clone())
                .unwrap_or_default(),
            problems: vec![],
        }))
    }

    fn bundle(&self, _lens: &DocsLens) -> impl Future<Output = Result<DocsBundle>> + Send {
        std::future::ready(
            self.compiled
                .ok_or_else(|| anyhow!("The examples could not be built for this export."))
                .map(|built| DocsBundle {
                    module: "./examples.js".into(),
                    stylesheet: built.stylesheet.then(|| "./examples.css".into()),
                    revision: built.revision.clone(),
                    examples: built.examples.clone(),
                    problems: built.problems.clone(),
                }),
        )
    }
}

/// A lens's outputs and every example's code for Show code.
fn add_built(
    root: &Path,
    directory: &str,
    built: &CompiledDocs,
    add: &mut impl FnMut(String, Vec<u8>),
) {
    for (name, body) in &built.outputs {
        add(format!("{directory}/{name}"), body.clone());
    }
    for example in &built.examples {
        let Some(file) = read_text(root, &example.file) else {
            continue;
        };
        let text = if example.export == "default" {
            Some(file)
        } else {
            export_source(&file, &example.export)
        };
        if let Some(text) = text {
            let html = highlight(&text, Some(&language_of(&example.file)));
            let code = json!({ "file": example.file, "text": text, "html": html });
            add(
                format!("{directory}/sources/{}", example.id),
                code.to_string().into_bytes(),
            );
        }
    }
}

/// Exports `entries` through `add(name, body)`, names relative to `browser/`.
/// `page_files` is Workbench's `ui/src/modules/docs/page/` folder.
///
/// # Errors
/// Fails when the docs page's own browser files can't be read.
pub(crate) async fn portable_docs(
    root: &Path,
    page_files: &Path,
    compiler: &impl PortableCompiler,
    entries: &[DocsPageEntry],
    add: &mut impl FnMut(String, Vec<u8>),
) -> Result<PortableDocs> {
    let mut out = PortableDocs::default();
    if entries.is_empty() {
        return Ok(out);
    }
    for (name, file) in [
        ("docs/page.js", "docs-page.js"),
        ("docs/mount-examples.js", "mount-examples.js"),
        ("docs/docs-page.css", "docs-page.css"),
    ] {
        let path = page_files.join(file);
        add(
            name.into(),
            std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?,
        );
    }
    let docs_pages: HashSet<String> = entries.iter().map(|entry| entry.src.clone()).collect();

    for entry in entries {
        let Some(markdown) = read_text(root, &entry.src) else {
            out.warnings
                .push(format!("{}: the Markdown file doesn’t exist.", entry.src));
            continue;
        };
        let examples = read_docs_markdown(&markdown)
            .placements
            .into_iter()
            .filter(|placement| !placement.id.is_empty() && !placement.duplicate)
            .map(|placement| PortableExample {
                id: placement.id,
                label: placement.label,
            })
            .collect();
        let mut page = PortableDocsPage {
            id: entry.src.clone(),
            title: entry.label.clone(),
            src: entry.src.clone(),
            lens: entry.lens.clone(),
            lenses: vec![],
            examples,
        };
        let lenses: Vec<Option<&DocsLens>> = if entry.lenses.is_empty() {
            vec![None]
        } else {
            entry.lenses.iter().map(Some).collect()
        };
        for lens in lenses {
            let directory = format!(
                "docs/{}/{}",
                page_slug(&entry.src),
                lens.map_or("page", |lens| lens.key.as_str())
            );
            let mut examples_built = None;
            if let Some(lens) = lens {
                match compiler.compile_docs(&lens_request(lens)).await {
                    Ok(built) => {
                        add_built(root, &directory, &built, add);
                        examples_built = Some(built);
                    }
                    Err(error) => out
                        .warnings
                        .push(format!("{} — {}: {error:#}", entry.src, lens.label)),
                }
            }
            let service = DocsService::new(Built {
                root,
                compiled: examples_built.as_ref(),
            });
            let locations = PageLocations {
                script: Some("../../page.js".into()),
                stylesheet: Some("../../docs-page.css".into()),
                source: Some("./sources/".into()),
                revision: Some(String::new()),
            };
            let request = DocsRequest {
                lens: lens.map(|lens| lens.key.as_str()),
                state: None,
            };
            let html = service
                .page(entry, request, &docs_pages, &locations, false)
                .await?;
            let html = copy_images(&html, root, &directory, add);
            add(format!("{directory}/index.html"), html.into_bytes());
            page.lenses.push(PortableLens {
                key: lens.map_or("page", |lens| lens.key.as_str()).into(),
                label: lens.map_or("Page", |lens| lens.label.as_str()).into(),
                directory: format!("browser/{directory}"),
            });
        }
        out.pages.push(page);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::tests::lens;
    use futures::executor::block_on;

    struct Compiler {
        fails: bool,
    }

    impl PortableCompiler for Compiler {
        fn compile_docs(
            &self,
            request: &Value,
        ) -> impl Future<Output = Result<CompiledDocs>> + Send {
            if self.fails {
                return std::future::ready(Err(anyhow::anyhow!("Build failed")));
            }
            assert_eq!(request["examples"], "docs/card.examples.tsx");
            std::future::ready(Ok(CompiledDocs {
                revision: "r1".into(),
                outputs: vec![
                    ("examples.js".into(), b"export {}".to_vec()),
                    ("examples.css".into(), b"a{}".to_vec()),
                ],
                stylesheet: true,
                examples: vec![ListedExample {
                    id: "basic".into(),
                    file: "docs/card.examples.tsx".into(),
                    export: "basic".into(),
                }],
                problems: vec![],
            }))
        }
    }

    fn fixture() -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("wb-docs-portable-{}", std::process::id()));
        let root = base.join("acme");
        let page = base.join("page");
        std::fs::create_dir_all(root.join("docs/images")).expect("root");
        std::fs::create_dir_all(&page).expect("page");
        std::fs::write(
            root.join("docs/card.md"),
            "# Card\n\n![Shape](images/shape.png)\n\n## Basic\n```example basic\n```\n",
        )
        .expect("md");
        std::fs::write(root.join("docs/images/shape.png"), b"png").expect("png");
        std::fs::write(
            root.join("docs/card.examples.tsx"),
            "export const basic = () => <Card />;\n",
        )
        .expect("examples");
        for file in ["docs-page.js", "mount-examples.js", "docs-page.css"] {
            std::fs::write(page.join(file), file).expect("page file");
        }
        (root, page)
    }

    #[test]
    fn each_page_lens_becomes_a_static_page_beside_its_bundle_sources_and_images() {
        let (root, page_files) = fixture();
        let entry = DocsPageEntry {
            src: "docs/card.md".into(),
            page: "docs/card.md".into(),
            label: "Card".into(),
            lens: Some("web".into()),
            lenses: vec![lens("web", "Web", "docs/card.examples.tsx")],
        };
        let missing = DocsPageEntry {
            src: "docs/gone.md".into(),
            ..entry.clone()
        };
        let mut files: Vec<(String, Vec<u8>)> = vec![];
        let exported = block_on(portable_docs(
            &root,
            &page_files,
            &Compiler { fails: false },
            &[entry, missing],
            &mut |name, body| files.push((name, body)),
        ))
        .expect("export");
        let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "docs/page.js",
                "docs/mount-examples.js",
                "docs/docs-page.css",
                "docs/docs-card/web/examples.js",
                "docs/docs-card/web/examples.css",
                "docs/docs-card/web/sources/basic",
                "docs/docs-card/web/files/docs/images/shape.png",
                "docs/docs-card/web/index.html",
            ]
        );
        let html = String::from_utf8(files[7].1.clone()).expect("html");
        assert!(
            html.contains(r#"<img src="./files/docs/images/shape.png""#),
            "{html}"
        );
        assert!(html.contains(r#"<script type="module" src="../../page.js">"#));
        assert!(html.contains(r#""bundle":{"module":"./examples.js"}"#));
        assert!(html.contains(r#""revisionUrl":"""#));
        let code: Value = serde_json::from_slice(&files[5].1).expect("code");
        assert_eq!(code["text"], "export const basic = () => <Card />;");
        assert_eq!(
            exported.warnings,
            ["docs/gone.md: the Markdown file doesn’t exist."]
        );
        assert_eq!(
            exported.pages[0].lenses[0].directory,
            "browser/docs/docs-card/web"
        );
        assert_eq!(
            exported.pages[0].examples,
            [PortableExample {
                id: "basic".into(),
                label: "Basic".into()
            }]
        );
    }

    #[test]
    fn a_lens_that_fails_to_build_still_exports_its_page_and_says_why() {
        let (root, page_files) = fixture();
        let entry = DocsPageEntry {
            src: "docs/card.md".into(),
            page: "docs/card.md".into(),
            label: "Card".into(),
            lens: None,
            lenses: vec![lens("web", "Web", "docs/card.examples.tsx")],
        };
        let mut files = vec![];
        let exported = block_on(portable_docs(
            &root,
            &page_files,
            &Compiler { fails: true },
            &[entry],
            &mut |name, _| files.push(name),
        ))
        .expect("export");
        assert_eq!(exported.warnings, ["docs/card.md — Web: Build failed"]);
        assert!(files.contains(&"docs/docs-card/web/index.html".to_owned()));
    }

    #[test]
    fn page_slugs_name_a_markdown_file_as_a_folder() {
        assert_eq!(page_slug("docs/card.md"), "docs-card");
        assert_eq!(page_slug("Docs/My Card.MD"), "docs-my-card");
        assert_eq!(page_slug("--.md"), "page");
    }
}
