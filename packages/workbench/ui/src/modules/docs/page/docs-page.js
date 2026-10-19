/* The script of a served docs page: it mounts the lens's examples, runs Show
   code and Copy code, reports readiness and actions to the canvas, and reloads
   the page, keeping its scroll position, when the Markdown or an example
   changes. Options come from window.__workbenchDocs (page-document.ts). The
   server answers with the Markdown at once; a page served that way asks for
   its bundle here, and its panels wait as `pending` until then. */

import { mountExamples } from './mount-examples.js';

/** What the canvas calls on a docs page in its frame (same origin). */

const options = window.__workbenchDocs;
const SCROLL_KEY = 'wb-docs-scroll:' + location.pathname;
const actions = [];
const sources = new Map();

function report(type, detail) {
  window.dispatchEvent(new CustomEvent('workbench:' + type, { detail }));
  if (parent !== window)
    parent.postMessage(
      { type: 'workbench-preview', event: type, id: options?.page, docsPage: true, ...detail },
      location.origin,
    );
}

function onAction(type, detail) {
  if (type === 'action') {
    actions.push({ name: String(detail.name), values: detail.values ?? [] });
    actions.splice(0, Math.max(0, actions.length - 30));
  }
  report(type, { ...detail, state: options?.state ?? 'default' });
}

function source(id) {
  let pending = sources.get(id);
  if (!pending) {
    pending = fetch(options.sourceUrl + encodeURIComponent(id), { cache: 'no-store' }).then(
      async (answer) => {
        const body = await answer.json();
        if (!answer.ok) throw new Error(body.error || 'Couldn’t load the example’s code.');
        return body;
      },
    );
    pending.catch(() => sources.delete(id));
    sources.set(id, pending);
  }
  return pending;
}

function wireTools() {
  document.addEventListener('click', (event) => {
    const button = event.target?.closest('[data-wb-show-code], [data-wb-copy-code]');
    const figure = button?.closest('[data-wb-example]');
    if (!button || !figure) return;
    const id = figure.dataset.wbExample ?? '';
    if (button.hasAttribute('data-wb-show-code')) void toggleCode(figure, id, button);
    else void copyCode(id, button);
  });
}

async function toggleCode(figure, id, button) {
  const panel = figure.querySelector('[data-wb-example-code]');
  if (!panel) return;
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
  try {
    const code = await source(id);
    const pre = document.createElement('pre');
    const element = document.createElement('code');
    element.className = 'hljs';
    element.innerHTML = code.html;
    pre.title = code.file;
    pre.append(element);
    panel.replaceChildren(pre);
    panel.dataset.loaded = 'true';
  } catch (error) {
    panel.replaceChildren(
      Object.assign(document.createElement('p'), {
        className: 'wb-docs-example-note',
        textContent: String(error.message),
      }),
    );
  }
}

async function copyCode(id, button) {
  try {
    await navigator.clipboard.writeText((await source(id)).text);
    button.classList.add('is-done');
    button.title = 'Copied';
    setTimeout(() => {
      button.classList.remove('is-done');
      button.title = 'Copy code';
    }, 1500);
  } catch (error) {
    button.title = 'Couldn’t copy: ' + String(error.message);
  }
}

/* A pending panel, once the bundle says what it shows: mounted, or a note. */
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

function stylesheet(href) {
  return new Promise((resolve) => {
    const link = Object.assign(document.createElement('link'), { rel: 'stylesheet', href });
    link.addEventListener('load', () => resolve(), { once: true });
    link.addEventListener('error', () => resolve(), { once: true });
    document.head.append(link);
  });
}

/* Where a deferred page's bundle is, with its panels settled; null when nothing mounts. */
async function resolveBundle(info) {
  const pending = figures().filter((figure) => figure.dataset.status === 'pending');
  let answer;
  try {
    const response = await fetch(info, { cache: 'no-store' });
    answer = await response.json();
    if (!response.ok) throw new Error(answer.error || 'Couldn’t load the examples.');
  } catch (error) {
    for (const figure of pending) settle(figure, 'unavailable', String(error.message ?? error));
    return null;
  }
  options.revision = answer.revision;
  if (answer.error || !answer.module) {
    for (const figure of pending)
      settle(figure, 'unavailable', answer.error ?? 'This page has no examples.');
    return null;
  }
  const exported = new Set(answer.examples);
  for (const figure of pending) {
    if (exported.has(figure.dataset.wbExample ?? '')) settle(figure, 'ready');
    else settle(figure, 'missing', 'Not available in ' + (options.lensLabel ?? options.lens));
  }
  if (answer.stylesheet) await stylesheet(answer.stylesheet);
  return answer.module;
}

function figures() {
  return Array.from(document.querySelectorAll('[data-wb-example]'));
}

function visibleExamples() {
  return figures()
    .filter((figure) => {
      const box = figure.getBoundingClientRect();
      return box.bottom > 0 && box.top < window.innerHeight;
    })
    .map((figure) => ({ id: figure.dataset.wbExample ?? '', status: figure.dataset.status ?? '' }));
}

/* An address that names an example opens the page scrolled to it. */
function scrollToExample(id) {
  document.getElementById('example-' + id)?.scrollIntoView({ block: 'start' });
}

/* Links between docs pages open the other page on the canvas; on its own, the
   page follows them like any link. Every other link is recorded as an action,
   as in a preview. */
function wireLinks() {
  if (parent === window) return;
  addEventListener(
    'click',
    (event) => {
      const link = event.target?.closest('a[data-wb-docs-page]');
      if (!link || event.defaultPrevented || event.button !== 0) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      const fragment = /#example-([a-z0-9-]+)$/.exec(link.getAttribute('href') ?? '');
      report('docs-navigate', { page: link.dataset.wbDocsPage, example: fragment?.[1] ?? null });
    },
    true,
  );
  if (window.wbPreviewActions) {
    window.wbPreviewActions.follow = (href, kind) => {
      onAction('action', { name: kind === 'submit' ? 'submit' : 'navigate', values: [href] });
      return true;
    };
  }
}

function restoreScroll() {
  try {
    const saved = sessionStorage.getItem(SCROLL_KEY);
    if (saved === null) return;
    sessionStorage.removeItem(SCROLL_KEY);
    const [x, y] = saved.split(',').map(Number);
    window.scrollTo(x ?? 0, y ?? 0);
  } catch {
    /* Storage can be unavailable; the page then opens at the top. */
  }
}

/**
 * The server pushes "docs-changed" when a docs page's Markdown changes; the
 * page checks its revision once and, when it changed, swaps its Markdown in
 * place. Mounted examples stay (they hot-update on their own), and so does
 * the scroll position.
 */
function watchRevision() {
  if (!options?.revisionUrl) return;
  const revisionUrl = options.revisionUrl;
  let hub = window;
  try {
    if (parent !== window && parent.location.origin === location.origin) hub = parent;
  } catch {
    /* Another origin: use this window. */
  }
  const source =
    hub.__wbPushSource ??
    (hub.__wbPushSource = new hub.EventSource('/_workbench/previews/rspack/events'));
  let checking = false;
  const check = async () => {
    if (checking) return;
    checking = true;
    try {
      const answer = await fetch(revisionUrl, { cache: 'no-store' });
      const { revision } = await answer.json();
      if (revision && revision !== options.revision) {
        options.revision = revision;
        await swapMarkdown();
      }
    } catch {
      /* The server can restart while the page stays open. */
    } finally {
      checking = false;
    }
  };
  const onMessage = (event) => {
    try {
      if (JSON.parse(event.data).type === 'docs-changed') void check();
    } catch {
      /* Not a pipeline message. */
    }
  };
  source.addEventListener('message', onMessage);
  addEventListener('pagehide', () => source.removeEventListener('message', onMessage), {
    once: true,
  });
}

async function swapMarkdown() {
  const answer = await fetch(location.href, { cache: 'no-store' });
  const next = new DOMParser()
    .parseFromString(await answer.text(), 'text/html')
    .querySelector('main.wb-docs');
  const current = document.querySelector('main.wb-docs');
  if (!next || !current) {
    try {
      sessionStorage.setItem(SCROLL_KEY, window.scrollX + ',' + window.scrollY);
    } catch {
      /* See restoreScroll. */
    }
    location.reload();
    return;
  }
  for (const figure of Array.from(next.querySelectorAll('[data-wb-example]'))) {
    const live = current.querySelector(
      '[data-wb-example="' + CSS.escape(figure.getAttribute('data-wb-example') ?? '') + '"]',
    );
    if (live) figure.replaceWith(live);
  }
  current.replaceChildren(...Array.from(next.childNodes));
}

async function start() {
  window.__workbenchReady = false;
  window.__workbenchError = null;
  window.wbDocsPage = { scrollToExample, visibleExamples };
  wireTools();
  wireLinks();
  let results = [];
  const wanted = options?.bundle;
  const module = !wanted
    ? null
    : 'info' in wanted
      ? await resolveBundle(wanted.info)
      : wanted.module;
  if (module && options) {
    try {
      // Against the page, not this script: in a portable export they sit in different folders.
      const bundle = await import(new URL(module, location.href).href);
      const mounted = await mountExamples(document, bundle, {
        page: options.page,
        state: options.state ?? 'default',
        report: onAction,
      });
      results = mounted.results;
      addEventListener(
        'pagehide',
        () => {
          void mounted.dispose().catch(() => {});
        },
        { once: true },
      );
    } catch (error) {
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
    await document.fonts.ready;
  }
  restoreScroll();
  window.__workbenchReady = true;
  report('ready', {
    state: options?.state ?? 'default',
    inputs: {},
    controls: {},
    docs: '',
    actions,
    examples: results,
  });
  watchRevision();
}

void start();
