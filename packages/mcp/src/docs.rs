//! Offline product guides embedded at build time, and the documentation tools
//! that list, search and read them.
//!
//! A consumer's build script writes a bundle with the `docs-build` feature's
//! [`build`] helper (not shown in builds without that feature). The executable
//! embeds it in a `static` [`Bundle`], usually through [`include_docs!`](crate::include_docs!), and
//! publishes the three [`Documentation::tools`]. Construction does nothing:
//! the bundle prepares itself in stages, each at most once per process, on the
//! first documentation call that needs it. Listing reads only the page index,
//! the first search or read decompresses the text, and the first search
//! lowercases it for matching. Calls to other tools never prepare anything.
//!
//! ```
//! use canonic_mcp::docs::{Bundle, Documentation};
//! use canonic_mcp::Catalog;
//!
//! // A real server embeds its build output: `static GUIDES: Bundle = canonic_mcp::include_docs!();`
//! static GUIDES: Bundle = Bundle::new(b"");
//! let docs = Documentation::new(&GUIDES, "acme", "Acme", "1.0.0");
//! let catalog = Catalog::new(docs.tools())?;
//! assert!(catalog.get("acme_docs_search").is_some());
//! assert!(docs.handles("acme_docs_read"));
//! # Ok::<(), canonic_mcp::CatalogError>(())
//! ```

#[cfg(feature = "docs-build")]
pub mod build;
#[cfg(all(test, not(feature = "docs-build")))]
mod build;
mod format;
#[cfg(test)]
mod tests;

use crate::{
    control::Interrupted,
    tool::{Annotations, Content, Tool, ToolResult},
};
use format::{Index, MAGIC, Page};
use miniz_oxide::inflate::decompress_to_vec_with_limit;
use serde_json::{Map, Value, json};
use std::sync::OnceLock;

/// The bundle's file name, as a literal for [`crate::include_docs!`] and [`FILE_NAME`].
#[doc(hidden)]
#[macro_export]
macro_rules! __docs_file_name {
    () => {
        "canonic-mcp-docs.bin"
    };
}

/// The bundle's file name in the build script's output folder.
pub const FILE_NAME: &str = crate::__docs_file_name!();
/// The most pages a bundle can hold.
pub const MAX_PAGES: usize = 1024;
/// The longest page path, in UTF-8 bytes.
pub const MAX_PATH_BYTES: usize = 512;
/// The largest page, in bytes.
pub const MAX_PAGE_BYTES: usize = 1024 * 1024;
/// The longest line, in bytes, excluding its terminator.
pub const MAX_LINE_BYTES: usize = 64 * 1024;
/// The largest total uncompressed text, in bytes.
pub const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
/// The longest search query, in UTF-8 bytes.
pub const MAX_QUERY_BYTES: usize = 1024;
/// The most rows a list or search page returns.
pub const MAX_ROWS: u64 = 100;
/// The default rows per list or search page.
pub const DEFAULT_ROWS: u64 = 50;
/// The most lines one read returns.
pub const MAX_READ_LINES: u64 = 400;
/// The default lines per read.
pub const DEFAULT_READ_LINES: u64 = 100;
/// The largest offset or line number, the largest integer JSON represents exactly.
pub const MAX_POSITION: u64 = (1 << 53) - 1;
/// The most text one result carries, in bytes once escaped for a JSON string;
/// a page ends early rather than exceed it.
pub const MAX_RESULT_TEXT_BYTES: usize = 512 * 1024;

/// How many lines a scan processes between interruption checks.
const CHECK_INTERVAL: usize = 256;

/// Embeds the bundle that [`build::embed`] wrote to the build script's output folder.
///
/// Expands to a constant [`Bundle`] expression for a `static`.
#[macro_export]
macro_rules! include_docs {
    () => {
        $crate::docs::Bundle::new(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/",
            $crate::__docs_file_name!()
        )))
    };
}

/// Why a documentation call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DocsError {
    /// An argument was missing, of the wrong type, out of range or unknown.
    #[error("{0}")]
    InvalidArguments(String),
    /// The path is not exactly a bundled page path.
    #[error("Unknown documentation page; call {list_tool}.")]
    NotFound {
        /// The consumer's list tool, which returns the valid paths.
        list_tool: String,
    },
    /// The request was cancelled or reached its deadline.
    #[error("Documentation request cancelled or timed out.")]
    Cancelled,
    /// The embedded bundle is missing or corrupt.
    #[error("Bundled documentation is unavailable in this build.")]
    Unavailable,
}

impl From<Interrupted> for DocsError {
    fn from(_: Interrupted) -> Self {
        Self::Cancelled
    }
}

/// Embedded bundle bytes and their state, prepared lazily and at most once.
///
/// Hold it in a `static`: its prepared state then lasts for the process and
/// every documentation call shares it. A failed stage is kept too, because
/// the embedded bytes cannot change.
#[derive(Debug)]
pub struct Bundle {
    bytes: &'static [u8],
    index: OnceLock<Option<(Index, usize)>>,
    text: OnceLock<Option<String>>,
    lowered: OnceLock<Vec<String>>,
    #[cfg(test)]
    stages: tests::Stages,
}

impl Bundle {
    /// Wraps embedded bundle bytes without reading them.
    #[must_use]
    pub const fn new(bytes: &'static [u8]) -> Self {
        Self {
            bytes,
            index: OnceLock::new(),
            text: OnceLock::new(),
            lowered: OnceLock::new(),
            #[cfg(test)]
            stages: tests::Stages::new(),
        }
    }

    /// The validated page index and where the compressed text starts.
    fn index(&self) -> Result<&(Index, usize), DocsError> {
        self.index
            .get_or_init(|| {
                #[cfg(test)]
                self.stages.index();
                parse_index(self.bytes)
            })
            .as_ref()
            .ok_or(DocsError::Unavailable)
    }

    /// The decompressed text of every page, checked against the index.
    fn text(&self) -> Result<&str, DocsError> {
        let (index, data) = self.index()?;
        self.text
            .get_or_init(|| {
                #[cfg(test)]
                self.stages.text();
                decode_text(&self.bytes[*data..], index)
            })
            .as_deref()
            .ok_or(DocsError::Unavailable)
    }

    /// Each page's lowercased text, for case-insensitive matching.
    fn lowered(&self) -> Result<&[String], DocsError> {
        let (index, _) = self.index()?;
        let text = self.text()?;
        Ok(self.lowered.get_or_init(|| {
            #[cfg(test)]
            self.stages.lowered();
            index
                .pages
                .iter()
                .map(|page| text[page.start..page.end].to_lowercase())
                .collect()
        }))
    }
}

fn parse_index(bytes: &[u8]) -> Option<(Index, usize)> {
    let rest = bytes.strip_prefix(MAGIC)?;
    let length = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?);
    let length = usize::try_from(length).ok()?;
    let start = MAGIC.len() + 4;
    let index: Index =
        serde_json::from_slice(bytes.get(start..start.checked_add(length)?)?).ok()?;
    valid_index(&index).then_some((index, start + length))
}

fn valid_index(index: &Index) -> bool {
    let pages = &index.pages;
    if pages.is_empty() || pages.len() > MAX_PAGES || index.size > MAX_TOTAL_BYTES {
        return false;
    }
    let mut next = 0;
    for (position, page) in pages.iter().enumerate() {
        let ordered = position == 0 || pages[position - 1].path.as_bytes() < page.path.as_bytes();
        if !ordered
            || page.path.is_empty()
            || page.path.len() > MAX_PATH_BYTES
            || page.start != next
            || page.end < page.start
            || page.end - page.start > MAX_PAGE_BYTES
        {
            return false;
        }
        next = page.end;
    }
    next == index.size
}

fn decode_text(compressed: &[u8], index: &Index) -> Option<String> {
    let bytes = decompress_to_vec_with_limit(compressed, index.size).ok()?;
    if bytes.len() != index.size {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    index
        .pages
        .iter()
        .all(|page| {
            text.get(page.start..page.end).is_some_and(|content| {
                format::line_count(content) == page.lines && format::title(content) == page.title
            })
        })
        .then_some(text)
}

/// A consumer's three documentation tools over one bundle.
#[derive(Debug, Clone)]
pub struct Documentation {
    bundle: &'static Bundle,
    product: String,
    version: String,
    list: String,
    search: String,
    read: String,
}

#[derive(Clone, Copy)]
enum Kind {
    List,
    Search,
    Read,
}

impl Documentation {
    /// Tools named `<prefix>_docs_list`, `_search` and `_read`, describing
    /// `product`'s guides and reporting `version` in every result.
    ///
    /// Tool names are checked when the tools are added to a [`crate::Catalog`].
    #[must_use]
    pub fn new(bundle: &'static Bundle, prefix: &str, product: &str, version: &str) -> Self {
        Self {
            bundle,
            product: product.to_owned(),
            version: version.to_owned(),
            list: format!("{prefix}_docs_list"),
            search: format!("{prefix}_docs_search"),
            read: format!("{prefix}_docs_read"),
        }
    }

    /// The list tool's name, which failures name for recovery.
    #[must_use]
    pub fn list_tool(&self) -> &str {
        &self.list
    }

    /// Whether `name` is one of these documentation tools.
    #[must_use]
    pub fn handles(&self, name: &str) -> bool {
        self.kind(name).is_some()
    }

    fn kind(&self, name: &str) -> Option<Kind> {
        if name == self.list {
            Some(Kind::List)
        } else if name == self.search {
            Some(Kind::Search)
        } else if name == self.read {
            Some(Kind::Read)
        } else {
            None
        }
    }

    /// The three tool definitions, for the consumer's catalog. Building them
    /// does not read the bundle.
    #[must_use]
    pub fn tools(&self) -> Vec<Tool> {
        let product = &self.product;
        let list = &self.list;
        let offset = json!({"type":"integer","minimum":0,"maximum":MAX_POSITION,"default":0,
            "description":"Zero-based position of the first row to return."});
        let rows = json!({"type":"integer","minimum":1,"maximum":MAX_ROWS,"default":DEFAULT_ROWS,
            "description":"Most rows to return; fewer are returned when more would exceed the result size."});
        let annotations = Annotations {
            title: None,
            read_only_hint: Some(true),
            destructive_hint: Some(false),
            idempotent_hint: Some(true),
            open_world_hint: Some(false),
        };
        let object = |properties: Value, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
        vec![
            Tool::new(
                list.clone(),
                format!("List the paths, titles and line counts of the {product} documentation bundled with this version."),
                object(json!({"offset":offset,"limit":rows}), &[]),
            ),
            Tool::new(
                self.search.clone(),
                format!("Search the bundled {product} documentation for literal text and return matching lines."),
                object(json!({
                    "query":{"type":"string","minLength":1,"maxLength":MAX_QUERY_BYTES,
                        "description":"Case-insensitive literal text, not a pattern; surrounding spaces are kept. Must not be blank or contain line breaks."},
                    "path":{"type":"string","minLength":1,
                        "description":format!("Search only this exact page path from {list}.")},
                    "offset":offset,"limit":rows}), &["query"]),
            ),
            Tool::new(
                self.read.clone(),
                format!("Read a range of lines from a bundled {product} documentation page."),
                object(json!({
                    "path":{"type":"string","minLength":1,
                        "description":format!("Exact page path returned by {list}.")},
                    "start_line":{"type":"integer","minimum":1,"maximum":MAX_POSITION,"default":1,
                        "description":"One-based first line."},
                    "limit":{"type":"integer","minimum":1,"maximum":MAX_READ_LINES,"default":DEFAULT_READ_LINES,
                        "description":"Most lines to return; fewer are returned when more would exceed the result size."}}),
                    &["path"]),
            ),
        ]
        .into_iter()
        .map(|tool| tool.with_annotations(annotations.clone()))
        .collect()
    }

    /// Runs a documentation call, or returns `None` when `name` is not one of
    /// these tools.
    ///
    /// `check` is polled before any work, between pages and periodically
    /// within one; once it fails the call returns [`DocsError::Cancelled`]
    /// without partial results. The arguments are validated before the bundle
    /// is prepared, so the same call also serves non-MCP callers.
    pub fn call(
        &self,
        name: &str,
        arguments: &Value,
        check: &dyn Fn() -> Result<(), Interrupted>,
    ) -> Option<Result<Value, DocsError>> {
        let kind = self.kind(name)?;
        Some(self.run(kind, arguments, check))
    }

    fn run(
        &self,
        kind: Kind,
        arguments: &Value,
        check: &dyn Fn() -> Result<(), Interrupted>,
    ) -> Result<Value, DocsError> {
        let arguments = Arguments::new(arguments)?;
        match kind {
            Kind::List => {
                arguments.only(&["offset", "limit"])?;
                let offset = arguments.integer("offset", 0, MAX_POSITION, 0)?;
                let limit = arguments.integer("limit", 1, MAX_ROWS, DEFAULT_ROWS)?;
                check()?;
                self.list(offset, limit, check)
            }
            Kind::Search => {
                arguments.only(&["query", "path", "offset", "limit"])?;
                let query = arguments.query()?;
                let path = arguments.string("path")?;
                let offset = arguments.integer("offset", 0, MAX_POSITION, 0)?;
                let limit = arguments.integer("limit", 1, MAX_ROWS, DEFAULT_ROWS)?;
                check()?;
                self.search(&query, path, offset, limit, check)
            }
            Kind::Read => {
                arguments.only(&["path", "start_line", "limit"])?;
                let path = arguments
                    .string("path")?
                    .ok_or_else(|| invalid("`path` is required."))?;
                let start = arguments.integer("start_line", 1, MAX_POSITION, 1)?;
                let limit = arguments.integer("limit", 1, MAX_READ_LINES, DEFAULT_READ_LINES)?;
                check()?;
                self.read(path, start, limit, check)
            }
        }
    }

    fn page(&self, index: &'static Index, path: &str) -> Result<(usize, &'static Page), DocsError> {
        index
            .pages
            .binary_search_by(|page| page.path.as_bytes().cmp(path.as_bytes()))
            .map(|position| (position, &index.pages[position]))
            .map_err(|_| DocsError::NotFound {
                list_tool: self.list.clone(),
            })
    }

    fn list(
        &self,
        offset: u64,
        limit: u64,
        check: &dyn Fn() -> Result<(), Interrupted>,
    ) -> Result<Value, DocsError> {
        let (index, _) = self.bundle.index()?;
        let mut rows = Vec::new();
        let mut budget = Budget::default();
        for page in index
            .pages
            .iter()
            .skip(position(offset))
            .take(position(limit))
        {
            check()?;
            if !budget.fits(escaped(&page.path) + escaped(&page.title), rows.is_empty()) {
                break;
            }
            rows.push(json!({"path":page.path,"title":page.title,"total_lines":page.lines}));
        }
        Ok(self.rows("documents", rows, index.pages.len(), offset))
    }

    fn search(
        &self,
        query: &str,
        path: Option<&str>,
        offset: u64,
        limit: u64,
        check: &dyn Fn() -> Result<(), Interrupted>,
    ) -> Result<Value, DocsError> {
        let (index, _) = self.bundle.index()?;
        let selected = path.map(|path| self.page(index, path)).transpose()?;
        let text = self.bundle.text()?;
        let lowered = self.bundle.lowered()?;
        let needle = query.to_lowercase();
        let (offset, limit) = (position(offset), position(limit));
        let mut rows = Vec::new();
        let mut budget = Budget::default();
        let mut full = false;
        let mut total = 0usize;
        for (position, page) in index.pages.iter().enumerate() {
            if selected.is_some_and(|(selected, _)| selected != position) {
                continue;
            }
            check()?;
            let lines = text[page.start..page.end].lines();
            for (number, (line, lower)) in lines.zip(lowered[position].lines()).enumerate() {
                if number % CHECK_INTERVAL == 0 {
                    check()?;
                }
                if !lower.contains(&needle) {
                    continue;
                }
                if total >= offset && !full {
                    if rows.len() < limit
                        && budget.fits(escaped(&page.path) + escaped(line), rows.is_empty())
                    {
                        rows.push(json!({"path":page.path,"line":number + 1,"text":line}));
                    } else {
                        full = true;
                    }
                }
                total += 1;
            }
        }
        Ok(self.rows("matches", rows, total, offset as u64))
    }

    fn read(
        &self,
        path: &str,
        start: u64,
        limit: u64,
        check: &dyn Fn() -> Result<(), Interrupted>,
    ) -> Result<Value, DocsError> {
        let (index, _) = self.bundle.index()?;
        let (_, page) = self.page(index, path)?;
        let text = self.bundle.text()?;
        let first = position(start - 1);
        let mut content = String::new();
        let mut taken = 0usize;
        let mut budget = Budget::default();
        // Polling counts from the page's first line, so every read checks once
        // after preparation and periodically through skipped lines too.
        for (number, line) in text[page.start..page.end].split_inclusive('\n').enumerate() {
            if number % CHECK_INTERVAL == 0 {
                check()?;
            }
            if number < first {
                continue;
            }
            if taken == position(limit) || !budget.fits(escaped(line), taken == 0) {
                break;
            }
            content.push_str(line);
            taken += 1;
        }
        let next = first.saturating_add(taken);
        Ok(json!({
            "version": self.version,
            "path": page.path,
            "title": page.title,
            "start_line": start,
            "total_lines": page.lines,
            "content": content,
            "next_start_line": (taken > 0 && next < page.lines).then_some(next + 1),
        }))
    }

    fn rows(&self, field: &str, rows: Vec<Value>, total: usize, offset: u64) -> Value {
        let next = position(offset).saturating_add(rows.len());
        let mut result = Map::new();
        result.insert("version".into(), json!(self.version));
        result.insert(field.into(), Value::Array(rows));
        result.insert("total".into(), json!(total));
        result.insert("next_offset".into(), json!((next < total).then_some(next)));
        Value::Object(result)
    }

    /// The default tool result: structured content on success, otherwise an
    /// error result whose text says how to recover.
    #[must_use]
    pub fn result(outcome: Result<Value, DocsError>) -> ToolResult {
        match outcome {
            Ok(value) => ToolResult::structured(value, false),
            Err(error) => ToolResult::error(vec![Content::text(error.to_string())]),
        }
    }
}

/// Converts a bounded `u64` to `usize`, saturating on narrow platforms.
fn position(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// The bytes `text` takes inside a JSON string, as `serde_json` escapes it.
///
/// Budgeting escaped bytes bounds the encoded result: a control character
/// takes six bytes, and the default result's JSON text copy at most doubles
/// the structured content again.
fn escaped(text: &str) -> usize {
    text.bytes()
        .map(|byte| match byte {
            b'"' | b'\\' | b'\x08' | b'\t' | b'\n' | b'\x0c' | b'\r' => 2,
            0..=0x1f => 6,
            _ => 1,
        })
        .sum()
}

/// Escaped text accumulated into one result, bounded by [`MAX_RESULT_TEXT_BYTES`].
#[derive(Default)]
struct Budget(usize);

impl Budget {
    /// Whether `bytes` more fit; the first item of a result always fits.
    fn fits(&mut self, bytes: usize, first: bool) -> bool {
        let used = self.0.saturating_add(bytes);
        if first || used <= MAX_RESULT_TEXT_BYTES {
            self.0 = used;
            true
        } else {
            false
        }
    }
}

fn invalid(message: &str) -> DocsError {
    DocsError::InvalidArguments(format!("Invalid documentation arguments: {message}"))
}

/// Checked access to a call's argument object.
struct Arguments<'a>(&'a Map<String, Value>);

impl<'a> Arguments<'a> {
    fn new(arguments: &'a Value) -> Result<Self, DocsError> {
        arguments
            .as_object()
            .map(Self)
            .ok_or_else(|| invalid("arguments must be an object."))
    }

    fn only(&self, allowed: &[&str]) -> Result<(), DocsError> {
        match self.0.keys().find(|key| !allowed.contains(&key.as_str())) {
            Some(key) => Err(invalid(&format!("`{key}` is not a supported argument."))),
            None => Ok(()),
        }
    }

    fn integer(&self, key: &str, min: u64, max: u64, default: u64) -> Result<u64, DocsError> {
        match self.0.get(key) {
            None => Ok(default),
            Some(value) => value
                .as_u64()
                .filter(|value| (min..=max).contains(value))
                .ok_or_else(|| {
                    invalid(&format!("`{key}` must be an integer from {min} to {max}."))
                }),
        }
    }

    fn string(&self, key: &str) -> Result<Option<&'a str>, DocsError> {
        match self.0.get(key) {
            None => Ok(None),
            Some(Value::String(text)) if !text.is_empty() => Ok(Some(text)),
            Some(_) => Err(invalid(&format!("`{key}` must be a nonempty string."))),
        }
    }

    fn query(&self) -> Result<String, DocsError> {
        let query = self
            .string("query")?
            .ok_or_else(|| invalid("`query` is required."))?;
        if query.len() > MAX_QUERY_BYTES {
            return Err(invalid(&format!(
                "`query` must be at most {MAX_QUERY_BYTES} bytes."
            )));
        }
        if query.trim().is_empty() {
            return Err(invalid("`query` must not be blank."));
        }
        if query.contains(['\n', '\r']) {
            return Err(invalid("`query` must not contain line breaks."));
        }
        Ok(query.to_owned())
    }
}
