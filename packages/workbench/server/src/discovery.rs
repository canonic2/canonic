//! Parallel directory discovery, reusing only unchanged real directories.
use crate::{files::stamp, model::Directory};
use anyhow::Result;
use globset::{GlobBuilder, GlobSetBuilder};
use rayon::prelude::*;
use std::{collections::HashMap, fs, path::Path};

pub(crate) struct Discovery {
    pub(crate) files: Vec<String>,
    pub(crate) directories: HashMap<String, Directory>,
    pub(crate) reads: usize,
}

pub(crate) fn discover(
    root: &Path,
    known: &HashMap<String, Directory>,
    walk: bool,
    patterns: Option<&[String]>,
) -> Result<Discovery> {
    fn visit(
        root: &Path,
        dir: &str,
        known: &HashMap<String, Directory>,
        walk: bool,
    ) -> Result<Discovery> {
        let path = root.join(dir);
        let current = stamp(&path);
        let reused = known
            .get(dir)
            .filter(|saved| !walk && saved.stamp.as_ref() == Some(&current));
        let record = if let Some(record) = reused {
            record.clone()
        } else {
            let mut children = Vec::new();
            for child in fs::read_dir(&path)? {
                let child = child?;
                let name = child.file_name().to_string_lossy().into_owned();
                let kind = child.file_type()?;
                if skipped(&name) || kind.is_symlink() {
                    continue;
                }
                children.push((name, kind.is_dir()));
            }
            Directory {
                stamp: Some(current),
                children,
            }
        };
        let subtrees: Vec<_> = record
            .children
            .par_iter()
            .map(|(name, directory)| {
                let relative = if dir.is_empty() {
                    name.clone()
                } else {
                    format!("{dir}/{name}")
                };
                if *directory {
                    visit(root, &relative, known, walk)
                } else {
                    Ok(Discovery {
                        files: vec![relative],
                        directories: HashMap::new(),
                        reads: 0,
                    })
                }
            })
            .collect();
        let mut result = Discovery {
            files: vec![],
            directories: HashMap::new(),
            reads: usize::from(reused.is_none()),
        };
        result.directories.insert(dir.into(), record);
        for subtree in subtrees {
            let subtree = subtree?;
            result.files.extend(subtree.files);
            result.directories.extend(subtree.directories);
            result.reads += subtree.reads;
        }
        Ok(result)
    }
    let mut result = Discovery {
        files: vec![],
        directories: HashMap::new(),
        reads: 0,
    };
    for base in bases(patterns) {
        if !reachable(root, &base) {
            continue;
        }
        let subtree = visit(root, &base, known, walk)?;
        result.files.extend(subtree.files);
        result.directories.extend(subtree.directories);
        result.reads += subtree.reads;
    }
    if let Some(patterns) = patterns {
        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            builder.add(GlobBuilder::new(pattern).literal_separator(true).build()?);
        }
        let matches = builder.build()?;
        result.files.retain(|file| matches.is_match(file));
    }
    Ok(result)
}

/// A folder the walk never enters.
pub(crate) fn skipped(name: &str) -> bool {
    name.starts_with('.') || ["node_modules", "dist", "build", "coverage"].contains(&name)
}

/// The folders a walk must start from to find every match: each pattern's
/// literal leading folders, without those inside another base. Patterns the
/// evaluator selects need the whole tree.
pub(crate) fn bases(patterns: Option<&[String]>) -> Vec<String> {
    let Some(patterns) = patterns else {
        return vec![String::new()];
    };
    let mut bases: Vec<String> = patterns
        .iter()
        .map(|pattern| {
            let segments: Vec<&str> = pattern.split('/').collect();
            let folders = &segments[..segments.len().saturating_sub(1)];
            folders
                .iter()
                .take_while(|segment| !segment.contains('*'))
                .copied()
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect();
    bases.sort();
    bases.dedup();
    let all = bases.clone();
    bases.retain(|base| {
        !all.iter().any(|other| {
            other != base && (other.is_empty() || base.starts_with(&format!("{other}/")))
        })
    });
    bases
}

/// Whether the full walk would reach this base: a relative folder inside the
/// root, with no skipped or linked folder on the way. An absolute pattern's
/// base starts with an empty segment and is never reached, as the full walk
/// never matches it.
fn reachable(root: &Path, base: &str) -> bool {
    if base.is_empty() {
        return true;
    }
    let mut path = root.to_path_buf();
    for segment in base.split('/') {
        if segment.is_empty() || skipped(segment) {
            return false;
        }
        path.push(segment);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {}
            _ => return false,
        }
    }
    true
}

/// Includes and the subset that globset can preserve without picomatch.
pub(crate) struct Includes {
    pub(crate) patterns: Vec<String>,
    pub(crate) custom: bool,
}
impl Includes {
    pub(crate) fn read(options: &serde_json::Value) -> Result<Self> {
        use serde_json::Value;
        let patterns = match options.get("include") {
            None | Some(Value::Null) => {
                vec!["**/*.workbench.ts".into(), "**/*.workbench.tsx".into()]
            }
            Some(Value::String(pattern)) => vec![pattern.clone()],
            Some(value) => serde_json::from_value::<Vec<String>>(value.clone())?,
        };
        let custom = patterns.iter().any(|pattern| {
            pattern
                .bytes()
                .any(|byte| !byte.is_ascii_alphanumeric() && !b"/.*_-".contains(&byte))
                || pattern.contains("***")
        });
        Ok(Self { patterns, custom })
    }
    pub(crate) fn native(&self) -> Option<&[String]> {
        (!self.custom).then_some(self.patterns.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::{bases, discover};
    use std::{collections::HashMap, fs};

    fn of(patterns: &[&str]) -> Vec<String> {
        let patterns: Vec<String> = patterns.iter().map(ToString::to_string).collect();
        bases(Some(&patterns))
    }

    #[test]
    fn walks_start_at_each_pattern_s_literal_folders() {
        assert_eq!(of(&["**/*.workbench.ts", "**/*.workbench.tsx"]), vec![""]);
        assert_eq!(of(&["demo/**/*.workbench.ts"]), vec!["demo"]);
        assert_eq!(
            of(&["demo/previews/*.workbench.ts", "demo/**/*.workbench.ts"]),
            vec!["demo"]
        );
        assert_eq!(of(&["a/b.workbench.ts", "c/**/*.ts"]), vec!["a", "c"]);
        assert_eq!(of(&["page.workbench.ts", "demo/**/*.ts"]), vec![""]);
        assert_eq!(bases(None), vec![""]);
    }

    #[test]
    fn a_base_finds_what_the_whole_walk_finds() {
        let root = std::env::temp_dir().join(format!("workbench-bases-{}", std::process::id()));
        let _cleared = fs::remove_dir_all(&root);
        for file in [
            "demo/a.workbench.ts",
            "demo/deep/b.workbench.ts",
            "other/c.workbench.ts",
            ".hidden/demo/d.workbench.ts",
            "node_modules/e.workbench.ts",
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        let found = |patterns: &[&str]| {
            let patterns: Vec<String> = patterns.iter().map(ToString::to_string).collect();
            let mut files = discover(&root, &HashMap::new(), true, Some(&patterns))
                .unwrap()
                .files;
            files.sort();
            files
        };
        assert_eq!(
            found(&["demo/**/*.workbench.ts"]),
            vec!["demo/a.workbench.ts", "demo/deep/b.workbench.ts"]
        );
        assert_eq!(
            found(&["demo/**/*.workbench.ts"]),
            found(&["**/*.workbench.ts"])
                .into_iter()
                .filter(|file| file.starts_with("demo/"))
                .collect::<Vec<_>>()
        );
        assert_eq!(found(&[".hidden/**/*.workbench.ts"]), Vec::<String>::new());
        assert_eq!(
            found(&["node_modules/*.workbench.ts"]),
            Vec::<String>::new()
        );
        assert_eq!(found(&["missing/**/*.workbench.ts"]), Vec::<String>::new());
        // An absolute pattern never reaches outside the root, even when the
        // root has a folder with the pattern's first name.
        fs::create_dir_all(root.join("acme")).unwrap();
        let outside = format!("{}/**/*.workbench.ts", root.join("demo").display());
        assert_eq!(found(&[outside.as_str()]), Vec::<String>::new());
        fs::remove_dir_all(root).unwrap();
    }
}
