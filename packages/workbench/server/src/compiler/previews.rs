//! The previews service: entries for previews and docs lenses, prepared from
//! their definitions and the project's configuration, built by one compiler
//! per profile and served from memory, with builds pushed to open canvases.
//!
//! Every discovered preview and docs lens is declared and built before the
//! first preview is served, because Rspack's hot updates do not follow a
//! module that moves to another chunk; the entry set stays fixed until the
//! configuration changes, which starts a new generation.
use super::client::{BASE, EntryAssets, EntryKind, Style, preview_module_source};
use super::docs;
use super::entries::{
    DocsEntry, DocsExamples, PreviewEntry, SourceImport, digest, docs_slug, entry_slug,
    preview_entry_source,
};
use super::exports::Resolution;
use super::host::{EntryRequest, Host, Module, Snapshot};
use super::options::{self, Build, Purpose};
use super::project::{self, ProjectConfig};
use super::{Package, Profile};
use crate::node::{Evaluator, Program};
use anyhow::{Context as _, Result, anyhow, bail};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{OnceCell, broadcast, mpsc};

/// What the Node server supplies when it lists the space's pages.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Catalog {
    /// Listed preview definitions, relative to the root.
    pub(crate) previews: Vec<String>,
    /// Every definition discovery found, listed or not, relative to the root:
    /// one that cannot be listed still builds to show its own error.
    pub(crate) declared: Vec<String>,
    /// Each lens of each discovered docs definition, as the lens's request.
    pub(crate) lenses: Vec<Value>,
    /// Docs pages' Markdown files, relative to the root.
    pub(crate) pages: Vec<String>,
}

/// Previews as the Node server configured them for this space.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Settings {
    /// `previews` from `workbench.yaml`: `include`, `config` and the rest.
    pub(crate) options: Value,
    pub(crate) catalog: Catalog,
    /// The Node runtime for the evaluator, supplied in a trusted workspace.
    pub(crate) node: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub(crate) slug: String,
    pub(crate) kind: EntryKind,
    pub(crate) profile: Profile,
    pub(crate) title: String,
    pub(crate) file: String,
    pub(crate) source: String,
    /// A docs lens's examples, with project-relative files.
    pub(crate) examples: Vec<Value>,
    pub(crate) problems: Vec<String>,
    /// Generated modules the entry imports, such as Astro renders.
    pub(crate) modules: Vec<Module>,
}

/// What renders again when a file an Astro render read changes.
#[derive(Clone, Debug)]
pub(crate) enum Render {
    Preview(String),
    Docs(Value),
}

/// The evaluated project configuration and the files it was read from.
pub(super) struct Project {
    pub(super) config: Arc<ProjectConfig>,
    pub(super) files: HashSet<PathBuf>,
}

struct State {
    settings: Option<Settings>,
    generation: u64,
    hosts: BTreeMap<Profile, Arc<Host>>,
    entries: HashMap<String, Entry>,
    warm: Arc<OnceCell<()>>,
    project: Option<Arc<Project>>,
}

pub(crate) struct Previews {
    root: PathBuf,
    virtual_dir: PathBuf,
    package: Package,
    script: PathBuf,
    cache_root: PathBuf,
    identity: String,
    evaluator: Arc<Evaluator>,
    events: broadcast::Sender<String>,
    state: tokio::sync::Mutex<State>,
    watcher: Mutex<Option<RecommendedWatcher>>,
    /// Folders outside the root that the watcher also watches, one level deep.
    outside: Mutex<HashSet<PathBuf>>,
    /// Asks for `outside` to follow what builds, renders and the
    /// configuration read now.
    rewatch: Arc<tokio::sync::Notify>,
    /// Serializes configuration reloads.
    reloading: tokio::sync::Mutex<()>,
    /// The production compilers of portable builds, for one generation.
    pub(super) portable: tokio::sync::Mutex<Option<(u64, Arc<super::portable::Portable>)>>,
    /// Each Astro render's entry, with the files it read.
    renders: Mutex<HashMap<String, (HashSet<PathBuf>, Render)>>,
    /// Recent Astro input edits, served by name.
    variants: Mutex<std::collections::VecDeque<(String, Arc<super::astro::Variant>)>>,
}

fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis())
}

fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The running binary's identity, so a new Workbench never reuses cached
/// output its predecessor's loaders produced.
fn binary_identity() -> String {
    let stamp = std::env::current_exe()
        .and_then(std::fs::metadata)
        .map(|metadata| {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |time| time.as_nanos());
            format!("{modified}:{}", metadata.len())
        })
        .unwrap_or_default();
    format!("{}-{}", env!("CARGO_PKG_VERSION"), digest(&stamp))
}

impl Previews {
    pub(crate) fn new(root: &Path, product: &Path) -> Result<Arc<Self>> {
        // Canonical, so the watcher's paths match the ones Rspack resolves.
        let root = std::fs::canonicalize(root).context("preview root")?;
        let package = Package::locate(product)?;
        Ok(Arc::new(Self {
            virtual_dir: root.join(".workbench-rspack"),
            cache_root: std::env::temp_dir()
                .join("canonic-workbench-rspack")
                .join(digest(&text(&root))),
            evaluator: crate::node::for_root(&root),
            script: product.join("server/node/preview/evaluator.cjs"),
            root,
            package,
            identity: binary_identity(),
            events: broadcast::channel(256).0,
            state: tokio::sync::Mutex::new(State {
                settings: None,
                generation: 1,
                hosts: BTreeMap::new(),
                entries: HashMap::new(),
                warm: Arc::new(OnceCell::new()),
                project: None,
            }),
            watcher: Mutex::new(None),
            outside: Mutex::new(HashSet::new()),
            rewatch: Arc::new(tokio::sync::Notify::new()),
            reloading: tokio::sync::Mutex::new(()),
            portable: tokio::sync::Mutex::new(None),
            renders: Mutex::new(HashMap::new()),
            variants: Mutex::new(std::collections::VecDeque::new()),
        }))
    }

    /// Where the evaluator asks for a runtime when it first needs one.
    pub(crate) fn provide_runtime(&self, runtime: crate::node::Runtime) {
        self.evaluator.provide(runtime, self.script.clone());
    }

    /// Applies the space's preview settings. `None` turns previews off.
    pub(crate) async fn configure(self: &Arc<Self>, settings: Option<Settings>) {
        if let Some(node) = settings.as_ref().and_then(|settings| settings.node.clone()) {
            self.evaluator
                .supply(Program {
                    node,
                    script: self.script.clone(),
                })
                .await;
        }
        let off = settings.is_none();
        let restart = {
            let mut state = self.state.lock().await;
            let previous = state
                .settings
                .as_ref()
                .map(|settings| settings.options.clone());
            let next = settings.as_ref().map(|settings| settings.options.clone());
            let changed = previous != next;
            state.settings = settings;
            changed && previous.is_some()
        };
        if off {
            // Off closes the compilers; turning previews on again starts anew.
            self.unwatch();
            let hosts = {
                let mut state = self.state.lock().await;
                state.generation += 1;
                state.entries.clear();
                state.project = None;
                state.warm = Arc::new(OnceCell::new());
                std::mem::take(&mut state.hosts)
            };
            self.renders.lock().expect("renders lock").clear();
            for host in hosts.values() {
                host.close().await;
            }
            return;
        }
        if restart {
            self.reload(false).await;
        }
        self.watch();
        // Pre-warm: build every preview and docs lens before anything asks.
        let previews = self.clone();
        tokio::spawn(async move { previews.warmed().await });
    }

    pub(crate) async fn enabled(&self) -> bool {
        self.state.lock().await.settings.is_some()
    }

    /// Frames' event stream: every build, docs edits and full reloads.
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<String> {
        self.events.subscribe()
    }

    fn broadcast(&self, message: &Value) {
        let _sent = self.events.send(message.to_string());
    }

    // ---------------------------------------------------------- project

    pub(super) async fn project(&self) -> Result<Arc<Project>> {
        if let Some(project) = self.state.lock().await.project.clone() {
            return Ok(project);
        }
        let options = self
            .state
            .lock()
            .await
            .settings
            .as_ref()
            .map(|settings| settings.options.clone())
            .unwrap_or_default();
        let name = options
            .get("config")
            .and_then(Value::as_str)
            .unwrap_or("workbench.config.ts");
        let file = crate::paths::resolve(&self.root, name);
        if !file.starts_with(&self.root) {
            bail!("Preview config must be inside the project.");
        }
        let project = if file.exists() {
            let answer = self
                .evaluator
                .call("config", json!({ "options": options }))
                .await
                .context("workbench.config.ts")?;
            let config: ProjectConfig = serde_json::from_value(answer["config"].clone())
                .context("workbench.config.ts as data")?;
            let mut files: HashSet<PathBuf> = answer["files"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(Value::as_str)
                // Relative to the root, and may lead out of it.
                .map(|file| crate::paths::resolve(&self.root, file))
                .collect();
            files.insert(file);
            Project {
                config: Arc::new(config),
                files,
            }
        } else {
            Project {
                config: Arc::new(ProjectConfig::default()),
                files: HashSet::from([file]),
            }
        };
        let project = Arc::new(project);
        self.state.lock().await.project = Some(project.clone());
        self.rewatch.notify_one();
        Ok(project)
    }

    /// Configuration problems, shown with the page tree.
    pub(crate) async fn problems(&self) -> Vec<String> {
        match self.project().await {
            Ok(project) => {
                let mut problems = project.config.problems.clone();
                problems.extend(super::plugins::problems(&project.config));
                problems
            }
            Err(error) => vec![format!("{error:#}")],
        }
    }

    // ---------------------------------------------------------- compilers

    fn host(&self, state: &mut State, profile: Profile, project: &Arc<Project>) -> Arc<Host> {
        if let Some(host) = state.hosts.get(&profile) {
            return host.clone();
        }
        let generation = state.generation;
        let root = self.root.clone();
        let package = self.package.clone();
        let config = project.config.clone();
        let cache_dir = self.cache_root.join(profile.name());
        let identity = format!(
            "{}-{}",
            self.identity,
            digest(&serde_json::to_string(&json!({
                "aliases": config.aliases, "dedupe": config.dedupe, "extensions": config.resolve_extensions,
                "define": config.define, "plugins": config.plugins, "rules": config.rules, "adapters": config.adapters,
            }))
            .unwrap_or_default())
        );
        let factory: super::host::Factory = Box::new(move |overlay, entries, out| {
            let (rules, plugins) = super::plugins::compile(&config, profile, &root, overlay)?;
            let hot_client = Module {
                file: hot_client_file(&root, profile),
                source: super::client::hot_client_source(profile.name(), generation),
            };
            overlay.write(&hot_client.file, &hot_client.source);
            options::compiler(
                &Build {
                    profile,
                    generation,
                    root: &root,
                    package: &package,
                    project: &config,
                    cache_dir: &cache_dir,
                    purpose: Purpose::Live,
                    identity,
                },
                overlay,
                entries,
                out,
                rules,
                plugins,
            )
        });
        let events = self.events.clone();
        let rewatch = self.rewatch.clone();
        let announce: super::host::Announce =
            Arc::new(move |profile, generation, snapshot: &Snapshot| {
                let message = json!({
                    "type": "built", "profile": profile.name(), "generation": generation,
                    "hash": snapshot.hash.clone().unwrap_or_else(|| format!("failed-{}", now())),
                    "errors": snapshot.errors_by_entry(), "time": now(), "ms": snapshot.ms.round(),
                });
                let _sent = events.send(message.to_string());
                rewatch.notify_one();
            });
        let host = Arc::new(Host::start(profile, generation, factory, announce));
        state.hosts.insert(profile, host.clone());
        host
    }

    /// Adds or refreshes entries, in one batch per profile.
    async fn put(&self, prepared: Vec<Entry>) -> Result<Vec<(Arc<Host>, String)>> {
        let project = self.project().await?;
        let mut state = self.state.lock().await;
        let mut batches: BTreeMap<Profile, (Vec<EntryRequest>, Vec<Module>)> = BTreeMap::new();
        let mut waits = vec![];
        for entry in prepared {
            let host = self.host(&mut state, entry.profile, &project);
            let previous = state.entries.get(&entry.slug);
            let unchanged = previous.is_some_and(|existing| existing.source == entry.source);
            let same_modules = previous.is_some_and(|existing| existing.modules == entry.modules);
            let batch = batches.entry(entry.profile).or_default();
            if !same_modules {
                batch.1.extend(entry.modules.iter().cloned());
            }
            if !unchanged {
                batch.0.push(EntryRequest {
                    slug: entry.slug.clone(),
                    requests: vec![
                        hot_client_file(&self.root, entry.profile),
                        entry.file.clone(),
                    ],
                });
                batch.1.push(Module {
                    file: entry.file.clone(),
                    source: entry.source.clone(),
                });
            }
            waits.push((host, entry.slug.clone()));
            state.entries.insert(entry.slug.clone(), entry);
        }
        for (profile, (entries, modules)) in batches {
            if entries.is_empty() && modules.is_empty() {
                continue;
            }
            if let Some(host) = state.hosts.get(&profile) {
                host.put(entries, modules);
            }
        }
        Ok(waits)
    }

    async fn ensure(&self, entry: Entry) -> Result<(Entry, Arc<Host>, Arc<Snapshot>)> {
        let waits = self.put(vec![entry.clone()]).await?;
        let (host, slug) = waits.into_iter().next().context("entry host")?;
        let snapshot = host.contains(&slug).await?;
        Ok((entry, host, snapshot))
    }

    /// Declares every discovered preview and docs lens, then builds them all.
    async fn prewarm(&self) {
        let catalog = self
            .state
            .lock()
            .await
            .settings
            .as_ref()
            .map(|settings| settings.catalog.clone())
            .unwrap_or_default();
        let mut prepared = vec![];
        for file in &catalog.previews {
            match self.preview_entry(file).await {
                Ok(entry) => prepared.push(entry),
                Err(error) => eprintln!("[workbench] preview entry skipped: {file}: {error:#}"),
            }
        }
        for lens in &catalog.lenses {
            match self.docs_entry(lens).await {
                Ok(entry) => prepared.push(entry),
                Err(error) => eprintln!("[workbench] docs entry skipped: {error:#}"),
            }
        }
        match self.put(prepared).await {
            Ok(waits) => {
                for (host, slug) in waits {
                    let _built = host.contains(&slug).await;
                }
            }
            Err(error) => eprintln!("[workbench] previews could not start: {error:#}"),
        }
    }

    pub(super) async fn warmed(&self) {
        let warm = self.state.lock().await.warm.clone();
        warm.get_or_init(|| self.prewarm()).await;
    }

    /// Replaces the compilers after a configuration change. Documents that
    /// loaded the old generation reload into a fresh warm host.
    async fn reload(self: &Arc<Self>, announce: bool) {
        let _one_at_a_time = self.reloading.lock().await;
        let hosts = {
            let mut state = self.state.lock().await;
            state.generation += 1;
            state.entries.clear();
            state.project = None;
            self.renders.lock().expect("renders lock").clear();
            state.warm = Arc::new(OnceCell::new());
            std::mem::take(&mut state.hosts)
        };
        for host in hosts.values() {
            host.close().await;
        }
        // The configuration and what it imports load again in a new evaluator.
        self.evaluator.stop().await;
        self.warmed().await;
        if announce {
            self.broadcast(&json!({ "type": "reload-all", "time": now() }));
        }
    }

    // ---------------------------------------------------------- entries

    pub(super) async fn definition(&self, relative: &str) -> Result<(PathBuf, Value)> {
        let declared = self
            .state
            .lock()
            .await
            .settings
            .as_ref()
            .is_some_and(|settings| {
                let catalog = &settings.catalog;
                catalog
                    .previews
                    .iter()
                    .chain(&catalog.declared)
                    .any(|file| file == relative)
            });
        let file = crate::paths::resolve(&self.root, relative);
        if !declared || !file.starts_with(&self.root) {
            bail!("Not a declared Workbench preview definition.");
        }
        let source = std::fs::read_to_string(&file).with_context(|| format!("read {relative}"))?;
        let tsx = file.extension().is_some_and(|extension| extension == "tsx");
        let real_root = std::fs::canonicalize(&self.root).ok();
        if let Some(raw) = crate::literal::parse(source, tsx)
            && let Some(result) = crate::definition::validated(&raw, &file, &self.root, |target| {
                std::fs::canonicalize(target)
                    .ok()
                    .zip(real_root.as_deref())
                    .is_some_and(|(target, root)| target.starts_with(root))
            })
        {
            return result
                .map(|value| (file, value))
                .map_err(|error| anyhow!(error));
        }
        let options = self
            .state
            .lock()
            .await
            .settings
            .as_ref()
            .map(|settings| settings.options.clone())
            .unwrap_or_default();
        let answer = self
            .evaluator
            .call(
                "definition",
                json!({ "file": relative, "options": options }),
            )
            .await?;
        Ok((file, answer["value"].clone()))
    }

    async fn preview_entry(&self, relative: &str) -> Result<Entry> {
        let project = self.project().await?;
        let config = &project.config;
        let (file, definition) = self.definition(relative).await?;
        let id = definition["id"].as_str().context("definition id")?;
        let adapter = definition["adapter"]
            .as_str()
            .context("definition adapter")?;
        if project::built_in_astro(config, adapter) {
            return super::astro::preview_entry(self, relative, &file, &definition, config).await;
        }
        let slug = entry_slug(id);
        let directory = file.parent().unwrap_or(&self.root);
        let base = &definition["source"];
        let mut sources = vec![];
        let mut states = vec![("default".to_owned(), json!({}))];
        if let Some(map) = definition["states"].as_object() {
            states.extend(
                map.iter()
                    .map(|(name, state)| (name.clone(), state.clone())),
            );
        }
        for (state, spec) in states {
            let source = if spec["source"].is_object() {
                &spec["source"]
            } else {
                base
            };
            let entry = source["entry"].as_str().context("source entry")?;
            sources.push(SourceImport::Module {
                state,
                file: text(&crate::paths::resolve(directory, entry)),
                export_name: source["export"].as_str().unwrap_or("default").to_owned(),
            });
        }
        let adapter_import =
            project::adapter_import(config, &self.root, &self.package.preview_dir, adapter)
                .map_err(|error| anyhow!(error))?;
        let project_environment = project::project_environment(config, &self.root, adapter)
            .map_err(|error| anyhow!(error))?
            .map(|file| text(&file));
        let own_environment = definition["environment"]
            .as_str()
            .map(|environment| text(&crate::paths::resolve(directory, environment)));
        let styles: Vec<String> = definition["styles"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_str)
            .map(|style| text(&crate::paths::resolve(directory, style)))
            .collect();
        let source = preview_entry_source(&PreviewEntry {
            slug: &slug,
            definition: &text(&file),
            adapter: &adapter_import,
            sources: &sources,
            project_environment: project_environment.as_deref(),
            own_environment: own_environment.as_deref(),
            styles: &styles,
            portable: None,
        });
        Ok(Entry {
            file: text(&self.virtual_dir.join(format!("{slug}.js"))),
            slug,
            kind: EntryKind::Preview,
            profile: Profile::of(adapter),
            title: definition["title"].as_str().unwrap_or(id).to_owned(),
            source,
            examples: vec![],
            problems: vec![],
            modules: vec![],
        })
    }

    fn resolution_aliases(
        &self,
        config: &ProjectConfig,
        profile: Profile,
    ) -> Vec<(String, String)> {
        options::aliases(&self.root, config, profile, &self.package.api)
            .into_iter()
            .map(|(key, target)| (key.trim_end_matches('$').to_owned(), target))
            .collect()
    }

    /// A lens's examples as the docs page lists them.
    pub(super) fn listed(
        &self,
        request: &docs::Request,
        config: &ProjectConfig,
    ) -> Result<docs::Listed> {
        let profile = Profile::of(&request.adapter);
        let extensions: Vec<String> = config.resolve_extensions.clone().unwrap_or_else(|| {
            [
                ".tsx", ".ts", ".jsx", ".js", ".mjs", ".cjs", ".json", ".vue", ".html",
            ]
            .iter()
            .map(|extension| (*extension).to_owned())
            .collect()
        });
        let aliases = self.resolution_aliases(config, profile);
        let resolution = Resolution {
            root: &self.root,
            extensions: &extensions,
            aliases: &aliases,
        };
        docs::examples(
            request,
            &resolution,
            project::built_in_astro(config, &request.adapter),
        )
        .map_err(|error| anyhow!(error))
    }

    async fn docs_entry(&self, raw: &Value) -> Result<Entry> {
        let project = self.project().await?;
        let config = &project.config;
        let request = docs::request(raw, &self.root).map_err(|error| anyhow!(error))?;
        let listed = self.listed(&request, config)?;
        let slug = docs_slug(raw);
        if project::built_in_astro(config, &request.adapter) {
            return super::astro::docs_entry(self, raw, &slug, &request, &listed, config).await;
        }
        let examples = if request.folder {
            DocsExamples::Folder(
                listed
                    .examples
                    .iter()
                    .map(|example| (example.id.clone(), text(&example.file)))
                    .collect(),
            )
        } else {
            DocsExamples::Exports {
                file: text(&request.examples),
                examples: listed
                    .examples
                    .iter()
                    .map(|example| (example.id.clone(), example.export.clone()))
                    .collect(),
            }
        };
        let adapter = project::adapter_import(
            config,
            &self.root,
            &self.package.preview_dir,
            &request.adapter,
        )
        .map_err(|error| anyhow!(error))?;
        let project_environment =
            project::project_environment(config, &self.root, &request.adapter)
                .map_err(|error| anyhow!(error))?
                .map(|file| text(&file));
        let lens_environment = request.environment.as_deref().map(text);
        let styles: Vec<String> = request.styles.iter().map(|style| text(style)).collect();
        let source = super::entries::docs_entry_source(&DocsEntry {
            slug: &slug,
            adapter: &adapter,
            project_environment: project_environment.as_deref(),
            lens_environment: lens_environment.as_deref(),
            styles: &styles,
            examples: &examples,
            portable: None,
        });
        Ok(Entry {
            file: text(&self.virtual_dir.join(format!("{slug}.js"))),
            slug: slug.clone(),
            kind: EntryKind::Docs,
            profile: Profile::of(&request.adapter),
            title: slug,
            source,
            examples: self.listed_examples(&listed),
            problems: listed.problems,
            modules: vec![],
        })
    }

    pub(super) fn listed_examples(&self, listed: &docs::Listed) -> Vec<Value> {
        listed
            .examples
            .iter()
            .map(|example| {
                json!({
                    "id": example.id,
                    "file": slash(example.file.strip_prefix(&self.root).unwrap_or(&example.file)),
                    "export": example.export,
                })
            })
            .collect()
    }

    /// The previews settings from `workbench.yaml`, as the evaluator reads them.
    pub(super) async fn options(&self) -> Value {
        self.state
            .lock()
            .await
            .settings
            .as_ref()
            .map(|settings| settings.options.clone())
            .unwrap_or_default()
    }

    pub(super) fn watch_render(&self, slug: &str, files: Vec<PathBuf>, render: Render) {
        self.renders
            .lock()
            .expect("renders lock")
            .insert(slug.to_owned(), (files.into_iter().collect(), render));
        self.rewatch.notify_one();
    }

    /// An Astro input edit, rendered with its own assets.
    pub(crate) async fn render(
        &self,
        relative: &str,
        state: &str,
        inputs: &Value,
    ) -> Result<Value> {
        self.warmed().await;
        let project = self.project().await?;
        let (_, definition) = self.definition(relative).await?;
        let adapter = definition["adapter"].as_str().unwrap_or_default();
        if !project::built_in_astro(&project.config, adapter) {
            bail!("This preview does not use the built-in Astro adapter.");
        }
        let variant = super::astro::render_variant(self, relative, state, inputs).await?;
        let name = digest(&format!("{relative}\0{state}\0{inputs}"));
        let html = variant.html.clone();
        let mut variants = self.variants.lock().expect("variants lock");
        variants.retain(|(existing, _)| *existing != name);
        variants.push_back((name.clone(), Arc::new(variant)));
        while variants.len() > 32 {
            variants.pop_front();
        }
        Ok(json!({ "html": html, "base": format!("/_workbench/previews/preview/{name}/") }))
    }

    /// One asset of an Astro input edit.
    pub(crate) fn variant_asset(&self, name: &str, asset: &str) -> Option<Vec<u8>> {
        let variants = self.variants.lock().expect("variants lock");
        let (_, variant) = variants.iter().find(|(existing, _)| existing == name)?;
        variant.assets.get(asset).cloned()
    }

    async fn rerender(self: &Arc<Self>, slug: String, render: Render) {
        let entry = match &render {
            Render::Preview(relative) => self.preview_entry(relative).await,
            Render::Docs(raw) => self.docs_entry(raw).await,
        };
        match entry {
            Ok(entry) => {
                if let Err(error) = self.put(vec![entry]).await {
                    eprintln!("[workbench] Astro render failed: {error:#}");
                }
            }
            Err(error) => {
                let generation = self.state.lock().await.generation;
                self.broadcast(&json!({
                    "type": "built", "profile": "web", "generation": generation,
                    "hash": format!("astro-error-{}", now()), "errors": { slug: [format!("{error:#}")] }, "time": now(),
                }));
            }
        }
    }

    pub(super) async fn generation(&self) -> u64 {
        self.state.lock().await.generation
    }

    pub(super) fn cache_identity(&self) -> &str {
        &self.identity
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn virtual_dir(&self) -> &Path {
        &self.virtual_dir
    }

    pub(super) fn evaluator(&self) -> &Arc<Evaluator> {
        &self.evaluator
    }

    pub(super) fn package(&self) -> &Package {
        &self.package
    }

    // ---------------------------------------------------------- routes

    fn assets(entry: &Entry, generation: u64, snapshot: &Snapshot) -> EntryAssets {
        let unique_name = entry.profile.unique_name();
        let url = |name: &str| format!("{BASE}{}/{generation}/{name}", entry.profile.name());
        if let Some(failure) = &snapshot.failure {
            return EntryAssets {
                unique_name,
                errors: vec![failure.clone()],
                ..EntryAssets::default()
            };
        }
        let Some(point) = snapshot.entrypoints.get(&entry.slug) else {
            return EntryAssets {
                unique_name,
                errors: vec![format!("The preview compiler has not built {}", entry.slug)],
                ..EntryAssets::default()
            };
        };
        EntryAssets {
            hash: snapshot.hash.clone(),
            runtime: point.runtime.iter().map(|name| url(name)).collect(),
            scripts: point
                .files
                .iter()
                .filter(|name| {
                    Path::new(name)
                        .extension()
                        .is_some_and(|extension| extension == "js")
                })
                .map(|name| url(name))
                .collect(),
            styles: point
                .css
                .iter()
                .map(|(id, file)| Style {
                    href: url(file),
                    id: id.clone(),
                })
                .collect(),
            unique_name,
            errors: point.errors.clone(),
        }
    }

    fn descriptor_of(entry: &Entry) -> Value {
        json!({
            "base": format!("{BASE}{}/", entry.profile.name()),
            "module": format!("{BASE}module/{}.js?load={}", entry.slug, now()),
            "stylesheet": Value::Null,
            "title": entry.title,
            "options": { "development": true, "push": true },
        })
    }

    pub(crate) async fn preview(&self, relative: &str) -> Result<Entry> {
        self.warmed().await;
        let entry = self.preview_entry(relative).await?;
        Ok(self.ensure(entry).await?.0)
    }

    pub(crate) async fn descriptor(&self, relative: &str) -> Result<Value> {
        Ok(Self::descriptor_of(&self.preview(relative).await?))
    }

    /// A standalone document for a preview, for screenshots and "Open on its own".
    pub(crate) async fn page(&self, relative: &str) -> Result<String> {
        let entry = self.preview(relative).await?;
        let title: String = entry
            .title
            .chars()
            .map(|character| match character {
                '&' | '<' | '>' | '"' | '\'' => format!("&#{};", u32::from(character)),
                other => other.to_string(),
            })
            .collect();
        let options = json!({ "development": true, "push": true });
        Ok(format!(
            "<!doctype html><html><head>{COMPAT}<meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title}</title><style>html,body{{margin:0;min-height:100%}}#workbench-preview{{width:100%;min-width:0;box-sizing:border-box;min-height:100vh}}</style></head><body><script>window.__workbenchOptions={options}</script><script type=\"module\">import {{ mount }} from {}; mount(window.__workbenchOptions);</script></body></html>",
            super::entries::quote(&format!("{BASE}module/{}.js", entry.slug))
        ))
    }

    /// The module a warm host imports for an entry, after its build.
    pub(crate) async fn module(&self, slug: &str) -> Result<Option<String>> {
        let found = {
            let state = self.state.lock().await;
            state.entries.get(slug).cloned().and_then(|entry| {
                state
                    .hosts
                    .get(&entry.profile)
                    .cloned()
                    .map(|host| (entry, host))
            })
        };
        let Some((entry, host)) = found else {
            return Ok(None);
        };
        let snapshot = host.contains(slug).await?;
        let assets = Self::assets(&entry, host.generation, &snapshot);
        Ok(Some(preview_module_source(
            &entry.slug,
            entry.profile.name(),
            entry.kind,
            &assets,
        )))
    }

    /// One built file, waiting for the build in progress unless it is a hot update.
    pub(crate) async fn output(&self, profile: &str, rest: &str) -> Option<(Vec<u8>, bool)> {
        let profile = Profile::parse(profile)?;
        let host = self.state.lock().await.hosts.get(&profile).cloned()?;
        let (generation, file) = rest.split_once('/')?;
        if generation.parse::<u64>().ok()? != host.generation {
            return None;
        }
        let update = file.starts_with("hot/") || file.contains("/hot/");
        if !update {
            let _settled = host.settled().await;
        }
        host.read(file).map(|body| (body, update))
    }

    pub(crate) async fn docs_index(&self, raw: &Value) -> Result<Value> {
        let project = self.project().await?;
        let request = docs::request(raw, &self.root).map_err(|error| anyhow!(error))?;
        let listed = self.listed(&request, &project.config)?;
        Ok(json!({ "problems": listed.problems, "examples": self.listed_examples(&listed) }))
    }

    pub(crate) async fn docs_bundle(&self, raw: &Value) -> Result<Value> {
        let warming = crate::trace::phase("warm-wait");
        self.warmed().await;
        drop(warming);
        let _building = crate::trace::phase("bundle");
        let entry = self.docs_entry(raw).await?;
        let (entry, _host, _snapshot) = self.ensure(entry).await?;
        let descriptor = Self::descriptor_of(&entry);
        // Examples update in place, so the bundle's revision stays the same.
        Ok(json!({
            "base": descriptor["base"], "module": descriptor["module"], "revision": "rspack",
            "stylesheet": Value::Null, "examples": entry.examples, "problems": entry.problems,
        }))
    }

    // ---------------------------------------------------------- watching

    fn watch(self: &Arc<Self>) {
        let mut watcher = self.watcher.lock().expect("watcher lock");
        if watcher.is_some() {
            return;
        }
        let (send, mut receive) = mpsc::unbounded_channel::<Vec<PathBuf>>();
        let started = notify::recommended_watcher(move |event: notify::Result<Event>| {
            if let Ok(event) = event
                && !matches!(event.kind, EventKind::Access(_))
            {
                let _sent = send.send(event.paths);
            }
        })
        .and_then(|mut created| {
            created.watch(&self.root, RecursiveMode::Recursive)?;
            Ok(created)
        });
        match started {
            Ok(created) => *watcher = Some(created),
            Err(error) => {
                eprintln!("[workbench] preview watcher failed: {error}");
                return;
            }
        }
        // Folders outside the root follow each build, render and reload.
        let (weak, rewatch) = (Arc::downgrade(self), self.rewatch.clone());
        tokio::spawn(async move {
            loop {
                rewatch.notified().await;
                let Some(previews) = weak.upgrade() else {
                    break;
                };
                if !previews.watch_outside().await {
                    break;
                }
            }
        });
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            while let Some(first) = receive.recv().await {
                let mut paths: HashSet<PathBuf> = first.into_iter().collect();
                tokio::time::sleep(Duration::from_millis(20)).await;
                while let Ok(more) = receive.try_recv() {
                    paths.extend(more);
                }
                let Some(previews) = weak.upgrade() else {
                    break;
                };
                previews.changed(paths).await;
            }
        });
    }

    async fn changed(self: &Arc<Self>, paths: HashSet<PathBuf>) {
        let paths: Vec<PathBuf> = paths
            .into_iter()
            .filter(|path| {
                !path.components().any(|part| {
                    let part = part.as_os_str();
                    part == ".git" || part == ".workbench-rspack"
                })
            })
            .collect();
        if paths.is_empty() {
            return;
        }
        let (hosts, project, pages) = {
            let state = self.state.lock().await;
            (
                state.hosts.values().cloned().collect::<Vec<_>>(),
                state.project.clone(),
                state
                    .settings
                    .as_ref()
                    .map(|settings| settings.catalog.pages.clone())
                    .unwrap_or_default(),
            )
        };
        if project.is_some_and(|project| paths.iter().any(|path| project.files.contains(path))) {
            let previews = self.clone();
            tokio::spawn(async move { previews.reload(true).await });
            return;
        }
        let rerenders: Vec<(String, Render)> = self
            .renders
            .lock()
            .expect("renders lock")
            .iter()
            .filter(|(_, (files, _))| paths.iter().any(|path| files.contains(path)))
            .map(|(slug, (_, render))| (slug.clone(), render.clone()))
            .collect();
        for (slug, render) in rerenders {
            let previews = self.clone();
            tokio::spawn(async move { previews.rerender(slug, render).await });
        }
        let (changed, deleted): (HashSet<String>, HashSet<String>) = {
            let mut changed = HashSet::new();
            let mut deleted = HashSet::new();
            for path in &paths {
                if path.exists() {
                    changed.insert(text(path));
                } else {
                    deleted.insert(text(path));
                }
            }
            (changed, deleted)
        };
        for host in hosts {
            host.files(&changed, &deleted);
        }
        // Docs Markdown renders on the server; open docs pages hear that it changed.
        if paths
            .iter()
            .any(|path| pages.iter().any(|page| self.root.join(page) == *path))
        {
            self.broadcast(&json!({ "type": "docs-changed", "time": now() }));
        }
    }

    /// Stops watching the root and every folder outside it.
    fn unwatch(&self) {
        self.watcher.lock().expect("watcher lock").take();
        self.outside.lock().expect("outside lock").clear();
    }

    /// Watches the folders of files read from outside the root: a linked
    /// workspace package, or a render or configuration import beside the
    /// project. Returns false once the watcher has stopped.
    async fn watch_outside(&self) -> bool {
        let mut files: Vec<PathBuf> = vec![];
        {
            let state = self.state.lock().await;
            for host in state.hosts.values() {
                files.extend(host.sources().into_iter().map(PathBuf::from));
            }
            if let Some(project) = &state.project {
                files.extend(project.files.iter().cloned());
            }
        }
        for (read, _) in self.renders.lock().expect("renders lock").values() {
            files.extend(read.iter().cloned());
        }
        let folders = outside_folders(&self.root, files);
        let mut watcher = self.watcher.lock().expect("watcher lock");
        let mut outside = self.outside.lock().expect("outside lock");
        let Some(watcher) = watcher.as_mut() else {
            outside.clear();
            return false;
        };
        let gone: Vec<PathBuf> = outside.difference(&folders).cloned().collect();
        for folder in gone {
            let _unwatched = watcher.unwatch(&folder);
            outside.remove(&folder);
        }
        for folder in folders {
            if outside.contains(&folder) {
                continue;
            }
            match watcher.watch(&folder, RecursiveMode::NonRecursive) {
                Ok(()) => {
                    outside.insert(folder);
                }
                Err(error) => {
                    eprintln!(
                        "[workbench] preview watcher failed for {}: {error}",
                        folder.display()
                    );
                }
            }
        }
        true
    }

    pub(crate) async fn close(&self) {
        self.unwatch();
        let hosts: Vec<_> = std::mem::take(&mut self.state.lock().await.hosts)
            .into_values()
            .collect();
        for host in hosts {
            host.close().await;
        }
    }
}

/// The folders to watch one level deep for `files` read from outside `root`:
/// each file's folder, or the folder itself. The recursive root watch covers
/// everything inside the root; `node_modules` is never watched, and neither
/// is a folder above the root, which would watch the root's whole tree.
fn outside_folders(root: &Path, files: impl IntoIterator<Item = PathBuf>) -> HashSet<PathBuf> {
    let inside = |path: &Path| path.starts_with(root);
    files
        .into_iter()
        .filter(|file| {
            file.is_absolute()
                && !inside(file)
                && !file
                    .components()
                    .any(|part| part.as_os_str() == "node_modules")
        })
        .filter_map(|file| {
            if file.is_dir() {
                Some(file)
            } else {
                file.parent().map(Path::to_path_buf)
            }
        })
        .filter(|folder| !inside(folder) && !root.starts_with(folder) && folder.is_dir())
        .collect()
}

const COMPAT: &str = "<script src=\"/_workbench/preview-compat.js\"></script>";

fn hot_client_file(root: &Path, profile: Profile) -> String {
    text(
        &root
            .join(".workbench-rspack")
            .join(format!("hot-client-{}.js", profile.name())),
    )
}

/// Each lens of each defineDocs page, as the request its bundle is built from.
pub(crate) fn lens_requests(docs: &[Value]) -> Vec<Value> {
    let mut requests = vec![];
    for page in docs {
        for lens in page["lenses"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let mut request = json!({
                "adapter": lens["adapter"], "examples": lens["examples"], "styles": lens["styles"],
            });
            if lens["environment"].is_string() {
                request["environment"] = lens["environment"].clone();
            }
            requests.push(request);
        }
    }
    requests
}

#[cfg(test)]
mod tests {
    use super::outside_folders;
    use std::collections::HashSet;
    use std::path::PathBuf;

    #[test]
    fn only_folders_read_from_outside_the_root_are_watched() {
        let base = std::env::temp_dir().join(format!("acme-outside-{}", std::process::id()));
        let root = base.join("apps/web");
        let ui = base.join("packages/ui/src");
        let icons = base.join("packages/icons");
        for folder in [
            &root.join("src"),
            &ui,
            &icons,
            &base.join("node_modules/acme"),
        ] {
            std::fs::create_dir_all(folder).unwrap();
        }
        std::fs::write(ui.join("Button.tsx"), "").unwrap();
        let folders = outside_folders(
            &root,
            [
                ui.join("Button.tsx"),
                // A folder a glob lists is watched itself.
                icons.clone(),
                root.join("src/App.tsx"),
                base.join("node_modules/acme/index.js"),
                // The resolver reads above the root; watching there would
                // watch the root's whole tree.
                base.join("apps/package.json"),
                base.join("package.json"),
                base.join("packages/missing/index.ts"),
                PathBuf::from("relative/file.ts"),
            ],
        );
        assert_eq!(folders, HashSet::from([ui, icons]));
        std::fs::remove_dir_all(base).unwrap();
    }
}
