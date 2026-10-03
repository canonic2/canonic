/* An adapter-neutral renderer. Its document, compatibility bridge and shared
   runtime stay alive while preview modules mount and dispose their content. */
import '/_workbench/preview-runtime.js';

let generation = 0;
let pending = Promise.resolve();
let fetching;
let activeModule;
const retainedStyles = new Set();
const moduleStyles = new Map();
const pageListeners = [];
const pageTimers = new Set();
const pageIntervals = new Set();
const pagePaints = new Set();
const nativeClearTimeout = window.clearTimeout.bind(window);
const nativeClearInterval = window.clearInterval.bind(window);
const nativeCancelPaint = window.cancelAnimationFrame.bind(window);

// A reusable document must also release page scripts that attach to roots or
// schedule work outside their canvas. Compatibility listeners predate this
// tracking, so the permanent bridge is preserved.
const nativeAdd = EventTarget.prototype.addEventListener;
const nativeRemove = EventTarget.prototype.removeEventListener;
EventTarget.prototype.addEventListener = function (type, listener, options) {
  if (activeModule && (this === window || this === document || this === document.body || this === document.documentElement)) {
    pageListeners.push([this, type, listener, typeof options === 'boolean' ? options : !!options?.capture]);
  }
  return nativeAdd.call(this, type, listener, options);
};
for (const [name, tracked] of [['setTimeout', pageTimers], ['setInterval', pageIntervals], ['requestAnimationFrame', pagePaints]]) {
  const original = window[name].bind(window);
  window[name] = function (...args) {
    const id = original(...args);
    if (activeModule) tracked.add(id);
    return id;
  };
}

function styleSize(style) {
  try { return style.sheet?.cssRules.length || 0; } catch (_) { return 0; }
}

function rememberStyles(baseline) {
  if (!activeModule) return;
  const owned = moduleStyles.get(activeModule) || new Set();
  for (const style of document.head.querySelectorAll('style:not([data-wb-host-style])')) {
    if (!retainedStyles.has(style) || (baseline && baseline.has(style) && baseline.get(style) !== styleSize(style))) {
      owned.add(style);
      retainedStyles.add(style);
    }
  }
  moduleStyles.set(activeModule, owned);
}

function enableStyles() {
  for (const style of moduleStyles.get(activeModule) || []) if (style.sheet) style.sheet.disabled = false;
}

async function clear() {
  const stop = window.__workbenchStop;
  window.__workbenchStop = null;
  if (stop) await stop();
  pageListeners.splice(0).forEach(([target, type, listener, capture]) => nativeRemove.call(target, type, listener, capture));
  pageTimers.forEach(nativeClearTimeout); pageTimers.clear();
  pageIntervals.forEach(nativeClearInterval); pageIntervals.clear();
  pagePaints.forEach(nativeCancelPaint); pagePaints.clear();
  rememberStyles();
  // Keep runtime-owned CSSOM sheets connected: libraries can hold references
  // to them across mounts. Disable other modules' sheets to prevent leakage.
  for (const child of Array.from(document.head.children)) if (!retainedStyles.has(child)) child.remove();
  for (const style of retainedStyles) if (style.sheet) style.sheet.disabled = true;
  activeModule = null;
  document.body.replaceChildren();
  for (const element of [document.documentElement, document.body]) {
    for (const attribute of Array.from(element.attributes)) element.removeAttribute(attribute.name);
  }
  window.__workbenchReady = false;
  window.__workbenchError = null;
  window.__workbenchOptions = null;
}

function reset() {
  generation++;
  fetching?.abort();
  if (window.__workbenchStop) void window.__workbenchStop().catch(() => {});
  pending = pending.catch(() => {}).then(clear);
  return pending;
}

function load(address) {
  const url = new URL(address, location.href);
  if (url.origin !== location.origin || !/\.workbench\.tsx?$/.test(url.pathname)) {
    return Promise.reject(new Error('The warm renderer accepts local Workbench preview definitions.'));
  }
  const ticket = ++generation;
  fetching?.abort();
  if (!window.__workbenchReady && window.__workbenchStop) void window.__workbenchStop().catch(() => {});
  const controller = fetching = new AbortController();
  const descriptor = fetch('/_workbench/previews/descriptor?file=' + encodeURIComponent(decodeURIComponent(url.pathname.slice(1))),
    { signal: controller.signal }).then(async response => {
    const result = await response.json();
    if (!response.ok) throw new Error(result.error || 'Preview compilation failed');
    return result;
  });
  // Superseded fetches may reject before their queued turn starts.
  descriptor.catch(() => {});
  pending = pending.catch(() => {}).then(async () => {
    try {
      const content = await descriptor;
      if (ticket !== generation) return;
      await clear();
      if (ticket !== generation) return;
      history.replaceState(null, '', url.href);
      const base = document.createElement('base');
      base.href = new URL(content.base, url).href;
      document.head.append(base);
      const viewport = document.createElement('meta');
      viewport.name = 'viewport'; viewport.content = 'width=device-width,initial-scale=1';
      document.head.append(viewport);
      const style = document.createElement('style');
      style.setAttribute('data-wb-host-style', '');
      style.textContent = 'html,body{margin:0;min-height:100%}#workbench-preview{min-height:100vh}';
      document.head.append(style);
      if (content.stylesheet) {
        const link = document.createElement('link');
        link.rel = 'stylesheet'; link.href = content.stylesheet;
        const loading = new Promise((resolve, reject) => {
          link.onload = resolve; link.onerror = () => reject(new Error('Preview stylesheet failed to load'));
        });
        document.head.append(link);
        let aborted;
        const cancelled = new Promise(resolve => {
          aborted = resolve;
          controller.signal.addEventListener('abort', aborted, { once: true });
        });
        try { await Promise.race([loading, cancelled]); }
        finally { controller.signal.removeEventListener('abort', aborted); }
        if (ticket !== generation) return;
      }
      document.title = content.title;
      window.__workbenchOptions = content.options;
      window.wbPreviewActions?.configure(url.search);
      activeModule = content.module;
      enableStyles();
      const stylesBeforeImport = new Map(Array.from(retainedStyles, style => [style, styleSize(style)]));
      const module = await import(content.module);
      if (ticket !== generation) return;
      rememberStyles(stylesBeforeImport);
      enableStyles();
      await module.mount(content.options);
      rememberStyles();
    } catch (error) {
      if (ticket !== generation) return;
      window.__workbenchError = String(error?.stack || error);
      window.__workbenchReady = false;
      document.body.replaceChildren(Object.assign(document.createElement('pre'), { textContent: window.__workbenchError }));
    }
  });
  return pending;
}

window.wbPreviewHost = { load, reset };
