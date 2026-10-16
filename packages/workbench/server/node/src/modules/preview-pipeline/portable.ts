/**
 * Portable builds for the design-system export and the CLI: each preview and
 * docs lens becomes a self-contained, minified ES module beside its assets,
 * built by an Rspack production compiler with the same format handling and
 * project plugins as live previews. One compiler per profile is reused, so
 * modules shared between previews compile once per export.
 */
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { docsEntrySource, docsSlug, entrySlug, previewEntrySource, type DocsExamples, type SourceImport } from './entries.ts';
import { compilerOptions, profileOf, type Profile } from './options.ts';
import type { PipelineHelpers, ProjectCompiler } from './pipeline.ts';
import { adapterImport, astroModules, builtInAstro, projectEnvironment } from './project.ts';

const require = createRequire(import.meta.url);
const HERE = path.dirname(fileURLToPath(import.meta.url));
const OMIT = new Set(['node_modules', '.git', 'dist', 'build', 'coverage']);
const slash = (file: string): string => file.split(path.sep).join('/');
const revisionOf = (outputs: Map<string, Buffer>): string => crypto.createHash('sha256')
  .update(Array.from(outputs, ([name, body]) => name + ':' + crypto.createHash('sha256').update(body).digest('hex')).join('\n')).digest('hex').slice(0, 16);

// Rspack's compilation objects and project definitions are untyped here.
type Any = any;

/** What one portable preview build hands the export. */
export interface PortablePreview {
  slug: string;
  id: string;
  title: string;
  file: string;
  revision: string;
  outputs: Map<string, Buffer>;
  /** Local source files the build read: the export's source closure. */
  localFiles: string[];
  /** Packages from node_modules the build read, by name and version. */
  packages: Array<{ name: string; version: string }>;
  portableNote: string;
  /** Each state's server render, for the built-in Astro adapter. */
  rendered: Record<string, { html: string; inputs: unknown }>;
}

export interface PortableDocs {
  revision: string;
  outputs: Map<string, Buffer>;
  stylesheet: boolean;
  examples: Array<{ id: string; file: string; export: string }>;
  problems: string[];
}

interface Built { outputs: Map<string, Buffer>; files: Set<string> }

/** One profile's production compiler, with the entry and generated modules of its next build. */
interface Compiling { compiler: Any; virtual: Any; memory: Any; current: { name: string; file: string } | null; pending: Map<string, string> }

const ASTRO_NOTE = 'Astro input edits require the live Workbench server. This export contains the authored states.';

export class PortableBuilder {
  readonly root: string;
  private compiler: ProjectCompiler;
  private helpers: PipelineHelpers;
  private previewDir: string;
  private virtualDir: string;
  private compilers = new Map<Profile, Compiling>();
  private queue: Promise<unknown> = Promise.resolve();

  constructor(compiler: ProjectCompiler, helpers: PipelineHelpers, options: { previewDir: string }) {
    this.compiler = compiler;
    this.helpers = helpers;
    this.root = compiler.root;
    this.previewDir = options.previewDir;
    this.virtualDir = path.join(this.root, '.workbench-portable');
  }

  private profile(name: Profile): Compiling {
    const existing = this.compilers.get(name);
    if (existing) return existing;
    const { rspack } = require('../../../preview/rspack.cjs');
    const { VueLoaderPlugin } = require('vue-loader');
    const { createFsFromVolume, Volume } = require('memfs');
    const config = this.compiler.config || {};
    const virtual = new rspack.experiments.VirtualModulesPlugin({});
    const state: Compiling = { compiler: null, virtual, memory: createFsFromVolume(new Volume()), current: null, pending: new Map() };
    const options = compilerOptions({
      profile: name, generation: 0, root: this.root, config, previewDir: this.previewDir, cacheDir: '', development: false, portable: true,
      entry: () => (state.current ? { [state.current.name]: [state.current.file] } : {}),
      plugins: [virtual, new rspack.DefinePlugin({ 'process.env.NODE_ENV': JSON.stringify('production'), __VUE_OPTIONS_API__: 'true', __VUE_PROD_DEVTOOLS__: 'false',
        __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: 'false', ...config.define }), new VueLoaderPlugin()],
      loaders: { vue: require.resolve('vue-loader'), style: require.resolve('style-loader'), css: require.resolve('css-loader'), html: path.join(HERE, "./html-loader.ts"), htmlPart: path.join(HERE, "./html-part-loader.ts") },
    });
    state.compiler = rspack(options);
    state.compiler.outputFileSystem = state.memory;
    // The virtual store accepts writes once a compilation starts.
    state.compiler.hooks.thisCompilation.tap('workbench-portable', () => { for (const [name_, contents] of state.pending) state.virtual.writeModule(name_, contents); });
    this.compilers.set(name, state);
    return state;
  }

  /** Builds one entry. Builds run one at a time; each replaces the last output. */
  private build(profile: Profile, name: string, file: string, source: string, extra: Map<string, string>): Promise<Built> {
    const run = this.queue.then(() => new Promise<Built>((resolve, reject) => {
      const state = this.profile(profile);
      state.current = { name, file };
      state.pending = new Map([...extra, [file, source]]);
      state.compiler.run((error: unknown, stats: Any) => {
        if (error) { reject(error); return; }
        if (stats.hasErrors()) {
          reject(new Error(stats.compilation.errors.slice(0, 3).map((item: Any) => String(item.message || item)).join('\n\n')));
          return;
        }
        const outputs = new Map<string, Buffer>();
        const walk = (directory: string) => {
          for (const item of state.memory.readdirSync(directory, { withFileTypes: true }) as Any[]) {
            const child = directory + '/' + item.name;
            if (item.isDirectory()) walk(child); else outputs.set(child.slice('/out/'.length), Buffer.from(state.memory.readFileSync(child)));
          }
        };
        walk('/out');
        resolve({ outputs, files: new Set<string>(stats.compilation.fileDependencies as Iterable<string>) });
      });
    }));
    this.queue = run.catch(() => {});
    return run;
  }

  /** Local files and packages a build read, plus the definition's own inputs and the config. */
  private closure(files: Iterable<string>, extra: Iterable<string>, astroPackages: Array<{ name: string; version: string }>): { localFiles: string[]; packages: Array<{ name: string; version: string }> } {
    const local = new Set<string>();
    const packages = new Map<string, string>(astroPackages.map(pkg => [pkg.name, pkg.version]));
    const realRoot = fs.realpathSync(this.root);
    for (const file of [...files, ...extra]) {
      if (!path.isAbsolute(file) || !fs.existsSync(file) || !fs.statSync(file).isFile()) continue;
      const relative = path.relative(realRoot, fs.realpathSync(file));
      if (relative.startsWith('..') || path.isAbsolute(relative)) continue;
      const parts = slash(relative).split('/');
      if (parts.includes('node_modules')) {
        let directory = path.dirname(file);
        while (directory !== path.dirname(directory)) {
          const manifest = path.join(directory, 'package.json');
          if (fs.existsSync(manifest)) {
            try { const pkg = JSON.parse(fs.readFileSync(manifest, 'utf8')); if (pkg.name && pkg.version) packages.set(pkg.name, pkg.version); } catch (_) { /* Unreadable manifests are left out. */ }
            break;
          }
          directory = path.dirname(directory);
        }
        continue;
      }
      if (parts[0].startsWith('.workbench-')) continue;
      local.add(path.join(this.root, relative));
    }
    return { localFiles: Array.from(local), packages: Array.from(packages, ([name, version]) => ({ name, version })) };
  }

  /** A preview's portable output: \`index.html\`, \`preview.js\`, its stylesheet, chunks and assets. */
  async compile(relative: string): Promise<PortablePreview> {
    await this.compiler.settings();
    const file = this.compiler.declared(relative);
    const evaluated = await this.compiler.definition(file) as { value: unknown; files?: string[] };
    const definition = this.helpers.validate(evaluated.value, file, this.root);
    const config = this.compiler.config || {};
    const slug = entrySlug(definition.id);
    const astro = builtInAstro(config, definition.adapter);
    const extra = new Map<string, string>();
    const sources: SourceImport[] = [];
    const watched = new Set<string>([...(evaluated.files || []), ...this.compiler.configFiles.keys()]);
    const astroPackages: Array<{ name: string; version: string }> = [];
    const renderers = new Map<string, Any>();
    const rendered: Record<string, { html: string; inputs: unknown }> = {};
    for (const [state, spec] of Object.entries({ default: { source: definition.source }, ...definition.states }) as Array<[string, Any]>) {
      const source = spec.source || definition.source;
      const target = path.resolve(path.dirname(file), source.entry);
      if (!astro) { sources.push({ state, kind: 'module', file: target, exportName: source.export || 'default' }); continue; }
      let renderer = renderers.get(target);
      if (!renderer) {
        renderer = await this.helpers.createAstro(this.root, target, { ...config, define: { 'import.meta.env.DEV': 'false', 'import.meta.env.PROD': 'true', ...config.define } });
        renderers.set(target, renderer);
        for (const dependency of renderer.dependencies) watched.add(dependency);
        if (renderer.package) astroPackages.push(renderer.package);
      }
      const inputs = { ...definition.inputs, ...spec.inputs };
      const active = renderer;
      const html = await this.helpers.mockFetches(definition, state, inputs, () => active.render(inputs, source.export));
      rendered[state] = { html, inputs };
      const module = astroModules(this.virtualDir, slug + '-' + state, target, html, renderer.resources, inputs, renderer.publicDir, (name, contents) => extra.set(name, contents));
      sources.push({ state, kind: 'astro', module, renderUrl: null });
    }
    const portableNote = astro ? ASTRO_NOTE : '';
    const entryFile = path.join(this.virtualDir, slug + '.js');
    const source = previewEntrySource({ slug, definition: file, adapter: adapterImport(config, this.root, this.previewDir, definition.adapter), sources,
      projectEnvironment: projectEnvironment(config, this.root, definition.adapter),
      ownEnvironment: definition.environment ? path.resolve(path.dirname(file), definition.environment) : null,
      styles: (definition.styles || []).map((style: string) => path.resolve(path.dirname(file), style)),
      portable: { runtime: path.join(this.previewDir, 'browser.js'), requests: path.join(this.previewDir, 'requests.js'), ...(portableNote ? { note: portableNote } : {}) } });
    const built = await this.build(profileOf(definition.adapter), 'preview', entryFile, source, extra);
    const outputs = built.outputs;
    // Declared assets travel as source, for a project to reuse.
    for (const asset of definition.assets || []) {
      const target = path.resolve(path.dirname(file), asset);
      const add = (item: string) => { watched.add(item); outputs.set('assets/source/' + slash(path.relative(this.root, item)), fs.readFileSync(item)); };
      if (fs.statSync(target).isDirectory()) {
        const walk = (directory: string) => {
          for (const item of fs.readdirSync(directory, { withFileTypes: true })) {
            if (item.isSymbolicLink() || item.name.startsWith('.') || OMIT.has(item.name)) continue;
            const child = path.join(directory, item.name);
            if (item.isDirectory()) walk(child); else add(child);
          }
        };
        walk(target);
      } else add(target);
    }
    const revision = revisionOf(outputs);
    const title = definition.title || definition.id;
    outputs.set('index.html', Buffer.from('<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">' +
      '<title>' + String(title).replace(/[&<>"']/g, char => '&#' + char.charCodeAt(0) + ';') + '</title>' +
      (outputs.has('preview.css') ? '<link rel="stylesheet" href="./preview.css">' : '') +
      '<style>html,body{margin:0;min-height:100%}#workbench-preview{width:100%;min-width:0;box-sizing:border-box;min-height:100vh}</style></head><body>' +
      '<script>window.__workbenchOptions=' + JSON.stringify({ development: false, revision }).replace(/</g, '\\u003c') + '</script>' +
      '<script type="module" src="./preview.js"></script></body></html>'));
    const closure = this.closure(built.files, watched, astroPackages);
    return { slug, id: definition.id, title, file: slash(path.relative(this.root, file)), revision, outputs, portableNote, rendered, ...closure };
  }

  /** A docs lens's portable examples: \`examples.js\`, exporting adapter, environment and examples. */
  async compileDocs(raw: unknown): Promise<PortableDocs> {
    await this.compiler.settings();
    const request = this.helpers.docsRequest(raw, this.root);
    const config = this.compiler.config || {};
    const astro = builtInAstro(config, request.adapter);
    if (astro && !request.folder) throw new Error('Astro examples are one component per file: point the lens at a folder of .astro files.');
    const listed = await this.compiler.docsExamples(request);
    const slug = docsSlug(raw);
    const extra = new Map<string, string>();
    let examples: DocsExamples;
    if (astro) {
      const modules: Array<{ id: string; module: string }> = [];
      for (const example of listed.examples) {
        const renderer = await this.helpers.createAstro(this.root, example.file, { ...config, define: { 'import.meta.env.DEV': 'false', 'import.meta.env.PROD': 'true', ...config.define } });
        modules.push({ id: example.id, module: astroModules(this.virtualDir, slug + '-' + example.id, example.file, await renderer.render({}, 'default'), renderer.resources, {}, renderer.publicDir,
          (name, contents) => extra.set(name, contents)) });
      }
      examples = { kind: 'astro', examples: modules };
    } else if (request.folder) examples = { kind: 'folder', examples: listed.examples.map(example => ({ id: example.id, file: example.file })) };
    else examples = { kind: 'exports', file: request.examples, examples: listed.examples.map(example => ({ id: example.id, exportName: example.export })) };
    const source = docsEntrySource({ slug, adapter: adapterImport(config, this.root, this.previewDir, request.adapter), projectEnvironment: projectEnvironment(config, this.root, request.adapter),
      lensEnvironment: request.environment, styles: request.styles, examples, portable: { runtime: path.join(this.previewDir, 'browser.js'), requests: path.join(this.previewDir, 'requests.js') } });
    const built = await this.build(profileOf(request.adapter), 'examples', path.join(this.virtualDir, slug + '.js'), source, extra);
    return { revision: revisionOf(built.outputs), outputs: built.outputs, stylesheet: built.outputs.has('examples.css'),
      examples: listed.examples.map(example => ({ id: example.id, file: slash(path.relative(this.root, example.file)), export: example.export })), problems: listed.problems };
  }

  close(): Promise<void> {
    return Promise.all(Array.from(this.compilers.values(), state => new Promise(resolve => state.compiler.close(resolve)))).then(() => {});
  }
}
