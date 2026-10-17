//! Stage 2's preview host: the preview worker's HTTP routes answered from
//! Rust, with one Rspack compiler that holds only the pages asked for.
//!
//! Routes, as `preview/worker.cjs` answers them (the Workbench server
//! forwards `/_workbench/previews/<route>` here):
//!
//! - `GET /descriptor?file=` and `GET /page?file=`: a page becomes an entry
//!   on its first request, its entry source generated here from the
//!   definition parsed as literals, or evaluated by Node when it is not;
//! - `GET /rspack/module/<slug>.js`: the preview module a warm host imports;
//! - `GET /rspack/web/<generation>/<file>`: built output from memory;
//! - `GET /rspack/events`: built events pushed to frames;
//! - `GET /lab/...`: the lab's own probes (state, entry and hot client
//!   sources for parity checks).
//!
//! One builder task owns the compiler; requests and the file watcher send it
//! commands, and it publishes each build.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, broadcast, mpsc, watch};

use crate::compile::{Built, Entry, Host, Job, Refresh};

const BASE: &str = "/_workbench/previews/rspack/";
const FRESH_HOST: &str = "/_workbench/preview-host.html?actions=off";
const LIVE_RUNTIME: &str = "/_workbench/preview-runtime.js";
const COMPAT_TAG: &str = "<script src=\"/_workbench/preview-compat.js\"></script>";

/// How the host is started, from the overlay's `--options` JSON.
#[derive(Clone, Deserialize)]
pub struct Options {
    pub root: String,
    pub preview_dir: String,
    pub cache_dir: String,
    pub refresh: Refresh,
    pub evaluator: String,
    pub node: String,
    /// "plain" (R), "neighbours" (RN) or "cap" (D2).
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub cap: Option<usize>,
    #[serde(default)]
    pub neighbours: Option<usize>,
    /// The page index's saved file, for RN's page order.
    #[serde(default)]
    pub index_saved: Option<String>,
    #[serde(default)]
    pub include: Vec<String>,
}

/// One page the compiler holds.
#[derive(Clone)]
struct Page {
    slug: String,
    title: String,
    entry: Entry,
    /// Files whose change regenerates the entry source.
    reads: Vec<String>,
}

/// The latest build, as requests see it.
#[derive(Clone, Default)]
struct Published {
    generation: u32,
    seq: u64,
    building: bool,
    built: Option<Arc<Built>>,
}

enum Command {
    Add(Vec<String>),
    Files(HashSet<String>, HashSet<String>),
}

struct State {
    options: Options,
    root: PathBuf,
    pages: Mutex<BTreeMap<String, Page>>,
    by_slug: std::sync::Mutex<HashMap<String, String>>,
    used: std::sync::Mutex<HashMap<String, u64>>,
    /// Pages a frame or the canvas asked for, as opposed to RN's neighbours.
    requested: std::sync::Mutex<HashSet<String>>,
    clock: AtomicU64,
    generation: AtomicU32,
    config: Mutex<Option<(Value, Vec<String>)>>,
    published: watch::Sender<Published>,
    events: broadcast::Sender<String>,
    commands: mpsc::UnboundedSender<Command>,
    timeline: std::sync::Mutex<Vec<Value>>,
    origin: Instant,
    /// The current compiler's in-memory output, read without the compiler.
    out: std::sync::Mutex<Option<Arc<rspack_fs::MemoryFileSystem>>>,
    chunk_of: std::sync::Mutex<HashMap<String, String>>,
    moved_modules: std::sync::Mutex<Vec<String>>,
}

impl State {
    fn mark(&self, event: &str, details: Value) {
        let mut entry = json!({ "event": event, "at": self.origin.elapsed().as_secs_f64() * 1000.0 });
        if let (Some(target), Value::Object(details)) = (entry.as_object_mut(), details) {
            target.extend(details);
        }
        let mut timeline = self.timeline.lock().expect("timeline");
        if timeline.len() < 20_000 {
            timeline.push(entry);
        }
    }

    fn touch(&self, slug: &str) {
        let now = self.clock.fetch_add(1, Ordering::SeqCst);
        self.used.lock().expect("used").insert(slug.to_string(), now);
    }
}

// ------------------------------------------------------------- entry sources

/// `entrySlug` in `entries.ts`.
fn entry_slug(id: &str) -> String {
    let digest = Sha256::digest(id.as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}-{}", id.replace('/', "--"), &hex[..12])
}

fn js(value: &str) -> String {
    serde_json::to_string(value).expect("string")
}

/// `path.resolve(dir, relative)` without touching the file system.
fn resolve(dir: &Path, relative: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for part in dir.join(relative).components() {
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

/// `hotClientSource` in `client-sources.ts`.
pub fn hot_client_source(profile: &str, generation: u32) -> String {
    let events = js(&format!("{BASE}events"));
    let fresh = js(FRESH_HOST);
    let profile = js(profile);
    format!(
        r#"const profile = {profile};
const generation = {generation};
const state = (window.__wbProfiles = window.__wbProfiles || {{}})[profile] || (window.__wbProfiles[profile] = {{}});
state.require = __webpack_require__;
if (module.hot && !state.client) {{
  state.client = true;
  let hub = window;
  try {{ if (parent !== window && parent.location.origin === location.origin) hub = parent; }} catch (_) {{}}
  if (!hub.__wbPushSource) hub.__wbPushSource = new hub.EventSource({events});
  const source = hub.__wbPushSource;
  let failed = false, latest = null, queue = Promise.resolve();
  const active = () => {{ try {{ return !window.frameElement || window.frameElement.classList.contains('is-active'); }} catch (_) {{ return true; }} }};
  const reload = () => {{
    if (window.__workbenchHost && window.wbPreviewHost) window.wbPreviewHost.load(location.href);
    else location.reload();
  }};
  // Each build's update starts from the build before it.
  const syncTo = target => (queue = queue.catch(() => {{}}).then(async () => {{
    for (let step = 0; step < 50 && target && __webpack_hash__ !== target; step++) {{
      for (let wait = 0; module.hot.status() !== 'idle'; wait++) {{
        if (/^(abort|fail)$/.test(module.hot.status()) || wait > 500) throw new Error('Hot updates stopped: ' + module.hot.status());
        await new Promise(resolve => setTimeout(resolve, 10));
      }}
      const updated = await module.hot.check(true);
      if (!updated) break;
    }}
    if (target && __webpack_hash__ !== target) throw new Error('This document cannot reach build ' + target);
  }}));
  // The next preview's module calls this before loading its chunks.
  state.sync = syncTo;
  const update = () => {{
    if (failed) {{ failed = false; reload(); return; }}
    syncTo(latest).catch(error => {{ console.info('[workbench] hot update not applied; reloading the preview', String(error && error.message || error)); reload(); }});
  }};
  const onMessage = event => {{
    let data; try {{ data = JSON.parse(event.data); }} catch (_) {{ return; }}
    if (data.type === 'reload-all') {{ if (window.__workbenchHost) location.replace({fresh}); else location.reload(); return; }}
    if (data.type !== 'built' || data.profile !== profile || (data.generation && data.generation !== generation)) return;
    latest = data.hash;
    const own = window.__wbCurrent && data.errors && data.errors[window.__wbCurrent];
    if (own && own.length) {{
      failed = true;
      if (active() && window.workbench && window.workbench.error) window.workbench.error(new Error(own.join('\n\n')));
      return;
    }}
    if (data.hash === __webpack_hash__) return;
    update();
  }};
  source.addEventListener('message', onMessage);
  window.addEventListener('pagehide', () => source.removeEventListener('message', onMessage));
}}
"#
    )
}

/// `previewModuleSource` in `client-sources.ts`, for a preview entry.
fn preview_module_source(slug: &str, generation: u32, built: &Built) -> String {
    let url = |name: &str| format!("{BASE}web/{generation}/{name}");
    let point = built.entrypoints.get(slug);
    let (hash, runtime, scripts, styles, errors) = match point {
        Some(point) => (
            json!(built.hash),
            point.runtime.iter().map(|name| url(name)).collect::<Vec<_>>(),
            point.files.iter().filter(|name| name.ends_with(".js")).map(|name| url(name)).collect::<Vec<_>>(),
            point.css.iter().map(|(id, file)| json!({ "href": url(file), "id": id })).collect::<Vec<_>>(),
            point.errors.clone(),
        ),
        None => (Value::Null, Vec::new(), Vec::new(), Vec::new(), vec![format!("The preview compiler has not built {slug}")]),
    };
    let unique = js("wb_web");
    let profile = js("web");
    let fresh = js(FRESH_HOST);
    format!(
        r#"import * as runtime from '/_workbench/preview-runtime.js';
import '/_workbench/preview-requests.js';
window.__workbenchRuntime = runtime;
const errors = {errors};
if (errors.length) throw new Error(errors.join('\n\n'));
const slug = {slug};
const scripts = {scripts};
const loaded = window.__wbLoadedScripts || (window.__wbLoadedScripts = new Set());
// Tagged the way Rspack's CSS runtime finds a chunk's stylesheet for hot updates.
for (const style of {styles}) {{
  if (document.querySelector('link[data-rspack="' + {unique} + ':' + style.id + '"]')) continue;
  const link = document.createElement('link'); link.rel = 'stylesheet'; link.href = style.href;
  link.setAttribute('data-rspack', {unique} + ':' + style.id);
  await new Promise(resolve => {{ link.onload = link.onerror = resolve; document.head.append(link); }});
}}
const profileState = window.__wbProfiles && window.__wbProfiles[{profile}];
const runtimeFiles = new Set({runtime});
if (profileState && profileState.require) {{
  try {{ if (profileState.sync) await profileState.sync({hash}); }}
  catch (error) {{ if (window.__workbenchHost) location.replace({fresh}); else location.reload(); await new Promise(() => {{}}); }}
  for (const src of runtimeFiles) loaded.add(src);
  const cache = profileState.require.c;
  // style-loader keeps its own registry of inserted styles, which must start
  // over with the project modules it styles.
  if (cache) for (const id of Object.keys(cache)) if ((!/node_modules/.test(id) || /style-loader|css-loader/.test(id)) && !/hot-client-/.test(id)) delete cache[id];
}}
let failure = null;
const before = window.__wbRspack && window.__wbRspack[slug];
const record = event => {{ failure = event.error || new Error(event.message); }};
window.addEventListener('error', record);
try {{
  for (const src of scripts) {{
    if (loaded.has(src)) continue;
    loaded.add(src);
    await new Promise((resolve, reject) => {{ const script = document.createElement('script'); script.src = src; script.onload = resolve; script.onerror = () => reject(new Error('Failed to load ' + src)); document.head.append(script); }});
  }}
  if (before && window.__wbRspack[slug] === before) window.__wbProfiles[{profile}].require(before.moduleId);
}} finally {{ window.removeEventListener('error', record); }}
if (failure) throw failure;
const entry = window.__wbRspack && window.__wbRspack[slug];
if (!entry) throw new Error('The preview module did not register: ' + slug);
window.__wbCurrent = slug;
export function mount(options) {{ return entry.mount(options); }}
"#,
        errors = text(&errors),
        slug = js(slug),
        scripts = text(&scripts),
        styles = text(&styles),
        runtime = text(&runtime),
        hash = hash,
    )
}

/// JSON text for a template's values, as `JSON.stringify` writes them.
fn text<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("json")
}

/// `previewEntrySource` in `entries.ts`, for a live (not portable) preview
/// with module sources. Returns the source and what the entry reads.
fn preview_entry_source(slug: &str, definition: &Path, value: &Value, config: &Value, root: &Path, preview_dir: &Path) -> Result<String, String> {
    let object = value.as_object().ok_or("default export must be definePreview({...})")?;
    let adapter = object.get("adapter").and_then(Value::as_str).ok_or("adapter is missing")?;
    let dir = definition.parent().unwrap_or(root);
    // `adapterImport` in project.ts.
    let adapter_file = match config.pointer(&format!("/adapters/{adapter}/runtime")).and_then(Value::as_str) {
        Some(runtime) => resolve(root, runtime),
        None => match adapter {
            "react" => preview_dir.join("react.js"),
            "html" => preview_dir.join("html.js"),
            "vue" => preview_dir.join("vue.js"),
            "astro" | "react-native-web" => return Err(format!("the {adapter} adapter is outside the lab's subset")),
            other => return Err(format!("Unknown adapter “{other}”. Register it in workbench.config.ts.")),
        },
    };
    let mut lines = vec![
        format!("import definition from {};", js(&definition.to_string_lossy())),
        format!("import * as adapter from {};", js(&adapter_file.to_string_lossy())),
        format!("import {{ boot, combine }} from {};", js(LIVE_RUNTIME)),
    ];
    // Object.entries({ default: { source: definition.source }, ...definition.states })
    let mut states: Vec<(String, Value)> = vec![("default".into(), json!({ "source": object.get("source").cloned().unwrap_or(Value::Null) }))];
    if let Some(Value::Object(more)) = object.get("states") {
        for (id, spec) in more {
            if id == "default" {
                states[0].1 = spec.clone();
            } else {
                states.push((id.clone(), spec.clone()));
            }
        }
    }
    let mut accepted = Vec::new();
    let mut sources = Vec::new();
    for (index, (state, spec)) in states.iter().enumerate() {
        let source = spec.get("source").filter(|source| !source.is_null()).or_else(|| object.get("source")).ok_or("source is missing")?;
        let entry = source.get("entry").and_then(Value::as_str).ok_or("source.entry is missing")?;
        let export = source.get("export").and_then(Value::as_str).unwrap_or("default");
        let file = resolve(dir, entry).to_string_lossy().into_owned();
        lines.push(format!("import * as source{index} from {};", js(&file)));
        sources.push(format!("{}: source{index}[{}]", js(state), js(export)));
        accepted.push(file);
    }
    // `projectEnvironment` and the definition's own environment.
    let project = match config.get("environment") {
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Object(by_adapter)) => by_adapter.get(adapter).and_then(Value::as_str).map(str::to_string),
        _ => None,
    };
    let mut names = Vec::new();
    if let Some(value) = project {
        let target = resolve(root, &value);
        if !target.exists() {
            return Err(format!("The preview config environment must be a file inside the project: {value}"));
        }
        lines.push(format!("import * as projectEnvironment from {};", js(&target.to_string_lossy())));
        names.push("projectEnvironment");
    }
    if let Some(own) = object.get("environment").and_then(Value::as_str) {
        lines.push(format!("import * as ownEnvironment from {};", js(&resolve(dir, own).to_string_lossy())));
        names.push("ownEnvironment");
    }
    let expression = match names.len() {
        2 => format!("combine({})", names.join(", ")),
        1 => names[0].to_string(),
        _ => "{}".to_string(),
    };
    if let Some(Value::Array(styles)) = object.get("styles") {
        for style in styles.iter().filter_map(Value::as_str) {
            lines.push(format!("import {};", js(&resolve(dir, style).to_string_lossy())));
        }
    }
    lines.push(format!("const sources = () => ({{{}}});", sources.join(", ")));
    lines.push(format!("const environment = {expression};"));
    lines.push("function mount(options) { return boot(definition, sources(), adapter, environment, options); }".into());
    lines.push(format!("(window.__wbRspack = window.__wbRspack || {{}})[{}] = {{ mount, moduleId: module.id }};", js(slug)));
    lines.push(format!(
        "if (module.hot) module.hot.accept({}, () => {{ if (window.__workbenchRerender) window.__workbenchRerender(sources()); }});",
        serde_json::to_string(&accepted).expect("accepted")
    ));
    Ok(lines.join("\n") + "\n")
}

// ------------------------------------------------------------- definitions

/// Evaluates files with the shipping compiler in Node, in one batch.
async fn evaluate(options: &Options, args: &[&str], stdin: &str) -> Result<Value, String> {
    let mut child = tokio::process::Command::new(&options.node)
        .arg(&options.evaluator)
        .arg(&options.root)
        .args(args)
        .env("ELECTRON_RUN_AS_NODE", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("evaluator: {error}"))?;
    if let Some(mut input) = child.stdin.take() {
        input.write_all(stdin.as_bytes()).await.map_err(|error| error.to_string())?;
    }
    let output = child.wait_with_output().await.map_err(|error| error.to_string())?;
    serde_json::from_slice(&output.stdout).map_err(|_| format!("evaluator failed: {}", String::from_utf8_lossy(&output.stderr).chars().take(500).collect::<String>()))
}

/// A file's modification time and size, to tell whether it changed.
fn stamp(file: &str) -> Value {
    match std::fs::metadata(file) {
        Ok(meta) => json!([meta.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map(|time| time.as_nanos() as f64).unwrap_or(0.0), meta.len()]),
        Err(_) => Value::Null,
    }
}

/// The project config's entry-generation fields, evaluated once per
/// generation. The answer is saved in the cache with the stamps of the files
/// the config read; a start with none of them changed reuses it.
async fn config(state: &State) -> Result<(Value, Vec<String>), String> {
    let mut cached = state.config.lock().await;
    if let Some(found) = cached.as_ref() {
        return Ok(found.clone());
    }
    let started = Instant::now();
    let saved_file = Path::new(&state.options.cache_dir).join("config.json");
    let saved: Option<Value> = std::fs::read(&saved_file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let fresh = saved.as_ref().and_then(|saved| {
        let stamps = saved["stamps"].as_object()?;
        (!stamps.is_empty() && stamps.iter().all(|(file, value)| stamp(file) == *value)).then(|| (saved["config"].clone(), stamps.keys().cloned().collect::<Vec<_>>()))
    });
    let found = match fresh {
        Some(found) => {
            state.mark("config.reused", json!({ "ms": started.elapsed().as_secs_f64() * 1000.0 }));
            found
        }
        None => {
            let answer = evaluate(&state.options, &["--config"], "").await?;
            let mut files: Vec<String> = answer["files"].as_array().map(|files| files.iter().filter_map(|file| file.as_str().map(str::to_string)).collect()).unwrap_or_default();
            let config_file = state.root.join("workbench.config.ts").to_string_lossy().into_owned();
            if !files.contains(&config_file) {
                files.push(config_file);
            }
            let stamps: Map<String, Value> = files.iter().map(|file| (file.clone(), stamp(file))).collect();
            let _ = std::fs::create_dir_all(&state.options.cache_dir);
            let _ = std::fs::write(&saved_file, json!({ "config": answer["config"], "stamps": stamps }).to_string());
            state.mark("config.evaluated", json!({ "ms": started.elapsed().as_secs_f64() * 1000.0 }));
            (answer["config"].clone(), files)
        }
    };
    *cached = Some(found.clone());
    Ok(found)
}

/// Generates a page's entry: the definition parsed as literals, or evaluated.
async fn generate(state: &State, relative: &str) -> Result<Page, String> {
    let started = Instant::now();
    let file = state.root.join(relative);
    if !file.starts_with(&state.root) || !relative.contains(".workbench.") || !file.is_file() {
        return Err("Not a declared Workbench preview definition.".into());
    }
    let (config, config_files) = config(state).await?;
    let (value, how, mut reads) = match crate::index::parse_definition(&file) {
        Some(value) => (value, "parsed", vec![file.to_string_lossy().into_owned()]),
        None => {
            let answer = evaluate(&state.options, &[], &file.to_string_lossy()).await?;
            let key = file.to_string_lossy();
            let definition = answer["definitions"].get(key.as_ref()).cloned().ok_or_else(|| answer["errors"].get(key.as_ref()).and_then(Value::as_str).unwrap_or("did not evaluate").to_string())?;
            let reads = definition["files"].as_array().map(|files| files.iter().filter_map(|file| file.as_str().map(str::to_string)).collect()).unwrap_or_default();
            (definition["value"].clone(), "evaluated", reads)
        }
    };
    reads.extend(config_files);
    let id = value.get("id").and_then(Value::as_str).ok_or("id is missing")?.to_string();
    let slug = entry_slug(&id);
    let title = value.get("title").and_then(Value::as_str).filter(|title| !title.is_empty()).unwrap_or(&id).to_string();
    let source = preview_entry_source(&slug, &file, &value, &config, &state.root, Path::new(&state.options.preview_dir))?;
    let entry = Entry { slug: slug.clone(), file: state.root.join(".workbench-rspack").join(format!("{slug}.js")).to_string_lossy().into_owned(), source };
    state.mark("entry.generated", json!({ "file": relative, "how": how, "ms": started.elapsed().as_secs_f64() * 1000.0 }));
    Ok(Page { slug, title, entry, reads })
}

// ------------------------------------------------------------- requests

/// Makes sure a page is an entry and a finished build contains it.
async fn ensure(state: &Arc<State>, relative: &str) -> Result<(Page, Arc<Built>, u32), String> {
    let requested = Instant::now();
    let existing = state.pages.lock().await.get(relative).cloned();
    let page = match existing {
        Some(page) => page,
        None => {
            let page = generate(state, relative).await?;
            let mut pages = state.pages.lock().await;
            let page = pages.entry(relative.to_string()).or_insert(page).clone();
            state.by_slug.lock().expect("slugs").insert(page.slug.clone(), relative.to_string());
            drop(pages);
            let _ = state.commands.send(Command::Add(vec![relative.to_string()]));
            page
        }
    };
    state.touch(&page.slug);
    state.requested.lock().expect("requested").insert(relative.to_string());
    let (built, generation) = contains(state, &page.slug).await?;
    state.mark("request.ready", json!({ "file": relative, "ms": requested.elapsed().as_secs_f64() * 1000.0 }));
    Ok((page, built, generation))
}

/// Waits for a finished build that contains the entry.
async fn contains(state: &State, slug: &str) -> Result<(Arc<Built>, u32), String> {
    let mut receiver = state.published.subscribe();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
    loop {
        {
            let published = receiver.borrow_and_update();
            if !published.building
                && let Some(built) = &published.built
            {
                if !built.ok {
                    return Err(built.error.clone().unwrap_or_else(|| "build failed".into()));
                }
                if built.entrypoints.contains_key(slug) {
                    return Ok((built.clone(), published.generation));
                }
            }
        }
        if tokio::time::timeout_at(deadline, receiver.changed()).await.is_err() {
            return Err(format!("The preview compiler did not build {slug}"));
        }
    }
}

/// Waits until no build is running.
async fn settled(state: &State) {
    let mut receiver = state.published.subscribe();
    loop {
        if !receiver.borrow_and_update().building {
            return;
        }
        if receiver.changed().await.is_err() {
            return;
        }
    }
}

fn mime(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "html" => "text/html; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

fn query(path: &str, name: &str) -> Option<String> {
    let (_, query) = path.split_once('?')?;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        (key == name).then(|| decode(value))
    })
}

fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'%' if at + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[at + 1..at + 3]).ok().and_then(|hex| u8::from_str_radix(hex, 16).ok());
                match hex {
                    Some(byte) => {
                        out.push(byte);
                        at += 3;
                    }
                    None => {
                        out.push(b'%');
                        at += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            byte => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct Reply {
    status: u16,
    content_type: &'static str,
    cache: &'static str,
    body: Vec<u8>,
}

impl Reply {
    fn json(status: u16, value: &Value) -> Self {
        Self { status, content_type: "application/json", cache: "no-store", body: value.to_string().into_bytes() }
    }
    fn text(status: u16, text: &str) -> Self {
        Self { status, content_type: "text/plain; charset=utf-8", cache: "no-store", body: text.as_bytes().to_vec() }
    }
}

async fn route(state: &Arc<State>, path: &str) -> Reply {
    let pathname = path.split('?').next().unwrap_or("");
    let error = |message: String| Reply::json(500, &json!({ "error": message }));
    match pathname {
        "/descriptor" | "/page" => {
            let Some(file) = query(path, "file") else { return Reply::json(400, &json!({ "error": "file is missing" })) };
            match ensure(state, &file).await {
                Ok((page, _, _)) => {
                    let module = format!("{BASE}module/{}.js", page.slug);
                    if pathname == "/descriptor" {
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|time| time.as_millis()).unwrap_or(0);
                        Reply::json(200, &json!({ "base": format!("{BASE}web/"), "module": format!("{module}?load={now}"), "stylesheet": null, "title": page.title, "options": { "development": true, "push": true } }))
                    } else {
                        let escaped: String = page.title.chars().map(|char| if "&<>\"'".contains(char) { format!("&#{};", char as u32) } else { char.to_string() }).collect();
                        let html = format!(
                            "<!doctype html><html><head>{COMPAT_TAG}<meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{escaped}</title><style>html,body{{margin:0;min-height:100%}}#workbench-preview{{width:100%;min-width:0;box-sizing:border-box;min-height:100vh}}</style></head><body><script>window.__workbenchOptions={}</script><script type=\"module\">import {{ mount }} from {}; mount(window.__workbenchOptions);</script></body></html>",
                            json!({ "development": true, "push": true }),
                            js(&module)
                        );
                        Reply { status: 200, content_type: "text/html; charset=utf-8", cache: "no-store", body: html.into_bytes() }
                    }
                }
                Err(message) => error(message),
            }
        }
        "/lab/state" => {
            let published = state.published.borrow().clone();
            let pages: Vec<String> = state.pages.lock().await.keys().cloned().collect();
            let timeline = state.timeline.lock().expect("timeline").clone();
            let moved = state.moved();
            Reply::json(200, &json!({ "generation": published.generation, "seq": published.seq, "building": published.building, "pages": pages, "timeline": timeline, "moved": moved }))
        }
        "/lab/entry" => {
            let Some(file) = query(path, "file") else { return Reply::json(400, &json!({ "error": "file is missing" })) };
            match generate(state, &file).await {
                Ok(page) => Reply::json(200, &json!({ "slug": page.slug, "source": page.entry.source, "title": page.title })),
                Err(message) => error(message),
            }
        }
        "/lab/hot-client" => Reply::text(200, &hot_client_source("web", state.generation.load(Ordering::SeqCst))),
        _ if pathname.starts_with("/rspack/module/") => {
            let slug = pathname.trim_start_matches("/rspack/module/").trim_end_matches(".js").to_string();
            let known = state.by_slug.lock().expect("slugs").get(&slug).cloned();
            let Some(relative) = known else { return Reply::text(404, "Unknown preview") };
            // A page D2 removed comes back when a frame asks for it again.
            match ensure(state, &relative).await {
                Ok((_, built, generation)) => Reply { status: 200, content_type: "text/javascript", cache: "no-store", body: preview_module_source(&slug, generation, &built).into_bytes() },
                Err(message) => error(message),
            }
        }
        _ if pathname.starts_with("/rspack/web/") => {
            let rest = decode(pathname.trim_start_matches("/rspack/web/"));
            let hot = rest.split('/').skip(1).any(|part| part == "hot");
            if !hot {
                settled(state).await;
            }
            let output = state.output(&format!("/out/web/{rest}")).await;
            match output {
                Some(body) => Reply { status: 200, content_type: mime(&rest), cache: if hot || rest.ends_with(".css") { "no-cache" } else { "public, max-age=31536000, immutable" }, body },
                None => Reply::text(404, "Not found"),
            }
        }
        "/index" | "/render" | "/export" => Reply::json(501, &json!({ "error": "outside the lab's subset" })),
        _ if pathname.starts_with("/docs/") => Reply::json(501, &json!({ "error": "docs lenses are outside the lab's subset" })),
        _ => Reply::text(404, "Preview asset not found"),
    }
}

impl State {
    async fn output(&self, path: &str) -> Option<Vec<u8>> {
        use rspack_fs::ReadableFileSystem as _;
        let out = self.out.lock().expect("out").clone()?;
        out.read_sync(rspack_paths::Utf8Path::new(path)).ok()
    }

    fn moved(&self) -> Vec<String> {
        self.moved_modules.lock().expect("moved").clone()
    }
}

async fn connection(state: Arc<State>, mut stream: TcpStream) {
    let mut head = Vec::new();
    let mut buffer = [0u8; 4096];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(count) => head.extend_from_slice(&buffer[..count]),
        }
        if head.len() > 65_536 {
            return;
        }
    }
    let text = String::from_utf8_lossy(&head);
    let mut parts = text.lines().next().unwrap_or("").split(' ');
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/").to_string();
    if method != "GET" {
        let _ = write(&mut stream, Reply::text(405, "GET only")).await;
        return;
    }
    if path.split('?').next() == Some("/rspack/events") {
        events(state, stream).await;
        return;
    }
    let reply = route(&state, &path).await;
    let _ = write(&mut stream, reply).await;
}

async fn write(stream: &mut TcpStream, reply: Reply) -> std::io::Result<()> {
    let reason = match reply.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        501 => "Not Implemented",
        _ => "Internal Server Error",
    };
    let head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nCache-Control: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reply.status,
        reply.content_type,
        reply.cache,
        reply.body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&reply.body).await?;
    stream.shutdown().await
}

/// One event stream per open canvas, as the pipeline's `events`.
async fn events(state: Arc<State>, mut stream: TcpStream) {
    let mut receiver = state.events.subscribe();
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n: connected\n\n";
    if stream.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.tick().await;
    loop {
        let frame = tokio::select! {
            message = receiver.recv() => match message {
                Ok(message) => format!("data: {message}\n\n"),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return,
            },
            _ = heartbeat.tick() => ": heartbeat\n\n".to_string(),
        };
        if stream.write_all(frame.as_bytes()).await.is_err() {
            return;
        }
    }
}

// ------------------------------------------------------------- the builder

struct Forward(mpsc::UnboundedSender<Command>);
impl rspack_watcher::EventAggregateHandler for Forward {
    fn on_event_handle(&self, changed: rustc_hash::FxHashSet<String>, deleted: rustc_hash::FxHashSet<String>) {
        let _ = self.0.send(Command::Files(changed.into_iter().collect(), deleted.into_iter().collect()));
    }
}
struct Quiet;
impl rspack_watcher::EventHandler for Quiet {}

fn job(state: &State, generation: u32) -> Job {
    let options = &state.options;
    Job {
        root: options.root.clone(),
        preview_dir: options.preview_dir.clone(),
        cache_dir: options.cache_dir.clone(),
        generation,
        hot_client: Entry { slug: "hot-client".into(), file: state.root.join(".workbench-rspack").join("hot-client-web.js").to_string_lossy().into_owned(), source: hot_client_source("web", generation) },
        entries: Vec::new(),
        refresh: options.refresh.clone(),
        stable_chunks: true,
    }
}

/// Runs the compiler: applies commands, builds, publishes and watches.
async fn builder(state: Arc<State>, mut commands: mpsc::UnboundedReceiver<Command>) {
    let mut generation = state.generation.load(Ordering::SeqCst);
    let mut host = Host::new(&job(&state, generation));
    *state.out.lock().expect("out") = Some(host.out());
    let watcher = rspack_watcher::FsWatcher::new(
        rspack_watcher::FsWatcherOptions { aggregate_timeout: Some(5), ..Default::default() },
        rspack_watcher::FsWatcherIgnored::Regex(rspack_regex::RspackRegex::new("/node_modules/|/\\.git/|/\\.workbench-").expect("regex")),
    );
    let mut watched: HashSet<String> = HashSet::new();
    let mut held: BTreeMap<String, String> = BTreeMap::new();
    let mut seq = 0u64;
    let mut first = true;
    let mut neighbours_done: HashSet<String> = HashSet::new();
    let mut order: Option<Vec<(String, String)>> = None;
    let config_file = state.root.join("workbench.config.ts").to_string_lossy().into_owned();
    loop {
        // Serve outputs and collect commands until there is work to build.
        let mut add = Vec::new();
        let mut remove = Vec::new();
        let mut changed = HashSet::new();
        let mut deleted = HashSet::new();
        let idle = if first { Some(Command::Files(HashSet::new(), HashSet::new())) } else { None };
        let next = match idle {
            Some(command) => Some(command),
            None if state.options.mode == "neighbours" && !held.is_empty() => match tokio::time::timeout(Duration::from_millis(500), commands.recv()).await {
                Ok(command) => command,
                Err(_) => {
                    // Idle: one neighbour of the most recently used page.
                    if let Some(relative) = neighbour(&state, &held, &mut neighbours_done, &mut order).await {
                        Some(Command::Add(vec![relative]))
                    } else {
                        commands.recv().await
                    }
                }
            },
            None => commands.recv().await,
        };
        let Some(next) = next else { break };
        let mut pending = vec![next];
        while let Ok(more) = commands.try_recv() {
            pending.push(more);
        }
        let mut restart = false;
        for command in pending {
            match command {
                Command::Add(files) => add.extend(files),
                Command::Files(c, d) => {
                    for file in c {
                        if file == config_file || state.config_files().await.contains(&file) {
                            restart = true;
                        }
                        changed.insert(file);
                    }
                    deleted.extend(d);
                }
            }
        }
        if add.is_empty() && remove.is_empty() && changed.is_empty() && deleted.is_empty() && !first && !restart {
            continue;
        }
        let building = Instant::now();
        state.published.send_modify(|published| published.building = true);
        if restart {
            // A config edit replaces the compiler; frames reload into a fresh host.
            state.mark("config.changed", json!({}));
            *state.config.lock().await = None;
            host.close().await;
            generation += 1;
            state.generation.store(generation, Ordering::SeqCst);
            host = Host::new(&job(&state, generation));
            *state.out.lock().expect("out") = Some(host.out());
            let pages: Vec<(String, Page)> = state.pages.lock().await.iter().map(|(key, page)| (key.clone(), page.clone())).collect();
            let mut regenerated = Vec::new();
            for (relative, _) in &pages {
                if let Ok(page) = generate(&state, relative).await {
                    regenerated.push(page.entry.clone());
                    state.pages.lock().await.insert(relative.clone(), page);
                }
            }
            host.put_entries(&regenerated);
            watched.clear();
            first = true;
            state.mark("compiler.created", json!({ "generation": generation }));
        }
        for relative in &add {
            if let Some(page) = state.pages.lock().await.get(relative).cloned() {
                host.put_entries(std::slice::from_ref(&page.entry));
                changed.insert(page.entry.file.clone());
                held.insert(page.slug.clone(), relative.clone());
            }
        }
        // A definition edit regenerates its page's entry source.
        let edited: Vec<(String, Page)> = state.pages.lock().await.iter().filter(|(_, page)| page.reads.iter().any(|file| changed.contains(file))).map(|(key, page)| (key.clone(), page.clone())).collect();
        for (relative, _) in edited {
            if let Ok(page) = generate(&state, &relative).await {
                host.put_entries(std::slice::from_ref(&page.entry));
                changed.insert(page.entry.file.clone());
                state.pages.lock().await.insert(relative, page);
            }
        }
        // D2: the least recently used pages leave the compiler above the cap.
        if let Some(cap) = state.options.cap.filter(|_| state.options.mode == "cap") {
            let used = state.used.lock().expect("used").clone();
            let mut slugs: Vec<&String> = held.keys().collect();
            slugs.sort_by_key(|slug| used.get(*slug).copied().unwrap_or(0));
            let over = slugs.len().saturating_sub(cap);
            remove.extend(slugs.into_iter().take(over).cloned());
        }
        // The slug stays known, so a frame that asks for it again brings it back.
        for slug in &remove {
            if let Some(relative) = held.remove(slug) {
                state.pages.lock().await.remove(&relative);
                state.mark("entry.removed", json!({ "file": relative }));
            }
        }
        host.remove_entries(&remove);
        let built = if first {
            first = false;
            host.build_files(Vec::new(), Vec::new()).await
        } else {
            host.build_files(changed.into_iter().collect(), deleted.into_iter().collect()).await
        };
        seq += 1;
        state.record_chunks(&built);
        let errors: Map<String, Value> = built.entrypoints.iter().filter(|(_, point)| !point.errors.is_empty()).map(|(slug, point)| (slug.clone(), json!(point.errors))).collect();
        state.mark("build", json!({ "entries": built.entrypoints.len(), "ms": built.ms, "wall": building.elapsed().as_secs_f64() * 1000.0, "ok": built.ok, "generation": generation, "restart": restart }));
        let hash = built.hash.clone();
        let built = Arc::new(built);
        state.published.send_modify(|published| {
            published.building = false;
            published.seq = seq;
            published.generation = generation;
            published.built = Some(built.clone());
        });
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|time| time.as_millis()).unwrap_or(0);
        if restart {
            let _ = state.events.send(json!({ "type": "reload-all", "time": now }).to_string());
        } else {
            let _ = state.events.send(json!({ "type": "built", "profile": "web", "generation": generation, "hash": hash, "errors": errors, "time": now }).to_string());
        }
        // Watch what this build read, the config and the pages' definitions.
        let mut wanted: HashSet<String> = host.file_dependencies().into_iter().collect();
        wanted.insert(config_file.clone());
        for page in state.pages.lock().await.values() {
            wanted.extend(page.reads.iter().cloned());
        }
        let added: Vec<String> = wanted.difference(&watched).cloned().collect();
        let gone: Vec<String> = watched.difference(&wanted).cloned().collect();
        if !added.is_empty() || !gone.is_empty() {
            watcher
                .watch(
                    (added.iter().map(|file| rspack_paths::InternedPath::from(file.as_str())), gone.iter().map(|file| rspack_paths::InternedPath::from(file.as_str()))),
                    (std::iter::empty(), std::iter::empty()),
                    (std::iter::empty(), std::iter::empty()),
                    std::time::SystemTime::now(),
                    Box::new(Forward(state.commands.clone())),
                    Box::new(Quiet),
                )
                .await;
            watched = wanted;
        }
    }
    let _ = watcher.close().await;
    host.close().await;
}

/// RN: the next page after the most recently used one, its group first.
async fn neighbour(state: &State, held: &BTreeMap<String, String>, done: &mut HashSet<String>, order: &mut Option<Vec<(String, String)>>) -> Option<String> {
    let limit = state.options.neighbours.unwrap_or(8);
    if order.is_none() {
        let options = crate::index::Options {
            root: state.root.clone(),
            include: if state.options.include.is_empty() { vec!["**/*.workbench.ts".into(), "**/*.workbench.tsx".into()] } else { state.options.include.clone() },
            saved: state.options.index_saved.as_ref().map(PathBuf::from),
            evaluator: PathBuf::from(&state.options.evaluator),
            node: PathBuf::from(&state.options.node),
            check: crate::index::Check::Dirs,
        };
        let answer = tokio::task::spawn_blocking(move || crate::index::index(&options).0).await.ok()?;
        let value: Value = serde_json::from_slice(&answer).ok()?;
        *order = Some(value["previews"].as_array()?.iter().filter_map(|item| Some((item["file"].as_str()?.to_string(), item["title"].as_str()?.to_string()))).collect());
    }
    let order = order.as_ref()?;
    let used = state.used.lock().expect("used").clone();
    let latest = held.iter().max_by_key(|(slug, _)| used.get(*slug).copied().unwrap_or(0)).map(|(_, relative)| relative.clone())?;
    // Up to `limit` neighbours per page that was asked for; neighbours do not
    // earn more (finding from the stage 2 smoke).
    if done.len() >= limit * state.requested.lock().expect("requested").len() {
        return None;
    }
    let at = order.iter().position(|(file, _)| *file == latest)?;
    let group = |title: &str| title.rsplit_once('/').map(|(group, _)| group.to_string()).unwrap_or_default();
    let own = group(&order[at].1);
    let pages = state.pages.lock().await;
    // Next to the page first: its group's pages by distance in the list, then
    // the pages after it (stage 2 smoke: list order picked the group's first).
    let mut candidates: Vec<(usize, &(String, String))> = order.iter().enumerate().filter(|(_, (_, title))| group(title) == own).collect();
    candidates.sort_by_key(|(index, _)| (index.abs_diff(at), *index < at));
    let mut candidates: Vec<&(String, String)> = candidates.into_iter().map(|(_, item)| item).collect();
    candidates.extend(order.iter().skip(at + 1));
    let picked = candidates.into_iter().map(|(file, _)| file).find(|file| !pages.contains_key(*file) && !done.contains(*file))?.clone();
    drop(pages);
    done.insert(picked.clone());
    if let Ok(page) = generate(state, &picked).await {
        state.by_slug.lock().expect("slugs").insert(page.slug.clone(), picked.clone());
        state.pages.lock().await.insert(picked.clone(), page);
        state.mark("neighbour.added", json!({ "file": picked }));
        Some(picked)
    } else {
        None
    }
}

impl State {
    async fn config_files(&self) -> Vec<String> {
        self.config.lock().await.as_ref().map(|(_, files)| files.clone()).unwrap_or_default()
    }

    /// Records each project module's split chunk; a change is a moved module.
    fn record_chunks(&self, built: &Built) {
        let mut known = self.chunk_of.lock().expect("chunks");
        let mut moved = self.moved_modules.lock().expect("moved");
        for (module, chunk) in &built.chunk_of {
            match known.get(module) {
                Some(previous) if previous != chunk && moved.len() < 50 => moved.push(format!("{}: {previous} -> {chunk}", module.rsplit('!').next().unwrap_or(module))),
                Some(_) => {}
                None => {
                    known.insert(module.clone(), chunk.clone());
                }
            }
        }
    }
}

/// Runs the host: listens on a loopback port, prints it, and builds.
pub async fn run(options: Options) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let port = listener.local_addr().expect("address").port();
    let (commands, receiver) = mpsc::unbounded_channel();
    let (published, _) = watch::channel(Published::default());
    let (events, _) = broadcast::channel(256);
    let state = Arc::new(State {
        root: PathBuf::from(&options.root),
        options,
        pages: Mutex::new(BTreeMap::new()),
        by_slug: std::sync::Mutex::new(HashMap::new()),
        used: std::sync::Mutex::new(HashMap::new()),
        requested: std::sync::Mutex::new(HashSet::new()),
        clock: AtomicU64::new(1),
        generation: AtomicU32::new(1),
        config: Mutex::new(None),
        published,
        events,
        commands,
        timeline: std::sync::Mutex::new(Vec::new()),
        origin: Instant::now(),
        out: std::sync::Mutex::new(None),
        chunk_of: std::sync::Mutex::new(HashMap::new()),
        moved_modules: std::sync::Mutex::new(Vec::new()),
    });
    // The host ends with the process that started it: that process holds
    // stdin open, and its end closes it.
    std::thread::spawn(|| {
        let mut sink = Vec::new();
        let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut sink);
        std::process::exit(0);
    });
    state.mark("host.listening", json!({ "port": port }));
    println!("{}", json!({ "port": port }));
    let accepting = state.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(connection(accepting.clone(), stream));
        }
    });
    // Evaluate the config while the canvas loads.
    let warming = state.clone();
    tokio::spawn(async move {
        let _ = config(&warming).await;
    });
    builder(state, receiver).await;
}
