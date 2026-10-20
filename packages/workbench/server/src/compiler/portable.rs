//! Portable builds for the design-system export and the static build: each
//! preview and docs lens becomes a self-contained, minified ES module beside
//! its assets, built by a production compiler with the same format handling
//! and project plugins as live previews. One compiler per profile is reused,
//! so modules shared between builds compile once; builds run one at a time.
use super::Profile;
use super::host::{EntrySet, Module};
use super::overlay::Overlay;
use anyhow::{Context as _, Result, anyhow};
use rspack_core::Compiler;
use rspack_fs::MemoryFileSystem;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

/// One build's output files and the files it read.
pub(crate) struct Built {
    pub(crate) outputs: BTreeMap<String, Vec<u8>>,
    pub(crate) files: Vec<PathBuf>,
}

/// Creates a profile's production compiler inside the builder task.
pub(crate) type Factory = Box<
    dyn Fn(
            Profile,
            &Overlay,
            Arc<Mutex<EntrySet>>,
            Arc<MemoryFileSystem>,
        ) -> anyhow::Result<Compiler>
        + Send
        + Sync,
>;

struct Job {
    profile: Profile,
    name: String,
    file: String,
    modules: Vec<Module>,
    reply: oneshot::Sender<Result<Built>>,
}

pub(crate) struct Portable {
    jobs: mpsc::UnboundedSender<Job>,
}

struct Compiling {
    compiler: Compiler,
    overlay: Overlay,
    set: Arc<Mutex<EntrySet>>,
}

impl Portable {
    pub(crate) fn start(factory: Factory) -> Self {
        let (jobs, mut receive) = mpsc::unbounded_channel::<Job>();
        let context = Arc::new(rspack_tasks::CompilerContext::new());
        tokio::spawn(rspack_tasks::within_compiler_context(context, async move {
            let mut compilers: HashMap<Profile, Compiling> = HashMap::new();
            while let Some(job) = receive.recv().await {
                let result = Self::run(&factory, &mut compilers, &job).await;
                let _sent = job.reply.send(result);
            }
            for (_, compiling) in compilers.drain() {
                let _closed = compiling.compiler.close().await;
            }
        }));
        Self { jobs }
    }

    async fn run(
        factory: &Factory,
        compilers: &mut HashMap<Profile, Compiling>,
        job: &Job,
    ) -> Result<Built> {
        let compiling = match compilers.entry(job.profile) {
            std::collections::hash_map::Entry::Occupied(found) => found.into_mut(),
            std::collections::hash_map::Entry::Vacant(slot) => {
                let overlay = Overlay::new();
                let set = Arc::new(Mutex::new(EntrySet::default()));
                let compiler = factory(
                    job.profile,
                    &overlay,
                    set.clone(),
                    Arc::new(MemoryFileSystem::default()),
                )?;
                slot.insert(Compiling {
                    compiler,
                    overlay,
                    set,
                })
            }
        };
        for module in &job.modules {
            compiling.overlay.write(&module.file, &module.source);
        }
        compiling
            .set
            .lock()
            .expect("entry set lock")
            .replace(&job.name, vec![job.file.clone()]);
        compiling
            .compiler
            .build()
            .await
            .map_err(|error| anyhow!("{error}"))?;
        let compilation = &compiling.compiler.compilation;
        let errors: Vec<String> = compilation
            .get_errors()
            .take(3)
            .map(|error| {
                error
                    .render_report(false)
                    .unwrap_or_else(|_| error.error.to_string())
            })
            .collect();
        if !errors.is_empty() {
            return Err(anyhow!(errors.join("\n\n")));
        }
        let outputs = compilation
            .assets()
            .iter()
            .filter(|(name, _)| !name.starts_with(super::options::IDLE))
            .filter_map(|(name, asset)| {
                asset
                    .get_source()
                    .map(|source| (name.clone(), source.buffer().into_owned()))
            })
            .collect();
        let files = compilation
            .file_dependencies()
            .0
            .map(|file| PathBuf::from(file.to_string_lossy().into_owned()))
            .collect();
        Ok(Built { outputs, files })
    }

    /// Builds one entry, `name` naming its output (`preview.js`, `examples.js`).
    pub(crate) async fn build(
        &self,
        profile: Profile,
        name: &str,
        file: String,
        modules: Vec<Module>,
    ) -> Result<Built> {
        let (reply, answer) = oneshot::channel();
        self.jobs
            .send(Job {
                profile,
                name: name.to_owned(),
                file,
                modules,
                reply,
            })
            .map_err(|_| anyhow!("the portable compiler stopped"))?;
        answer
            .await
            .map_err(|_| anyhow!("the portable compiler stopped"))?
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// A build's identity from its outputs' names and contents.
pub(crate) fn revision(outputs: &BTreeMap<String, Vec<u8>>) -> String {
    let lines: Vec<String> = outputs
        .iter()
        .map(|(name, body)| format!("{name}:{}", hex(&Sha256::digest(body))))
        .collect();
    hex(&Sha256::digest(lines.join("\n").as_bytes()))[..16].to_owned()
}

// ------------------------------------------------------------- the service

use super::docs;
use super::entries::{
    DocsEntry, DocsExamples, PortableEntry, PreviewEntry, SourceImport, docs_slug, entry_slug,
    preview_entry_source,
};
use super::options::{self, Build, Purpose};
use super::previews::Previews;
use super::project;
use base64::Engine as _;
use serde_json::{Map, Value, json};
use std::path::Path;

const ASTRO_NOTE: &str = "Astro input edits require the live Workbench server. This export contains the authored states.";
const OMIT: [&str; 5] = ["node_modules", ".git", "dist", "build", "coverage"];

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn files(outputs: &BTreeMap<String, Vec<u8>>) -> Vec<Value> {
    outputs
        .iter()
        .map(|(path, body)| {
            json!({ "path": path, "data": base64::engine::general_purpose::STANDARD.encode(body) })
        })
        .collect()
}

fn escape(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '&' | '<' | '>' | '"' | '\'' => format!("&#{};", u32::from(character)),
            other => other.to_string(),
        })
        .collect()
}

/// Local files a build read, relative to the root, and the packages it read files from.
fn closure(root: &Path, read: impl IntoIterator<Item = PathBuf>) -> (Vec<String>, Vec<Value>) {
    let real_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut local = std::collections::BTreeSet::new();
    let mut packages = BTreeMap::new();
    for file in read {
        if !file.is_absolute() || !file.is_file() {
            continue;
        }
        let Ok(canonical) = std::fs::canonicalize(&file) else {
            continue;
        };
        let Ok(relative) = canonical.strip_prefix(&real_root) else {
            continue;
        };
        let parts: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        if parts.iter().any(|part| part == "node_modules") {
            let manifest = file
                .ancestors()
                .skip(1)
                .map(|folder| folder.join("package.json"))
                .find(|manifest| manifest.is_file());
            if let Some(manifest) = manifest
                && let Ok(text) = std::fs::read_to_string(manifest)
                && let Ok(package) = serde_json::from_str::<Value>(&text)
                && let (Some(name), Some(version)) =
                    (package["name"].as_str(), package["version"].as_str())
            {
                packages.insert(name.to_owned(), version.to_owned());
            }
            continue;
        }
        if parts
            .first()
            .is_some_and(|first| first.starts_with(".workbench-"))
        {
            continue;
        }
        local.insert(relative.to_string_lossy().replace('\\', "/"));
    }
    let packages = packages
        .into_iter()
        .map(|(name, version)| json!({ "name": name, "version": version }))
        .collect();
    (local.into_iter().collect(), packages)
}

fn declared_assets(
    root: &Path,
    directory: &Path,
    definition: &Value,
    outputs: &mut BTreeMap<String, Vec<u8>>,
    read: &mut Vec<PathBuf>,
) -> Result<()> {
    fn walk(folder: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || OMIT.contains(&name.as_str()) {
                continue;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_symlink() => {}
                Ok(kind) if kind.is_dir() => walk(&entry.path(), found),
                Ok(_) => found.push(entry.path()),
                Err(_) => {}
            }
        }
    }
    for asset in definition["assets"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let Some(asset) = asset.as_str() else {
            continue;
        };
        let target = crate::paths::resolve(directory, asset);
        if !target.starts_with(root) {
            anyhow::bail!("Declared assets must be inside the project.");
        }
        let mut found = vec![];
        if target.is_dir() {
            walk(&target, &mut found);
        } else {
            found.push(target);
        }
        for file in found {
            let body = std::fs::read(&file).with_context(|| format!("read {}", file.display()))?;
            let relative = file
                .strip_prefix(root)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            outputs.insert(format!("assets/source/{relative}"), body);
            read.push(file);
        }
    }
    Ok(())
}

impl Previews {
    async fn portable(&self) -> Result<Arc<Portable>> {
        let generation = self.generation().await;
        let mut slot = self.portable.lock().await;
        if let Some((built, portable)) = slot.as_ref()
            && *built == generation
        {
            return Ok(portable.clone());
        }
        let project = self.project().await?;
        let config = project.config.clone();
        let root = self.root().to_path_buf();
        let package = self.package().clone();
        let identity = self.cache_identity().to_owned();
        let factory: Factory = Box::new(move |profile, overlay, entries, out| {
            let (rules, plugins) = super::plugins::compile(&config, profile, &root, overlay)?;
            options::compiler(
                &Build {
                    profile,
                    generation: 0,
                    root: &root,
                    package: &package,
                    project: &config,
                    cache_dir: &root,
                    purpose: Purpose::Portable,
                    identity: identity.clone(),
                },
                overlay,
                entries,
                out,
                rules,
                plugins,
            )
        });
        let portable = Arc::new(Portable::start(factory));
        *slot = Some((generation, portable.clone()));
        Ok(portable)
    }

    fn portable_entry(&self) -> PortableEntry {
        let preview_dir = &self.package().preview_dir;
        PortableEntry {
            runtime: text(&preview_dir.join("browser.js")),
            requests: text(&preview_dir.join("requests.js")),
            note: None,
        }
    }

    fn virtual_portable(&self) -> PathBuf {
        self.root().join(".workbench-portable")
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one preview's build: sources, entry, output, assets and closure"
    )]
    /// One preview's portable output: `index.html`, `preview.js`, its
    /// stylesheet, chunks and assets, with its source closure.
    pub(crate) async fn portable_preview(&self, relative: &str) -> Result<Value> {
        let project_files = self.project().await?;
        let config = project_files.config.clone();
        let (file, definition) = self.definition(relative).await?;
        let id = definition["id"].as_str().context("definition id")?;
        let adapter = definition["adapter"]
            .as_str()
            .context("definition adapter")?;
        let slug = entry_slug(id);
        let astro = project::built_in_astro(&config, adapter);
        let directory = file.parent().unwrap_or(self.root());
        let mut sources = vec![];
        let mut modules = vec![];
        let mut rendered = Map::new();
        let mut read: Vec<PathBuf> = vec![file.clone()];
        read.extend(project_files.files.iter().cloned());
        let mut states = vec![("default".to_owned(), json!({}))];
        if let Some(map) = definition["states"].as_object() {
            states.extend(
                map.iter()
                    .map(|(name, state)| (name.clone(), state.clone())),
            );
        }
        let renders = if astro {
            Some(
                self.evaluator()
                    .call("astro", json!({ "file": relative, "options": self.options().await, "production": true }))
                    .await
                    .with_context(|| format!("{relative}: Astro render"))?,
            )
        } else {
            None
        };
        for (state, spec) in states {
            let source = if spec["source"].is_object() {
                &spec["source"]
            } else {
                &definition["source"]
            };
            if let Some(answer) = &renders {
                let render = &answer["states"][&state];
                let empty = Map::new();
                let component = render["component"]
                    .as_str()
                    .map(PathBuf::from)
                    .unwrap_or_default();
                let (module, written) = super::astro::render_modules(
                    &self.virtual_portable(),
                    &format!("{slug}-{state}"),
                    &component,
                    render["html"].as_str().unwrap_or_default(),
                    answer["resources"].as_object().unwrap_or(&empty),
                    &render["inputs"],
                    answer["publicDir"].as_str().map(Path::new),
                );
                modules.extend(written);
                rendered.insert(
                    state.clone(),
                    json!({ "html": render["html"], "inputs": render["inputs"] }),
                );
                for dependency in answer["dependencies"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                {
                    if let Some(dependency) = dependency.as_str() {
                        read.push(PathBuf::from(dependency));
                    }
                }
                sources.push(SourceImport::Astro {
                    state,
                    module,
                    render_url: None,
                });
            } else {
                let entry = source["entry"].as_str().context("source entry")?;
                sources.push(SourceImport::Module {
                    state,
                    file: text(&crate::paths::resolve(directory, entry)),
                    export_name: source["export"].as_str().unwrap_or("default").to_owned(),
                });
            }
        }
        let note = astro.then(|| ASTRO_NOTE.to_owned());
        let portable_entry = PortableEntry {
            note: note.clone(),
            ..self.portable_entry()
        };
        let adapter_import =
            project::adapter_import(&config, self.root(), &self.package().preview_dir, adapter)
                .map_err(|error| anyhow!(error))?;
        let project_environment = project::project_environment(&config, self.root(), adapter)
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
            portable: Some(&portable_entry),
        });
        let entry_file = text(&self.virtual_portable().join(format!("{slug}.js")));
        modules.push(Module {
            file: entry_file.clone(),
            source,
        });
        let built = self
            .portable()
            .await?
            .build(super::Profile::of(adapter), "preview", entry_file, modules)
            .await?;
        let mut outputs = built.outputs;
        read.extend(built.files);
        declared_assets(self.root(), directory, &definition, &mut outputs, &mut read)?;
        let revision = revision(&outputs);
        let title = definition["title"].as_str().unwrap_or(id).to_owned();
        let options = json!({ "development": false, "revision": revision })
            .to_string()
            .replace('<', "\\u003c");
        outputs.insert(
            "index.html".to_owned(),
            format!(
                "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title>{}<style>html,body{{margin:0;min-height:100%}}#workbench-preview{{width:100%;min-width:0;box-sizing:border-box;min-height:100vh}}</style></head><body><script>window.__workbenchOptions={options}</script><script type=\"module\" src=\"./preview.js\"></script></body></html>",
                escape(&title),
                if outputs.contains_key("preview.css") { "<link rel=\"stylesheet\" href=\"./preview.css\">" } else { "" },
            )
            .into_bytes(),
        );
        let (local, mut packages) = closure(self.root(), read);
        // Astro renders in Node with the project's own Astro, which the build never reads.
        if let Some(astro) = renders.as_ref().map(|answer| &answer["package"])
            && astro["name"].is_string()
            && !packages
                .iter()
                .any(|package| package["name"] == astro["name"])
        {
            packages.push(json!({ "name": astro["name"], "version": astro["version"] }));
        }
        Ok(json!({
            "slug": slug, "id": id, "title": title,
            "file": file.strip_prefix(self.root()).map(text).unwrap_or_default().replace('\\', "/"),
            "revision": revision, "outputs": files(&outputs), "localFiles": local, "packages": packages,
            "portableNote": note.unwrap_or_default(), "rendered": rendered,
        }))
    }

    /// A docs lens's portable examples: `examples.js`, exporting adapter,
    /// environment and examples.
    pub(crate) async fn portable_docs(&self, raw: &Value) -> Result<Value> {
        let project_files = self.project().await?;
        let config = project_files.config.clone();
        let request = docs::request(raw, self.root()).map_err(|error| anyhow!(error))?;
        let listed = self.listed(&request, &config)?;
        let slug = docs_slug(raw);
        let mut modules = vec![];
        let examples = if project::built_in_astro(&config, &request.adapter) {
            let mut examples = vec![];
            for example in &listed.examples {
                let answer = self
                    .evaluator()
                    .call("astroExample", json!({ "file": text(&example.file), "options": self.options().await, "production": true }))
                    .await?;
                let empty = Map::new();
                let (module, written) = super::astro::render_modules(
                    &self.virtual_portable(),
                    &format!("{slug}-{}", example.id),
                    &example.file,
                    answer["html"].as_str().unwrap_or_default(),
                    answer["resources"].as_object().unwrap_or(&empty),
                    &json!({}),
                    answer["publicDir"].as_str().map(Path::new),
                );
                modules.extend(written);
                examples.push((example.id.clone(), module));
            }
            DocsExamples::Astro(examples)
        } else if request.folder {
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
            &config,
            self.root(),
            &self.package().preview_dir,
            &request.adapter,
        )
        .map_err(|error| anyhow!(error))?;
        let project_environment =
            project::project_environment(&config, self.root(), &request.adapter)
                .map_err(|error| anyhow!(error))?
                .map(|file| text(&file));
        let lens_environment = request.environment.as_deref().map(text);
        let styles: Vec<String> = request.styles.iter().map(|style| text(style)).collect();
        let portable_entry = self.portable_entry();
        let source = super::entries::docs_entry_source(&DocsEntry {
            slug: &slug,
            adapter: &adapter,
            project_environment: project_environment.as_deref(),
            lens_environment: lens_environment.as_deref(),
            styles: &styles,
            examples: &examples,
            portable: Some(&portable_entry),
        });
        let entry_file = text(&self.virtual_portable().join(format!("{slug}.js")));
        modules.push(Module {
            file: entry_file.clone(),
            source,
        });
        let built = self
            .portable()
            .await?
            .build(
                super::Profile::of(&request.adapter),
                "examples",
                entry_file,
                modules,
            )
            .await?;
        Ok(json!({
            "revision": revision(&built.outputs), "stylesheet": built.outputs.contains_key("examples.css"),
            "outputs": files(&built.outputs), "examples": self.listed_examples(&listed), "problems": listed.problems,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_follow_names_and_contents() {
        let mut outputs = BTreeMap::new();
        outputs.insert("preview.js".to_owned(), b"one".to_vec());
        let first = revision(&outputs);
        assert_eq!(first.len(), 16);
        outputs.insert("preview.css".to_owned(), b"two".to_vec());
        assert_ne!(revision(&outputs), first);
    }
}
