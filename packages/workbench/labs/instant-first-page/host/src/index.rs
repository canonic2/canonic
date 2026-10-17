//! The page index: discovery, a saved index checked by file stamps, list
//! fields parsed from literal definitions, and the Node fallback for the
//! rest. Its output matches the shipping `Compiler.index()` for previews.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Instant, UNIX_EPOCH};

use globset::{Glob, GlobSet, GlobSetBuilder};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use swc_common::{BytePos, FileName, SourceMap, sync::Lrc};
use swc_ecma_ast as ast;
use swc_ecma_parser::{Parser, StringInput, Syntax, TsSyntax};

/// Folders discovery never enters, as in `preview/compiler.cjs`.
const OMIT: &[&str] = &["node_modules", "dist", "build", "coverage"];

#[derive(Clone, PartialEq)]
struct Stamp {
    file: PathBuf,
    /// Modification time in nanoseconds and size; `None` when missing.
    stamp: Option<(u128, u64)>,
}

#[derive(Clone, PartialEq)]
struct Saved {
    /// The definition and every file its list fields depend on.
    watched: Vec<Stamp>,
    /// The entry's ID, for the duplicate check.
    id: String,
    /// The validated index entry as JSON text, or the problem it reports.
    entry: Result<String, String>,
    /// Whether the list fields were parsed (or else evaluated).
    parsed: bool,
}

/// What one directory held when it was last read: its subdirectories and
/// matching definitions, in discovery order.
#[derive(Clone, PartialEq)]
struct Dir {
    modified: u128,
    items: Vec<(PathBuf, bool)>,
}

#[derive(Default)]
struct SavedIndex {
    definitions: HashMap<PathBuf, Saved>,
    dirs: HashMap<PathBuf, Dir>,
}

/// The saved index on disk: postcard, with paths relative to the project.
/// The complete answer is saved beside it, so an unchanged start serves it
/// without assembling or parsing anything.
#[derive(Serialize, Deserialize)]
struct Disk {
    dirs: Vec<(String, u128, Vec<(String, bool)>)>,
    definitions: Vec<(String, Vec<(String, Option<(u128, u64)>)>, String, Result<String, String>, bool)>,
}

impl SavedIndex {
    fn load(file: &Path, root: &Path) -> Option<Self> {
        let disk: Disk = postcard::from_bytes(&fs::read(file).ok()?).ok()?;
        let dirs = disk
            .dirs
            .into_par_iter()
            .map(|(dir, modified, items)| {
                let dir = root.join(dir);
                let items = items.into_iter().map(|(name, is_dir)| (dir.join(name), is_dir)).collect();
                (dir, Dir { modified, items })
            })
            .collect();
        let definitions = disk
            .definitions
            .into_par_iter()
            .map(|(file, watched, id, entry, parsed)| {
                let watched = watched.into_iter().map(|(file, stamp)| Stamp { file: root.join(file), stamp }).collect();
                (root.join(file), Saved { watched, id, entry, parsed })
            })
            .collect();
        Some(Self { definitions, dirs })
    }

    fn save(&self, file: &Path, root: &Path) {
        let rel = |path: &Path| path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned();
        let name = |path: &Path| path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let disk = Disk {
            dirs: self.dirs.iter().map(|(dir, record)| (rel(dir), record.modified, record.items.iter().map(|(item, is_dir)| (name(item), *is_dir)).collect())).collect(),
            definitions: self
                .definitions
                .iter()
                .map(|(file, saved)| (rel(file), saved.watched.iter().map(|stamp| (rel(&stamp.file), stamp.stamp)).collect(), saved.id.clone(), saved.entry.clone(), saved.parsed))
                .collect(),
        };
        fs::write(file, postcard::to_allocvec(&disk).expect("saved index")).expect("write saved index");
    }
}

fn answer_file(saved: &Path) -> PathBuf {
    saved.with_extension("answer.json")
}

/// One index entry for the saved index.
fn read_entry(watched: Vec<Stamp>, entry: Result<Value, String>, parsed: bool) -> Saved {
    let id = entry.as_ref().ok().and_then(|entry| entry["id"].as_str()).unwrap_or_default().to_string();
    Saved { watched, id, entry: entry.map(|entry| entry.to_string()), parsed }
}

/// How the index finds definitions on a warm start.
#[derive(Clone, Copy, PartialEq)]
pub enum Check {
    /// Read every directory.
    Walk,
    /// Stat every known directory; read only those whose modification time
    /// changed (an entry was added, removed or renamed in it).
    Dirs,
}

/// Where the index spent its time, in milliseconds.
#[derive(Default, Serialize)]
pub struct Timings {
    pub load_ms: f64,
    pub walk_ms: f64,
    pub check_ms: f64,
    pub parse_ms: f64,
    pub evaluate_ms: f64,
    pub answer_ms: f64,
    pub save_ms: f64,
    pub total_ms: f64,
    pub definitions: usize,
    pub reused: usize,
    pub parsed: usize,
    pub evaluated: usize,
    pub dirs: usize,
    pub dirs_read: usize,
    pub saved: bool,
}

pub struct Options {
    pub root: PathBuf,
    pub include: Vec<String>,
    pub saved: Option<PathBuf>,
    pub evaluator: PathBuf,
    pub node: PathBuf,
    pub check: Check,
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

fn stamp(file: &Path) -> Stamp {
    let stamp = fs::metadata(file).ok().map(|meta| {
        let modified = meta.modified().ok().and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map(|time| time.as_nanos()).unwrap_or(0);
        (modified, meta.len())
    });
    Stamp { file: file.to_path_buf(), stamp }
}

fn globs(include: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in include {
        builder.add(Glob::new(pattern).expect("include pattern"));
    }
    builder.build().expect("include patterns")
}

fn modified(dir: &Path) -> u128 {
    fs::metadata(dir).ok().and_then(|meta| meta.modified().ok()).and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map(|time| time.as_nanos()).unwrap_or(0)
}

/// The definitions discovery lists, in its sorted order, and what each
/// directory held. With `Check::Dirs`, a known directory whose modification
/// time is unchanged is not read again.
fn discover(root: &Path, include: &GlobSet, known: &HashMap<PathBuf, Dir>, check: Check) -> (Vec<PathBuf>, HashMap<PathBuf, Dir>, usize) {
    fn read(root: &Path, dir: &Path, include: &GlobSet) -> Dir {
        let modified = modified(dir);
        let Ok(read) = fs::read_dir(dir) else { return Dir { modified, items: Vec::new() } };
        let mut items: Vec<(String, PathBuf, bool)> = read
            .filter_map(Result::ok)
            .filter_map(|item| Some((item.file_name().into_string().ok()?, item.path(), item.file_type().ok()?)))
            .filter(|(name, _, kind)| !name.starts_with('.') && !OMIT.contains(&name.as_str()) && !kind.is_symlink())
            .filter_map(|(name, file, kind)| {
                if kind.is_dir() {
                    return Some((name, file, true));
                }
                let relative = file.strip_prefix(root).unwrap_or(&file).to_string_lossy().replace('\\', "/");
                include.is_match(&relative).then_some((name, file, false))
            })
            .collect();
        items.sort_by(|a, b| a.0.cmp(&b.0));
        Dir { modified, items: items.into_iter().map(|(_, file, is_dir)| (file, is_dir)).collect() }
    }
    fn walk(root: &Path, dir: &Path, include: &GlobSet, known: &HashMap<PathBuf, Dir>, check: Check) -> (Vec<PathBuf>, Vec<(PathBuf, Dir)>, usize) {
        let reuse = match (check, known.get(dir)) {
            (Check::Dirs, Some(saved)) if saved.modified == modified(dir) => Some(saved.clone()),
            _ => None,
        };
        let was_read = usize::from(reuse.is_none());
        let record = reuse.unwrap_or_else(|| read(root, dir, include));
        let children: Vec<(Vec<PathBuf>, Vec<(PathBuf, Dir)>, usize)> = record
            .items
            .par_iter()
            .map(|(file, is_dir)| if *is_dir { walk(root, file, include, known, check) } else { (vec![file.clone()], Vec::new(), 0) })
            .collect();
        let mut files = Vec::new();
        let mut dirs = vec![(dir.to_path_buf(), record)];
        let mut reads = was_read;
        for (child_files, child_dirs, child_reads) in children {
            files.extend(child_files);
            dirs.extend(child_dirs);
            reads += child_reads;
        }
        (files, dirs, reads)
    }
    let (files, dirs, reads) = walk(root, root, include, known, check);
    (files, dirs.into_iter().collect(), reads)
}

/// A literal expression as JSON, or `None` when it needs evaluating.
fn literal(expr: &ast::Expr) -> Option<Value> {
    use ast::{Expr, Lit};
    match expr {
        Expr::Lit(Lit::Str(text)) => Some(Value::String(text.value.to_string_lossy().into_owned())),
        Expr::Lit(Lit::Num(number)) => serde_json::Number::from_f64(number.value).map(Value::Number),
        Expr::Lit(Lit::Bool(value)) => Some(Value::Bool(value.value)),
        Expr::Lit(Lit::Null(_)) => Some(Value::Null),
        Expr::Tpl(template) if template.exprs.is_empty() => template.quasis.first().and_then(|quasi| quasi.cooked.as_ref()).map(|text| Value::String(text.to_string_lossy().into_owned())),
        Expr::Unary(unary) if unary.op == ast::UnaryOp::Minus => match literal(&unary.arg)? {
            Value::Number(number) => serde_json::Number::from_f64(-number.as_f64()?).map(Value::Number),
            _ => None,
        },
        Expr::Paren(inner) => literal(&inner.expr),
        Expr::TsAs(inner) => literal(&inner.expr),
        Expr::TsSatisfies(inner) => literal(&inner.expr),
        Expr::TsConstAssertion(inner) => literal(&inner.expr),
        Expr::Array(array) => array
            .elems
            .iter()
            .map(|item| match item {
                Some(ast::ExprOrSpread { spread: None, expr }) => literal(expr),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        Expr::Object(object) => {
            let mut map = Map::new();
            for prop in &object.props {
                let ast::PropOrSpread::Prop(prop) = prop else { return None };
                let ast::Prop::KeyValue(pair) = prop.as_ref() else { return None };
                let key = match &pair.key {
                    ast::PropName::Ident(name) => name.sym.to_string(),
                    ast::PropName::Str(text) => text.value.to_string_lossy().into_owned(),
                    ast::PropName::Num(number) => number.value.to_string(),
                    _ => return None,
                };
                map.insert(key, literal(&pair.value)?);
            }
            Some(Value::Object(map))
        }
        _ => None,
    }
}

/// The definition's `definePreview({...})` argument as JSON, when every
/// value in it is a literal. Docs definitions and anything else need Node.
/// A definition's `definePreview({...})` argument as JSON, when every value
/// is a literal; stage 2's host generates entries from it.
pub(crate) fn parse_definition(file: &Path) -> Option<Value> {
    parse(file)
}

fn parse(file: &Path) -> Option<Value> {
    let source = fs::read_to_string(file).ok()?;
    let map: Lrc<SourceMap> = Default::default();
    let tsx = file.extension().is_some_and(|extension| extension == "tsx");
    let input = map.new_source_file(FileName::Real(file.to_path_buf()).into(), source);
    let syntax = Syntax::Typescript(TsSyntax { tsx, ..Default::default() });
    let mut parser = Parser::new(syntax, StringInput::new(&input.src, BytePos(input.start_pos.0), input.end_pos), None);
    let module = parser.parse_module().ok()?;
    for item in &module.body {
        let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDefaultExpr(export)) = item else { continue };
        let ast::Expr::Call(call) = export.expr.as_ref() else { return None };
        let ast::Callee::Expr(callee) = &call.callee else { return None };
        let ast::Expr::Ident(name) = callee.as_ref() else { return None };
        if name.sym != *"definePreview" || call.args.len() != 1 || call.args[0].spread.is_some() {
            return None;
        }
        return literal(&call.args[0].expr);
    }
    None
}

fn kebab_path(value: &str) -> bool {
    !value.is_empty() && value.split('/').all(kebab)
}

fn kebab(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn inside(root: &Path, file: &Path) -> bool {
    file.starts_with(root)
}

fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root).unwrap_or(file).to_string_lossy().replace('\\', "/")
}

/// Normalizes `a/./b/../c` without touching the file system, like `path.resolve`.
fn resolve(base: &Path, entry: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for part in base.join(entry).components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn title_case(id: &str) -> String {
    id.split('-')
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map(|first| first.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The shipping `validate` and index projection, for preview definitions.
/// Returns the entry and the source files it checked.
fn entry(raw: &Value, file: &Path, root: &Path, real_root: &Path) -> (Result<Value, String>, Vec<PathBuf>) {
    let shown = file.to_string_lossy();
    let mut sources = Vec::new();
    let result = (|| {
        let object = raw.as_object().ok_or(format!("{shown}: default export must be definePreview({{...}})."))?;
        if object.get("kind").and_then(Value::as_str) == Some("docs") {
            return Err("docs definitions are not indexed by the lab".to_string());
        }
        let id = object.get("id").and_then(Value::as_str).filter(|id| kebab_path(id)).ok_or(format!("{shown}: id must use kebab-case segments separated by /."))?;
        let adapter = object.get("adapter").and_then(Value::as_str).filter(|adapter| kebab(adapter)).ok_or(format!("{shown}: adapter must be a registered kebab-case name."))?;
        if let Some(icon) = object.get("icon")
            && !icon.as_str().is_some_and(kebab)
        {
            return Err(format!("{shown}: icon must be a kebab-case Lucide icon name."));
        }
        let states_raw = object.get("states");
        let mut source_values = vec![object.get("source")];
        if let Some(Value::Object(states)) = states_raw {
            source_values.extend(states.values().filter_map(|state| state.get("source")).map(Some));
        }
        let dir = file.parent().unwrap_or(root);
        for source in source_values {
            let entry = source.and_then(|source| source.get("entry")).and_then(Value::as_str).filter(|entry| !entry.trim().is_empty()).ok_or(format!("{shown}: source.entry must name a local source file."))?;
            let target = resolve(dir, entry);
            sources.push(target.clone());
            let real = fs::canonicalize(&target).ok();
            if !inside(root, &target) || !real.as_deref().is_some_and(|real| inside(real_root, real)) {
                return Err(format!("{shown}: source must exist inside the project: {entry}"));
            }
        }
        if let Some(states) = states_raw
            && !states.is_object()
        {
            return Err(format!("{shown}: states must be a map."));
        }
        let default_states = json!({ "default": {} });
        let states = match states_raw {
            Some(Value::Object(states)) if !states.is_empty() => states,
            _ => default_states.as_object().expect("default states"),
        };
        let mut listed = Vec::new();
        for (state_id, state) in states {
            if !kebab(state_id) || !state.is_object() {
                return Err(format!("{shown}: states need kebab-case IDs and object definitions."));
            }
            let label = state.get("label").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| title_case(state_id));
            listed.push(json!({ "id": state_id, "label": label }));
        }
        let source = object.get("source").and_then(|source| source.get("entry")).and_then(Value::as_str).unwrap_or_default();
        let mut out = Map::new();
        out.insert("id".into(), json!(id));
        out.insert("title".into(), object.get("title").filter(|title| title.as_str().is_some_and(|title| !title.is_empty())).cloned().unwrap_or(json!(id)));
        out.insert("adapter".into(), json!(adapter));
        out.insert("file".into(), json!(relative(root, file)));
        out.insert("source".into(), json!(relative(root, &resolve(dir, source))));
        out.insert("states".into(), Value::Array(listed));
        out.insert("controls".into(), object.get("controls").cloned().unwrap_or(json!({})));
        if let Some(sizes) = object.get("sizes") {
            out.insert("sizes".into(), sizes.clone());
        }
        out.insert("docs".into(), object.get("docs").cloned().unwrap_or(Value::Null));
        if let Some(icon) = object.get("icon") {
            out.insert("icon".into(), icon.clone());
        }
        let mut links = Map::new();
        if let Some(Value::Object(raw_links)) = object.get("links") {
            for (href, to) in raw_links {
                let link = match to {
                    Value::String(preview) => json!({ "preview": preview }),
                    _ => {
                        let mut link = Map::new();
                        link.insert("preview".into(), to.get("preview").cloned().unwrap_or(json!(id)));
                        if let Some(state) = to.get("state").filter(|state| !state.is_null()) {
                            link.insert("state".into(), state.clone());
                        }
                        Value::Object(link)
                    }
                };
                links.insert(href.clone(), link);
            }
        }
        out.insert("links".into(), Value::Object(links));
        Ok(Value::Object(out))
    })();
    (result, sources)
}

#[derive(Deserialize)]
struct Evaluated {
    definitions: HashMap<PathBuf, EvaluatedDefinition>,
    #[serde(default)]
    errors: HashMap<PathBuf, String>,
}

#[derive(Deserialize)]
struct EvaluatedDefinition {
    value: Value,
    files: Vec<PathBuf>,
}

/// Builds the index. Returns it as the shipping `{ previews, docs, errors }`,
/// as JSON text.
pub fn index(options: &Options) -> (Vec<u8>, Timings) {
    let started = Instant::now();
    let mut timings = Timings::default();
    let root = options.root.clone();
    let real_root = fs::canonicalize(&root).unwrap_or_else(|_| root.clone());

    let step = Instant::now();
    let saved: SavedIndex = options.saved.as_ref().and_then(|file| SavedIndex::load(file, &root)).unwrap_or_default();
    timings.load_ms = ms(step);

    let step = Instant::now();
    let (files, dirs, dirs_read) = discover(&root, &globs(&options.include), &saved.dirs, options.check);
    timings.walk_ms = ms(step);
    timings.dirs = dirs.len();
    timings.dirs_read = dirs_read;
    timings.definitions = files.len();

    // Reuse every saved entry whose watched files are unchanged.
    let step = Instant::now();
    let reused: Vec<Option<Saved>> = files
        .par_iter()
        .map(|file| saved.definitions.get(file).filter(|saved| saved.watched.iter().all(|watched| stamp(&watched.file) == *watched)).cloned())
        .collect();
    timings.check_ms = ms(step);
    timings.reused = reused.iter().filter(|saved| saved.is_some()).count();

    let step = Instant::now();
    let parsed: Vec<Option<Saved>> = files
        .par_iter()
        .zip(reused.into_par_iter())
        .map(|(file, reused)| {
            if reused.is_some() {
                return reused;
            }
            let raw = parse(file)?;
            let (entry, sources) = entry(&raw, file, &root, &real_root);
            let mut watched = vec![stamp(file)];
            watched.extend(sources.iter().map(|source| stamp(source)));
            Some(read_entry(watched, entry, true))
        })
        .collect();
    timings.parse_ms = ms(step);

    let step = Instant::now();
    let pending: Vec<&PathBuf> = files.iter().zip(&parsed).filter(|(_, saved)| saved.is_none()).map(|(file, _)| file).collect();
    let mut evaluated: HashMap<PathBuf, Saved> = HashMap::new();
    if !pending.is_empty() {
        // The bundled Electron runtime runs as Node only with this set; plain
        // Node ignores it (stage 2 smoke: without it the host panicked).
        let list: Vec<String> = pending.iter().map(|file| file.to_string_lossy().into_owned()).collect();
        let run = || -> Option<Evaluated> {
            let mut child = Command::new(&options.node)
                .arg(&options.evaluator)
                .arg(&root)
                .env("ELECTRON_RUN_AS_NODE", "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .ok()?;
            {
                use std::io::Write as _;
                let mut stdin = child.stdin.take()?;
                stdin.write_all(list.join("\n").as_bytes()).ok()?;
            }
            let output = child.wait_with_output().ok()?;
            serde_json::from_slice(&output.stdout).ok()
        };
        // A failed evaluator is each pending definition's error, not a crash.
        let result = run().unwrap_or_else(|| Evaluated { definitions: HashMap::new(), errors: HashMap::new() });
        for file in &pending {
            let saved_entry = if let Some(definition) = result.definitions.get(*file) {
                let (entry, sources) = entry(&definition.value, file, &root, &real_root);
                let mut watched: Vec<Stamp> = definition.files.iter().map(|file| stamp(file)).collect();
                watched.extend(sources.iter().map(|source| stamp(source)));
                read_entry(watched, entry, false)
            } else {
                let message = result.errors.get(*file).cloned().unwrap_or_else(|| "did not evaluate".into());
                read_entry(vec![stamp(file)], Err(message), false)
            };
            evaluated.insert((*file).clone(), saved_entry);
        }
    }
    timings.evaluate_ms = ms(step);

    let mut previews: Vec<&str> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut ids = HashSet::new();
    // Every directory was reused unchanged when none was read and none went away.
    let dirs_changed = dirs_read > 0 || dirs.len() != saved.dirs.len();
    let mut next = SavedIndex { dirs, ..Default::default() };
    for (file, read) in files.iter().zip(parsed) {
        let read = read.or_else(|| evaluated.remove(file)).expect("every definition read");
        next.definitions.insert(file.clone(), read);
    }
    timings.evaluated = pending.len();
    timings.parsed = timings.definitions - timings.reused - timings.evaluated;
    let changed = dirs_changed || timings.reused != timings.definitions || saved.definitions.len() != next.definitions.len();

    // An unchanged index serves the saved answer as it is.
    let step = Instant::now();
    let answer = options.saved.as_ref().filter(|_| !changed).and_then(|file| fs::read(answer_file(file)).ok());
    let answer = answer.unwrap_or_else(|| {
        for file in &files {
            let read = &next.definitions[file];
            match &read.entry {
                Ok(entry) if ids.insert(read.id.as_str()) => previews.push(entry),
                Ok(_) => errors.push(serde_json::to_string(&format!("{}: Duplicate preview id: {}", relative(&root, file), read.id)).expect("error JSON")),
                Err(message) => errors.push(serde_json::to_string(&format!("{}: {message}", relative(&root, file))).expect("error JSON")),
            }
        }
        format!("{{\"previews\":[{}],\"docs\":[],\"errors\":[{}]}}", previews.join(","), errors.join(",")).into_bytes()
    });
    timings.answer_ms = ms(step);

    let step = Instant::now();
    if let Some(file) = options.saved.as_ref().filter(|_| changed) {
        timings.saved = true;
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        next.save(file, &root);
        fs::write(answer_file(file), &answer).expect("write saved answer");
    }
    timings.save_ms = ms(step);
    timings.total_ms = ms(started);
    // The process exits after this; freeing the maps is not part of an answer.
    std::mem::forget(next);
    std::mem::forget(saved);
    (answer, timings)
}
