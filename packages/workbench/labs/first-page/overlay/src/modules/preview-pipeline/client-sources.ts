/**
 * Browser code the pipeline generates: the hot client each profile's runtime
 * includes, and the preview module a warm host imports for a preview or docs
 * lens. Both are strings, served or built as modules.
 */

/** Where the pipeline's output, preview modules and event stream are served. */
export const BASE = '/_workbench/previews/rspack/';
/** The fresh warm host a document on replaced compilers navigates to. */
const FRESH_HOST = '/_workbench/preview-host.html?actions=off';

/**
 * Runs once per document and profile runtime. Every open frame applies hot
 * updates as builds arrive, one build at a time, so a retained frame is
 * current when it is shown. An update that cannot apply reloads the preview
 * into its warm host. Build errors go to the preview whose graph has them.
 */
export function hotClientSource(profile: string, generation: number): string {
  return `const profile = ${JSON.stringify(profile)};
const generation = ${JSON.stringify(generation)};
const state = (window.__wbProfiles = window.__wbProfiles || {})[profile] || (window.__wbProfiles[profile] = {});
state.require = __webpack_require__;
if (module.hot && !state.client) {
  state.client = true;
  let hub = window;
  try { if (parent !== window && parent.location.origin === location.origin) hub = parent; } catch (_) {}
  if (!hub.__wbPushSource) hub.__wbPushSource = new hub.EventSource(${JSON.stringify(BASE + 'events')});
  const source = hub.__wbPushSource;
  let failed = false, latest = null, queue = Promise.resolve();
  const active = () => { try { return !window.frameElement || window.frameElement.classList.contains('is-active'); } catch (_) { return true; } };
  const reload = () => {
    if (window.__workbenchHost && window.wbPreviewHost) window.wbPreviewHost.load(location.href);
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
    syncTo(latest).catch(error => { console.info('[workbench] hot update not applied; reloading the preview', String(error && error.message || error)); reload(); });
  };
  const onMessage = event => {
    let data; try { data = JSON.parse(event.data); } catch (_) { return; }
    // First page lab, variant 3: pages from the first-page compiler move to the full build.
    if (data.type === 'reload-generation' && data.generation === generation) { if (window.__workbenchHost) location.replace(${JSON.stringify(FRESH_HOST)}); else location.reload(); return; }
    if (data.type === 'reload-all') { if (window.__workbenchHost) location.replace(${JSON.stringify(FRESH_HOST)}); else location.reload(); return; }
    if (data.type !== 'built' || data.profile !== profile || (data.generation && data.generation !== generation)) return;
    latest = data.hash;
    const own = window.__wbCurrent && data.errors && data.errors[window.__wbCurrent];
    if (own && own.length) {
      failed = true;
      if (active() && window.workbench && window.workbench.error) window.workbench.error(new Error(own.join('\\n\\n')));
      return;
    }
    if (data.hash === __webpack_hash__) return;
    update();
  };
  source.addEventListener('message', onMessage);
  window.addEventListener('pagehide', () => source.removeEventListener('message', onMessage));
}
`;
}

export interface EntryAssets {
  hash: string | null;
  runtime: string[];
  scripts: string[];
  styles: Array<{ href: string; id: string | number | null }>;
  uniqueName: string;
  errors: string[];
}

/**
 * The module a warm host imports for one entry. Workbench's runtime and
 * request mocks load first, before any project module. A document that
 * already runs this profile's runtime catches up with the build these chunks
 * come from, keeps its dependency modules, and evaluates every project module
 * again, so nothing from the previous preview's modules carries over.
 */
export function previewModuleSource(entry: { slug: string; profile: string; kind: 'preview' | 'docs' }, assets: EntryAssets): string {
  const exports = entry.kind === 'docs'
    ? 'export const adapter = entry.adapter;\nexport const environment = entry.environment;\nexport const examples = entry.examples;\n'
    : 'export function mount(options) { return entry.mount(options); }\n';
  return `import * as runtime from '/_workbench/preview-runtime.js';
import '/_workbench/preview-requests.js';
window.__workbenchRuntime = runtime;
const errors = ${JSON.stringify(assets.errors)};
if (errors.length) throw new Error(errors.join('\\n\\n'));
const slug = ${JSON.stringify(entry.slug)};
const scripts = ${JSON.stringify(assets.scripts)};
const loaded = window.__wbLoadedScripts || (window.__wbLoadedScripts = new Set());
// Tagged the way Rspack's CSS runtime finds a chunk's stylesheet for hot updates.
for (const style of ${JSON.stringify(assets.styles)}) {
  if (document.querySelector('link[data-rspack="' + ${JSON.stringify(assets.uniqueName)} + ':' + style.id + '"]')) continue;
  const link = document.createElement('link'); link.rel = 'stylesheet'; link.href = style.href;
  link.setAttribute('data-rspack', ${JSON.stringify(assets.uniqueName)} + ':' + style.id);
  await new Promise(resolve => { link.onload = link.onerror = resolve; document.head.append(link); });
}
const profileState = window.__wbProfiles && window.__wbProfiles[${JSON.stringify(entry.profile)}];
const runtimeFiles = new Set(${JSON.stringify(assets.runtime)});
if (profileState && profileState.require) {
  try { if (profileState.sync) await profileState.sync(${JSON.stringify(assets.hash)}); }
  catch (error) { if (window.__workbenchHost) location.replace(${JSON.stringify(FRESH_HOST)}); else location.reload(); await new Promise(() => {}); }
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
  if (before && window.__wbRspack[slug] === before) window.__wbProfiles[${JSON.stringify(entry.profile)}].require(before.moduleId);
} finally { window.removeEventListener('error', record); }
if (failure) throw failure;
// First page lab, variant 2: a lazily built entry registers once Rspack builds it.
for (let waited = 0; ${JSON.stringify(process.env.WORKBENCH_LAB_START === 'lazy')} && !(window.__wbRspack && window.__wbRspack[slug] && window.__wbRspack[slug] !== before) && waited < 60000; waited += 20) await new Promise(resolve => setTimeout(resolve, 20));
const entry = window.__wbRspack && window.__wbRspack[slug];
if (!entry) throw new Error('The preview module did not register: ' + slug);
window.__wbCurrent = slug;
${exports}`;
}
