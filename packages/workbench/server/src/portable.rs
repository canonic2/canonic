//! A space's portable browser build: every listed preview and docs page built
//! for production by the native compiler, with the viewer that opens them
//! from a static folder. The design export's browser format and the `build`
//! command both write it.
//!
//! A preview or docs page that fails to build is a warning, never a failed
//! build, so one broken definition never costs the rest.
use crate::catalog::IndexAnswer;
use crate::compiler::Previews;
use crate::docs::portable::{CompiledDocs, PortableCompiler, portable_docs};
use crate::docs::{DocsPageEntry, ListedExample, docs_pages};
use crate::export::sizes::{default_sizes, supported};
use crate::export::{PortableExport, PortablePreview, Selection};
use anyhow::{Context as _, Result};
use base64::Engine as _;
use serde_json::{Map, Value, json};
use std::future::Future;
use std::path::Path;
use std::sync::Arc;

/// The native compiler, as the docs export builds a lens with it.
struct Compiler<'a>(&'a Previews);

fn decoded(outputs: &Value) -> Vec<(String, Vec<u8>)> {
    outputs
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| {
            let path = file["path"].as_str()?.to_owned();
            let body = base64::engine::general_purpose::STANDARD
                .decode(file["data"].as_str()?)
                .ok()?;
            Some((path, body))
        })
        .collect()
}

impl PortableCompiler for Compiler<'_> {
    fn compile_docs(&self, request: &Value) -> impl Future<Output = Result<CompiledDocs>> + Send {
        let request = request.clone();
        let previews = self.0;
        async move {
            let built = previews.portable_docs(&request).await?;
            Ok(CompiledDocs {
                revision: built["revision"].as_str().unwrap_or_default().to_owned(),
                outputs: decoded(&built["outputs"]),
                stylesheet: built["stylesheet"].as_bool().unwrap_or(false),
                examples: serde_json::from_value::<Vec<ListedExample>>(built["examples"].clone())
                    .unwrap_or_default(),
                problems: built["problems"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            })
        }
    }
}

/// What the build is for: which pages, scenarios and sizes, and its name.
pub(crate) struct Request<'a> {
    pub(crate) root: &'a Path,
    pub(crate) product: &'a Path,
    pub(crate) index: &'a IndexAnswer,
    /// The configuration of the root's workbench, for docs pages and sizes.
    pub(crate) config: Result<Option<Value>, String>,
    pub(crate) name: String,
    pub(crate) selection: Option<&'a Selection>,
}

fn selected(selection: Option<&Selection>, page: &str) -> bool {
    selection.is_none_or(|selection| selection.pages.iter().any(|wanted| wanted == page))
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// One preview's entry for the viewer, from its definition and build.
fn preview_entry(
    preview: &Value,
    built: &Value,
    root: &Path,
    selection: Option<&Selection>,
) -> Value {
    let mut entry = preview.clone();
    if let Some(selection) = selection.filter(|selection| !selection.scenarios.is_empty())
        && let Some(scenarios) = entry["scenarios"].as_array_mut()
    {
        scenarios.retain(|scenario| {
            selection
                .scenarios
                .iter()
                .any(|wanted| scenario["id"] == wanted.as_str())
        });
    }
    let note = text(&built["portableNote"]);
    if !note.is_empty() {
        entry["controls"] = json!({});
        entry["docs"] = json!(format!(
            "{}\n{note}",
            preview["docs"].as_str().unwrap_or_default()
        ));
    }
    let slug = text(&built["slug"]);
    entry["directory"] = json!(format!("browser/{slug}"));
    entry["files"] = json!(
        built["localFiles"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|file| root.join(file).to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    );
    entry["packages"] = built["packages"].clone();
    entry["revision"] = built["revision"].clone();
    entry
}

/// Each preview with its sizes as the space resolves them, so the viewer
/// needs no space; only the selected sizes when there is a selection.
fn sized(
    entries: &mut [Value],
    config: Option<&Value>,
    selection: Option<&Selection>,
    warnings: &mut Vec<String>,
) {
    let space_sizes = config
        .and_then(|config| config["sizes"].as_array().cloned())
        .unwrap_or_else(default_sizes);
    for entry in entries {
        let file = text(&entry["file"]);
        let page = crate::config::page_sizes(entry.get("sizes"), &space_sizes, &file, warnings);
        let keys = page
            .as_ref()
            .and_then(|page| page["sizes"].as_array())
            .map(Vec::as_slice);
        let own = page
            .as_ref()
            .and_then(|page| page["ownSizes"].as_array())
            .map_or(&[][..], Vec::as_slice);
        let mut sizes = supported(&space_sizes, keys, own);
        if let Some(selection) = selection.filter(|selection| !selection.sizes.is_empty()) {
            sizes.retain(|size| {
                selection
                    .sizes
                    .iter()
                    .any(|wanted| size["key"] == wanted.as_str())
            });
        }
        entry["sizes"] = Value::Array(sizes);
    }
}

/// The docs pages to build: those the configuration declares and those
/// definitions declare, within the selection.
fn docs_entries(
    index: &IndexAnswer,
    config: Option<&Value>,
    selection: Option<&Selection>,
) -> Vec<DocsPageEntry> {
    let discovered: Vec<DocsPageEntry> = index
        .docs
        .iter()
        .filter_map(|page| {
            let title = text(&page["title"]);
            let label = title.split('/').rfind(|part| !part.is_empty()).map_or_else(|| text(&page["id"]), str::to_owned);
            serde_json::from_value(json!({ "src": page["src"], "page": page["src"], "label": label, "lens": page["lens"], "lenses": page["lenses"] })).ok()
        })
        .collect();
    docs_pages(config.unwrap_or(&Value::Null), &discovered)
        .into_iter()
        .filter(|page| {
            selected(
                selection,
                if page.page.is_empty() {
                    &page.src
                } else {
                    &page.page
                },
            )
        })
        .collect()
}

/// The viewer, the shared preview controls and the licence.
fn viewer_files(product: &Path, add: &mut impl FnMut(String, Vec<u8>)) -> Result<()> {
    for (source, name) in [
        ("ui/src/app/viewer/index.html", "index.html"),
        ("ui/src/app/viewer/viewer.css", "viewer.css"),
        ("ui/src/app/viewer/viewer.js", "ui/src/app/viewer/viewer.js"),
        (
            "ui/src/modules/preview-controls/index.js",
            "ui/src/modules/preview-controls/index.js",
        ),
        (
            "ui/src/views/top-bar/preview-bindings.js",
            "ui/src/views/top-bar/preview-bindings.js",
        ),
        (
            "ui/src/components/preview-controls/element.js",
            "ui/src/components/preview-controls/element.js",
        ),
        (
            "ui/src/components/preview-controls/styles.js",
            "ui/src/components/preview-controls/styles.js",
        ),
        (
            "ui/src/components/preview-controls/register.js",
            "ui/src/components/preview-controls/register.js",
        ),
        (
            "node_modules/@canonic2/lattice/src/index.js",
            "node_modules/@canonic2/lattice/src/index.js",
        ),
        (
            "node_modules/@canonic2/lattice/src/report.js",
            "node_modules/@canonic2/lattice/src/report.js",
        ),
        (
            "node_modules/@canonic2/lattice/src/modules/store/store.js",
            "node_modules/@canonic2/lattice/src/modules/store/store.js",
        ),
        (
            "node_modules/@canonic2/lattice/src/modules/store/shallow.js",
            "node_modules/@canonic2/lattice/src/modules/store/shallow.js",
        ),
        (
            "node_modules/@canonic2/lattice/src/modules/elements/elements.js",
            "node_modules/@canonic2/lattice/src/modules/elements/elements.js",
        ),
    ] {
        let path = product.join(source);
        add(
            name.to_owned(),
            std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?,
        );
    }
    let license = product.join("LICENSE");
    add(
        "CANONIC-LICENSE.txt".into(),
        std::fs::read(&license).with_context(|| format!("reading {}", license.display()))?,
    );
    Ok(())
}

/// Builds the browser folder's files, names relative to the export root.
///
/// # Errors
/// Fails only when Workbench's own viewer files can't be read.
pub(crate) async fn build(
    previews: &Arc<Previews>,
    request: Request<'_>,
) -> Result<PortableExport> {
    let mut files = Vec::new();
    let mut add = |name: String, body: Vec<u8>| {
        files.push(crate::export::PortableFile {
            path: format!("browser/{name}"),
            data: base64::engine::general_purpose::STANDARD.encode(body),
        });
    };
    let mut warnings = request.index.errors.clone();
    let mut entries: Vec<Value> = Vec::new();
    for preview in &request.index.previews {
        let file = text(&preview["file"]);
        if !selected(request.selection, &file) {
            continue;
        }
        match previews.portable_preview(&file).await {
            Ok(built) => {
                let slug = text(&built["slug"]);
                for (name, body) in decoded(&built["outputs"]) {
                    add(format!("{slug}/{name}"), body);
                }
                entries.push(preview_entry(
                    preview,
                    &built,
                    request.root,
                    request.selection,
                ));
            }
            Err(error) => warnings.push(format!("{file}: {error:#}")),
        }
    }
    let config = match request.config {
        Ok(config) => config,
        Err(error) => {
            warnings.push(format!("workbench.yaml: {error}"));
            None
        }
    };
    sized(
        &mut entries,
        config.as_ref(),
        request.selection,
        &mut warnings,
    );
    let pages = docs_entries(request.index, config.as_ref(), request.selection);
    let page_files = request.product.join("ui/src/views/docs");
    let docs = portable_docs(
        request.root,
        &page_files,
        &Compiler(previews),
        &pages,
        &mut add,
    )
    .await?;
    warnings.extend(docs.warnings.iter().cloned());
    viewer_files(request.product, &mut add)?;
    let listed: Vec<Value> = entries
        .iter()
        .map(|entry| {
            let mut entry = entry.as_object().cloned().unwrap_or_default();
            entry.remove("files");
            Value::Object(entry)
        })
        .collect();
    let docs_pages = serde_json::to_value(&docs.pages)?;
    let mut manifest = Map::new();
    manifest.insert("version".into(), json!(1));
    manifest.insert("name".into(), json!(request.name));
    manifest.insert("previews".into(), Value::Array(listed));
    manifest.insert("docs".into(), docs_pages.clone());
    manifest.insert("warnings".into(), json!(warnings));
    add(
        "workbench.json".into(),
        serde_json::to_string_pretty(&Value::Object(manifest))?.into_bytes(),
    );
    let previews: Vec<PortablePreview> = entries
        .iter()
        .filter_map(|entry| serde_json::from_value(entry.clone()).ok())
        .collect();
    Ok(PortableExport {
        docs: docs_pages.as_array().cloned().unwrap_or_default(),
        files,
        previews,
        warnings,
    })
}
