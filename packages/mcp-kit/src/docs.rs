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
//! use canonic_mcp_kit::docs::{Bundle, Documentation};
//! use canonic_mcp_kit::Catalog;
//!
//! // A real server embeds its build output: `static GUIDES: Bundle = canonic_mcp_kit::include_docs!();`
//! static GUIDES: Bundle = Bundle::new(b"");
//! let docs = Documentation::new(&GUIDES, "Acme", "1.0.0");
//! let catalog = Catalog::new(docs.tools())?;
//! assert!(catalog.get("docs_search").is_some());
//! assert!(docs.handles("docs_read"));
//! # Ok::<(), canonic_mcp_kit::CatalogError>(())
//! ```

#[cfg(feature = "docs-build")]
pub mod build;
#[cfg(all(test, not(feature = "docs-build")))]
mod build;
mod format;
mod presentation;
pub use presentation::human;
#[cfg(test)]
mod tests;

use crate::{
    control::Interrupted,
    page::{self, Builder, Casing, Limits, Position, Request, Scope},
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
pub const MAX_ROWS: u64 = Limits::STANDARD.max;
/// The default rows per list or search page.
pub const DEFAULT_ROWS: u64 = Limits::STANDARD.default;
/// The most lines one read returns.
pub const MAX_READ_LINES: u64 = 400;
/// The default lines per read.
pub const DEFAULT_READ_LINES: u64 = 100;
/// The default encoded items-array budget; a page ends early before the next
/// item exceeds it, except for one oversized first item within transport bounds.
pub const MAX_RESULT_TEXT_BYTES: usize = Limits::STANDARD.max_bytes;

const READ_LIMITS: Limits = Limits {
    default: DEFAULT_READ_LINES,
    max: MAX_READ_LINES,
    max_bytes: MAX_RESULT_TEXT_BYTES,
};

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
    /// Shared page request or cursor failure.
    #[error(transparent)]
    Pagination(#[from] page::Error),
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
    lowered_lines: OnceLock<Vec<Vec<usize>>>,
    lines: OnceLock<Vec<Vec<usize>>>,
    digest: OnceLock<[u8; 32]>,
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
            lowered_lines: OnceLock::new(),
            lines: OnceLock::new(),
            digest: OnceLock::new(),
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

    /// Sparse byte starts bound resume scans to at most one block of lines.
    fn lines(&self) -> Result<&[Vec<usize>], DocsError> {
        let (index, _) = self.index()?;
        let text = self.text()?;
        Ok(self.lines.get_or_init(|| {
            index
                .pages
                .iter()
                .map(|page| line_starts(&text[page.start..page.end]))
                .collect()
        }))
    }

    fn lowered_lines(&self) -> Result<&[Vec<usize>], DocsError> {
        let text = self.lowered()?;
        Ok(self
            .lowered_lines
            .get_or_init(|| text.iter().map(|page| line_starts(page)).collect()))
    }

    fn scope(&self, list: &str, query: &Value) -> Scope {
        Scope::offset(
            list,
            page::digest(query.to_string().as_bytes()),
            *self.digest.get_or_init(|| page::digest(self.bytes)),
        )
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
    /// Tools named `docs_list`, `docs_search` and `docs_read`, describing
    /// `product`'s guides and reporting `version` in every result.
    ///
    /// The MCP server supplies the namespace; tool names need no product prefix.
    #[must_use]
    pub fn new(bundle: &'static Bundle, product: &str, version: &str) -> Self {
        Self {
            bundle,
            product: product.to_owned(),
            version: version.to_owned(),
            list: "docs_list".into(),
            search: "docs_search".into(),
            read: "docs_read".into(),
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
        let properties = Limits::STANDARD.properties();
        let cursor = &properties["cursor"];
        let rows = &properties["limit"];
        let mut read_properties = READ_LIMITS.properties();
        read_properties["limit"]["description"] = json!(
            "Most lines to return; fewer are returned when more would exceed the result size."
        );
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
                object(properties.clone(), &[]),
            ),
            Tool::new(
                self.search.clone(),
                format!("Search the bundled {product} documentation for literal text and return matching lines."),
                object(json!({
                    "query":{"type":"string","minLength":1,"maxLength":MAX_QUERY_BYTES,
                        "description":"Case-insensitive literal text, not a pattern; surrounding spaces are kept. Must not be blank or contain line breaks."},
                    "path":{"type":"string","minLength":1,
                        "description":format!("Search only this exact page path from {list}.")},
                    "cursor":cursor,"limit":rows}), &["query"]),
            ),
            Tool::new(
                self.read.clone(),
                format!("Read a range of lines from a bundled {product} documentation page."),
                object(json!({
                    "path":{"type":"string","minLength":1,
                        "description":format!("Exact page path returned by {list}.")},
                    "cursor":cursor,
                    "limit":read_properties["limit"]}),
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
        input: &Value,
        check: &dyn Fn() -> Result<(), Interrupted>,
    ) -> Result<Value, DocsError> {
        let arguments = Arguments::new(input)?;
        match kind {
            Kind::List => {
                arguments.only(&["cursor", "limit"])?;
                let request = Request::from_mcp(input, Limits::STANDARD)?;
                check()?;
                self.list(&request, check)
            }
            Kind::Search => {
                arguments.only(&["query", "path", "cursor", "limit"])?;
                let query = arguments.query()?;
                let path = arguments.string("path")?;
                let request = Request::from_mcp(input, Limits::STANDARD)?;
                check()?;
                self.search(&query, path, &request, check)
            }
            Kind::Read => {
                arguments.only(&["path", "cursor", "limit"])?;
                let path = arguments
                    .string("path")?
                    .ok_or_else(|| invalid("`path` is required."))?;
                let request = Request::from_mcp_units(input, READ_LIMITS)?;
                check()?;
                self.read(path, &request, check)
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
        request: &Request,
        check: &dyn Fn() -> Result<(), Interrupted>,
    ) -> Result<Value, DocsError> {
        let (index, _) = self.bundle.index()?;
        let scope = self
            .bundle
            .scope(&format!("{}:{}", self.product, self.list), &json!({}));
        let offset = offset(&scope, request)?;
        if offset > index.pages.len() {
            return Err(page::Error::InvalidCursor.into());
        }
        let mut builder = Builder::new(
            &scope,
            request.limit,
            Limits::STANDARD,
            1024 * 1024,
            &self.read,
        )?;
        let mut next = offset;
        for page in &index.pages[offset..] {
            check()?;
            if !builder.push(
                json!({"path":page.path,"title":page.title,"total_lines":page.lines}),
                &Position::Offset((next + 1) as u64),
            )? {
                break;
            }
            next += 1;
            if builder.is_full() {
                break;
            }
        }
        let mut result = self.rows("documents", builder.finish(next < index.pages.len()));
        result["total"] = json!(index.pages.len());
        Ok(result)
    }

    fn search(
        &self,
        query: &str,
        path: Option<&str>,
        request: &Request,
        check: &dyn Fn() -> Result<(), Interrupted>,
    ) -> Result<Value, DocsError> {
        let (index, _) = self.bundle.index()?;
        let selected = path.map(|path| self.page(index, path)).transpose()?;
        let scope = self.bundle.scope(
            &format!("{}:{}", self.product, self.search),
            &json!({"query":query,"path":path}),
        );
        let offset = offset(&scope, request)?;
        let total_lines = index.pages.iter().map(|page| page.lines).sum::<usize>();
        if offset > total_lines {
            return Err(page::Error::InvalidCursor.into());
        }
        let text = self.bundle.text()?;
        let lowered = self.bundle.lowered()?;
        let needle = query.to_lowercase();
        let starts = self.bundle.lines()?;
        let lowered_starts = self.bundle.lowered_lines()?;
        let mut builder = Builder::new(
            &scope,
            request.limit,
            Limits::STANDARD,
            1024 * 1024,
            &self.read,
        )?;
        let mut base = 0;
        for (position, page) in index.pages.iter().enumerate() {
            let page_base = base;
            base += page.lines;
            if base <= offset || selected.is_some_and(|(selected, _)| selected != position) {
                continue;
            }
            check()?;
            let first = offset.saturating_sub(page_base);
            let lines = lines_from(&text[page.start..page.end], &starts[position], first);
            let lower = lines_from(&lowered[position], &lowered_starts[position], first);
            for (relative, (line, lower)) in lines.zip(lower).enumerate() {
                let number = first + relative;
                if number % CHECK_INTERVAL == 0 {
                    check()?;
                }
                if !lower.contains(&needle) {
                    continue;
                }
                let next = page_base + number + 1;
                let line = line.lines().next().unwrap_or("");
                if !builder.push(
                    json!({"path":page.path,"line":number + 1,"text":line}),
                    &Position::Offset(next as u64),
                )? || builder.is_full()
                {
                    let more = if selected.is_some() {
                        number + 1 < page.lines
                    } else {
                        next < total_lines
                    };
                    return Ok(self.rows("matches", builder.finish(more)));
                }
            }
        }
        Ok(self.rows("matches", builder.finish(false)))
    }

    fn read(
        &self,
        path: &str,
        request: &Request,
        check: &dyn Fn() -> Result<(), Interrupted>,
    ) -> Result<Value, DocsError> {
        let (index, _) = self.bundle.index()?;
        let (page_index, page) = self.page(index, path)?;
        let text = self.bundle.text()?;
        let scope = self.bundle.scope(
            &format!("{}:{}", self.product, self.read),
            &json!({"path":path}),
        );
        let first = offset(&scope, request)?;
        let lines = &self.bundle.lines()?[page_index];
        if first > page.lines {
            return Err(page::Error::InvalidCursor.into());
        }
        let mut content = String::new();
        let mut taken = 0usize;
        let mut builder =
            Builder::new(&scope, request.limit, READ_LIMITS, 1024 * 1024, &self.read)?;
        for (relative, line) in lines_from(&text[page.start..page.end], lines, first).enumerate() {
            let number = first + relative;
            if (number - first) % CHECK_INTERVAL == 0 {
                check()?;
            }
            if !builder.push(json!(line), &Position::Offset((number + 1) as u64))? {
                break;
            }
            content.push_str(line);
            taken += 1;
            if builder.is_full() {
                break;
            }
        }
        let next = first.saturating_add(taken);
        Ok(json!({
            "version": self.version,
            "path": page.path,
            "title": page.title,
            "start_line": first + 1,
            "total_lines": page.lines,
            "content": content,
            "next_cursor": builder.finish(next < page.lines).next_cursor,
        }))
    }

    fn rows(&self, field: &str, page: page::Page) -> Value {
        let mut result = Map::new();
        result.insert("version".into(), json!(self.version));
        result.insert(field.into(), Value::Array(page.items));
        result.insert(Casing::Snake.next_cursor().into(), json!(page.next_cursor));
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

/// One seek checkpoint per scan block, rather than an allocation per line.
fn line_starts(text: &str) -> Vec<usize> {
    let mut offset = 0;
    text.split_inclusive('\n')
        .enumerate()
        .filter_map(|(number, line)| {
            let start = offset;
            offset += line.len();
            number.is_multiple_of(CHECK_INTERVAL).then_some(start)
        })
        .collect()
}

fn lines_from<'a>(text: &'a str, starts: &[usize], first: usize) -> impl Iterator<Item = &'a str> {
    let start = starts
        .get(first / CHECK_INTERVAL)
        .copied()
        .unwrap_or(text.len());
    text[start..]
        .split_inclusive('\n')
        .skip(first % CHECK_INTERVAL)
}

fn offset(scope: &Scope, request: &Request) -> Result<usize, DocsError> {
    match request
        .cursor
        .as_deref()
        .map(|cursor| scope.decode(cursor))
        .transpose()?
    {
        None => Ok(0),
        Some(Position::Offset(value)) => {
            usize::try_from(value).map_err(|_| page::Error::InvalidCursor.into())
        }
        Some(Position::Keyset(_)) => Err(page::Error::InvalidCursor.into()),
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
