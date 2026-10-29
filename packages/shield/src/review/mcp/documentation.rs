//! Read-only, bounded access to the Markdown guides embedded at build time.
use super::{Checkpoint, Error, fields, required, text};
use miniz_oxide::inflate::decompress_to_vec_with_limit;
use serde_json::{Value, json};

include!(concat!(env!("OUT_DIR"), "/shield_docs.rs"));
const COMPRESSED_DOCUMENTS: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/shield_docs.deflate"));

pub(super) fn call(name: &str, args: &Value, checkpoint: Checkpoint<'_>) -> Result<Value, Error> {
    checkpoint()?;
    let source = decode(COMPRESSED_DOCUMENTS, DOCUMENT_SIZE)?;
    checkpoint()?;
    match name {
        "shield_docs_list" => {
            fields(args, &["offset", "limit"])?;
            let rows = DOCUMENTS
                .iter()
                .map(|(path, start, end)| {
                    let content = page_text(&source, *start, *end)?;
                    Ok(json!({"path":path,"title":title(content),"total_lines":content.lines().count()}))
                })
                .collect::<Result<Vec<_>, Error>>()?;
            page(args, &rows, "documents")
        }
        "shield_docs_search" => search(&source, args, checkpoint),
        "shield_docs_read" => read(&source, args),
        _ => Err(Error::Invalid("unknown documentation tool")),
    }
}

fn decode(compressed: &[u8], expected_size: usize) -> Result<String, Error> {
    let bytes = decompress_to_vec_with_limit(compressed, expected_size)
        .map_err(|_| Error::Invalid("invalid bundled documentation compression"))?;
    if bytes.len() != expected_size {
        return Err(Error::Invalid("invalid bundled documentation size"));
    }
    String::from_utf8(bytes).map_err(|_| Error::Invalid("invalid bundled documentation text"))
}

fn page_text(source: &str, start: usize, end: usize) -> Result<&str, Error> {
    source
        .get(start..end)
        .ok_or(Error::Invalid("invalid bundled documentation range"))
}

fn title(content: &str) -> &str {
    content
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .unwrap_or_default()
}

fn document<'a>(source: &'a str, path: &str) -> Result<&'a str, Error> {
    let (_, start, end) = DOCUMENTS
        .iter()
        .find(|(id, _, _)| *id == path)
        .ok_or(Error::Missing)?;
    page_text(source, *start, *end)
}

fn number(args: &Value, key: &str, default: usize, min: usize, max: usize) -> Result<usize, Error> {
    match args.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| (min..=max).contains(value))
            .ok_or(Error::Invalid("invalid documentation page bounds")),
    }
}

fn page(args: &Value, rows: &[Value], field: &str) -> Result<Value, Error> {
    let offset = number(args, "offset", 0, 0, usize::MAX)?;
    let limit = number(args, "limit", 50, 1, 100)?;
    let end = offset.saturating_add(limit).min(rows.len());
    let mut result = json!({"version":env!("CARGO_PKG_VERSION"),"total":rows.len(),
        "next_offset":(end < rows.len()).then_some(end)});
    result[field] = json!(&rows[offset.min(rows.len())..end]);
    Ok(result)
}

fn search(source: &str, args: &Value, checkpoint: Checkpoint<'_>) -> Result<Value, Error> {
    fields(args, &["query", "path", "offset", "limit"])?;
    let query = required(args, "query")?.to_lowercase();
    if query.trim().is_empty() {
        return Err(Error::Invalid(
            "documentation search query must not be blank",
        ));
    }
    let path = text(args, "path")?;
    if let Some(path) = path {
        document(source, path)?;
    }
    let mut rows = Vec::new();
    for (id, start, end) in DOCUMENTS {
        checkpoint()?;
        if path.is_some_and(|path| path != *id) {
            continue;
        }
        let content = page_text(source, *start, *end)?;
        for (index, line) in content.lines().enumerate() {
            checkpoint()?;
            if line.to_lowercase().contains(&query) {
                rows.push(json!({"path":id,"line":index + 1,"text":line}));
            }
        }
    }
    page(args, &rows, "matches")
}

fn read(source: &str, args: &Value) -> Result<Value, Error> {
    fields(args, &["path", "start_line", "limit"])?;
    let path = required(args, "path")?;
    let content = document(source, path)?;
    let start = number(args, "start_line", 1, 1, usize::MAX)?;
    let limit = number(args, "limit", 100, 1, 400)?;
    let lines = content.lines().collect::<Vec<_>>();
    let offset = (start - 1).min(lines.len());
    let end = offset.saturating_add(limit).min(lines.len());
    Ok(
        json!({"version":env!("CARGO_PKG_VERSION"),"path":path,"title":title(content),
        "start_line":start,"total_lines":lines.len(),"content":lines[offset..end].join("\n"),
        "next_start_line":(end < lines.len()).then_some(end + 1)}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use miniz_oxide::deflate::compress_to_vec;

    #[test]
    fn compressed_documentation_restores_every_source_byte() {
        assert!(COMPRESSED_DOCUMENTS.len() < DOCUMENT_SIZE);
        let source = decode(COMPRESSED_DOCUMENTS, DOCUMENT_SIZE).unwrap();
        let mut previous_end = 0;
        for (path, start, end) in DOCUMENTS {
            assert_eq!(*start, previous_end);
            let expected = std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("docs")
                    .join(path),
            )
            .unwrap();
            assert_eq!(
                document(&source, path).unwrap().as_bytes(),
                expected,
                "{path}"
            );
            previous_end = *end;
        }
        assert_eq!(previous_end, source.len());
    }

    #[test]
    fn documentation_decode_checks_compression_size_and_utf8() {
        let source = "Acme café\r\n规则\n";
        let compressed = compress_to_vec(source.as_bytes(), 10);
        assert_eq!(decode(&compressed, source.len()).unwrap(), source);
        assert!(decode(&compressed, source.len() - 1).is_err());
        assert!(decode(&compressed, source.len() + 1).is_err());
        assert!(decode(&compressed[..compressed.len() / 2], source.len()).is_err());
        assert!(decode(&compress_to_vec(&[0xff], 10), 1).is_err());
    }

    #[test]
    fn documentation_ranges_reject_invalid_utf8_boundaries_and_offsets() {
        assert_eq!(page_text("é\nAcme", 0, 2).unwrap(), "é");
        for (start, end) in [(0, 1), (1, 2), (3, 2), (0, 100)] {
            assert!(page_text("é\nAcme", start, end).is_err());
        }
    }
}
