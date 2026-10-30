//! The documentation bundle's byte layout, shared by the build helper and the runtime.
//!
//! A bundle is a marker line, a little-endian `u32` index length, the JSON
//! index and the deflate-compressed text of every page in index order.

use serde::{Deserialize, Serialize};

/// The marker that starts every bundle; its suffix is the layout version.
pub(crate) const MAGIC: &[u8] = b"canonic-mcp-docs/1\n";

/// One page in a bundle's index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Page {
    /// The path relative to the guide folder, with `/` separators.
    pub(crate) path: String,
    /// The page's first byte in the decompressed text.
    pub(crate) start: usize,
    /// One past the page's last byte in the decompressed text.
    pub(crate) end: usize,
    /// The number of lines, each ending at `\n` or at the end of the page.
    pub(crate) lines: usize,
    /// The text after `# ` on the page's first line that starts with `# `.
    pub(crate) title: String,
}

/// A bundle's page index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Index {
    /// The decompressed text's length in bytes.
    pub(crate) size: usize,
    /// The pages in bundle order: bytewise ascending paths.
    pub(crate) pages: Vec<Page>,
}

/// A page's title: the text after `# ` on its first line starting with `# `.
pub(crate) fn title(content: &str) -> &str {
    content
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .unwrap_or_default()
}

/// The number of lines, each ending at `\n` or at the end of the text.
pub(crate) fn line_count(content: &str) -> usize {
    content.split_inclusive('\n').count()
}
