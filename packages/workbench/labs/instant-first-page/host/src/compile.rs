//! The compiler host: one Rspack compiler driven from Rust, with the
//! pipeline's options for the `web` profile, virtual entry modules, entries
//! that join a running compiler, hot updates and React Fast Refresh.
//!
//! The runner drives it with one JSON request per line on stdin and reads one
//! JSON answer per line on stdout:
//!
//! - `{"op":"start", ...Job}` builds the job's entries;
//! - `{"op":"add", "entries":[Entry]}` adds entries and rebuilds;
//! - `{"op":"rebuild", "changed":[path], "deleted":[path]}` rebuilds after edits;
//! - `{"op":"asset", "name":"..."}` returns one emitted file;
//! - `{"op":"watch"}` watches the last build's files and the project root;
//! - `{"op":"events"}` returns the changed and deleted files seen since the
//!   last `events`;
//! - `{"op":"close"}` closes the compiler.
//!
//! Each build answer lists the build's hash, time, entrypoints with their
//! files, hot-update files and errors.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io::{BufRead as _, Write as _};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use rspack::builder::Builder as _;
use rspack_core::{
    Compilation, CompilationParams, Compiler, CompilerCompilation, CompilerMake, Context, DependencyRef, DependencyType, EntryDependency, EntryOptions, Experiments,
    ModuleOptions, ModuleRule, ModuleRuleEffect, ModuleRuleUse, ModuleRuleUseLoader, Plugin, RuleSetCondition,
};
use rspack_fs::{FileMetadata, FilePermissions, MemoryFileSystem, NativeFileSystem, ReadableFileSystem};
use rspack_hook::{plugin, plugin_hook};
use rspack_paths::{Utf8Path, Utf8PathBuf};
use rspack_regex::RspackRegex;
use serde::Deserialize;
use serde_json::{Value, json};

/// One generated entry module, from the shipping pipeline's `previewEntry`.
#[derive(Clone, Deserialize)]
pub struct Entry {
    pub slug: String,
    /// The virtual module's absolute path.
    pub file: String,
    pub source: String,
}

#[derive(Deserialize)]
pub struct Job {
    pub root: String,
    /// Workbench's `ui/preview/` folder: browser adapters and request mocks.
    pub preview_dir: String,
    pub cache_dir: String,
    pub generation: u32,
    pub hot_client: Entry,
    pub entries: Vec<Entry>,
    /// Paths the React Refresh plugin uses, resolved by the runner.
    pub refresh: Refresh,
    #[serde(default)]
    pub stable_chunks: bool,
}

#[derive(Clone, Deserialize)]
pub struct Refresh {
    pub entry: String,
    pub runtime: String,
    pub utils: String,
    pub runtime_dir: String,
}

// ------------------------------------------------------------- file system

/// The native file system with virtual entry modules laid over it.
#[derive(Clone)]
pub struct Overlay {
    native: Arc<NativeFileSystem>,
    files: Arc<RwLock<HashMap<Utf8PathBuf, Vec<u8>>>>,
}

impl fmt::Debug for Overlay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Overlay")
    }
}

impl Overlay {
    fn new() -> Self {
        Self { native: Arc::new(NativeFileSystem::new(false)), files: Default::default() }
    }

    fn write(&self, file: &str, source: &str) {
        self.files.write().expect("overlay").insert(Utf8PathBuf::from(file), source.as_bytes().to_vec());
    }

    fn get(&self, path: &Utf8Path) -> Option<Vec<u8>> {
        self.files.read().expect("overlay").get(path).cloned()
    }

    fn metadata_of(&self, path: &Utf8Path) -> Option<FileMetadata> {
        let files = self.files.read().expect("overlay");
        if let Some(bytes) = files.get(path) {
            return Some(FileMetadata { is_file: true, is_directory: false, is_symlink: false, atime_ms: 0, mtime_ms: 0, ctime_ms: 0, size: bytes.len() as u64 });
        }
        files.keys().any(|file| file.starts_with(path)).then_some(FileMetadata { is_file: false, is_directory: true, is_symlink: false, atime_ms: 0, mtime_ms: 0, ctime_ms: 0, size: 0 })
    }

    fn names_in(&self, dir: &Utf8Path) -> Vec<String> {
        self.files.read().expect("overlay").keys().filter(|file| file.parent() == Some(dir)).filter_map(|file| file.file_name().map(str::to_string)).collect()
    }
}

#[async_trait::async_trait]
impl ReadableFileSystem for Overlay {
    async fn read(&self, path: &Utf8Path) -> rspack_fs::Result<Vec<u8>> {
        match self.get(path) {
            Some(bytes) => Ok(bytes),
            None => self.native.read(path).await,
        }
    }
    fn read_sync(&self, path: &Utf8Path) -> rspack_fs::Result<Vec<u8>> {
        match self.get(path) {
            Some(bytes) => Ok(bytes),
            None => self.native.read_sync(path),
        }
    }
    async fn metadata(&self, path: &Utf8Path) -> rspack_fs::Result<FileMetadata> {
        match self.native.metadata(path).await {
            Ok(meta) => Ok(meta),
            Err(error) => self.metadata_of(path).ok_or(error),
        }
    }
    fn metadata_sync(&self, path: &Utf8Path) -> rspack_fs::Result<FileMetadata> {
        match self.native.metadata_sync(path) {
            Ok(meta) => Ok(meta),
            Err(error) => self.metadata_of(path).ok_or(error),
        }
    }
    async fn symlink_metadata(&self, path: &Utf8Path) -> rspack_fs::Result<FileMetadata> {
        match self.native.symlink_metadata(path).await {
            Ok(meta) => Ok(meta),
            Err(error) => self.metadata_of(path).ok_or(error),
        }
    }
    async fn read_link(&self, path: &Utf8Path) -> rspack_fs::Result<Utf8PathBuf> {
        self.native.read_link(path).await
    }
    async fn canonicalize(&self, path: &Utf8Path) -> rspack_fs::Result<Utf8PathBuf> {
        if self.metadata_of(path).is_some() && self.native.metadata_sync(path).is_err() {
            return Ok(path.to_path_buf());
        }
        self.native.canonicalize(path).await
    }
    async fn read_dir(&self, dir: &Utf8Path) -> rspack_fs::Result<Vec<String>> {
        let mut names = self.native.read_dir(dir).await.unwrap_or_default();
        names.extend(self.names_in(dir));
        Ok(names)
    }
    fn read_dir_sync(&self, dir: &Utf8Path) -> rspack_fs::Result<Vec<String>> {
        let mut names = self.native.read_dir_sync(dir).unwrap_or_default();
        names.extend(self.names_in(dir));
        Ok(names)
    }
    async fn permissions(&self, path: &Utf8Path) -> rspack_fs::Result<Option<FilePermissions>> {
        if self.get(path).is_some() {
            return Ok(None);
        }
        self.native.permissions(path).await
    }
}

// ------------------------------------------------------------- entries

/// The entry set, which can grow while the compiler runs. Each entry's
/// dependencies are created once, on the first build that includes it, so
/// incremental builds reuse their modules. Creating them earlier takes
/// dependency IDs before the persistent cache restores (finding C5).
#[derive(Default, Debug)]
pub struct EntrySet {
    entries: BTreeMap<String, (Vec<String>, Option<Vec<DependencyRef>>)>,
}

#[plugin]
#[derive(Debug)]
pub struct EntriesPlugin {
    context: Context,
    set: Arc<Mutex<EntrySet>>,
}

impl EntriesPlugin {
    fn create(context: Context, set: Arc<Mutex<EntrySet>>) -> Self {
        Self::new_inner(context, set)
    }
}

impl EntrySet {
    fn add(&mut self, name: &str, requests: &[String]) {
        self.entries.entry(name.to_string()).or_insert_with(|| (requests.to_vec(), None));
    }

    fn remove(&mut self, name: &str) {
        self.entries.remove(name);
    }

    fn dependencies(&mut self, context: &Context) -> Vec<(String, Vec<DependencyRef>)> {
        self.entries
            .iter_mut()
            .map(|(name, (requests, deps))| {
                let deps = deps.get_or_insert_with(|| requests.iter().map(|request| DependencyRef::new(EntryDependency::new(request.clone(), context.clone(), None, false))).collect());
                (name.clone(), deps.clone())
            })
            .collect()
    }
}

#[plugin_hook(CompilerCompilation for EntriesPlugin)]
async fn entries_compilation(&self, compilation: &mut Compilation, params: &mut CompilationParams) -> rspack_error::Result<()> {
    compilation.set_dependency_factory(DependencyType::Entry, params.normal_module_factory.clone());
    Ok(())
}

#[plugin_hook(CompilerMake for EntriesPlugin)]
async fn entries_make(&self, compilation: &mut Compilation) -> rspack_error::Result<()> {
    let entries = self.set.lock().expect("entries").dependencies(&self.context);
    for (name, deps) in entries {
        for dependency in deps {
            compilation.add_entry(dependency, EntryOptions { name: Some(name.clone()), ..Default::default() }).await?;
        }
    }
    Ok(())
}

impl Plugin for EntriesPlugin {
    fn apply(&self, ctx: &mut rspack_core::ApplyContext<'_>) -> rspack_error::Result<()> {
        ctx.compiler_hooks.compilation.tap(entries_compilation::new(self));
        ctx.compiler_hooks.make.tap(entries_make::new(self));
        Ok(())
    }
}

/// React Refresh needs the module cache in every runtime, as the JS plugin adds.
#[plugin]
#[derive(Debug, Default)]
pub struct RefreshRuntimePlugin;

#[plugin_hook(rspack_core::CompilationAdditionalTreeRuntimeRequirements for RefreshRuntimePlugin)]
async fn refresh_runtime(
    &self,
    _compilation: &Compilation,
    _chunk: &rspack_core::ChunkUkey,
    runtime_requirements: &mut rspack_core::RuntimeGlobals,
    _runtime_modules: &mut Vec<Box<dyn rspack_core::RuntimeModule>>,
) -> rspack_error::Result<()> {
    runtime_requirements.insert(rspack_core::RuntimeGlobals::MODULE_CACHE);
    Ok(())
}

impl Plugin for RefreshRuntimePlugin {
    fn apply(&self, ctx: &mut rspack_core::ApplyContext<'_>) -> rspack_error::Result<()> {
        ctx.compilation_hooks.additional_tree_runtime_requirements.tap(refresh_runtime::new(self));
        Ok(())
    }
}

/// One entry's served files, as the shipping pipeline's snapshot keeps them.
#[derive(Clone, Default, serde::Serialize)]
pub struct EntryPoint {
    pub files: Vec<String>,
    pub runtime: Vec<String>,
    pub css: Vec<(Value, String)>,
    pub errors: Vec<String>,
}

/// One build's outcome.
#[derive(Clone, Default, serde::Serialize)]
pub struct Built {
    pub ok: bool,
    pub ms: f64,
    pub error: Option<String>,
    pub hash: Option<String>,
    pub entrypoints: BTreeMap<String, EntryPoint>,
    /// Which split chunk holds each project module, for the moved-module check.
    pub chunk_of: BTreeMap<String, String>,
    pub modules: usize,
}

// ------------------------------------------------------------- watching

/// Collects what Rspack's watcher reports, for C8.
#[derive(Default, Clone)]
pub struct Seen {
    events: Arc<Mutex<(Vec<String>, Vec<String>)>>,
}

impl rspack_watcher::EventAggregateHandler for Seen {
    fn on_event_handle(&self, changed: rustc_hash::FxHashSet<String>, deleted: rustc_hash::FxHashSet<String>) {
        let mut events = self.events.lock().expect("events");
        events.0.extend(changed);
        events.1.extend(deleted);
    }
}

struct Ignore;
impl rspack_watcher::EventHandler for Ignore {}

// ------------------------------------------------------------- options

fn regex(source: &str) -> RspackRegex {
    RspackRegex::new(source).expect("regex")
}

fn loader(name: &str, options: Value) -> ModuleRuleUseLoader {
    ModuleRuleUseLoader { loader: name.to_string(), options: Some(options.to_string()), cache: true, options_cache_key: String::new() }
}

fn rules(job: &Job) -> Vec<ModuleRule> {
    let swc = json!({ "jsc": { "parser": { "syntax": "typescript", "tsx": true }, "transform": { "react": { "runtime": "automatic", "development": true, "refresh": true } }, "target": "es2020" } });
    let refresh_excluded = format!("node_modules|{}|{}|{}|{}", regex_escape(&job.refresh.entry), regex_escape(&job.refresh.runtime), regex_escape(&job.refresh.utils), regex_escape(&job.refresh.runtime_dir));
    vec![
        // React Refresh's loader runs first, as the plugin's unshifted rule.
        ModuleRule {
            test: Some(RuleSetCondition::Regexp(regex(r"\.(?:js|jsx|mjs|cjs|ts|tsx|mts|cts)$"))),
            exclude: Some(RuleSetCondition::Regexp(regex(&refresh_excluded))),
            effect: ModuleRuleEffect { r#use: ModuleRuleUse::Array(vec![loader("builtin:react-refresh-loader", json!({}))]), ..Default::default() },
            ..Default::default()
        },
        ModuleRule {
            test: Some(RuleSetCondition::Regexp(regex(r"\.[cm]?[jt]sx?$"))),
            exclude: Some(RuleSetCondition::Regexp(regex("node_modules"))),
            effect: ModuleRuleEffect { r#type: Some("javascript/auto".into()), r#use: ModuleRuleUse::Array(vec![loader("builtin:swc-loader", swc)]), ..Default::default() },
            ..Default::default()
        },
        ModuleRule {
            test: Some(RuleSetCondition::Regexp(regex(r"\.module\.css$"))),
            // Projects import CSS modules as a default object. The CSS plugin
            // reads `css/module` options as the CssAutoOrModule variant, the
            // one the builder's defaults use (finding C2-format).
            effect: ModuleRuleEffect {
                r#type: Some("css/module".into()),
                parser: Some(rspack_core::ParserOptions::CssAutoOrModule(rspack_core::CssAutoOrModuleParserOptions { named_exports: Some(false), ..Default::default() })),
                ..Default::default()
            },
            ..Default::default()
        },
        ModuleRule {
            test: Some(RuleSetCondition::Regexp(regex(r"\.css$"))),
            exclude: Some(RuleSetCondition::Regexp(regex(r"\.module\.css$"))),
            effect: ModuleRuleEffect { r#type: Some("css".into()), ..Default::default() },
            ..Default::default()
        },
        ModuleRule {
            test: Some(RuleSetCondition::Regexp(regex(r"\.(svg|png|jpe?g|gif|webp|avif|ico|bmp|woff2?|ttf|otf|eot|mp4|webm|mov|mp3|ogg|wav|m4a)$"))),
            effect: ModuleRuleEffect { r#type: Some("asset/resource".into()), ..Default::default() },
            ..Default::default()
        },
    ]
}

/// The JavaScript parser defaults the Node binding applies. The Rust builder
/// leaves `import_meta` unset for a `web` target and Rspack then panics
/// (finding C2), so the lab passes them all.
fn javascript_parser() -> rspack_core::ParserOptionsMap {
    use rspack_core::{
        DynamicImportMode, ImportMeta, JavascriptParserCommonjsExportsOption, JavascriptParserCommonjsOptions, JavascriptParserOptions, JavascriptParserOrder, JavascriptParserUrl,
        JavascriptParserWorkerOptions, ParserOptions,
    };
    let options = JavascriptParserOptions {
        dynamic_import_mode: Some(DynamicImportMode::Lazy),
        dynamic_import_preload: Some(JavascriptParserOrder::Disable),
        dynamic_import_prefetch: Some(JavascriptParserOrder::Disable),
        url: Some(JavascriptParserUrl::Enable),
        expr_context_critical: Some(true),
        unknown_context_critical: Some(true),
        wrapped_context_critical: Some(false),
        strict_this_context_on_imports: Some(false),
        wrapped_context_reg_exp: Some(RspackRegex::new(".*").expect("regex")),
        worker: Some(JavascriptParserWorkerOptions::new(vec!["...".to_string()], None)),
        import_meta: Some(ImportMeta::Enabled),
        require_alias: Some(false),
        require_as_expression: Some(true),
        require_dynamic: Some(true),
        require_resolve: Some(true),
        commonjs: Some(JavascriptParserCommonjsOptions { exports: JavascriptParserCommonjsExportsOption::Enable }),
        import_dynamic: Some(true),
        commonjs_magic_comments: Some(false),
        jsx: Some(false),
        ..Default::default()
    };
    [("javascript".to_string(), ParserOptions::Javascript(options))].into_iter().collect()
}

/// The first page lab's stable layout: one chunk per package and per project
/// folder, so a module's chunk never depends on which entries exist.
fn stable_chunks(root: &str) -> rspack_plugin_split_chunks::SplitChunksPlugin {
    use rspack_plugin_split_chunks::{CacheGroup, CacheGroupTest, ChunkFilter, ChunkNameGetter, FallbackCacheGroup, PluginOptions, SplitChunkSizes};
    let root = root.to_string();
    let file_of = |module: &dyn rspack_core::Module| module.name_for_condition().map(|name| name.to_string()).unwrap_or_default();
    let test = CacheGroupTest::Fn(Arc::new(move |ctx| {
        let file = file_of(ctx.module);
        let keep = !file.is_empty() && !file.contains("/.workbench-rspack") && !file.contains("/.workbench-astro");
        Box::pin(async move { Ok(Some(keep)) })
    }));
    let name = ChunkNameGetter::Fn(Arc::new(move |ctx| {
        let file = file_of(ctx.module);
        let name = if let Some(at) = file.rfind("/node_modules/") {
            let rest: Vec<&str> = file[at + "/node_modules/".len()..].split('/').collect();
            let package = if rest[0].starts_with('@') && rest.len() > 1 { format!("{}-{}", rest[0], rest[1]) } else { rest[0].to_string() };
            format!("pkg-{}", clean(&package))
        } else {
            let folder = std::path::Path::new(&file).parent().and_then(|parent| parent.strip_prefix(&root).ok()).map(|path| path.to_string_lossy().replace('/', "-")).unwrap_or_default();
            format!("dir-{}", clean(if folder.is_empty() { "root" } else { &folder }))
        };
        Box::pin(async move { Ok(Some(name)) })
    }));
    let group = CacheGroup {
        key: "stable".into(),
        chunk_filter: ChunkFilter::All,
        test,
        r#type: rspack_plugin_split_chunks::create_default_module_type_filter(),
        layer: rspack_plugin_split_chunks::create_default_module_layer_filter(),
        name,
        priority: 0.0,
        min_size: SplitChunkSizes::empty(),
        min_size_reduction: SplitChunkSizes::empty(),
        enforce_size_threshold: SplitChunkSizes::empty(),
        reuse_existing_chunk: false,
        min_chunks: 1,
        id_hint: "stable".into(),
        max_initial_requests: f64::INFINITY,
        max_async_requests: f64::INFINITY,
        max_async_size: SplitChunkSizes::empty(),
        max_initial_size: SplitChunkSizes::empty(),
        filename: None,
        automatic_name_delimiter: "-".into(),
        used_exports: false,
    };
    let fallback = FallbackCacheGroup {
        chunks_filter: ChunkFilter::All,
        min_size: SplitChunkSizes::empty(),
        max_async_size: SplitChunkSizes::empty(),
        max_initial_size: SplitChunkSizes::empty(),
        automatic_name_delimiter: "-".into(),
    };
    rspack_plugin_split_chunks::SplitChunksPlugin::new(PluginOptions { dedup_depth: 0, cache_groups: vec![group], fallback_cache_group: fallback, hide_path_info: None })
}

fn clean(name: &str) -> String {
    name.chars().map(|char| if char.is_ascii_alphanumeric() || char == '-' { char } else { '-' }).collect()
}

fn regex_escape(text: &str) -> String {
    regex::escape(text)
}

pub struct Host {
    compiler: Compiler,
    overlay: Overlay,
    set: Arc<Mutex<EntrySet>>,
    hot_client: String,
    out: Arc<MemoryFileSystem>,
    builds: u32,
    root: String,
    watcher: Option<rspack_watcher::FsWatcher>,
    seen: Seen,
}

impl Host {
    pub fn new(job: &Job) -> Self {
        let overlay = Overlay::new();
        overlay.write(&job.hot_client.file, &job.hot_client.source);
        let set = Arc::new(Mutex::new(EntrySet::default()));
        let context = Context::from(job.root.as_str());
        let out = Arc::new(MemoryFileSystem::default());
        let compiler = build_compiler(job, &overlay, set.clone(), context.clone(), out.clone());
        let mut host = Self { compiler, overlay, set, hot_client: job.hot_client.file.clone(), out, builds: 0, root: job.root.clone(), watcher: None, seen: Seen::default() };
        host.add_entries(&job.entries);
        host
    }

    fn add_entries(&mut self, entries: &[Entry]) {
        let mut set = self.set.lock().expect("entries");
        for entry in entries {
            self.overlay.write(&entry.file, &entry.source);
            set.add(&entry.slug, &[self.hot_client.clone(), entry.file.clone()]);
        }
    }

    pub async fn build(&mut self) -> Value {
        let started = Instant::now();
        let result = if self.builds == 0 { self.compiler.build().await } else { self.compiler.rebuild(Default::default(), Default::default()).await };
        self.builds += 1;
        self.answer(started, result)
    }

    pub async fn add(&mut self, entries: &[Entry]) -> Value {
        self.add_entries(entries);
        let started = Instant::now();
        let changed = entries.iter().map(|entry| entry.file.clone()).collect();
        let result = self.compiler.rebuild(changed, Default::default()).await;
        self.builds += 1;
        self.answer(started, result)
    }

    pub async fn rebuild(&mut self, changed: Vec<String>, deleted: Vec<String>) -> Value {
        let started = Instant::now();
        let result = self.compiler.rebuild(changed.into_iter().collect(), deleted.into_iter().collect()).await;
        self.builds += 1;
        self.answer(started, result)
    }

    fn answer(&self, started: Instant, result: rspack_error::Result<()>) -> Value {
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        if let Err(error) = result {
            return json!({ "ok": false, "ms": ms, "error": error.to_string() });
        }
        let compilation = &self.compiler.compilation;
        let errors: Vec<String> = compilation.get_errors().map(|error| error.message.clone()).collect();
        let mut entrypoints = serde_json::Map::new();
        for (name, ukey) in compilation.entrypoints() {
            let group = compilation.build_chunk_graph_artifact.chunk_group_by_ukey.expect_get(ukey);
            let mut files = Vec::new();
            for chunk in &group.chunks {
                let chunk = compilation.build_chunk_graph_artifact.chunk_by_ukey.expect_get(chunk);
                files.extend(chunk.files().iter().cloned());
            }
            entrypoints.insert(name.clone(), json!(files));
        }
        // Which split chunk holds each project module, for the moved-module check.
        let mut chunk_of = serde_json::Map::new();
        for chunk in compilation.build_chunk_graph_artifact.chunk_by_ukey.values() {
            let Some(name) = chunk.name() else { continue };
            if !(name.starts_with("dir-") || name.starts_with("pkg-")) {
                continue;
            }
            for module in compilation.build_chunk_graph_artifact.chunk_graph.get_chunk_modules_identifier(&chunk.ukey()) {
                let id = module.to_string();
                if id.contains("node_modules") {
                    continue;
                }
                chunk_of.insert(id, json!(name));
            }
        }
        let mut assets: Vec<String> = compilation.assets().keys().cloned().collect();
        assets.sort();
        let hash = compilation.get_hash().map(str::to_string);
        json!({ "ok": true, "ms": ms, "hash": hash, "errors": errors, "entrypoints": entrypoints, "assets": assets, "chunk_of": chunk_of, "modules": compilation.get_module_graph().modules().count() })
    }

    // --------------------------------------------------------- stage 2

    /// A build's outcome for serving: each entry's files, runtime files,
    /// stylesheet chunks and the errors in its module graph, as the shipping
    /// pipeline's `snapshot` copies them out.
    pub fn built(&self, started: Instant, result: rspack_error::Result<()>) -> Built {
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        if let Err(error) = result {
            return Built { ok: false, ms, error: Some(error.to_string()), ..Default::default() };
        }
        let compilation = &self.compiler.compilation;
        let artifact = &compilation.build_chunk_graph_artifact;
        let errors: Vec<(Option<String>, String)> = compilation.get_errors().map(|error| (error.module_identifier.map(|id| id.to_string()), error.message.clone())).collect();
        let mut entrypoints = BTreeMap::new();
        for (name, ukey) in compilation.entrypoints() {
            let group = artifact.chunk_group_by_ukey.expect_get(ukey);
            let runtime_chunk = artifact.chunk_by_ukey.expect_get(&group.get_runtime_chunk(&artifact.chunk_group_by_ukey));
            let runtime_files: std::collections::HashSet<String> = runtime_chunk.files().iter().cloned().collect();
            let mut point = EntryPoint::default();
            let mut modules = std::collections::HashSet::new();
            for chunk_ukey in &group.chunks {
                let chunk = artifact.chunk_by_ukey.expect_get(chunk_ukey);
                for module in artifact.chunk_graph.get_chunk_modules_identifier(chunk_ukey) {
                    modules.insert(module.to_string());
                }
                for file in chunk.files() {
                    if file.starts_with("hot/") || file.contains(".hot-update.") {
                        continue;
                    }
                    point.files.push(file.clone());
                    if file.ends_with(".css") {
                        point.css.push((chunk.id().map(|id| json!(id.to_string())).unwrap_or(Value::Null), file.clone()));
                    }
                }
            }
            point.runtime = point.files.iter().filter(|file| runtime_files.contains(*file)).cloned().collect();
            point.errors = errors.iter().filter(|(module, _)| module.as_ref().is_none_or(|module| modules.contains(module))).map(|(_, message)| message.clone()).collect();
            entrypoints.insert(name.clone(), point);
        }
        let mut chunk_of = BTreeMap::new();
        for chunk in artifact.chunk_by_ukey.values() {
            let Some(name) = chunk.name() else { continue };
            if !(name.starts_with("dir-") || name.starts_with("pkg-")) {
                continue;
            }
            for module in artifact.chunk_graph.get_chunk_modules_identifier(&chunk.ukey()) {
                let id = module.to_string();
                if !id.contains("node_modules") {
                    chunk_of.insert(id, name.to_string());
                }
            }
        }
        Built { ok: true, ms, error: None, hash: compilation.get_hash().map(str::to_string), entrypoints, chunk_of, modules: compilation.get_module_graph().modules().count() }
    }

    /// The first build, or a rebuild after files changed.
    pub async fn build_files(&mut self, changed: Vec<String>, deleted: Vec<String>) -> Built {
        let started = Instant::now();
        let result = if self.builds == 0 { self.compiler.build().await } else { self.compiler.rebuild(changed.into_iter().collect(), deleted.into_iter().collect()).await };
        self.builds += 1;
        self.built(started, result)
    }

    /// Adds or replaces entry sources; the next build includes them.
    pub fn put_entries(&mut self, entries: &[Entry]) -> Vec<String> {
        self.add_entries(entries);
        entries.iter().map(|entry| entry.file.clone()).collect()
    }

    /// Removes entries; the next build leaves them out.
    pub fn remove_entries(&mut self, slugs: &[String]) {
        let mut set = self.set.lock().expect("entries");
        for slug in slugs {
            set.remove(slug);
        }
    }

    /// The compiler's in-memory output, shared for serving.
    pub fn out(&self) -> Arc<MemoryFileSystem> {
        self.out.clone()
    }

    /// Every file the last build read, outside `node_modules`.
    pub fn file_dependencies(&self) -> Vec<String> {
        self.compiler.compilation.file_dependencies().0.map(|file| file.to_string_lossy().into_owned()).filter(|file| !file.contains("/node_modules/")).collect()
    }

    pub fn asset(&self, name: &str) -> Value {
        let compilation = &self.compiler.compilation;
        match compilation.assets().get(name).and_then(|asset| asset.get_source()) {
            Some(source) => json!({ "ok": true, "source": source.source().into_string_lossy() }),
            None => json!({ "ok": false, "error": "no such asset" }),
        }
    }

    pub async fn watch(&mut self, extra: Vec<String>) -> Value {
        use rspack_paths::InternedPath;
        let started = Instant::now();
        let mut files: Vec<InternedPath> = self.compiler.compilation.file_dependencies().0.filter(|file| !file.to_string_lossy().contains("/node_modules/")).cloned().collect();
        // Files outside the module graph, such as workbench.config.ts.
        files.extend(extra.iter().map(|file| InternedPath::from(file.as_str())));
        let count = files.len();
        let watcher = rspack_watcher::FsWatcher::new(
            rspack_watcher::FsWatcherOptions { aggregate_timeout: Some(5), ..Default::default() },
            rspack_watcher::FsWatcherIgnored::Regex(regex("/node_modules/|/\\.git/")),
        );
        watcher
            .watch(
                (files.into_iter(), std::iter::empty()),
                (std::iter::once(InternedPath::from(self.root.as_str())), std::iter::empty()),
                (std::iter::empty(), std::iter::empty()),
                std::time::SystemTime::now(),
                Box::new(self.seen.clone()),
                Box::new(Ignore),
            )
            .await;
        self.watcher = Some(watcher);
        json!({ "ok": true, "ms": started.elapsed().as_secs_f64() * 1000.0, "files": count })
    }

    pub fn events(&self) -> Value {
        let mut events = self.seen.events.lock().expect("events");
        let (changed, deleted) = std::mem::take(&mut *events);
        json!({ "ok": true, "changed": changed, "deleted": deleted })
    }

    pub async fn close(&self) {
        if let Some(watcher) = &self.watcher {
            let _ = watcher.close().await;
        }
        let _ = self.compiler.close().await;
        let _ = &self.out;
    }
}

fn build_compiler(job: &Job, overlay: &Overlay, set: Arc<Mutex<EntrySet>>, context: Context, out: Arc<MemoryFileSystem>) -> Compiler {
    use rspack::builder::{OptimizationOptionsBuilder, OutputOptionsBuilder};
    use rspack_core::{CacheOptions, ExternalItem, ExternalItemValue, Filename, Mode, PersistentCacheOptions, PublicPath, Resolve, StorageOptions};
    let public_path = format!("/_workbench/previews/rspack/web/{}/", job.generation);
    let unique_name = "wb_web".to_string();
    let alias = vec![
        ("@canonic2/workbench$".to_string(), vec![rspack_resolver::AliasValue::Path(format!("{}/../../sdk/api.js", job.preview_dir))]),
        ("react-refresh".to_string(), vec![rspack_resolver::AliasValue::Path(job.refresh.runtime_dir.clone())]),
    ];
    let extensions = [".tsx", ".ts", ".jsx", ".js", ".mjs", ".cjs", ".json", ".vue", ".html"].iter().map(|ext| ext.to_string()).collect();
    let resolve = Resolve {
        alias: Some(rspack_core::Alias::MergeAlias(alias)),
        extensions: Some(extensions),
        modules: Some(vec![format!("{}/node_modules", job.root), "node_modules".to_string()]),
        ..Default::default()
    };
    let mut output = OutputOptionsBuilder::default();
    output
        .path(format!("/out/web/{}", job.generation))
        .public_path(PublicPath::Filename(Filename::from(public_path)))
        .filename(Filename::from("[name].[contenthash:8].js"))
        .chunk_filename(Filename::from("chunks/[name].[contenthash:8].js"))
        .css_filename(Filename::from("[name].css"))
        .css_chunk_filename(Filename::from("chunks/[name].css"))
        .asset_module_filename(Filename::from("assets/[name].[hash:8][ext]"))
        .unique_name(unique_name.clone())
        .hot_update_chunk_filename(Filename::from("hot/[id].[fullhash].hot-update.js"))
        .hot_update_main_filename(Filename::from("hot/[runtime].[fullhash].hot-update.json"));
    let mut optimization = OptimizationOptionsBuilder::default();
    optimization.minimize(false).module_ids("named".to_string()).runtime_chunk(rspack_plugin_runtime_chunk::RuntimeChunkOptions { name: rspack_plugin_runtime_chunk::RuntimeChunkName::Single });
    let externals: rspack_core::ExternalItemObject =
        [("/_workbench/preview-runtime.js".to_string(), ExternalItemValue::String("__workbenchRuntime".to_string()))].into_iter().collect();
    let provide: rustc_hash::FxHashMap<String, Vec<String>> = [
        ("$ReactRefreshRuntime$".to_string(), vec![job.refresh.runtime.clone()]),
        ("__react_refresh_utils__".to_string(), vec![job.refresh.utils.clone()]),
    ]
    .into_iter()
    .collect();
    let define: rspack_plugin_javascript::define_plugin::DefineValue = [
        ("process.env.NODE_ENV".to_string(), json!("\"development\"")),
        ("__react_refresh_library__".to_string(), json!(serde_json::to_string(&unique_name).expect("name"))),
        ("__reload_on_runtime_errors__".to_string(), json!("false")),
    ]
    .into_iter()
    .collect();
    let cache = CacheOptions::Persistent(PersistentCacheOptions {
        build_dependencies: Vec::new(),
        version: "instant-first-page".to_string(),
        storage: StorageOptions::FileSystem { directory: Utf8PathBuf::from(&job.cache_dir) },
        portable: false,
        readonly: false,
        max_age: 1_000_000_000,
        max_memory_generations: rspack_core::MaxMemoryGenerations::Infinity,
    });
    let mut builder = Compiler::builder();
    builder
        .context(job.root.as_str())
        .mode(Mode::Development)
        .target(vec!["web".to_string()])
        .output(output)
        .optimization(optimization)
        .externals(ExternalItem::Object(externals))
        .externals_type("window".to_string())
        .resolve(resolve)
        .cache(cache)
        .module(ModuleOptions::builder().rules(rules(job)).parser(javascript_parser()))
        .experiments(Experiments::builder().css(true))
        .enable_loader_swc()
        .enable_loader_react_refresh()
        .input_filesystem(Arc::new(overlay.clone()))
        .intermediate_filesystem(Arc::new(NativeFileSystem::new(false)))
        .output_filesystem(out)
        .plugin(Box::new(EntriesPlugin::create(context.clone(), set)))
        .plugin(Box::new(rspack_plugin_hmr::HotModuleReplacementPlugin::default()))
        .plugin(Box::new(rspack_plugin_entry::EntryPlugin::new(context, job.refresh.entry.clone(), EntryOptions::default())))
        .plugin(Box::new(rspack_plugin_javascript::provide_plugin::ProvidePlugin::new(provide)))
        .plugin(Box::new(rspack_plugin_javascript::define_plugin::DefinePlugin::new(define)))
        .plugin(Box::new(RefreshRuntimePlugin::default()))
        // A compiler needs an entry to start; the hot client alone is harmless.
        .entry("__workbench_idle", job.hot_client.file.as_str());
    if job.stable_chunks {
        builder.plugin(Box::new(stable_chunks(&job.root)));
    }
    builder.build().expect("compiler options")
}

/// Runs the line protocol on stdin and stdout until `close` or end of input.
pub async fn serve() {
    let stdin = std::io::stdin();
    let mut host: Option<Host> = None;
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                writeln!(out, "{}", json!({ "ok": false, "error": error.to_string() })).ok();
                continue;
            }
        };
        let answer = match request["op"].as_str() {
            Some("start") => match serde_json::from_value::<Job>(request.clone()) {
                Ok(job) => {
                    let started = Instant::now();
                    if std::env::var_os("LAB_HOST_TRACE").is_some() {
                        eprintln!("[host] creating the compiler");
                    }
                    let mut created = Host::new(&job);
                    if std::env::var_os("LAB_HOST_TRACE").is_some() {
                        eprintln!("[host] building {} entries", job.entries.len());
                    }
                    let created_ms = started.elapsed().as_secs_f64() * 1000.0;
                    let mut answer = created.build().await;
                    answer["created_ms"] = json!(created_ms);
                    host = Some(created);
                    answer
                }
                Err(error) => json!({ "ok": false, "error": error.to_string() }),
            },
            Some("add") => match (host.as_mut(), serde_json::from_value::<Vec<Entry>>(request["entries"].clone())) {
                (Some(host), Ok(entries)) => host.add(&entries).await,
                _ => json!({ "ok": false, "error": "no host or bad entries" }),
            },
            Some("rebuild") => match host.as_mut() {
                Some(host) => {
                    let list = |key: &str| request[key].as_array().map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    host.rebuild(list("changed"), list("deleted")).await
                }
                None => json!({ "ok": false, "error": "no host" }),
            },
            Some("asset") => match host.as_ref() {
                Some(host) => host.asset(request["name"].as_str().unwrap_or_default()),
                None => json!({ "ok": false, "error": "no host" }),
            },
            Some("watch") => match host.as_mut() {
                Some(host) => {
                    let extra = request["files"].as_array().map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    host.watch(extra).await
                }
                None => json!({ "ok": false, "error": "no host" }),
            },
            Some("events") => match host.as_ref() {
                Some(host) => host.events(),
                None => json!({ "ok": false, "error": "no host" }),
            },
            Some("close") => {
                if let Some(host) = host.take() {
                    host.close().await;
                }
                writeln!(out, "{}", json!({ "ok": true })).ok();
                break;
            }
            _ => json!({ "ok": false, "error": "unknown op" }),
        };
        writeln!(out, "{answer}").ok();
        out.flush().ok();
    }
}
