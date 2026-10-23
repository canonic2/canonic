import * as page from '../page/docs-page.js';

page.wireOnce();
window.__workbenchHost = true;
window.__workbenchDocsHost = true;

const lens = new URL(location.href).searchParams.get('lens');
const KEPT_GLOBALS =
  /^(?:__wb|__workbench|wb[A-Z]|workbench$|webpackChunk|rspack|wb_|\$Refresh|__REACT|__reactRefresh|__VUE|__vue|__rspack|__webpack|regeneratorRuntime)/;
const baselineGlobals = new Set(Object.keys(window));
const baselineHead = new Set(document.head.children);
const attributes = (element) => Array.from(element.attributes, (a) => [a.name, a.value]);
const baselineHtml = attributes(document.documentElement);
const baselineBody = attributes(document.body);

let tracking = false;
const pageListeners = [];
const runtimeListeners = [];
let retainingRuntimeListeners = 0;
// Adapters scope dependency initialization separately from project rendering.
// Its document listeners live as long as the cached dependency runtime.
function retainRuntimeListeners(initialize) {
  retainingRuntimeListeners++;
  try {
    return initialize();
  } finally {
    retainingRuntimeListeners--;
  }
}
const pageTimers = new Set();
const pageIntervals = new Set();
const pagePaints = new Set();
const nativeClearTimeout = window.clearTimeout.bind(window);
const nativeClearInterval = window.clearInterval.bind(window);
const nativeCancelPaint = window.cancelAnimationFrame.bind(window);
const nativeAdd = EventTarget.prototype.addEventListener;
const nativeRemove = EventTarget.prototype.removeEventListener;
EventTarget.prototype.addEventListener = function (type, listener, options) {
  if (
    tracking &&
    (this === window ||
      this === document ||
      this === document.body ||
      this === document.documentElement)
  )
    (retainingRuntimeListeners ? runtimeListeners : pageListeners).push([
      this,
      type,
      listener,
      typeof options === 'boolean' ? options : !!options?.capture,
    ]);
  return nativeAdd.call(this, type, listener, options);
};
for (const [name, tracked] of [
  ['setTimeout', pageTimers],
  ['setInterval', pageIntervals],
  ['requestAnimationFrame', pagePaints],
]) {
  const original = window[name].bind(window);
  window[name] = function (...args) {
    const id = original(...args);
    if (tracking) tracked.add(id);
    return id;
  };
}

/* A style built through the CSSOM (empty text, inserted rules) belongs to a
   library runtime that keeps a reference to it, such as React Native Web's
   sheet. Text styles (style-loader, page code) belong to the page. */
const libraryStyle = (style) => {
  try {
    return !style.textContent && (style.sheet?.cssRules.length || 0) > 0;
  } catch {
    return false;
  }
};

function restore(element, baseline) {
  for (const attribute of Array.from(element.attributes)) element.removeAttribute(attribute.name);
  for (const [name, value] of baseline) element.setAttribute(name, value);
}

async function clear() {
  tracking = false;
  await page.stop();
  pageListeners
    .splice(0)
    .forEach(([target, type, listener, capture]) =>
      nativeRemove.call(target, type, listener, capture),
    );
  pageTimers.forEach(nativeClearTimeout);
  pageTimers.clear();
  pageIntervals.forEach(nativeClearInterval);
  pageIntervals.clear();
  pagePaints.forEach(nativeCancelPaint);
  pagePaints.clear();
  for (const child of Array.from(document.head.children)) {
    if (baselineHead.has(child)) continue;
    if (child.matches('link[data-rspack]')) continue;
    if (child.localName === 'style' && libraryStyle(child)) continue;
    child.remove();
  }
  const main = document.querySelector('main.wb-docs');
  for (const child of Array.from(document.body.childNodes)) if (child !== main) child.remove();
  restore(document.documentElement, baselineHtml);
  restore(document.body, baselineBody);
  for (const key of Object.keys(window)) {
    if (!baselineGlobals.has(key) && !KEPT_GLOBALS.test(key)) {
      try {
        delete window[key];
      } catch {
        window[key] = undefined;
      }
    }
  }
}

let ticket = 0;
let pending = Promise.resolve();
let shown = null;
let fetching;
let disposed = false;
// Scroll memory is bounded independently of the retained runtime.
const scrolls = new Map();

function parse(text) {
  const parsed = new DOMParser().parseFromString(text, 'text/html');
  const main = parsed.querySelector('main.wb-docs');
  const script = Array.from(parsed.scripts).find((s) =>
    s.textContent.includes('window.__workbenchDocs='),
  );
  if (!main || !script) throw new Error('The docs page has no content.');
  const json = script.textContent
    .slice(script.textContent.indexOf('=') + 1)
    .trim()
    .replace(/;$/, '');
  return { main, title: parsed.title, options: JSON.parse(json) };
}

function load(address) {
  const url = new URL(address, location.href);
  if (
    disposed ||
    url.origin !== location.origin ||
    !url.pathname.endsWith('.md') ||
    url.searchParams.get('lens') !== lens
  )
    return Promise.reject(new Error('The warm docs host accepts local docs pages.'));
  const mine = ++ticket;
  const leavingScroll = [window.scrollX, window.scrollY];
  fetching?.abort();
  const controller = (fetching = new AbortController());
  const stopped = page.stop();
  stopped.catch(() => {});
  window.__workbenchReady = false;
  window.__workbenchError = null;
  const html = fetch(url.href, { cache: 'no-store', signal: controller.signal }).then(
    async (response) => {
      const text = await response.text();
      if (!response.ok) throw new Error('The docs page did not load: ' + response.status);
      return text;
    },
  );
  html.catch(() => {});
  let answer = null;
  {
    const info = new URL('/_workbench/docs/bundle', location.href);
    info.searchParams.set('page', decodeURIComponent(url.pathname.slice(1)));
    info.searchParams.set('lens', url.searchParams.get('lens') || '');
    answer = {
      url: info.pathname + info.search,
      response: fetch(info.href, { cache: 'no-store', signal: controller.signal }).then(
        async (r) => ({ ok: r.ok, body: await r.json() }),
      ),
    };
    answer.response.catch(() => {});
  }
  let swapped;
  const swap = new Promise((resolve) => (swapped = resolve));
  pending = pending
    .catch(() => {})
    .then(async () => {
      try {
        let next = parse(await html);
        // Both requests can straddle a Markdown save. Retry the pair rather
        // than marking old prose with the newer bundle's revision.
        for (let attempt = 0; attempt < 3; attempt++) {
          const info = await answer.response.catch(() => null);
          if (
            !info?.ok ||
            !next.options.revision ||
            !info.body.revision ||
            next.options.revision.split(':')[0] === info.body.revision.split(':')[0]
          )
            break;
          if (attempt === 2) throw new Error('The Docs page changed while loading. Try again.');
          const response = fetch(answer.url, { cache: 'no-store', signal: controller.signal });
          answer.response = response.then(async (value) => ({
            ok: value.ok,
            body: await value.json(),
          }));
          answer.response.catch(() => {});
          const refreshed = await fetch(url.href, { cache: 'no-store', signal: controller.signal });
          if (!refreshed.ok) throw new Error('Couldn’t refresh the Docs page.');
          next = parse(await refreshed.text());
        }
        if (mine !== ticket) return;
        const key = url.pathname + '|' + url.searchParams.get('lens');
        if (shown && shown !== key) {
          scrolls.delete(shown);
          scrolls.set(shown, leavingScroll);
        }
        const scroll = shown === key ? leavingScroll : scrolls.get(key);
        scrolls.delete(key);
        while (scrolls.size > 3) scrolls.delete(scrolls.keys().next().value);
        try {
          await stopped;
          await clear();
        } catch (error) {
          if (mine === ticket && !disposed) {
            console.warn('[workbench] Docs cleanup failed; replacing document', error);
            replace();
          }
          return;
        }
        if (mine !== ticket) return;
        history.replaceState(null, '', url.href);
        document.title = next.title;
        document.querySelector('main.wb-docs').replaceWith(document.adoptNode(next.main));
        window.__workbenchDocs = next.options;
        window.wbPreviewActions?.configure(url.search);
        shown = key;
        tracking = true;
        const started = page.start(next.options, { scroll, answer });
        swapped();
        await started;
      } catch (error) {
        if (mine !== ticket) return;
        window.__workbenchError = String(error?.stack || error);
        window.__workbenchReady = false;
        swapped();
      } finally {
        swapped();
      }
    });
  return swap;
}

function reset() {
  ticket++;
  fetching?.abort();
  const stopped = page.stop();
  stopped.catch(() => {});
  shown = null;
  scrolls.clear();
  pending = pending
    .catch(() => {})
    .then(async () => {
      await stopped;
      await clear();
    });
  return pending;
}

function resume(address) {
  const url = new URL(address, location.href);
  const current = new URL(location.href);
  const identity = (value) => {
    for (const key of ['wbTrace', 'wbPage', 'wbState', 'wbLens']) value.searchParams.delete(key);
    return value.pathname + value.search;
  };
  if (!window.__workbenchReady || !shown || identity(url) !== identity(current))
    return load(address);
  // The retained document is ready now; freshness never delays promotion.
  void page.checkRevision();
  return Promise.resolve();
}

function replace() {
  if (disposed) return;
  disposed = true;
  ticket++;
  fetching?.abort();
  window.__workbenchReady = false;
  const fresh = new URL('/_workbench/ui/src/modules/docs/host/docs-host.html', location.href);
  fresh.searchParams.set('lens', lens);
  releaseRuntime();
  location.replace(fresh.href);
}

function releaseRuntime() {
  for (const stop of window.__wbDocsRuntimeStops || []) stop();
  window.__wbDocsRuntimeStops?.clear();
  window.__wbPushSource?.close();
  delete window.__wbPushSource;
}

async function dispose() {
  disposed = true;
  releaseRuntime();
  try {
    await reset();
  } finally {
    runtimeListeners
      .splice(0)
      .forEach(([target, type, listener, capture]) =>
        nativeRemove.call(target, type, listener, capture),
      );
    if (window.__workbenchRetainRuntimeListeners === retainRuntimeListeners)
      delete window.__workbenchRetainRuntimeListeners;
  }
}

window.__workbenchRetainRuntimeListeners = retainRuntimeListeners;
window.wbDocsHost = { load, reset: dispose, resume, replace };
