//! The export's records: the resolved space it reads, the references it
//! captures, the portable previews it receives and the report and archives
//! it produces. The report serializes as `canonic-export.json`.
use super::request::ExportRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

/// One implementation as the source export reads it.
#[derive(Debug, Clone, Default)]
pub(crate) struct ExportImplementation {
    pub(crate) root: Option<PathBuf>,
    pub(crate) kind: Option<String>,
}

/// One place a page's implementation lives.
#[derive(Debug, Clone)]
pub(crate) struct CodeEntry {
    pub(crate) path: Option<PathBuf>,
    pub(crate) exists: bool,
    /// The implementation key; `null` in the config for a docs page's Markdown.
    pub(crate) implementation: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ExportPage {
    pub(crate) label: String,
    pub(crate) design: Option<PathBuf>,
    pub(crate) code: Vec<CodeEntry>,
}

/// The resolved-space subset the source export reads, in the config's order.
#[derive(Debug, Clone, Default)]
pub(crate) struct ExportView {
    pub(crate) name: Option<String>,
    pub(crate) implementations: Vec<(String, ExportImplementation)>,
    pub(crate) pages: Vec<(String, ExportPage)>,
}

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

impl ExportView {
    /// Reads the parts of a `/_workbench/config` answer the export uses.
    pub(crate) fn from_config(view: &Value) -> Self {
        let implementations = view["implementations"]
            .as_object()
            .map(|all| {
                all.iter()
                    .map(|(key, implementation)| {
                        (
                            key.clone(),
                            ExportImplementation {
                                root: text(&implementation["root"]).map(PathBuf::from),
                                kind: text(&implementation["kind"]),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let pages = view["pages"]
            .as_object()
            .map(|all| {
                all.iter()
                    .map(|(src, page)| {
                        let code = page["code"]
                            .as_array()
                            .map(|entries| {
                                entries
                                    .iter()
                                    .map(|entry| CodeEntry {
                                        path: text(&entry["path"]).map(PathBuf::from),
                                        exists: entry["exists"].as_bool().unwrap_or(false),
                                        implementation: entry["implementation"]
                                            .as_str()
                                            .map(str::to_owned),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        (
                            src.clone(),
                            ExportPage {
                                label: page["label"].as_str().unwrap_or_default().to_owned(),
                                design: text(&page["design"]).map(PathBuf::from),
                                code,
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            name: text(&view["name"]),
            implementations,
            pages,
        }
    }
}

/// One reference screenshot the export plans: a page in a state at a size.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Reference {
    pub(crate) page: String,
    pub(crate) state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) artboard: Option<String>,
    pub(crate) variant: String,
    pub(crate) label: String,
    pub(crate) size: String,
    pub(crate) size_label: String,
    pub(crate) url: String,
    pub(crate) external: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) docs_page: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) selector: Option<String>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// A captured reference, ready for an archive, a PDF or an image folder.
#[derive(Debug, Clone, Default)]
pub(crate) struct Screenshot {
    pub(crate) page: String,
    pub(crate) state: String,
    pub(crate) variant: String,
    pub(crate) label: String,
    pub(crate) size: String,
    pub(crate) size_label: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) body: Vec<u8>,
}

impl Screenshot {
    pub(crate) fn of(reference: &Reference, body: Vec<u8>) -> Self {
        Self {
            page: reference.page.clone(),
            state: reference.state.clone(),
            variant: reference.variant.clone(),
            label: reference.label.clone(),
            size: reference.size.clone(),
            size_label: reference.size_label.clone(),
            width: reference.width,
            height: reference.height,
            body,
        }
    }
}

/// A preview the native compiler built for the portable viewer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PortablePreview {
    pub(crate) id: String,
    pub(crate) file: String,
    pub(crate) directory: String,
    pub(crate) states: Vec<Value>,
    /// The local files the preview was built from, absolute.
    pub(crate) files: Vec<String>,
    pub(crate) packages: Vec<PortablePackage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PortablePackage {
    pub(crate) name: String,
    pub(crate) version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PortableFile {
    pub(crate) path: String,
    /// The file's bytes in base64.
    pub(crate) data: String,
}

/// The portable viewer's files and previews.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PortableExport {
    pub(crate) docs: Vec<Value>,
    pub(crate) files: Vec<PortableFile>,
    pub(crate) previews: Vec<PortablePreview>,
    pub(crate) warnings: Vec<String>,
}

/// What the source export is given besides the space.
#[derive(Debug, Clone, Default)]
pub(crate) struct ExportOptions {
    pub(crate) request: Option<ExportRequest>,
    /// Build warnings recorded alongside unresolved source references.
    pub(crate) warnings: Vec<String>,
    /// The space's `workbench.yaml`; the root's when not given.
    pub(crate) manifest: Option<PathBuf>,
    pub(crate) portable: Option<PortableExport>,
    pub(crate) screenshots: Vec<Screenshot>,
    pub(crate) capture_warnings: Vec<String>,
    pub(crate) max_archive_bytes: Option<usize>,
    /// Workbench's own folder, whose `sdk/api.d.ts` a portable export ships.
    pub(crate) product: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct PageEntry {
    pub(crate) kind: &'static str,
    pub(crate) path: String,
    /// A source entry's implementation key, `null` when it has none; absent for a design.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) implementation: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageScreenshot {
    pub(crate) state: Option<String>,
    pub(crate) label: String,
    pub(crate) size: String,
    pub(crate) path: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) size_label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PageReport {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) hash: String,
    pub(crate) readme: String,
    pub(crate) entries: Vec<PageEntry>,
    pub(crate) files: Vec<String>,
    pub(crate) screenshots: Vec<PageScreenshot>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct Dependency {
    pub(crate) name: String,
    pub(crate) version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BrowserPreview {
    pub(crate) id: String,
    pub(crate) source: String,
    pub(crate) entry: String,
    pub(crate) states: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BrowserReport {
    pub(crate) entry: String,
    pub(crate) manifest: String,
    pub(crate) previews: Vec<BrowserPreview>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Part {
    pub(crate) number: usize,
    pub(crate) filename: String,
    pub(crate) files: usize,
}

/// `canonic-export.json`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) request: Option<ExportRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    pub(crate) generated_at: String,
    pub(crate) selection: String,
    pub(crate) files: Vec<String>,
    pub(crate) pages: Vec<PageReport>,
    pub(crate) dependencies: Vec<Dependency>,
    pub(crate) warnings: Vec<String>,
    pub(crate) capture_warnings: Vec<String>,
    pub(crate) browser: Option<BrowserReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parts: Option<Vec<Part>>,
}

impl ExportReport {
    /// Every warning a finished export reports, in report order.
    pub(crate) fn all_warnings(&self) -> Vec<String> {
        let mut out = self.warnings.clone();
        out.extend(self.capture_warnings.iter().cloned());
        if let Some(browser) = &self.browser {
            out.extend(browser.warnings.iter().cloned());
        }
        out
    }
}

/// One file in an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ZipEntry {
    pub(crate) name: String,
    pub(crate) body: Vec<u8>,
}

impl ZipEntry {
    pub(crate) fn new(name: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            body: body.into(),
        }
    }
}

/// A file the export answers with: a ZIP, a part of one, or a PDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Archive {
    pub(crate) filename: String,
    pub(crate) body: Vec<u8>,
    pub(crate) content_type: &'static str,
    /// The payload files a ZIP part holds, relative to its folder.
    pub(crate) files: Vec<String>,
}

/// A finished export: its archives, the one file a download gets, and its report.
#[derive(Debug, Clone)]
pub(crate) struct ExportResult {
    pub(crate) archives: Vec<Archive>,
    /// The outer ZIP of the parts; `None` when the only archive is the download.
    pub(crate) bundle: Option<Archive>,
    pub(crate) report: ExportReport,
}

impl ExportResult {
    /// The file a download without `part` gets.
    pub(crate) fn download(&self) -> &Archive {
        self.bundle.as_ref().unwrap_or(&self.archives[0])
    }
}
