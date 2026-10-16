//! One index request: discover, check, parse, evaluate, persist and answer.
use crate::{
    definition, discovery, evaluate, literal,
    saved::{Record, Saved, stamp},
};
use anyhow::{Context, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, fs, path::PathBuf, time::Instant};

#[derive(Deserialize)]
pub(crate) struct Request {
    pub(crate) root: PathBuf,
    pub(crate) saved: PathBuf,
    pub(crate) identity: String,
    pub(crate) evaluator: PathBuf,
    pub(crate) node: PathBuf,
    #[serde(default)]
    pub(crate) options: Value,
    #[serde(default)]
    walk: bool,
    #[serde(default)]
    cached_generation: String,
}

#[derive(Serialize)]
pub(crate) struct Response {
    records: Vec<Listed>,
    timings: Value,
    needs_evaluator: bool,
    generation: String,
    reuse_answer: bool,
}

#[derive(Serialize)]
struct Listed {
    file: String,
    docs: bool,
    entry: Result<String, String>,
}
fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

#[expect(
    clippy::too_many_lines,
    reason = "one sequential index transaction with separately timed stages"
)]
pub(crate) fn index(request: &Request) -> Result<Response> {
    let started = Instant::now();
    let mut timings = json!({});
    let step = Instant::now();
    let previous = Saved::load(&request.saved, &request.identity);
    timings["loadMs"] = json!(ms(step));
    let step = Instant::now();
    let patterns: Vec<String> = match request.options.get("include") {
        None | Some(Value::Null) => vec!["**/*.workbench.ts".into(), "**/*.workbench.tsx".into()],
        Some(Value::String(pattern)) => vec![pattern.clone()],
        Some(value) => serde_json::from_value(value.clone())?,
    };
    // Only the plain path/globstar subset is interpreted natively. Picomatch
    // handles extglobs, braces, character classes, escapes and negation.
    let custom = patterns.iter().any(|pattern| {
        pattern
            .bytes()
            .any(|byte| !byte.is_ascii_alphanumeric() && !b"/.*_-".contains(&byte))
            || pattern.contains("***")
    });
    let discovered = discovery::discover(
        &request.root,
        &previous.directories,
        request.walk,
        if custom { None } else { Some(&patterns) },
    )?;
    timings["walkMs"] = json!(ms(step));
    timings["directories"] = json!(discovered.directories.len());
    timings["directoriesRead"] = json!(discovered.reads);
    let mut files = discovered.files;
    // Picomatch remains authoritative for custom patterns. Reuse selection on
    // an unchanged tree; no JavaScript process is needed on warm reads.
    if custom {
        if discovered.reads == 0 {
            files.retain(|file| previous.records.contains_key(file));
        } else {
            if request.node.as_os_str().is_empty() {
                return Ok(Response {
                    records: vec![],
                    timings,
                    needs_evaluator: true,
                    generation: String::new(),
                    reuse_answer: false,
                });
            }
            files = serde_json::from_value(evaluate::call(request, "select", &files)?)?;
        }
    }
    let step = Instant::now();
    let reused: Vec<_> = files
        .par_iter()
        .map(|file| {
            previous.records.get(file).filter(|record| {
                !record.watched.is_empty()
                    && record
                        .watched
                        .iter()
                        .all(|(file, saved)| stamp(&request.root.join(file)) == *saved)
            })
        })
        .collect();
    let reused_count = reused.iter().flatten().count();
    timings["checkMs"] = json!(ms(step));
    let config = request.root.join(
        request
            .options
            .get("config")
            .and_then(Value::as_str)
            .unwrap_or("workbench.config.ts"),
    );
    let settings_changed = previous
        .settings
        .iter()
        .any(|(file, saved)| stamp(&request.root.join(file)) != *saved);
    let check_settings = config.exists() && (previous.settings.is_empty() || settings_changed);
    if reused_count == files.len()
        && previous.records.len() == files.len()
        && discovered.reads == 0
        && !settings_changed
        && !check_settings
        && !previous.generation.is_empty()
        && request.cached_generation == previous.generation
    {
        timings["definitions"] = json!(files.len());
        timings["reused"] = json!(reused_count);
        for key in ["parsed", "evaluated", "parseMs", "evaluateMs", "saveMs"] {
            timings[key] = json!(0);
        }
        timings["totalMs"] = json!(ms(started));
        return Ok(Response {
            records: vec![],
            timings,
            needs_evaluator: false,
            generation: previous.generation,
            reuse_answer: true,
        });
    }
    let step = Instant::now();
    let mut records: HashMap<String, Record> = files
        .par_iter()
        .zip(reused)
        .filter_map(|(file, reused)| {
            if let Some(record) = reused {
                return Some((file.clone(), record.clone()));
            }
            let path = request.root.join(file);
            let before = stamp(&path);
            let raw = literal::parse(
                fs::read_to_string(&path).ok()?,
                path.extension().is_some_and(|ext| ext == "tsx"),
            )?;
            let real_root = fs::canonicalize(&request.root).ok();
            let projection = definition::project(&raw, &path, &request.root, |source| {
                fs::canonicalize(source)
                    .ok()
                    .zip(real_root.as_ref())
                    .is_some_and(|(source, root)| source.starts_with(root))
            })?;
            let mut watched = vec![(file.clone(), before)];
            watched.extend(projection.sources.iter().map(|source| {
                (
                    source
                        .strip_prefix(&request.root)
                        .unwrap_or(source)
                        .to_string_lossy()
                        .into_owned(),
                    stamp(source),
                )
            }));
            let entry = projection.entry.map(|entry| entry.to_string());
            Some((
                file.clone(),
                Record {
                    file: file.clone(),
                    watched,
                    entry,
                    docs: false,
                },
            ))
        })
        .collect();
    let parsed_count = records.len() - reused_count;
    timings["parseMs"] = json!(ms(step));
    let pending: Vec<_> = files
        .iter()
        .filter(|file| !records.contains_key(*file))
        .cloned()
        .collect();
    let step = Instant::now();
    let mut settings = previous.settings.clone();
    if !pending.is_empty() || check_settings {
        if request.node.as_os_str().is_empty() {
            // Preserve completed literal work while the adapter prepares a
            // runtime. Missing records remain pending on the second pass.
            // Complex picomatch selection is represented by complete records;
            // never publish a partial selection that would omit pending files.
            if !custom {
                let mut partial = Saved::default();
                partial.records = records;
                partial.directories = discovered.directories;
                partial.settings = settings;
                if let Err(error) = partial.save(&request.saved, &request.identity) {
                    timings["saveError"] = json!(error.to_string());
                }
            }
            timings["definitions"] = json!(files.len());
            timings["parsed"] = json!(parsed_count);
            timings["reused"] = json!(reused_count);
            timings["evaluated"] = json!(0);
            timings["totalMs"] = json!(ms(started));
            return Ok(Response {
                records: vec![],
                timings,
                needs_evaluator: true,
                generation: String::new(),
                reuse_answer: false,
            });
        }
        let evaluated: evaluate::Batch =
            serde_json::from_value(evaluate::call(request, "evaluate", &pending)?)?;
        settings = evaluated
            .settings
            .iter()
            .map(|file| (file.clone(), stamp(&request.root.join(file))))
            .collect();
        for entry in evaluated.entries {
            let watched = if entry.cacheable {
                entry
                    .files
                    .iter()
                    .map(|file| (file.clone(), stamp(&request.root.join(file))))
                    .collect()
            } else {
                vec![]
            };
            let result = entry.entry.map(|entry| entry.to_string()).ok_or_else(|| {
                entry
                    .error
                    .unwrap_or_else(|| "definition evaluation failed".into())
            });
            records.insert(
                entry.file.clone(),
                Record {
                    file: entry.file,
                    watched,
                    entry: result,
                    docs: entry.docs,
                },
            );
        }
    }
    timings["evaluateMs"] = json!(ms(step));
    let answer: Vec<_> = files
        .iter()
        .map(|file| {
            records
                .get(file)
                .map(|record| Listed {
                    file: record.file.clone(),
                    docs: record.docs,
                    entry: record.entry.clone(),
                })
                .context("evaluator omitted a definition")
        })
        .collect::<Result<_>>()?;
    let step = Instant::now();
    let mut generation = previous.generation.clone();
    if discovered.reads > 0
        || reused_count != files.len()
        || previous.records.len() != files.len()
        || check_settings
        || settings_changed
    {
        let mut next = Saved::default();
        next.records = records;
        next.directories = discovered.directories;
        next.settings = settings;
        generation = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        next.generation.clone_from(&generation);
        if let Err(error) = next.save(&request.saved, &request.identity) {
            timings["saveError"] = json!(error.to_string());
        }
    }
    timings["saveMs"] = json!(ms(step));
    timings["definitions"] = json!(files.len());
    timings["reused"] = json!(reused_count);
    timings["parsed"] = json!(parsed_count);
    timings["evaluated"] = json!(pending.len());
    timings["totalMs"] = json!(ms(started));
    Ok(Response {
        records: answer,
        timings,
        needs_evaluator: false,
        generation,
        reuse_answer: false,
    })
}
