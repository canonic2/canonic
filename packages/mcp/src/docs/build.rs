//! Build-script helper that selects, validates and writes a consumer's guide bundle.
//!
//! Only this module reads files. Call [`embed`] from `build.rs`, with this
//! library in `[build-dependencies]` and its `docs-build` feature enabled:
//!
//! ```no_run
//! fn main() -> Result<(), canonic_mcp::docs::build::BuildError> {
//!     canonic_mcp::docs::build::embed()?;
//!     Ok(())
//! }
//! ```

use super::{
    FILE_NAME, MAX_LINE_BYTES, MAX_PAGE_BYTES, MAX_PAGES, MAX_PATH_BYTES, MAX_TOTAL_BYTES,
    format::{self, Index, MAGIC, Page},
};
use miniz_oxide::deflate::compress_to_vec;
use std::{
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

/// The compression level; fixed so the same sources give the same bytes.
const LEVEL: u8 = 10;

/// Why a guide folder cannot be bundled. Paths are relative to the folder.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// The build script did not provide `CARGO_MANIFEST_DIR` or `OUT_DIR`.
    #[error("{0} is not set; call embed from a Cargo build script")]
    Environment(&'static str),
    /// The guide folder is a symbolic link or not a directory.
    #[error("the documentation folder must be a directory, not a symbolic link")]
    Root,
    /// A file or folder, including a missing guide folder, could not be read,
    /// or the bundle could not be written.
    #[error("documentation I/O failed at {path}: {source}")]
    Io {
        /// The relative path, `.` for the guide folder, or the bundle file name.
        path: String,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// A symbolic link inside the folder; it could publish outside content.
    #[error("documentation must not contain symbolic links: {path}")]
    Symlink {
        /// The link's relative path.
        path: String,
    },
    /// A name that is not UTF-8, or contains `\` or a control character.
    #[error("documentation names must be UTF-8 without backslashes or control characters: {path}")]
    Name {
        /// The relative path, with invalid characters replaced.
        path: String,
    },
    /// A page that is not valid UTF-8.
    #[error("documentation page is not valid UTF-8: {path}")]
    Text {
        /// The page's relative path.
        path: String,
    },
    /// No Markdown pages were found.
    #[error("the documentation folder contains no Markdown pages")]
    Empty,
    /// More pages than [`MAX_PAGES`].
    #[error("the documentation folder has {count} pages; at most {MAX_PAGES} are supported")]
    Pages {
        /// The number of pages found.
        count: usize,
    },
    /// A path longer than [`MAX_PATH_BYTES`].
    #[error("documentation path exceeds {MAX_PATH_BYTES} bytes: {path}")]
    Path {
        /// The relative path.
        path: String,
    },
    /// A page larger than [`MAX_PAGE_BYTES`].
    #[error("documentation page exceeds {MAX_PAGE_BYTES} bytes: {path}")]
    Page {
        /// The page's relative path.
        path: String,
    },
    /// A line longer than [`MAX_LINE_BYTES`], excluding its terminator.
    #[error("documentation line {line} exceeds {MAX_LINE_BYTES} bytes: {path}")]
    Line {
        /// The page's relative path.
        path: String,
        /// The one-based line number.
        line: usize,
    },
    /// More text than [`MAX_TOTAL_BYTES`] in total.
    #[error("the documentation exceeds {MAX_TOTAL_BYTES} bytes in total")]
    Total,
}

/// A selected page: its relative path and its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The path relative to the guide folder, with `/` separators.
    pub path: String,
    /// The file on disk.
    pub file: PathBuf,
}

/// Bundles `docs/` beside the package's `Cargo.toml` into the build output
/// folder as [`FILE_NAME`], for [`crate::include_docs!`], and asks Cargo to
/// rebuild when the folder changes.
///
/// Returns the selected sources in bundle order, for a consumer that includes
/// its published guides in its own build identity.
///
/// # Errors
///
/// Returns [`BuildError`] when the environment is not a build script, the
/// folder cannot be bundled or the bundle cannot be written.
pub fn embed() -> Result<Vec<Source>, BuildError> {
    let root =
        env::var_os("CARGO_MANIFEST_DIR").ok_or(BuildError::Environment("CARGO_MANIFEST_DIR"))?;
    let out = env::var_os("OUT_DIR").ok_or(BuildError::Environment("OUT_DIR"))?;
    let docs = Path::new(&root).join("docs");
    println!("cargo:rerun-if-changed={}", docs.display());
    embed_into(&docs, Path::new(&out))
}

/// Bundles `docs` into `out` as [`FILE_NAME`] and returns the selected sources.
///
/// # Errors
///
/// Returns [`BuildError`] when the folder cannot be bundled or the bundle
/// cannot be written.
pub fn embed_into(docs: &Path, out: &Path) -> Result<Vec<Source>, BuildError> {
    let sources = sources(docs)?;
    let bytes = encode(&sources)?;
    fs::write(out.join(FILE_NAME), bytes).map_err(|source| BuildError::Io {
        path: FILE_NAME.to_owned(),
        source,
    })?;
    Ok(sources)
}

/// The bundle for `docs`, without writing it.
///
/// # Errors
///
/// Returns [`BuildError`] when the folder cannot be bundled.
pub fn bundle(docs: &Path) -> Result<Vec<u8>, BuildError> {
    encode(&sources(docs)?)
}

/// The Markdown pages under `docs`, in bundle order: bytewise by relative path.
///
/// Recurses into folders, skips entries whose names start with `.` and
/// ignores files not ending in `.md`.
///
/// # Errors
///
/// Returns [`BuildError`] for a missing root or one that is not a plain
/// directory, a symbolic link, an invalid name, an unreadable folder or an
/// empty selection.
pub fn sources(docs: &Path) -> Result<Vec<Source>, BuildError> {
    let root = fs::symlink_metadata(docs).map_err(|source| BuildError::Io {
        path: ".".into(),
        source,
    })?;
    if !root.is_dir() {
        return Err(BuildError::Root);
    }
    let mut found = Vec::new();
    collect(docs, "", &mut found)?;
    if found.is_empty() {
        return Err(BuildError::Empty);
    }
    found.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    Ok(found)
}

fn collect(folder: &Path, prefix: &str, found: &mut Vec<Source>) -> Result<(), BuildError> {
    let io = |source| BuildError::Io {
        path: if prefix.is_empty() {
            ".".into()
        } else {
            prefix.to_owned()
        },
        source,
    };
    for entry in fs::read_dir(folder).map_err(io)? {
        let entry = entry.map_err(io)?;
        let name = entry.file_name();
        let lossy = name.to_string_lossy();
        if lossy.starts_with('.') {
            continue;
        }
        let path = format!("{prefix}{lossy}");
        let kind = entry.file_type().map_err(|source| BuildError::Io {
            path: path.clone(),
            source,
        })?;
        if kind.is_symlink() {
            return Err(BuildError::Symlink { path });
        }
        let published = kind.is_dir() || (kind.is_file() && lossy.ends_with(".md"));
        if !published {
            continue;
        }
        let valid = name
            .to_str()
            .is_some_and(|name| !name.contains(|c: char| c == '\\' || c.is_control()));
        if !valid {
            return Err(BuildError::Name { path });
        }
        if kind.is_dir() {
            collect(&entry.path(), &format!("{path}/"), found)?;
        } else {
            found.push(Source {
                path,
                file: entry.path(),
            });
        }
    }
    Ok(())
}

fn encode(sources: &[Source]) -> Result<Vec<u8>, BuildError> {
    if sources.len() > MAX_PAGES {
        return Err(BuildError::Pages {
            count: sources.len(),
        });
    }
    let mut text = String::new();
    let mut pages = Vec::with_capacity(sources.len());
    for source in sources {
        let path = source.path.clone();
        if path.len() > MAX_PATH_BYTES {
            return Err(BuildError::Path { path });
        }
        let mut bytes = Vec::new();
        fs::File::open(&source.file)
            .and_then(|file| file.take(MAX_PAGE_BYTES as u64 + 1).read_to_end(&mut bytes))
            .map_err(|error| BuildError::Io {
                path: path.clone(),
                source: error,
            })?;
        if bytes.len() > MAX_PAGE_BYTES {
            return Err(BuildError::Page { path });
        }
        let content =
            String::from_utf8(bytes).map_err(|_| BuildError::Text { path: path.clone() })?;
        if let Some(line) = content
            .split('\n')
            .position(|line| line.strip_suffix('\r').unwrap_or(line).len() > MAX_LINE_BYTES)
        {
            return Err(BuildError::Line {
                path,
                line: line + 1,
            });
        }
        if text.len() + content.len() > MAX_TOTAL_BYTES {
            return Err(BuildError::Total);
        }
        let start = text.len();
        text.push_str(&content);
        pages.push(Page {
            lines: format::line_count(&content),
            title: format::title(&content).to_owned(),
            path,
            start,
            end: text.len(),
        });
    }
    let index = serde_json::to_vec(&Index {
        size: text.len(),
        pages,
    })
    .map_err(|error| BuildError::Io {
        path: FILE_NAME.to_owned(),
        source: io::Error::other(error),
    })?;
    let length = u32::try_from(index.len()).map_err(|_| BuildError::Total)?;
    let mut bundle = Vec::with_capacity(MAGIC.len() + 4 + index.len());
    bundle.extend_from_slice(MAGIC);
    bundle.extend_from_slice(&length.to_le_bytes());
    bundle.extend_from_slice(&index);
    bundle.extend_from_slice(&compress_to_vec(text.as_bytes(), LEVEL));
    Ok(bundle)
}
