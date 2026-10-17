//! Parallel directory discovery, reusing only unchanged real directories.
//!
//! The walk skips a fixed set of folders and what the project's `.gitignore`
//! files ignore: each applies to its own folder, a deeper one decides first,
//! and a negation cannot bring back a path inside an ignored folder, as in
//! Git. Global and repository-local exclude files do not apply.
use crate::{files::stamp, model::Directory};
use anyhow::Result;
use globset::{GlobBuilder, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use rayon::prelude::*;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) struct Discovery {
    pub(crate) files: Vec<String>,
    pub(crate) directories: HashMap<String, Directory>,
    pub(crate) reads: usize,
    /// The `.gitignore` files that applied, relative to the root and sorted.
    pub(crate) ignores: Vec<String>,
}

/// The `.gitignore` rules that apply in a folder, outermost first.
pub(crate) type Rules = Vec<Arc<Gitignore>>;

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
        rules: &Rules,
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
            let mut gitignore = false;
            for child in fs::read_dir(&path)? {
                let child = child?;
                let name = child.file_name().to_string_lossy().into_owned();
                let kind = child.file_type()?;
                if name == ".gitignore" && kind.is_file() {
                    gitignore = true;
                }
                if skipped(&name) || kind.is_symlink() {
                    continue;
                }
                children.push((name, kind.is_dir()));
            }
            Directory {
                stamp: Some(current),
                children,
                gitignore,
            }
        };
        let mut rules = rules.clone();
        let mut ignores = vec![];
        if record.gitignore
            && let Some(own) = gitignore_in(&path)
        {
            rules.push(own);
            ignores.push(relative(dir, ".gitignore"));
        }
        let subtrees: Vec<_> = record
            .children
            .par_iter()
            .filter(|(name, directory)| !ignored(&rules, &path.join(name), *directory))
            .map(|(name, directory)| {
                let relative = relative(dir, name);
                if *directory {
                    visit(root, &relative, known, walk, &rules)
                } else {
                    Ok(Discovery {
                        files: vec![relative],
                        directories: HashMap::new(),
                        reads: 0,
                        ignores: vec![],
                    })
                }
            })
            .collect();
        let mut result = Discovery {
            files: vec![],
            directories: HashMap::new(),
            reads: usize::from(reused.is_none()),
            ignores,
        };
        result.directories.insert(dir.into(), record);
        for subtree in subtrees {
            result.merge(subtree?);
        }
        Ok(result)
    }
    let mut result = Discovery {
        files: vec![],
        directories: HashMap::new(),
        reads: 0,
        ignores: vec![],
    };
    for base in bases(patterns) {
        let Some((rules, ignores)) = reach(root, &base) else {
            continue;
        };
        result.ignores.extend(ignores);
        result.merge(visit(root, &base, known, walk, &rules)?);
    }
    result.ignores.sort();
    result.ignores.dedup();
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

impl Discovery {
    fn merge(&mut self, other: Self) {
        self.files.extend(other.files);
        self.directories.extend(other.directories);
        self.reads += other.reads;
        self.ignores.extend(other.ignores);
    }
}

fn relative(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

/// A folder the walk never enters.
pub(crate) fn skipped(name: &str) -> bool {
    name.starts_with('.') || ["node_modules", "dist", "build", "coverage"].contains(&name)
}

/// The rules of the `.gitignore` in `folder`, if it has a readable one. A line
/// that is not a valid pattern is left out, and the rest still apply.
pub(crate) fn gitignore_in(folder: &Path) -> Option<Arc<Gitignore>> {
    let file = folder.join(".gitignore");
    // A regular file only, as the walk counts it: not a link.
    if !fs::symlink_metadata(&file).is_ok_and(|metadata| metadata.is_file()) {
        return None;
    }
    let mut builder = GitignoreBuilder::new(folder);
    let _partial = builder.add(&file);
    builder.build().ok().map(Arc::new)
}

/// Whether `rules` ignore this path itself; its folders are checked as the
/// walk reaches them. The deepest rule that matches decides.
pub(crate) fn ignored(rules: &[Arc<Gitignore>], path: &Path, is_dir: bool) -> bool {
    for rules in rules.iter().rev() {
        let matched = rules.matched(path, is_dir);
        if matched.is_ignore() {
            return true;
        }
        if matched.is_whitelist() {
            return false;
        }
    }
    false
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

/// Whether the full walk would reach this base, and with which rules: a
/// relative folder inside the root, with no skipped, linked or ignored folder
/// on the way. Answers the rules of the folders above the base, which the walk
/// from the base does not read itself, and their `.gitignore` files. An
/// absolute pattern's base starts with an empty segment and is never reached,
/// as the full walk never matches it.
fn reach(root: &Path, base: &str) -> Option<(Rules, Vec<String>)> {
    let mut rules = Rules::new();
    let mut ignores = vec![];
    if base.is_empty() {
        return Some((rules, ignores));
    }
    let mut path: PathBuf = root.to_path_buf();
    let mut dir = String::new();
    for segment in base.split('/') {
        if segment.is_empty() || skipped(segment) {
            return None;
        }
        if let Some(own) = gitignore_in(&path) {
            rules.push(own);
            ignores.push(relative(&dir, ".gitignore"));
        }
        path.push(segment);
        dir = relative(&dir, segment);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {}
            _ => return None,
        }
        if ignored(&rules, &path, true) {
            return None;
        }
    }
    Some((rules, ignores))
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

    #[test]
    fn gitignore_files_leave_out_what_git_would() {
        let root = std::env::temp_dir().join(format!("workbench-gitignore-{}", std::process::id()));
        let _cleared = fs::remove_dir_all(&root);
        let write = |file: &str, body: &str| {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        };
        write(
            ".gitignore",
            "target/\ngenerated/*.workbench.ts\n!generated/kept.workbench.ts\nignored/\n!ignored/back.workbench.ts\n",
        );
        write("sub/.gitignore", "local.workbench.ts\n");
        for file in [
            "demo/a.workbench.ts",
            "target/debug/a.workbench.ts",
            "generated/x.workbench.ts",
            "generated/kept.workbench.ts",
            "ignored/back.workbench.ts",
            "sub/local.workbench.ts",
            "sub/keep.workbench.ts",
        ] {
            write(file, "");
        }
        let found = |patterns: &[&str]| {
            let patterns: Vec<String> = patterns.iter().map(ToString::to_string).collect();
            let found = discover(&root, &HashMap::new(), true, Some(&patterns)).unwrap();
            let mut files = found.files;
            files.sort();
            (files, found.ignores)
        };
        let (files, ignores) = found(&["**/*.workbench.ts"]);
        assert_eq!(
            files,
            vec![
                "demo/a.workbench.ts",
                "generated/kept.workbench.ts",
                "sub/keep.workbench.ts"
            ]
        );
        assert_eq!(ignores, vec![".gitignore", "sub/.gitignore"]);
        // A narrowed walk applies the rules of the folders above it.
        let (files, ignores) = found(&["generated/*.workbench.ts"]);
        assert_eq!(files, vec!["generated/kept.workbench.ts"]);
        assert_eq!(ignores, vec![".gitignore"]);
        assert_eq!(found(&["target/**/*.workbench.ts"]).0, Vec::<String>::new());
        fs::remove_dir_all(root).unwrap();
    }
}
