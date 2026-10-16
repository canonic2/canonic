//! Parallel directory discovery, reusing only unchanged real directories.
use crate::saved::{Directory, stamp};
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
        let reused = known.get(dir).filter(|saved| !walk && saved.stamp.as_ref() == Some(&current));
        let record = if let Some(record) = reused {
            record.clone()
        } else {
            let mut children = Vec::new();
            for child in fs::read_dir(&path)? {
                let child = child?;
                let name = child.file_name().to_string_lossy().into_owned();
                let kind = child.file_type()?;
                if name.starts_with('.')
                    || ["node_modules", "dist", "build", "coverage"].contains(&name.as_str())
                    || kind.is_symlink()
                {
                    continue;
                }
                children.push((name, kind.is_dir()));
            }
            Directory { stamp: Some(current), children }
        };
        let subtrees: Vec<_> = record
            .children
            .par_iter()
            .map(|(name, directory)| {
                let relative = if dir.is_empty() { name.clone() } else { format!("{dir}/{name}") };
                if *directory {
                    visit(root, &relative, known, walk)
                } else {
                    Ok(Discovery { files: vec![relative], directories: HashMap::new(), reads: 0 })
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
    let mut result = visit(root, "", known, walk)?;
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
