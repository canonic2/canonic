//! The page index's records as the catalog reads them: previews and docs
//! definitions in discovery order, with each definition's error named by its
//! file and a repeated preview id reported rather than listed twice.
use crate::order::locale_compare;
use anyhow::{Result, bail};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

/// What the index found: previews, docs pages and problems.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct IndexAnswer {
    pub(crate) previews: Vec<Value>,
    pub(crate) docs: Vec<Value>,
    pub(crate) errors: Vec<String>,
}

/// One listed record: its file, whether it defines docs, and its entry as
/// JSON text or the error that kept it out.
pub(crate) struct Record<'a> {
    pub(crate) file: &'a str,
    pub(crate) docs: bool,
    pub(crate) entry: Result<&'a str, &'a str>,
}

enum Node<'a> {
    Folder(BTreeMap<String, Node<'a>>),
    File(Record<'a>),
}

/// Records in the order the compiler's depth-first walk visits files:
/// each folder's entries by name, a folder's files where its name sorts.
fn ordered(records: Vec<Record<'_>>) -> Result<Vec<Record<'_>>> {
    let mut root: BTreeMap<String, Node<'_>> = BTreeMap::new();
    for record in records {
        let segments: Vec<&str> = record.file.split('/').collect();
        let (last, folders) = segments.split_last().expect("split yields one segment");
        let mut tree = &mut root;
        for segment in folders {
            let child = tree
                .entry((*segment).to_owned())
                .or_insert_with(|| Node::Folder(BTreeMap::new()));
            let Node::Folder(children) = child else {
                bail!("Invalid native index path");
            };
            tree = children;
        }
        tree.insert((*last).to_owned(), Node::File(record));
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    Ok(out)
}

fn walk<'a>(tree: BTreeMap<String, Node<'a>>, out: &mut Vec<Record<'a>>) {
    let mut entries: Vec<_> = tree.into_iter().collect();
    entries.sort_by(|(left, _), (right, _)| locale_compare(left, right));
    for (_, node) in entries {
        match node {
            Node::Folder(children) => walk(children, out),
            Node::File(record) => out.push(record),
        }
    }
}

fn valid(entry: &Value, file: &str, docs: bool) -> bool {
    let shaped = entry["id"].is_string()
        && entry["file"].as_str() == Some(file)
        && entry["states"].is_array();
    shaped
        && if docs {
            entry["src"].is_string() && entry["lenses"].is_array()
        } else {
            entry["adapter"].is_string() && entry["source"].is_string()
        }
}

/// The answer the records make.
///
/// # Errors
/// Returns an error when a record's entry is not a definition's entry.
pub(crate) fn read(records: Vec<Record<'_>>) -> Result<IndexAnswer> {
    let mut answer = IndexAnswer::default();
    let mut ids = HashSet::new();
    for record in ordered(records)? {
        let text = match record.entry {
            Err(error) => {
                answer.errors.push(format!("{}: {error}", record.file));
                continue;
            }
            Ok(text) => text,
        };
        let entry: Value = serde_json::from_str(text)?;
        if !valid(&entry, record.file, record.docs) {
            bail!("Invalid native page index entry");
        }
        let id = entry["id"].as_str().unwrap_or_default().to_owned();
        if !ids.insert(id.clone()) {
            answer
                .errors
                .push(format!("{}: Duplicate preview id: {id}", record.file));
            continue;
        }
        if record.docs {
            answer.docs.push(entry);
        } else {
            answer.previews.push(entry);
        }
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn preview(id: &str, file: &str) -> String {
        json!({ "id": id, "file": file, "states": [], "adapter": "react", "source": file })
            .to_string()
    }

    #[test]
    fn records_follow_the_walk_with_errors_and_repeated_ids_reported() {
        let a = preview("button", "src/b/button.workbench.tsx");
        let b = preview("card", "src/a.workbench.tsx");
        let c = preview("button", "src/c.workbench.tsx");
        let docs = json!({ "id": "guide", "file": "docs/guide.workbench.ts", "states": [], "src": "docs/guide.md", "lenses": [] }).to_string();
        let answer = read(vec![
            Record {
                file: "src/b/button.workbench.tsx",
                docs: false,
                entry: Ok(&a),
            },
            Record {
                file: "src/c.workbench.tsx",
                docs: false,
                entry: Ok(&c),
            },
            Record {
                file: "src/a.workbench.tsx",
                docs: false,
                entry: Ok(&b),
            },
            Record {
                file: "src/broken.workbench.tsx",
                docs: false,
                entry: Err("Unexpected token"),
            },
            Record {
                file: "docs/guide.workbench.ts",
                docs: true,
                entry: Ok(&docs),
            },
        ])
        .unwrap();
        let files: Vec<_> = answer
            .previews
            .iter()
            .map(|entry| entry["file"].as_str().unwrap())
            .collect();
        assert_eq!(files, ["src/a.workbench.tsx", "src/b/button.workbench.tsx"]);
        assert_eq!(answer.docs.len(), 1);
        assert_eq!(
            answer.errors,
            [
                "src/broken.workbench.tsx: Unexpected token",
                "src/c.workbench.tsx: Duplicate preview id: button",
            ]
        );
    }

    #[test]
    fn an_entry_for_another_file_is_refused() {
        let wrong = preview("button", "src/other.workbench.tsx");
        assert!(
            read(vec![Record {
                file: "src/button.workbench.tsx",
                docs: false,
                entry: Ok(&wrong)
            }])
            .is_err()
        );
    }
}
