//! Compresses the Markdown guides into an embedded bundle with a page index.
use miniz_oxide::deflate::compress_to_vec;
use std::{env, fmt::Write as _, fs, io, path::Path};

fn main() -> io::Result<()> {
    let root = env::var_os("CARGO_MANIFEST_DIR")
        .ok_or_else(|| io::Error::other("missing package root"))?;
    let docs = Path::new(&root).join("docs");
    println!("cargo:rerun-if-changed=docs");
    let mut files = Vec::new();
    collect(&docs, &mut files)?;
    let mut pages = Vec::new();
    for file in files {
        let relative = file.strip_prefix(&docs).map_err(io::Error::other)?;
        let id = relative
            .to_str()
            .ok_or_else(|| io::Error::other("non-UTF-8 documentation path"))?
            .replace('\\', "/");
        pages.push((id, file));
    }
    pages.sort_by(|left, right| left.0.cmp(&right.0));
    let mut source = String::from("const DOCUMENTS: &[(&str, usize, usize)] = &[\n");
    let mut bundle = String::new();
    for (id, file) in pages {
        let start = bundle.len();
        bundle.push_str(&fs::read_to_string(file)?);
        writeln!(
            source,
            "    ({id:?}, {}, {}),",
            literal(start),
            literal(bundle.len())
        )
        .map_err(io::Error::other)?;
    }
    source.push_str("];\n");
    writeln!(
        source,
        "const DOCUMENT_SIZE: usize = {};",
        literal(bundle.len())
    )
    .map_err(io::Error::other)?;
    let output = env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("missing build output"))?;
    fs::write(
        Path::new(&output).join("shield_docs.deflate"),
        compress_to_vec(bundle.as_bytes(), 10),
    )?;
    fs::write(Path::new(&output).join("shield_docs.rs"), source)
}

/// Decimal Rust literals with separators, including offsets into large guide bundles.
fn literal(value: usize) -> String {
    let digits = value.to_string();
    let mut result = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push('_');
        }
        result.push(digit);
    }
    result
}

fn collect(directory: &Path, files: &mut Vec<std::path::PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_symlink() {
            return Err(io::Error::other("documentation must not contain symlinks"));
        }
        if kind.is_dir() {
            collect(&path, files)?;
        } else if kind.is_file() && path.extension().is_some_and(|extension| extension == "md") {
            files.push(path);
        }
    }
    Ok(())
}
