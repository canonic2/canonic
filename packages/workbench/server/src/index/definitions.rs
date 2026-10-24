//! Index input acquisition: reuse observations, read literals and merge evaluator results.
use crate::{
    definition,
    evaluate::Batch,
    files::{stamp, watch},
    literal,
    model::Record,
};
use rayon::prelude::*;
use std::{collections::HashMap, fs, path::Path};

pub(super) fn reusable<'a>(
    root: &Path,
    files: &[String],
    records: &'a HashMap<String, Record>,
) -> Vec<Option<&'a Record>> {
    files
        .par_iter()
        .map(|file| {
            records.get(file).filter(|record| {
                !record.watched.is_empty()
                    && record
                        .watched
                        .iter()
                        .all(|(file, saved)| stamp(&root.join(file)) == *saved)
            })
        })
        .collect()
}

pub(super) struct Definitions {
    pub(super) records: HashMap<String, Record>,
    pub(super) parsed: usize,
    pub(super) pending: Vec<String>,
}
impl Definitions {
    pub(super) fn load(root: &Path, files: &[String], reused: Vec<Option<&Record>>) -> Self {
        let reused_count = reused.iter().flatten().count();
        let real_root = fs::canonicalize(root).ok();
        let records: HashMap<_, _> = files
            .par_iter()
            .zip(reused)
            .filter_map(|(file, reused)| {
                let record = reused
                    .cloned()
                    .or_else(|| read_literal(root, real_root.as_deref(), file))?;
                Some((file.clone(), record))
            })
            .collect();
        let parsed = records.len() - reused_count;
        let pending = files
            .iter()
            .filter(|file| !records.contains_key(*file))
            .cloned()
            .collect();
        Self {
            records,
            parsed,
            pending,
        }
    }
    pub(super) fn apply(
        &mut self,
        root: &Path,
        batch: Batch,
    ) -> Vec<(String, crate::model::Stamp)> {
        let settings = batch
            .settings
            .iter()
            .map(|file| (file.clone(), stamp(&root.join(file))))
            .collect();
        for entry in batch.entries {
            let watched = if entry.cacheable {
                entry
                    .files
                    .iter()
                    .map(|file| (file.clone(), stamp(&root.join(file))))
                    .collect()
            } else {
                vec![]
            };
            let result = entry.entry.map(|entry| entry.to_string()).ok_or_else(|| {
                entry
                    .error
                    .unwrap_or_else(|| "definition evaluation failed".into())
            });
            self.records.insert(
                entry.file.clone(),
                Record {
                    file: entry.file,
                    watched,
                    entry: result,
                    docs: entry.docs,
                },
            );
        }
        settings
    }
}

fn read_literal(root: &Path, real_root: Option<&Path>, file: &str) -> Option<Record> {
    let path = root.join(file);
    let before = stamp(&path);
    // An address could not name a path holding one of its marks. The catalog
    // reports that problem before reading the entry, so the definition is
    // neither parsed nor sent to the evaluator.
    if file.contains([':', '+', '!', '~']) {
        return Some(Record {
            file: file.into(),
            watched: vec![(file.into(), before)],
            entry: Err("the path holds an address mark".into()),
            docs: false,
        });
    }
    let raw = literal::parse(
        fs::read_to_string(&path).ok()?,
        path.extension().is_some_and(|ext| ext == "tsx"),
    )?;
    let projection = definition::project(&raw, &path, root, |source| {
        fs::canonicalize(source)
            .ok()
            .zip(real_root)
            .is_some_and(|(source, root)| source.starts_with(root))
    })?;
    let mut watched = vec![(file.into(), before)];
    watched.extend(projection.sources.iter().map(|source| watch(root, source)));
    Some(Record {
        file: file.into(),
        watched,
        entry: projection.entry.map(|entry| entry.to_string()),
        docs: false,
    })
}
