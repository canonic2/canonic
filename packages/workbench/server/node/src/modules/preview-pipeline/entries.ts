/**
 * The generated entry modules: one per preview and one per docs lens. Each
 * imports its definition, adapter, sources, environments and styles, and
 * registers what the preview module mounts. Pure string building, so the
 * shapes are tested without a compiler.
 */
import crypto from 'node:crypto';

export const digest = (value: string): string => crypto.createHash('sha256').update(value).digest('hex').slice(0, 12);

/** Entry names are file-safe and stay unique when IDs differ only by `/` and `--`. */
export function entrySlug(id: string): string {
  return id.replace(/\//g, '--') + '-' + digest(id);
}

export function docsSlug(request: unknown): string {
  return 'docs-' + digest(JSON.stringify(request));
}

/** The adapter's mount: the built-in React Native Web runtime mounts natively. */
export interface AdapterImport { file: string; native: boolean; nativeStyles?: string }

export type SourceImport =
  | { state: string; kind: 'module'; file: string; exportName: string }
  | { state: string; kind: 'astro'; module: string; renderUrl: string | null };

/**
 * A portable entry bundles Workbench's runtime and request mocks from their
 * files and exports what the viewer or docs page imports. `note` is appended
 * to a preview's docs, with its controls removed (Astro input edits need the
 * live server).
 */
export interface PortableEntry { runtime: string; requests: string; note?: string }

export interface PreviewEntryInput {
  slug: string;
  definition: string;
  adapter: AdapterImport;
  sources: SourceImport[];
  projectEnvironment?: string | null;
  ownEnvironment?: string | null;
  styles: string[];
  portable?: PortableEntry;
}

const LIVE_RUNTIME = '/_workbench/preview-runtime.js';

function adapterLines(adapter: AdapterImport): string[] {
  if (adapter.native) {
    return [
      ...(adapter.nativeStyles ? ['import ' + JSON.stringify(adapter.nativeStyles) + ';'] : []),
      'import { mountNative } from ' + JSON.stringify(adapter.file) + ';',
      'const adapter = { mount: mountNative };',
    ];
  }
  return ['import * as adapter from ' + JSON.stringify(adapter.file) + ';'];
}

function environmentLines(project: string | null | undefined, own: string | null | undefined): { lines: string[]; expression: string } {
  const lines: string[] = [];
  const names: string[] = [];
  if (project) { lines.push('import * as projectEnvironment from ' + JSON.stringify(project) + ';'); names.push('projectEnvironment'); }
  if (own) { lines.push('import * as ownEnvironment from ' + JSON.stringify(own) + ';'); names.push('ownEnvironment'); }
  return { lines, expression: names.length === 2 ? 'combine(' + names.join(', ') + ')' : names[0] || '{}' };
}

/**
 * A preview's entry. Sources are read when `mount` runs, so a hot update to a
 * source re-renders with the new module. Sources that refresh themselves
 * (React components) stop an update there; anything else bubbles to the
 * entry, which re-renders the selected state in place.
 */
export function previewEntrySource(input: PreviewEntryInput): string {
  const portable = input.portable;
  // Request mocks first: they replace fetch before any project module runs.
  const lines = [...(portable ? ['import ' + JSON.stringify(portable.requests) + ';'] : []),
    'import definition from ' + JSON.stringify(input.definition) + ';', ...adapterLines(input.adapter),
    'import { boot, combine } from ' + JSON.stringify(portable ? portable.runtime : LIVE_RUNTIME) + ';'];
  const accepted: string[] = [];
  const sources: string[] = [];
  input.sources.forEach((source, index) => {
    const name = 'source' + index;
    if (source.kind === 'astro') {
      lines.push('import * as ' + name + ' from ' + JSON.stringify(source.module) + ';');
      accepted.push(source.module);
      sources.push(JSON.stringify(source.state) + ': { html: ' + name + '.html, inputs: ' + name + '.inputs, renderUrl: ' + JSON.stringify(source.renderUrl) + ' }');
    } else {
      lines.push('import * as ' + name + ' from ' + JSON.stringify(source.file) + ';');
      accepted.push(source.file);
      sources.push(JSON.stringify(source.state) + ': ' + name + '[' + JSON.stringify(source.exportName) + ']');
    }
  });
  const environment = environmentLines(input.projectEnvironment, input.ownEnvironment);
  lines.push(...environment.lines);
  for (const style of input.styles) lines.push('import ' + JSON.stringify(style) + ';');
  const shown = portable && portable.note
    ? '{ ...definition, controls: {}, docs: (definition.docs || "") + "\\n" + ' + JSON.stringify(portable.note) + ' }' : 'definition';
  lines.push('const sources = () => ({' + sources.join(', ') + '});',
    'const environment = ' + environment.expression + ';');
  if (portable) {
    lines.push('export function mount(options) { return boot(' + shown + ', sources(), adapter, environment, options); }',
      'if (!window.__workbenchHost) mount(window.__workbenchOptions);');
    return lines.join('\n') + '\n';
  }
  lines.push('function mount(options) { return boot(definition, sources(), adapter, environment, options); }',
    '(window.__wbRspack = window.__wbRspack || {})[' + JSON.stringify(input.slug) + '] = { mount, moduleId: module.id };',
    'if (module.hot) module.hot.accept(' + JSON.stringify(accepted) + ', () => { if (window.__workbenchRerender) window.__workbenchRerender(sources()); });');
  return lines.join('\n') + '\n';
}

export type DocsExamples =
  | { kind: 'folder'; examples: Array<{ id: string; file: string }> }
  | { kind: 'exports'; file: string; examples: Array<{ id: string; exportName: string }> }
  | { kind: 'astro'; examples: Array<{ id: string; module: string }> };

export interface DocsEntryInput {
  slug: string;
  adapter: AdapterImport;
  projectEnvironment?: string | null;
  lensEnvironment?: string | null;
  styles: string[];
  examples: DocsExamples;
  portable?: PortableEntry;
}

/** A docs lens's entry: its examples by ID, with the lens's adapter and environment. */
export function docsEntrySource(input: DocsEntryInput): string {
  const lines = [...adapterLines(input.adapter), 'import { combine } from ' + JSON.stringify(input.portable ? input.portable.runtime : LIVE_RUNTIME) + ';'];
  const environment = environmentLines(input.projectEnvironment, input.lensEnvironment);
  lines.push(...environment.lines);
  for (const style of input.styles) lines.push('import ' + JSON.stringify(style) + ';');
  const entries: string[] = [];
  if (input.examples.kind === 'folder') {
    input.examples.examples.forEach((example, index) => {
      lines.push('import * as example' + index + ' from ' + JSON.stringify(example.file) + ';');
      entries.push(JSON.stringify(example.id) + ': example' + index + '.default');
    });
  } else if (input.examples.kind === 'astro') {
    // Astro examples render in the worker; each is a module of rendered HTML.
    input.examples.examples.forEach((example, index) => {
      lines.push('import * as example' + index + ' from ' + JSON.stringify(example.module) + ';');
      entries.push(JSON.stringify(example.id) + ': { html: example' + index + '.html, inputs: {}, renderUrl: null }');
    });
  } else {
    lines.push('import * as source from ' + JSON.stringify(input.examples.file) + ';');
    for (const example of input.examples.examples) entries.push(JSON.stringify(example.id) + ': source[' + JSON.stringify(example.exportName) + ']');
  }
  if (input.portable) {
    lines.push('export { adapter };', 'export const environment = ' + environment.expression + ';', 'export const examples = {' + entries.join(', ') + '};');
    return lines.join('\n') + '\n';
  }
  lines.push('const environment = ' + environment.expression + ';',
    '(window.__wbRspack = window.__wbRspack || {})[' + JSON.stringify(input.slug) + '] = { adapter, environment, examples: {' + entries.join(', ') + '}, moduleId: module.id };');
  return lines.join('\n') + '\n';
}
