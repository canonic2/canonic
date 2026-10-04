/* The script of a served docs page: it mounts the lens's examples, runs Show
   code and Copy code, reports readiness and actions to the canvas, and reloads
   the page, keeping its scroll position, when the Markdown or an example
   changes. Options come from window.__workbenchDocs (page-document.ts). */

import type { DocsPageOptions } from '../page-document.ts';
import { mountExamples, type ExamplesBundle, type ExampleResult } from './mount-examples.ts';

/** What the canvas calls on a docs page in its frame (same origin). */
export interface DocsPageApi {
  scrollToExample(id: string): void;
  /** The examples at least partly in view, top to bottom. */
  visibleExamples(): { id: string; status: string }[];
}

declare global {
  interface Window {
    __workbenchDocs?: DocsPageOptions;
    __workbenchReady?: boolean;
    __workbenchError?: string | null;
    wbDocsPage?: DocsPageApi;
    wbPreviewActions?: { follow?: ((href: string, kind: string, element: Element) => boolean) | null };
  }
}

interface ExampleSource {
  file: string;
  text: string;
  html: string;
}

const options = window.__workbenchDocs;
const SCROLL_KEY = 'wb-docs-scroll:' + location.pathname;
const actions: { name: string; values: string[] }[] = [];
const sources = new Map<string, Promise<ExampleSource>>();

function report(type: string, detail: Record<string, unknown>): void {
  window.dispatchEvent(new CustomEvent('workbench:' + type, { detail }));
  if (parent !== window) parent.postMessage({ type: 'workbench-preview', event: type, id: options?.page, docsPage: true, ...detail }, location.origin);
}

function onAction(type: string, detail: Record<string, unknown>): void {
  if (type === 'action') {
    actions.push({ name: String(detail.name), values: (detail.values as string[]) ?? [] });
    actions.splice(0, Math.max(0, actions.length - 30));
  }
  report(type, { ...detail, state: options?.state ?? 'default' });
}

function source(id: string): Promise<ExampleSource> {
  let pending = sources.get(id);
  if (!pending) {
    pending = fetch(options!.sourceUrl + encodeURIComponent(id), { cache: 'no-store' }).then(async answer => {
      const body = await answer.json();
      if (!answer.ok) throw new Error(body.error || 'Couldn’t load the example’s code.');
      return body as ExampleSource;
    });
    pending.catch(() => sources.delete(id));
    sources.set(id, pending);
  }
  return pending;
}

function wireTools(): void {
  document.addEventListener('click', event => {
    const button = (event.target as Element | null)?.closest<HTMLButtonElement>('[data-wb-show-code], [data-wb-copy-code]');
    const figure = button?.closest<HTMLElement>('[data-wb-example]');
    if (!button || !figure) return;
    const id = figure.dataset.wbExample ?? '';
    if (button.hasAttribute('data-wb-show-code')) void toggleCode(figure, id, button);
    else void copyCode(id, button);
  });
}

async function toggleCode(figure: HTMLElement, id: string, button: HTMLButtonElement): Promise<void> {
  const panel = figure.querySelector<HTMLElement>('[data-wb-example-code]');
  if (!panel) return;
  const opening = panel.hidden;
  button.setAttribute('aria-expanded', String(opening));
  button.setAttribute('aria-label', opening ? 'Hide code' : 'Show code');
  button.title = opening ? 'Hide code' : 'Show code';
  if (!opening) { panel.hidden = true; return; }
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
    panel.replaceChildren(Object.assign(document.createElement('p'), { className: 'wb-docs-example-note', textContent: String((error as Error).message) }));
  }
}

async function copyCode(id: string, button: HTMLButtonElement): Promise<void> {
  try {
    await navigator.clipboard.writeText((await source(id)).text);
    button.classList.add('is-done');
    button.title = 'Copied';
    setTimeout(() => { button.classList.remove('is-done'); button.title = 'Copy code'; }, 1500);
  } catch (error) {
    button.title = 'Couldn’t copy: ' + String((error as Error).message);
  }
}

function figures(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>('[data-wb-example]'));
}

function visibleExamples(): { id: string; status: string }[] {
  return figures().filter(figure => {
    const box = figure.getBoundingClientRect();
    return box.bottom > 0 && box.top < window.innerHeight;
  }).map(figure => ({ id: figure.dataset.wbExample ?? '', status: figure.dataset.status ?? '' }));
}

/* An address that names an example opens the page scrolled to it. */
function scrollToExample(id: string): void {
  document.getElementById('example-' + id)?.scrollIntoView({ block: 'start' });
}

/* Links between docs pages open the other page on the canvas; on its own, the
   page follows them like any link. Every other link is recorded as an action,
   as in a preview. */
function wireLinks(): void {
  if (parent === window) return;
  addEventListener('click', event => {
    const link = (event.target as Element | null)?.closest<HTMLAnchorElement>('a[data-wb-docs-page]');
    if (!link || event.defaultPrevented || event.button !== 0) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    const fragment = /#example-([a-z0-9-]+)$/.exec(link.getAttribute('href') ?? '');
    report('docs-navigate', { page: link.dataset.wbDocsPage, example: fragment?.[1] ?? null });
  }, true);
  if (window.wbPreviewActions) {
    window.wbPreviewActions.follow = (href, kind) => {
      onAction('action', { name: kind === 'submit' ? 'submit' : 'navigate', values: [href] });
      return true;
    };
  }
}

function restoreScroll(): void {
  try {
    const saved = sessionStorage.getItem(SCROLL_KEY);
    if (saved === null) return;
    sessionStorage.removeItem(SCROLL_KEY);
    const [x, y] = saved.split(',').map(Number);
    window.scrollTo(x ?? 0, y ?? 0);
  } catch { /* Storage can be unavailable; the page then opens at the top. */ }
}

function watchRevision(): void {
  if (!options?.revisionUrl) return;
  const timer = setInterval(async () => {
    try {
      const answer = await fetch(options.revisionUrl, { cache: 'no-store' });
      const { revision } = await answer.json();
      if (revision && revision !== options.revision) {
        clearInterval(timer);
        try { sessionStorage.setItem(SCROLL_KEY, window.scrollX + ',' + window.scrollY); } catch { /* See restoreScroll. */ }
        location.reload();
      }
    } catch { /* The server can restart while the page stays open. */ }
  }, 1000);
  addEventListener('pagehide', () => clearInterval(timer), { once: true });
}

async function start(): Promise<void> {
  window.__workbenchReady = false;
  window.__workbenchError = null;
  window.wbDocsPage = { scrollToExample, visibleExamples };
  wireTools();
  wireLinks();
  let results: ExampleResult[] = [];
  if (options?.bundle) {
    try {
      // Against the page, not this script: in a portable export they sit in different folders.
      const bundle = await import(new URL(options.bundle.module, location.href).href) as ExamplesBundle;
      const mounted = await mountExamples(document, bundle, { page: options.page, state: options.state ?? 'default', report: onAction });
      results = mounted.results;
      addEventListener('pagehide', () => { void mounted.dispose().catch(() => {}); }, { once: true });
    } catch (error) {
      window.__workbenchError = String((error as Error)?.stack ?? error);
      for (const stage of document.querySelectorAll('[data-wb-example-stage]')) {
        stage.replaceWith(Object.assign(document.createElement('pre'), { className: 'wb-docs-example-error', textContent: window.__workbenchError }));
      }
      report('error', { message: String((error as Error)?.message ?? error) });
    }
  } else if (document.fonts) {
    await document.fonts.ready;
  }
  restoreScroll();
  window.__workbenchReady = true;
  report('ready', { state: options?.state ?? 'default', inputs: {}, controls: {}, docs: '', actions, examples: results });
  watchRevision();
}

void start();
