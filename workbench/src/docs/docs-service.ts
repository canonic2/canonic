/* Docs pages on the server: the page document for a lens, the problems the
   shared list shows, an example's code for Show code, and the revision that
   reloads an open page. Reading files and building examples are passed in,
   so this module decides what a page shows without owning any I/O. */

import { createHash } from 'node:crypto';
import { highlight, languageOf } from './code-highlight.ts';
import { readDocsMarkdown, type ExamplePlacement } from './docs-markdown.ts';
import { exportSource } from './export-source.ts';
import { pageDocument } from './page-document.ts';
import { matchLens } from './placement.ts';
import { renderDocs, type PanelView } from './render-markdown.ts';

/** One lens of a docs page: an examples implementation and the source the page maps to it. */
export interface DocsLens {
  key: string;
  label: string;
  adapter: string;
  styles: string[];
  environment?: string;
  /** The example folder (ending in /) or file, relative to the project root. */
  examples: string;
}

export interface DocsPageEntry {
  /** The Markdown file, relative to the project root, with `/` separators. */
  src: string;
  label: string;
  /** The lens the page opens with. */
  lens: string | null;
  lenses: DocsLens[];
}

/** An example as the worker lists it. */
export interface ListedExample {
  id: string;
  file: string;
  export: string;
}

export interface DocsBundle {
  module: string;
  stylesheet: string | null;
  revision: string;
  examples: ListedExample[];
  problems: string[];
}

export interface DocsServiceOptions {
  /** A project file's text, or null when it doesn't exist. */
  readFile(file: string): Promise<string | null>;
  /** Lists a lens's examples, or throws why it can't. */
  listExamples(lens: DocsLens): Promise<{ examples: ListedExample[]; problems: string[] }>;
  /** Builds a lens's examples bundle, or throws why it can't. */
  bundle(lens: DocsLens): Promise<DocsBundle>;
  /** Why examples can't run here (they run project code), or null when they can. */
  blocked(): string | null;
}

export interface DocsRequest {
  lens?: string | null;
  state?: string | null;
}

/** Where a page finds its script, stylesheet, and example code: the server's routes unless given. */
export interface PageLocations {
  script?: string;
  stylesheet?: string;
  /** Show code fetches this followed by the example's ID. */
  source?: string;
  /** Polled for edits; empty for a page that never changes. */
  revision?: string;
}

export const SOURCE_PATH = '/_workbench/docs/source';
export const REVISION_PATH = '/_workbench/docs/revision';

export class DocsPageError extends Error {
  readonly status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

export function lensRequest(lens: DocsLens): { adapter: string; examples: string; styles: string[]; environment?: string } {
  return { adapter: lens.adapter, examples: lens.examples, styles: lens.styles, ...(lens.environment ? { environment: lens.environment } : {}) };
}

/** The lens a request shows: the one it names, else the page's own, else the first. */
export function chooseLens(entry: DocsPageEntry, requested?: string | null): DocsLens | null {
  return entry.lenses.find(lens => lens.key === requested)
    ?? entry.lenses.find(lens => lens.key === entry.lens)
    ?? entry.lenses[0]
    ?? null;
}

export function createDocsService(options: DocsServiceOptions) {
  async function markdown(entry: DocsPageEntry): Promise<string> {
    const text = await options.readFile(entry.src);
    if (text === null) throw new DocsPageError(404, entry.src + ' doesn’t exist.');
    return text;
  }

  async function page(entry: DocsPageEntry, request: DocsRequest, docsPages: ReadonlySet<string>, locations: PageLocations = {}): Promise<string> {
    const source = await markdown(entry);
    const lens = chooseLens(entry, request.lens);
    let bundle: DocsBundle | null = null;
    let unavailable: string | null = lens ? null : 'This page has no examples lens.';
    const blocked = options.blocked();
    if (lens && blocked) unavailable = blocked;
    else if (lens) {
      try { bundle = await options.bundle(lens); }
      catch (error) { unavailable = lens.label + ': ' + String((error as Error).message ?? error); }
    }
    const available = new Set((bundle?.examples ?? []).map(example => example.id));
    const panel = (placement: ExamplePlacement): PanelView => {
      if (placement.duplicate) {
        return { status: 'invalid', note: placement.id ? 'This example is placed more than once on the page.' : 'This example block names no example.' };
      }
      if (unavailable) return { status: 'unavailable', note: unavailable };
      if (!available.has(placement.id)) return { status: 'missing', note: 'Not available in ' + lens!.label };
      return { status: 'ready' };
    };
    const rendered = renderDocs({ source, file: entry.src, docsPages, panel });
    const query = '?page=' + encodeURIComponent(entry.src) + (lens ? '&lens=' + encodeURIComponent(lens.key) : '');
    return pageDocument({
      ...(locations.script ? { script: locations.script } : {}),
      ...(locations.stylesheet ? { stylesheet: locations.stylesheet } : {}),
      title: rendered.title ?? entry.label,
      body: rendered.html,
      stylesheets: bundle?.stylesheet ? [bundle.stylesheet] : [],
      options: {
        page: entry.src,
        lens: lens?.key ?? null,
        state: request.state ?? null,
        bundle: bundle && !unavailable ? { module: bundle.module } : null,
        sourceUrl: locations.source ?? SOURCE_PATH + query + '&example=',
        revisionUrl: locations.revision ?? REVISION_PATH + query,
        revision: revisionOf(source, bundle),
      },
    });
  }

  /** The current revision of a page in a lens; it changes when the Markdown or the examples do. */
  async function revision(entry: DocsPageEntry, lensKey?: string | null): Promise<string> {
    const source = await markdown(entry);
    const lens = chooseLens(entry, lensKey);
    let bundle: DocsBundle | null = null;
    if (lens && !options.blocked()) {
      try { bundle = await options.bundle(lens); } catch { /* A failing build shows on the page; its revision is the Markdown's. */ }
    }
    return revisionOf(source, bundle);
  }

  /** One example's code: a whole file for a folder source, the export's statement for a file source. */
  async function exampleSource(entry: DocsPageEntry, lensKey: string | null, id: string): Promise<{ file: string; text: string; html: string }> {
    const lens = chooseLens(entry, lensKey);
    if (!lens) throw new DocsPageError(404, entry.src + ' has no examples lens.');
    const blocked = options.blocked();
    if (blocked) throw new DocsPageError(403, blocked);
    const listed = await options.listExamples(lens);
    const example = listed.examples.find(candidate => candidate.id === id);
    if (!example) throw new DocsPageError(404, lens.label + ' has no example “' + id + '”.');
    const file = await options.readFile(example.file);
    if (file === null) throw new DocsPageError(404, example.file + ' doesn’t exist.');
    const text = example.export === 'default' ? file : exportSource(file, example.export);
    if (text === null) throw new DocsPageError(404, 'Couldn’t find the export ' + example.export + ' in ' + example.file + '.');
    return { file: example.file, text, html: highlight(text, languageOf(example.file)) };
  }

  /** The examples a page places, for the export's references; null when its Markdown is missing. */
  async function outline(entry: DocsPageEntry): Promise<{ id: string; label: string }[] | null> {
    const text = await options.readFile(entry.src);
    if (text === null) return null;
    return readDocsMarkdown(text).placements
      .filter(placement => placement.id && !placement.duplicate)
      .map(placement => ({ id: placement.id, label: placement.label }));
  }

  /** Problems for the shared list, named by page. */
  async function problems(entries: readonly DocsPageEntry[]): Promise<string[]> {
    const out: string[] = [];
    const blocked = options.blocked();
    let skipped = false;
    for (const entry of entries) {
      const where = entry.label + ' (' + entry.src + ')';
      const text = await options.readFile(entry.src);
      if (text === null) { out.push(where + ': the Markdown file doesn’t exist.'); continue; }
      const doc = readDocsMarkdown(text);
      for (const problem of doc.problems) out.push(where + ', line ' + problem.line + ': ' + problem.message);
      for (const lens of entry.lenses) {
        if (blocked) { skipped = true; continue; }
        try {
          const listed = await options.listExamples(lens);
          for (const problem of listed.problems) out.push(where + ': ' + lens.label + ': ' + problem);
          const match = matchLens(doc.placements, listed.examples.map(example => example.id));
          for (const id of match.unplaced) out.push(where + ': example “' + id + '” in ' + lens.label + ' is not placed in ' + entry.src + '.');
        } catch (error) {
          out.push(where + ': ' + lens.label + ': ' + String((error as Error).message ?? error));
        }
      }
    }
    if (skipped) out.push('Docs page examples: ' + blocked);
    return out;
  }

  return { page, revision, exampleSource, outline, problems };
}

function revisionOf(markdown: string, bundle: DocsBundle | null): string {
  return createHash('sha256').update(markdown).update('\0' + (bundle?.revision ?? '')).digest('hex').slice(0, 16);
}
