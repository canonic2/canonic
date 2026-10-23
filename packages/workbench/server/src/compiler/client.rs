//! Browser code the compiler generates: the hot client each profile's runtime
//! includes, and the preview module a warm host imports for a preview or docs
//! lens. Both are strings, built as modules or served.
use super::entries::{js_json, quote};
use serde::Serialize;
use serde_json::{Value, json};

/// Where the compiler's output, preview modules and event stream are served.
pub(crate) const BASE: &str = "/_workbench/previews/rspack/";
/// The fresh warm host a document on replaced compilers navigates to.
const FRESH_HOST: &str = "/_workbench/preview-host.html?actions=off";

/// Runs once per document and profile runtime. Every open frame applies hot
/// updates as builds arrive, one build at a time, so a retained frame is
/// current when it is shown. An update that cannot apply reloads the preview
/// into its warm host. Build errors go to the preview whose graph has them.
pub(crate) fn hot_client_source(profile: &str, generation: u64) -> String {
    HOT_CLIENT
        .replace("__PROFILE__", &quote(profile))
        .replace("__GENERATION__", &generation.to_string())
        .replace("__EVENTS__", &quote(&format!("{BASE}events")))
        .replace("__FRESH_HOST__", &quote(FRESH_HOST))
}

const HOT_CLIENT: &str = r"const profile = __PROFILE__;
const generation = __GENERATION__;
const state = (window.__wbProfiles = window.__wbProfiles || {})[profile] || (window.__wbProfiles[profile] = {});
state.require = __webpack_require__;
if (module.hot && !state.client) {
  state.client = true;
  let hub = window;
  try { if (parent !== window && parent.location.origin === location.origin) hub = parent; } catch (_) {}
  if (!hub.__wbPushSource) hub.__wbPushSource = new hub.EventSource(__EVENTS__);
  const source = hub.__wbPushSource;
  let failed = false, latest = null, revisions = {}, queue = Promise.resolve();
  const active = () => { try { return !window.frameElement || window.frameElement.classList.contains('is-active'); } catch (_) { return true; } };
  const reload = () => {
    if (window.wbDocsHost) window.wbDocsHost.load(location.href);
    else if (window.__workbenchHost && window.wbPreviewHost) window.wbPreviewHost.load(location.href);
    else location.reload();
  };
  // Each build's update starts from the build before it.
  const syncTo = target => (queue = queue.catch(() => {}).then(async () => {
    for (let step = 0; step < 50 && target && __webpack_hash__ !== target; step++) {
      for (let wait = 0; module.hot.status() !== 'idle'; wait++) {
        if (/^(abort|fail)$/.test(module.hot.status()) || wait > 500) throw new Error('Hot updates stopped: ' + module.hot.status());
        await new Promise(resolve => setTimeout(resolve, 10));
      }
      const updated = await module.hot.check(true);
      if (!updated) break;
    }
    if (target && __webpack_hash__ !== target) throw new Error('This document cannot reach build ' + target);
  }));
  // The next preview's module calls this before loading its chunks.
  state.sync = syncTo;
  const update = () => {
    if (failed) { failed = false; reload(); return; }
    const target = latest, entry = window.__wbCurrent, revision = revisions[entry];
    syncTo(target).then(() => {
      if (!failed && __webpack_hash__ === target && window.__wbCurrent === entry && revision && window.wbDocsPage) window.wbDocsPage.examplesUpdated(entry, revision);
    }).catch(error => { console.info('[workbench] hot update not applied; reloading the preview', String(error && error.message || error)); reload(); });
  };
  const onMessage = event => {
    let data; try { data = JSON.parse(event.data); } catch (_) { return; }
    if (data.type === 'reload-all') { if (window.wbDocsHost) window.wbDocsHost.replace(); else if (window.__workbenchHost) location.replace(__FRESH_HOST__); else location.reload(); return; }
    if (data.type !== 'built' || data.profile !== profile || (data.generation && data.generation !== generation)) return;
    latest = data.hash;
    revisions = data.revisions || {};
    const own = window.__wbCurrent && data.errors && data.errors[window.__wbCurrent];
    if (own && own.length) {
      failed = true;
      if (active() && window.workbench && window.workbench.error) window.workbench.error(new Error(own.join('\n\n')));
      return;
    }
    if (data.hash === __webpack_hash__) return;
    update();
  };
  source.addEventListener('message', onMessage);
  const stop = () => source.removeEventListener('message', onMessage);
  window.addEventListener('pagehide', stop);
  if (window.wbDocsHost) (window.__wbDocsRuntimeStops || (window.__wbDocsRuntimeStops = new Set())).add(stop);
}
";

/// What a preview module loads for one entry, from the last build.
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct EntryAssets {
    pub(crate) hash: Option<String>,
    pub(crate) runtime: Vec<String>,
    pub(crate) scripts: Vec<String>,
    pub(crate) styles: Vec<Style>,
    pub(crate) unique_name: String,
    pub(crate) errors: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Style {
    pub(crate) href: String,
    pub(crate) id: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryKind {
    Preview,
    Docs,
}

/// The module a warm host imports for one entry. Workbench's runtime and
/// request mocks load first, before any project module. A document that
/// already runs this profile's runtime catches up with the build these chunks
/// come from, keeps its dependency modules, and evaluates every project module
/// again, so nothing from the previous preview's modules carries over.
pub(crate) fn preview_module_source(
    slug: &str,
    profile: &str,
    kind: EntryKind,
    assets: &EntryAssets,
) -> String {
    let exports = match kind {
        EntryKind::Docs => {
            "export const adapter = entry.adapter;\nexport const environment = entry.environment;\nexport const examples = entry.examples;\n"
        }
        EntryKind::Preview => "export function mount(options) { return entry.mount(options); }\n",
    };
    let styles = assets
        .styles
        .iter()
        .map(|style| json!({ "href": style.href, "id": style.id }))
        .collect::<Vec<_>>();
    PREVIEW_MODULE
        .replace("__ERRORS__", &js_json(&json!(assets.errors)))
        .replace("__SLUG__", &quote(slug))
        .replace("__SCRIPTS__", &js_json(&json!(assets.scripts)))
        .replace("__STYLES__", &js_json(&Value::Array(styles)))
        .replace("__UNIQUE__", &quote(&assets.unique_name))
        .replace("__PROFILE__", &quote(profile))
        .replace("__RUNTIME__", &js_json(&json!(assets.runtime)))
        .replace(
            "__HASH__",
            &assets.hash.as_deref().map_or("null".to_owned(), quote),
        )
        .replace("__FRESH_HOST__", &quote(FRESH_HOST))
        + exports
}

const PREVIEW_MODULE: &str = r#"import * as runtime from '/_workbench/preview-runtime.js';
import '/_workbench/preview-requests.js';
window.__workbenchRuntime = runtime;
const errors = __ERRORS__;
if (errors.length) throw new Error(errors.join('\n\n'));
const slug = __SLUG__;
const scripts = __SCRIPTS__;
const loaded = window.__wbLoadedScripts || (window.__wbLoadedScripts = new Set());
const profileState = window.__wbProfiles && window.__wbProfiles[__PROFILE__];
const runtimeFiles = new Set(__RUNTIME__);
// Fetch chunks together while styles load. Execute them below in dependency
// order, after styles are ready: project modules can read computed styles.
const preloads = scripts.filter(src => !loaded.has(src) && !(profileState && profileState.require && runtimeFiles.has(src))).map(src => {
  const link = document.createElement('link');
  link.rel = 'preload'; link.as = 'script'; link.href = src;
  document.head.append(link);
  return link;
});
// Tagged the way Rspack's CSS runtime finds a chunk's stylesheet for hot updates.
for (const style of __STYLES__) {
  if (document.querySelector('link[data-rspack="' + __UNIQUE__ + ':' + style.id + '"]')) continue;
  const link = document.createElement('link'); link.rel = 'stylesheet'; link.href = style.href;
  link.setAttribute('data-rspack', __UNIQUE__ + ':' + style.id);
  await new Promise(resolve => { link.onload = link.onerror = resolve; document.head.append(link); });
}
if (profileState && profileState.require) {
  try { if (profileState.sync) await profileState.sync(__HASH__); }
  catch (error) { if (window.wbDocsHost) window.wbDocsHost.replace(); else if (window.__workbenchHost) location.replace(__FRESH_HOST__); else location.reload(); await new Promise(() => {}); }
  for (const src of runtimeFiles) loaded.add(src);
  const cache = profileState.require.c;
  // style-loader keeps its own registry of inserted styles, which must start
  // over with the project modules it styles.
  if (cache) for (const id of Object.keys(cache)) if ((!/node_modules/.test(id) || /style-loader|css-loader/.test(id)) && !/hot-client-/.test(id)) delete cache[id];
}
let failure = null;
const before = window.__wbRspack && window.__wbRspack[slug];
const record = event => { failure = event.error || new Error(event.message); };
window.addEventListener('error', record);
try {
  for (const src of scripts) {
    if (loaded.has(src)) continue;
    loaded.add(src);
    await new Promise((resolve, reject) => { const script = document.createElement('script'); script.src = src; script.onload = resolve; script.onerror = () => reject(new Error('Failed to load ' + src)); document.head.append(script); });
  }
  if (before && window.__wbRspack[slug] === before) window.__wbProfiles[__PROFILE__].require(before.moduleId);
} finally { window.removeEventListener('error', record); for (const link of preloads) link.remove(); }
if (failure) throw failure;
const entry = window.__wbRspack && window.__wbRspack[slug];
if (!entry) throw new Error('The preview module did not register: ' + slug);
window.__wbCurrent = slug;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialized_profile_runtimes_are_excluded_before_preloads_are_inserted() {
        let assets = EntryAssets {
            runtime: vec!["/runtime.js".into()],
            scripts: vec!["/runtime.js".into(), "/acme.js".into()],
            ..EntryAssets::default()
        };
        for kind in [EntryKind::Preview, EntryKind::Docs] {
            let source = preview_module_source("acme", "web", kind, &assets);
            let runtime = source
                .find("const runtimeFiles = new Set([\"/runtime.js\"])")
                .unwrap();
            let preload = source.find("const preloads = scripts.filter").unwrap();
            assert!(runtime < preload);
            assert!(source.contains("!loaded.has(src) && !(profileState && profileState.require && runtimeFiles.has(src))"));
            assert!(source.contains("if (loaded.has(src)) continue;"));
        }
    }

    #[test]
    fn the_hot_client_listens_for_its_own_profile_and_generation() {
        let source = hot_client_source("react-native-web", 3);
        assert!(source.starts_with("const profile = \"react-native-web\";\nconst generation = 3;"));
        assert!(source.contains("new hub.EventSource(\"/_workbench/previews/rspack/events\")"));
        assert!(source.contains("location.replace(\"/_workbench/preview-host.html?actions=off\")"));
        assert!(!source.contains("__PROFILE__") && !source.contains("__EVENTS__"));
    }

    #[test]
    fn a_preview_module_loads_styles_before_running_preloaded_chunks() {
        let assets = EntryAssets {
            hash: Some("v1".into()),
            runtime: vec!["/runtime.js".into()],
            scripts: vec!["/runtime.js".into(), "/acme.js".into()],
            styles: vec![Style {
                href: "/acme.css".into(),
                id: json!("acme"),
            }],
            unique_name: "wb_web".into(),
            errors: vec![],
        };
        let source = preview_module_source("acme", "web", EntryKind::Docs, &assets);
        let preload = source.find("link.rel = 'preload'").unwrap();
        let styles = source.find("link.rel = 'stylesheet'").unwrap();
        let execute = source.find("script.src = src").unwrap();
        assert!(preload < styles && styles < execute);
        assert!(source.contains("profileState.sync(\"v1\")"));
        assert!(source.contains("[{\"href\":\"/acme.css\",\"id\":\"acme\"}]"));
        assert!(source.ends_with("export const examples = entry.examples;\n"));
        let placeholder = regex::Regex::new("__[A-Z]+__").unwrap();
        assert!(
            !placeholder.is_match(&source),
            "every placeholder is filled"
        );
    }
}
