//! One profile's Rspack compiler, owned by its builder task. Requests send it
//! entries, generated modules and file changes; it applies them in batches,
//! builds, and publishes what serving needs from each build, because Rspack
//! frees a compilation when the next build starts.
#![expect(
    clippy::unused_async,
    reason = "Rspack's hook macros need async functions, whether or not they await"
)]
use super::Profile;
use super::overlay::Overlay;
use rspack_core::{
    ChunkUkey, Compilation, CompilationAdditionalTreeRuntimeRequirements, CompilationParams,
    Compiler, CompilerCompilation, CompilerMake, Context, DependencyRef, DependencyType,
    EntryDependency, EntryOptions, Plugin, RuntimeGlobals, RuntimeModule,
};
use rspack_fs::{MemoryFileSystem, ReadableFileSystem as _};
use rspack_hook::{plugin, plugin_hook};
use rspack_paths::Utf8Path;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, watch};

// ------------------------------------------------------------------ entries

/// The entry set, which can grow while the compiler runs. Each entry's
/// dependencies are created once, on the first build that includes it, so
/// rebuilds reuse their modules. Creating them earlier would take dependency
/// IDs before the persistent cache restores, which Rspack does not allow.
#[derive(Default, Debug)]
pub(crate) struct EntrySet {
    entries: BTreeMap<String, (Vec<String>, Option<Vec<DependencyRef>>)>,
}

impl EntrySet {
    /// Adds an entry; returns whether it is new.
    fn add(&mut self, name: &str, requests: Vec<String>) -> bool {
        if self.entries.contains_key(name) {
            return false;
        }
        self.entries.insert(name.to_owned(), (requests, None));
        true
    }

    /// One entry, replacing every other: a portable build's next output.
    pub(crate) fn replace(&mut self, name: &str, requests: Vec<String>) {
        self.entries.clear();
        self.entries.insert(name.to_owned(), (requests, None));
    }

    fn dependencies(&mut self, context: &Context) -> Vec<(String, Vec<DependencyRef>)> {
        self.entries
            .iter_mut()
            .map(|(name, (requests, dependencies))| {
                let dependencies = dependencies.get_or_insert_with(|| {
                    requests
                        .iter()
                        .map(|request| {
                            DependencyRef::new(EntryDependency::new(
                                request.clone(),
                                context.clone(),
                                None,
                                false,
                            ))
                        })
                        .collect()
                });
                (name.clone(), dependencies.clone())
            })
            .collect()
    }
}

#[plugin]
#[derive(Debug)]
pub(crate) struct EntriesPlugin {
    context: Context,
    set: Arc<Mutex<EntrySet>>,
}

impl EntriesPlugin {
    pub(crate) fn create(context: Context, set: Arc<Mutex<EntrySet>>) -> Self {
        Self::new_inner(context, set)
    }
}

#[plugin_hook(CompilerCompilation for EntriesPlugin)]
async fn entries_compilation(
    &self,
    compilation: &mut Compilation,
    params: &mut CompilationParams,
) -> rspack_error::Result<()> {
    compilation.set_dependency_factory(DependencyType::Entry, params.normal_module_factory.clone());
    Ok(())
}

#[plugin_hook(CompilerMake for EntriesPlugin)]
async fn entries_make(&self, compilation: &mut Compilation) -> rspack_error::Result<()> {
    let entries = self
        .set
        .lock()
        .expect("entry set lock")
        .dependencies(&self.context);
    for (name, dependencies) in entries {
        for dependency in dependencies {
            compilation
                .add_entry(
                    dependency,
                    EntryOptions {
                        name: Some(name.clone()),
                        ..Default::default()
                    },
                )
                .await?;
        }
    }
    Ok(())
}

impl Plugin for EntriesPlugin {
    fn name(&self) -> &'static str {
        "WorkbenchEntriesPlugin"
    }

    fn apply(&self, context: &mut rspack_core::ApplyContext<'_>) -> rspack_error::Result<()> {
        context
            .compiler_hooks
            .compilation
            .tap(entries_compilation::new(self));
        context.compiler_hooks.make.tap(entries_make::new(self));
        Ok(())
    }
}

/// React Refresh needs the module cache in every runtime, as Rspack's
/// JavaScript refresh plugin adds.
#[plugin]
#[derive(Debug, Default)]
pub(crate) struct RefreshRuntimePlugin;

impl RefreshRuntimePlugin {
    pub(crate) fn create() -> Self {
        Self::new_inner()
    }
}

#[plugin_hook(CompilationAdditionalTreeRuntimeRequirements for RefreshRuntimePlugin)]
async fn refresh_runtime(
    &self,
    _compilation: &Compilation,
    _chunk: &ChunkUkey,
    runtime_requirements: &mut RuntimeGlobals,
    _runtime_modules: &mut Vec<Box<dyn RuntimeModule>>,
) -> rspack_error::Result<()> {
    runtime_requirements.insert(RuntimeGlobals::MODULE_CACHE);
    Ok(())
}

impl Plugin for RefreshRuntimePlugin {
    fn name(&self) -> &'static str {
        "WorkbenchRefreshRuntimePlugin"
    }

    fn apply(&self, context: &mut rspack_core::ApplyContext<'_>) -> rspack_error::Result<()> {
        context
            .compilation_hooks
            .additional_tree_runtime_requirements
            .tap(refresh_runtime::new(self));
        Ok(())
    }
}

// ----------------------------------------------------------------- snapshots

/// One entry's served files, runtime files, stylesheet chunks and the errors
/// in its module graph.
#[derive(Clone, Debug, Default)]
pub(crate) struct Point {
    pub(crate) files: Vec<String>,
    pub(crate) runtime: Vec<String>,
    pub(crate) css: Vec<(Value, String)>,
    pub(crate) errors: Vec<String>,
}

/// What serving needs from one finished build.
#[derive(Clone, Debug, Default)]
pub(crate) struct Snapshot {
    pub(crate) hash: Option<String>,
    pub(crate) entrypoints: BTreeMap<String, Point>,
    /// The whole build failed: every entry shows this error.
    pub(crate) failure: Option<String>,
    pub(crate) ms: f64,
}

impl Snapshot {
    /// Each entry with build errors, at most three of them, as pushed to frames.
    pub(crate) fn errors_by_entry(&self) -> serde_json::Map<String, Value> {
        self.entrypoints
            .iter()
            .filter(|(_, point)| !point.errors.is_empty())
            .map(|(slug, point)| {
                (
                    slug.clone(),
                    Value::from(point.errors.iter().take(3).cloned().collect::<Vec<_>>()),
                )
            })
            .collect()
    }
}

fn snapshot(compilation: &Compilation, ms: f64) -> Snapshot {
    let artifact = &compilation.build_chunk_graph_artifact;
    let errors: Vec<(Option<String>, String)> = compilation
        .get_errors()
        .map(|error| {
            (
                error.module_identifier.map(|id| id.to_string()),
                error
                    .render_report(false)
                    .unwrap_or_else(|_| error.error.to_string()),
            )
        })
        .collect();
    let mut entrypoints = BTreeMap::new();
    for (name, ukey) in compilation.entrypoints() {
        let group = artifact.chunk_group_by_ukey.expect_get(ukey);
        let runtime_chunk = artifact
            .chunk_by_ukey
            .expect_get(&group.get_runtime_chunk(&artifact.chunk_group_by_ukey));
        let runtime_files: HashSet<String> = runtime_chunk.files().iter().cloned().collect();
        let mut point = Point::default();
        let mut modules = HashSet::new();
        for chunk_ukey in &group.chunks {
            let chunk = artifact.chunk_by_ukey.expect_get(chunk_ukey);
            for module in artifact
                .chunk_graph
                .get_chunk_modules_identifier(chunk_ukey)
            {
                modules.insert(module.to_string());
            }
            // Rspack also lists an entry's hot-update files; only hot updates load those.
            for file in chunk.files() {
                if file.starts_with("hot/") || file.contains(".hot-update.") {
                    continue;
                }
                point.files.push(file.clone());
                if Utf8Path::new(file).extension() == Some("css") {
                    let id = chunk
                        .id()
                        .map_or(Value::Null, |id| Value::String(id.to_string()));
                    point.css.push((id, file.clone()));
                }
            }
        }
        point.runtime = point
            .files
            .iter()
            .filter(|file| runtime_files.contains(*file))
            .cloned()
            .collect();
        point.errors = errors
            .iter()
            .filter(|(module, _)| {
                module
                    .as_ref()
                    .is_none_or(|module| modules.contains(module))
            })
            .map(|(_, message)| message.clone())
            .collect();
        entrypoints.insert(name.clone(), point);
    }
    Snapshot {
        hash: compilation.get_hash().map(str::to_owned),
        entrypoints,
        failure: None,
        ms,
    }
}

/// Files outside `node_modules` whose change can change a build's output.
fn watched(compilation: &Compilation) -> HashSet<String> {
    let mut files = HashSet::new();
    let mut add = |path: &rspack_paths::InternedPath| {
        let path = path.to_string_lossy();
        if !path.contains("/node_modules/") {
            files.insert(path.into_owned());
        }
    };
    compilation.file_dependencies().0.for_each(&mut add);
    compilation.context_dependencies().0.for_each(&mut add);
    compilation.missing_dependencies().0.for_each(&mut add);
    files
}

/// Files and folders outside `node_modules` the build read, for watching
/// those that lie outside the project root.
fn sources(compilation: &Compilation) -> HashSet<String> {
    let mut files = HashSet::new();
    let mut add = |path: &rspack_paths::InternedPath| {
        let path = path.to_string_lossy();
        if !path.contains("/node_modules/") {
            files.insert(path.into_owned());
        }
    };
    compilation.file_dependencies().0.for_each(&mut add);
    compilation.context_dependencies().0.for_each(&mut add);
    files
}

// --------------------------------------------------------------------- host

/// A module the server generates, written over the input file system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Module {
    pub(crate) file: String,
    pub(crate) source: String,
}

/// An entry: its name and the modules it starts from.
#[derive(Clone, Debug)]
pub(crate) struct EntryRequest {
    pub(crate) slug: String,
    pub(crate) requests: Vec<String>,
}

enum Command {
    Put {
        entries: Vec<EntryRequest>,
        modules: Vec<Module>,
    },
    Files {
        changed: HashSet<String>,
        deleted: HashSet<String>,
    },
    Close(oneshot::Sender<()>),
}

#[derive(Clone, Default)]
struct Published {
    /// The last command number a finished build included.
    done: u64,
    snapshot: Option<Arc<Snapshot>>,
}

/// What a finished build announces: its profile, generation and snapshot.
pub(crate) type Announce = Arc<dyn Fn(Profile, u64, &Snapshot) + Send + Sync>;

/// Builds a profile's compiler inside its builder task.
pub(crate) type Factory = Box<
    dyn FnOnce(&Overlay, Arc<Mutex<EntrySet>>, Arc<MemoryFileSystem>) -> anyhow::Result<Compiler>
        + Send,
>;

pub(crate) struct Host {
    pub(crate) profile: Profile,
    pub(crate) generation: u64,
    commands: mpsc::UnboundedSender<(u64, Command)>,
    sent: AtomicU64,
    published: watch::Receiver<Published>,
    out: Arc<MemoryFileSystem>,
    /// Files the last build depends on.
    depends: Arc<Mutex<HashSet<String>>>,
    /// Files and folders the last successful build read.
    sources: Arc<Mutex<HashSet<String>>>,
}

/// How long a request waits for a build before it fails.
const BUILD_DEADLINE: Duration = Duration::from_mins(2);

impl Host {
    pub(crate) fn start(
        profile: Profile,
        generation: u64,
        factory: Factory,
        announce: Announce,
    ) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let (publish, published) = watch::channel(Published::default());
        let out = Arc::new(MemoryFileSystem::default());
        let depends = Arc::new(Mutex::new(HashSet::new()));
        let sources = Arc::new(Mutex::new(HashSet::new()));
        let task = Builder {
            profile,
            generation,
            out: out.clone(),
            depends: depends.clone(),
            sources: sources.clone(),
            publish,
            announce,
        };
        let context = Arc::new(rspack_tasks::CompilerContext::new());
        tokio::spawn(rspack_tasks::within_compiler_context(
            context,
            task.run(factory, receiver),
        ));
        Self {
            profile,
            generation,
            commands,
            sent: AtomicU64::new(0),
            published,
            out,
            depends,
            sources,
        }
    }

    /// Files and folders the last successful build read.
    pub(crate) fn sources(&self) -> Vec<String> {
        self.sources
            .lock()
            .expect("sources lock")
            .iter()
            .cloned()
            .collect()
    }

    fn send(&self, command: Command) -> u64 {
        let number = self.sent.fetch_add(1, Ordering::SeqCst) + 1;
        // A closed builder answers nothing; waiting requests time out.
        let _sent = self.commands.send((number, command));
        number
    }

    /// Adds entries and writes generated modules; the next build includes them.
    pub(crate) fn put(&self, entries: Vec<EntryRequest>, modules: Vec<Module>) {
        self.send(Command::Put { entries, modules });
    }

    /// Reports changed files; the builder rebuilds when one is in its graph.
    pub(crate) fn files(&self, changed: &HashSet<String>, deleted: &HashSet<String>) {
        let depends = self.depends.lock().expect("dependencies lock");
        // A file in the graph, or a file in a folder the graph lists, which
        // also changes that folder: a glob or sprite reads its entries.
        let mut folders = HashSet::new();
        let mut relevant = |files: &HashSet<String>| -> HashSet<String> {
            let mut found = HashSet::new();
            for file in files {
                let listed: Vec<String> = std::path::Path::new(file)
                    .ancestors()
                    .skip(1)
                    .map(|folder| folder.to_string_lossy().into_owned())
                    .filter(|folder| depends.contains(folder))
                    .collect();
                if depends.contains(file) || !listed.is_empty() {
                    found.insert(file.clone());
                }
                folders.extend(listed);
            }
            found
        };
        let (mut changed, deleted) = (relevant(changed), relevant(deleted));
        changed.extend(folders);
        drop(depends);
        if changed.is_empty() && deleted.is_empty() {
            return;
        }
        self.send(Command::Files { changed, deleted });
    }

    /// The first finished build that includes everything sent before this call.
    pub(crate) async fn settled(&self) -> anyhow::Result<Arc<Snapshot>> {
        let wanted = self.sent.load(Ordering::SeqCst);
        let mut published = self.published.clone();
        let wait = async {
            loop {
                {
                    let current = published.borrow_and_update();
                    if current.done >= wanted
                        && let Some(snapshot) = &current.snapshot
                    {
                        return Ok(snapshot.clone());
                    }
                }
                if published.changed().await.is_err() {
                    anyhow::bail!("the preview compiler stopped");
                }
            }
        };
        tokio::time::timeout(BUILD_DEADLINE, wait)
            .await
            .map_err(|_| anyhow::anyhow!("the preview compiler did not finish a build in time"))?
    }

    /// A finished build that contains the entry.
    pub(crate) async fn contains(&self, slug: &str) -> anyhow::Result<Arc<Snapshot>> {
        let snapshot = self.settled().await?;
        if snapshot.entrypoints.contains_key(slug) || snapshot.failure.is_some() {
            return Ok(snapshot);
        }
        anyhow::bail!("the preview compiler did not build {slug}")
    }

    /// One built file, by its name under this generation's output.
    pub(crate) fn read(&self, name: &str) -> Option<Vec<u8>> {
        let path = format!("/out/{}/{}/{name}", self.profile.name(), self.generation);
        self.out.read_sync(Utf8Path::new(&path)).ok()
    }

    pub(crate) async fn close(&self) {
        let (done, closed) = oneshot::channel();
        self.send(Command::Close(done));
        let _closed = tokio::time::timeout(Duration::from_secs(10), closed).await;
    }
}

struct Builder {
    profile: Profile,
    generation: u64,
    out: Arc<MemoryFileSystem>,
    depends: Arc<Mutex<HashSet<String>>>,
    sources: Arc<Mutex<HashSet<String>>>,
    publish: watch::Sender<Published>,
    announce: Announce,
}

impl Builder {
    async fn run(self, factory: Factory, mut commands: mpsc::UnboundedReceiver<(u64, Command)>) {
        let overlay = Overlay::new();
        let set = Arc::new(Mutex::new(EntrySet::default()));
        let mut compiler = match factory(&overlay, set.clone(), self.out.clone()) {
            Ok(compiler) => compiler,
            Err(error) => {
                self.fail(u64::MAX, &format!("{error:#}"));
                return;
            }
        };
        let mut first = true;
        let mut closing = None;
        while closing.is_none() {
            let Some(next) = commands.recv().await else {
                break;
            };
            let mut batch = vec![next];
            // Edits that arrive together build together.
            tokio::time::sleep(Duration::from_millis(5)).await;
            while let Ok(more) = commands.try_recv() {
                batch.push(more);
            }
            let mut done = 0;
            let mut changed = HashSet::new();
            let mut deleted = HashSet::new();
            let mut added = false;
            for (number, command) in batch {
                done = done.max(number);
                match command {
                    Command::Put { entries, modules } => {
                        for module in modules {
                            if overlay.write(&module.file, &module.source) {
                                changed.insert(module.file);
                            }
                        }
                        let mut set = set.lock().expect("entry set lock");
                        for entry in entries {
                            added |= set.add(&entry.slug, entry.requests);
                        }
                    }
                    Command::Files {
                        changed: files,
                        deleted: gone,
                    } => {
                        changed.extend(files);
                        deleted.extend(gone);
                    }
                    Command::Close(done) => closing = Some(done),
                }
            }
            if closing.is_some() {
                break;
            }
            // Nothing new: the last build already includes everything sent.
            if !first && !added && changed.is_empty() && deleted.is_empty() {
                self.publish.send_modify(|published| published.done = done);
                continue;
            }
            let started = Instant::now();
            let result = if first {
                compiler.build().await
            } else {
                compiler
                    .rebuild(changed.into_iter().collect(), deleted.into_iter().collect())
                    .await
            };
            first = false;
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            let snapshot = match result {
                Ok(()) => {
                    *self.depends.lock().expect("dependencies lock") =
                        watched(&compiler.compilation);
                    *self.sources.lock().expect("sources lock") = sources(&compiler.compilation);
                    snapshot(&compiler.compilation, ms)
                }
                Err(error) => Snapshot {
                    failure: Some(error.to_string()),
                    ms,
                    ..Snapshot::default()
                },
            };
            (self.announce)(self.profile, self.generation, &snapshot);
            let snapshot = Arc::new(snapshot);
            self.publish.send_modify(|published| {
                published.done = done;
                published.snapshot = Some(snapshot);
            });
        }
        let _closed = compiler.close().await;
        if let Some(done) = closing {
            let _sent = done.send(());
        }
    }

    fn fail(&self, done: u64, message: &str) {
        let snapshot = Snapshot {
            failure: Some(message.to_owned()),
            ..Snapshot::default()
        };
        (self.announce)(self.profile, self.generation, &snapshot);
        self.publish.send_modify(|published| {
            published.done = done;
            published.snapshot = Some(Arc::new(snapshot));
        });
    }
}
