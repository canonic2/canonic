import { mountExamples } from './mount-examples.js';
import { documentTrace, phase, address } from '../../trace/browser.js';
import { createHighlighting } from './highlighting.js';

let current = null;
let generation = 0;
let transition = Promise.resolve();
let cleanupFailure;
const live = (page) => page === current && !page.controller.signal.aborted;

function report(type, detail) {
  const options = current?.options;
  window.dispatchEvent(new CustomEvent('workbench:' + type, { detail }));
  if (parent !== window)
    parent.postMessage(
      { type: 'workbench-preview', event: type, id: options?.page, docsPage: true, ...detail },
      location.origin,
    );
}

function onAction(page, type, detail) {
  if (!live(page)) return;
  if (type === 'action') {
    page.actions.push({ name: String(detail.name), values: detail.values ?? [] });
    page.actions.splice(0, Math.max(0, page.actions.length - 30));
  }
  report(type, { ...detail, scenario: page.options?.scenario ?? 'default' });
}

function source(page, id) {
  let pending = page.sources.get(id);
  if (!pending) {
    const url =
      page.options.sourceUrl +
      encodeURIComponent(id) +
      (page.options.highlightUrl ? '&plain=1' : '');
    pending = fetch(address(url, page.trace), {
      cache: 'no-store',
      signal: page.controller.signal,
    }).then(async (answer) => {
      const body = await answer.json();
      if (!answer.ok) throw new Error(body.error || 'Couldn’t load the example’s code.');
      return body;
    });
    pending.catch(() => {
      if (page.sources.get(id) === pending) page.sources.delete(id);
    });
    page.sources.set(id, pending);
  }
  return pending;
}

async function toggleCode(figure, id, button) {
  const page = current;
  const panel = figure.querySelector('[data-wb-example-code]');
  if (!panel || !page || !live(page)) return;
  const opening = panel.hidden;
  button.setAttribute('aria-expanded', String(opening));
  button.setAttribute('aria-label', opening ? 'Hide code' : 'Show code');
  button.title = opening ? 'Hide code' : 'Show code';
  if (!opening) {
    panel.hidden = true;
    return;
  }
  panel.hidden = false;
  if (panel.dataset.loaded) return;
  const revision = page.sourceRevision;
  try {
    const code = await source(page, id);
    if (!live(page) || revision !== page.sourceRevision || !panel.isConnected) return;
    const pre = document.createElement('pre');
    const element = document.createElement('code');
    element.className = 'hljs';
    if (page.options.highlightUrl) element.textContent = code.text;
    else element.innerHTML = code.html;
    pre.title = code.file;
    pre.append(element);
    panel.replaceChildren(pre);
    panel.dataset.loaded = 'true';
    if (page.options.highlightUrl) page.highlighting.source(element, code.file?.split('.').pop());
  } catch (error) {
    if (!live(page) || revision !== page.sourceRevision || !panel.isConnected) return;
    panel.replaceChildren(
      Object.assign(document.createElement('p'), {
        className: 'wb-docs-example-note',
        textContent: String(error.message),
      }),
    );
  }
}

async function copyCode(id, button) {
  const page = current;
  if (!page || !live(page)) return;
  try {
    const code = await source(page, id);
    if (!live(page)) return;
    await navigator.clipboard.writeText(code.text);
    if (!live(page)) return;
    button.classList.add('is-done');
    button.title = 'Copied';
    const timer = setTimeout(() => {
      page.timers.delete(timer);
      if (!live(page)) return;
      button.classList.remove('is-done');
      button.title = 'Copy code';
    }, 1500);
    page.timers.add(timer);
  } catch (error) {
    if (live(page)) button.title = 'Couldn’t copy: ' + String(error.message);
  }
}

function settle(figure, status, note) {
  figure.dataset.status = status;
  if (status === 'ready') return;
  figure.replaceChildren(
    Object.assign(document.createElement('p'), {
      className: 'wb-docs-example-note',
      textContent: note ?? '',
    }),
  );
}

function stylesheet(href, signal) {
  return new Promise((resolve) => {
    const link = Object.assign(document.createElement('link'), { rel: 'stylesheet', href });
    const done = () => {
      link.removeEventListener('load', done);
      link.removeEventListener('error', done);
      signal.removeEventListener('abort', done);
      resolve();
    };
    if (signal.aborted) {
      resolve();
      return;
    }
    link.addEventListener('load', done, { once: true });
    link.addEventListener('error', done, { once: true });
    signal.addEventListener('abort', done, { once: true });
    document.head.append(link);
  });
}

function figures() {
  return Array.from(document.querySelectorAll('[data-wb-example]'));
}

async function resolveBundle(page, info, early) {
  const pending = figures().filter((figure) => figure.dataset.status === 'pending');
  let answer;
  try {
    const target = new URL(info, location.href);
    const response =
      early && early.url === target.pathname + target.search
        ? await early.response
        : await fetch(address(info, page.trace), {
            cache: 'no-store',
            signal: page.controller.signal,
          }).then(async (r) => ({ ok: r.ok, body: await r.json() }));
    if (!live(page)) return null;
    answer = response.body;
    phase(page.trace, 'bundle-info-fetched');
    if (!response.ok) throw new Error(answer.error || 'Couldn’t load the examples.');
  } catch (error) {
    if (!live(page)) return null;
    for (const figure of pending) settle(figure, 'unavailable', String(error.message ?? error));
    return null;
  }
  page.options.revision = answer.revision;
  if (answer.error || !answer.module) {
    for (const figure of pending)
      settle(figure, 'unavailable', answer.error ?? 'This page has no examples.');
    return null;
  }
  const exported = new Set(answer.examples);
  for (const figure of pending) {
    if (exported.has(figure.dataset.wbExample ?? '')) settle(figure, 'ready');
    else
      settle(
        figure,
        'missing',
        'Not available in ' + (page.options.lensLabel ?? page.options.lens),
      );
  }
  if (answer.stylesheet) await stylesheet(answer.stylesheet, page.controller.signal);
  return answer.module;
}

/* Imports a page module and returns which module stylesheets it asked for,
   so stylesheets only an earlier page used can be removed and shared ones
   stay parsed, with their fonts. */
async function importPage(url) {
  const requested = new Set();
  const owned = Object.hasOwn(document, 'querySelector');
  const original = document.querySelector;
  document.querySelector = function (selector) {
    const match = /^link\[data-rspack="([^"]+)"\]$/.exec(selector);
    if (match) requested.add(match[1]);
    return original.call(this, selector);
  };
  try {
    const bundle = await import(url);
    return { bundle, requested };
  } finally {
    if (owned) document.querySelector = original;
    else delete document.querySelector;
  }
}

function pruneModuleStyles(requested) {
  for (const link of Array.from(document.head.querySelectorAll('link[data-rspack]')))
    if (!requested.has(link.getAttribute('data-rspack'))) link.remove();
}

function visibleExamples() {
  return figures()
    .filter((figure) => {
      const box = figure.getBoundingClientRect();
      return box.bottom > 0 && box.top < window.innerHeight;
    })
    .map((figure) => ({ id: figure.dataset.wbExample ?? '', status: figure.dataset.status ?? '' }));
}

function scrollToExample(id) {
  document.getElementById('example-' + id)?.scrollIntoView({ block: 'start' });
}

function restorePosition([left = 0, top = 0] = []) {
  // Page replacement must not inherit a project's smooth-scroll animation.
  window.scrollTo({ left, top, behavior: 'instant' });
}

let wired = false;
/** Installs the document's own listeners; the host calls it before it tracks page listeners. */
export function wireOnce() {
  if (wired) return;
  wired = true;
  document.addEventListener('click', (event) => {
    const button = event.target?.closest('[data-wb-show-code], [data-wb-copy-code]');
    const figure = button?.closest('[data-wb-example]');
    if (!button || !figure) return;
    const id = figure.dataset.wbExample ?? '';
    if (button.hasAttribute('data-wb-show-code')) void toggleCode(figure, id, button);
    else void copyCode(id, button);
  });
  if (parent !== window) {
    addEventListener(
      'click',
      (event) => {
        const link = event.target?.closest('a[data-wb-docs-page]');
        if (
          !current ||
          current.controller.signal.aborted ||
          !link ||
          event.defaultPrevented ||
          event.button !== 0
        )
          return;
        event.preventDefault();
        event.stopImmediatePropagation();
        const fragment = /#example-([a-z0-9-]+)$/.exec(link.getAttribute('href') ?? '');
        report('docs-navigate', { page: link.dataset.wbDocsPage, example: fragment?.[1] ?? null });
      },
      true,
    );
    if (window.wbPreviewActions) {
      window.wbPreviewActions.follow = (href, kind) => {
        if (current)
          onAction(current, 'action', {
            name: kind === 'submit' ? 'submit' : 'navigate',
            values: [href],
          });
        return true;
      };
    }
  }
  addEventListener(
    'pagehide',
    () => {
      void stop().catch((error) => console.warn('[workbench] Docs cleanup failed', error));
    },
    { once: true },
  );
  window.wbDocsPage = {
    scrollToExample,
    visibleExamples,
    whenHighlighted: () => current?.highlighting.settled() ?? Promise.resolve(),
    checkRevision,
    examplesUpdated: (entry, revision) => {
      const page = current;
      if (page && live(page) && page.entry === entry && revision) {
        page.options.revision = (page.options.revision?.split(':')[0] ?? '') + ':' + revision;
      }
    },
    refresh: () => (current ? swapMarkdown(current) : Promise.resolve()),
  };
}

function watchRevision(page) {
  if (!page.options?.revisionUrl) return;
  let hub = window;
  try {
    if (parent !== window && parent.location.origin === location.origin) hub = parent;
  } catch {
    /* A standalone page owns its connection. */
  }
  const owned = !hub.__wbPushSource;
  const events =
    hub.__wbPushSource ??
    (hub.__wbPushSource = new hub.EventSource('/_workbench/previews/rspack/events'));
  const onMessage = (event) => {
    try {
      if (JSON.parse(event.data).type === 'docs-changed') void checkRevision();
    } catch {
      /* Not a pipeline message. */
    }
  };
  events.addEventListener('message', onMessage);
  page.unsubscribe = () => {
    events.removeEventListener('message', onMessage);
    if (owned && hub === window && !window.__workbenchDocsHost) {
      events.close();
      if (hub.__wbPushSource === events) delete hub.__wbPushSource;
    }
  };
}

/** Return-time freshness shares the pushed Markdown update path. */
export async function checkRevision() {
  const page = current;
  if (!page?.options?.revisionUrl || !live(page)) return;
  if (page.checking) return page.checking;
  page.checking = (async () => {
    try {
      const answer = await fetch(page.options.revisionUrl, {
        cache: 'no-store',
        signal: page.controller.signal,
      });
      if (!answer.ok) throw new Error('Couldn’t check the Docs revision.');
      const { revision } = await answer.json();
      if (live(page) && revision && revision !== page.options.revision) {
        if (revision.split(':')[1] !== page.options.revision?.split(':')[1]) {
          if (window.wbDocsHost) {
            await window.wbDocsHost.load(location.href);
          } else {
            try {
              sessionStorage.setItem(
                'wb-docs-scroll:' + location.pathname,
                window.scrollX + ',' + window.scrollY,
              );
            } catch {
              /* Scroll persistence is optional; fresh examples are required. */
            }
            location.reload();
          }
          return;
        }
        await swapMarkdown(page);
        if (live(page)) page.options.revision = revision;
      }
    } catch (error) {
      // Keep retained content usable; a later push or return retries.
      if (live(page))
        phase(page.trace, 'revision-check-failed', { message: String(error.message) });
    } finally {
      page.checking = null;
    }
  })();
  return page.checking;
}

async function swapMarkdown(page) {
  const answer = await fetch(location.href, { cache: 'no-store', signal: page.controller.signal });
  if (!answer.ok) throw new Error('Couldn’t refresh the Docs page.');
  const next = new DOMParser()
    .parseFromString(await answer.text(), 'text/html')
    .querySelector('main.wb-docs');
  const main = document.querySelector('main.wb-docs');
  if (!live(page)) return;
  if (!next || !main) throw new Error('The Docs page has no content.');
  const scroll = [window.scrollX, window.scrollY];
  const refreshing = [];
  for (const figure of Array.from(next.querySelectorAll('[data-wb-example]'))) {
    const live = main.querySelector(
      '[data-wb-example="' + CSS.escape(figure.getAttribute('data-wb-example') ?? '') + '"]',
    );
    if (live) figure.replaceWith(live);
  }
  page.sourceRevision += 1;
  page.highlighting.reset();
  main.replaceChildren(...Array.from(next.childNodes));
  window.wbPageScenarios?.apply(page.options?.scenario ?? 'default');
  page.sources.clear();
  for (const figure of figures()) {
    const panel = figure.querySelector('[data-wb-example-code]');
    const button = figure.querySelector('[data-wb-show-code]');
    if (!panel || !button) continue;
    const open = !panel.hidden;
    delete panel.dataset.loaded;
    panel.replaceChildren();
    if (open) {
      panel.hidden = true;
      refreshing.push(toggleCode(figure, figure.dataset.wbExample ?? '', button));
    }
  }
  if (page.options.highlightUrl) page.highlighting.fences(document);
  await Promise.all(refreshing);
  if (live(page)) restorePosition(scroll);
}

/** Starts a page whose Markdown is already in the document. */
async function run(options, { scroll, answer: early } = {}) {
  const page = {
    options,
    trace: documentTrace(options),
    actions: [],
    sources: new Map(),
    sourceRevision: 0,
    mounted: null,
    controller: new AbortController(),
    timers: new Set(),
    highlighting: createHighlighting({ report: (name) => live(page) && phase(page.trace, name) }),
  };
  current = page;
  window.__workbenchReady = false;
  window.__workbenchError = null;
  window.wbPageScenarios?.apply(options?.scenario ?? 'default');
  watchRevision(page);
  let results = [];
  const wanted = options?.bundle;
  phase(page.trace, 'document-script');
  const module = !wanted
    ? null
    : 'info' in wanted
      ? await resolveBundle(page, wanted.info, early)
      : wanted.module;
  if (!live(page)) return;
  if (module) {
    try {
      const { bundle, requested } = await importPage(new URL(module, location.href).href);
      if (!live(page)) return;
      page.entry = window.__wbCurrent;
      pruneModuleStyles(requested);
      phase(page.trace, 'bundle-fetched');
      page.mounted = await mountExamples(document, bundle, {
        phase: (name) => phase(page.trace, name),
        signal: page.controller.signal,
        page: options.page,
        scenario: options.scenario ?? 'default',
        report: (type, detail) => onAction(page, type, detail),
      });
      if (!live(page)) return;
      results = page.mounted.results;
    } catch (error) {
      if (!live(page)) return;
      window.__workbenchError = String(error?.stack ?? error);
      for (const stage of document.querySelectorAll('[data-wb-example-stage]')) {
        stage.replaceWith(
          Object.assign(document.createElement('pre'), {
            className: 'wb-docs-example-error',
            textContent: window.__workbenchError,
          }),
        );
      }
      report('error', { message: String(error?.message ?? error) });
    }
  } else if (document.fonts) {
    phase(page.trace, 'examples-mounted', { examples: 0 });
    await Promise.race([
      document.fonts.ready,
      new Promise((resolve) =>
        page.controller.signal.addEventListener('abort', resolve, { once: true }),
      ),
    ]);
    if (!live(page)) return;
    phase(page.trace, 'readiness-fonts');
  }
  restorePosition(scroll);
  if (!scroll) {
    try {
      const key = 'wb-docs-scroll:' + location.pathname;
      const saved = sessionStorage.getItem(key);
      if (saved) {
        sessionStorage.removeItem(key);
        restorePosition(saved.split(',').map(Number));
      }
    } catch {
      /* Storage can be unavailable. */
    }
  }
  if (!live(page)) return;
  if (options?.highlightUrl) page.highlighting.fences(document);
  window.__workbenchReady = true;
  report('ready', {
    scenario: options?.scenario ?? 'default',
    inputs: {},
    controls: {},
    docs: '',
    actions: page.actions,
    examples: results,
  });
}

function cancelCurrent() {
  if (!current) return;
  current.controller.abort();
  current.highlighting.dispose();
  current.unsubscribe?.();
  for (const timer of current.timers) clearTimeout(timer);
  current.timers.clear();
  window.__workbenchReady = false;
}

async function disposeCurrent() {
  const page = current;
  current = null;
  page?.sources.clear();
  if (page?.mounted) {
    try {
      await page.mounted.dispose();
    } catch (error) {
      cleanupFailure = error;
    }
  }
  if (cleanupFailure) throw cleanupFailure;
}

/** Serialize cleanup and mounts, including imports that cannot be aborted. */
export function start(options, settings = {}) {
  wireOnce();
  const ticket = ++generation;
  cancelCurrent();
  const next = transition
    .catch(() => {})
    .then(async () => {
      await disposeCurrent();
      if (ticket === generation) await run(options, settings);
    });
  transition = next;
  return next;
}

export function stop() {
  ++generation;
  cancelCurrent();
  const stopped = transition.catch(() => {}).then(disposeCurrent);
  transition = stopped;
  return stopped;
}
