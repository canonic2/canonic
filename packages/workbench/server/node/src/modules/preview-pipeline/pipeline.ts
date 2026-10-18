/**
 * The preview pipeline in the compile worker: one Rspack development
 * compiler per project and profile, building every preview and docs lens as
 * entries of one compilation, with shared chunks and hot updates. Output
 * lives in memory and the persistent cache in the system temporary folder,
 * so nothing is written inside the project. Builds are pushed to open
 * canvases over one event stream. See specs/preview-pipeline/.
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import Module, { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import type { IncomingMessage, ServerResponse } from 'node:http';
import { BASE, hotClientSource, previewModuleSource, type EntryAssets } from './client-sources.ts';
import { digest, docsEntrySource, docsSlug, entrySlug, previewEntrySource, type DocsExamples, type SourceImport } from './entries.ts';
import { compilerOptions, profileOf, projectExtensions, type Profile, type ProjectConfig } from './options.ts';
import { adapterImport, astroModules, builtInAstro, projectEnvironment } from './project.ts';

const require = createRequire(import.meta.url);
const HERE = path.dirname(fileURLToPath(import.meta.url));
const MIME: Record<string, string> = { '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.gif': 'image/gif', '.webp': 'image/webp', '.avif': 'image/avif',
  '.ico': 'image/x-icon', '.bmp': 'image/bmp', '.woff2': 'font/woff2', '.woff': 'font/woff', '.ttf': 'font/ttf', '.otf': 'font/otf', '.eot': 'application/vnd.ms-fontobject',
  '.mp4': 'video/mp4', '.webm': 'video/webm', '.mov': 'video/quicktime', '.mp3': 'audio/mpeg', '.ogg': 'audio/ogg', '.wav': 'audio/wav', '.m4a': 'audio/mp4', '.map': 'application/json' };
const slash = (file: string): string => file.split(path.sep).join('/');

// Rspack's compilation objects and project definitions are untyped here.
type Any = any;

/** The worker's compiler, which evaluates definitions and the project config. */
export interface ProjectCompiler {
  root: string;
  config: ProjectConfig;
  configFiles: Map<string, string>;
  settings(): Promise<unknown>;
  /** The absolute path of a discovered definition; throws for anything else. */
  declared(file: string): string;
  definition(file: string): Promise<{ value: unknown }>;
  index(): Promise<{ previews: Array<{ file: string }>; docs: Array<{ id: string; src: string; lenses: Array<Record<string, unknown>> }>; errors: string[] }>;
  docsExamples(request: Any): Promise<{ examples: Array<{ id: string; file: string; export: string }>; problems: string[] }>;
}

interface AstroRenderer { dependencies: ReadonlySet<string>; resources: Map<string, { contents: string; resolveDir?: string }>; publicDir: string | null; render(inputs: unknown, exportName?: string): Promise<string> }

/** Helpers the worker supplies from its CommonJS modules. */
export interface PipelineHelpers {
  validate(raw: unknown, file: string, root: string): Any;
  docsRequest(raw: unknown, root: string): Any;
  mockFetches(definition: Any, state: string, inputs: unknown, render: () => Promise<string>): Promise<string>;
  createAstro(root: string, target: string, config: unknown): Promise<AstroRenderer>;
}

export interface PipelineOptions {
  previewDir: string;
  log?: (event: string, details: Record<string, unknown>) => void;
  cacheRoot?: string;
}

export interface Entry {
  slug: string;
  kind: 'preview' | 'docs';
  key: string;
  profile: Profile;
  title: string;
  file: string;
  source: string;
  examples?: Array<{ id: string; file: string; export: string }>;
  problems?: string[];
}

interface Snapshot {
  hash: string;
  entrypoints: Map<string, { files: string[]; cssChunks: Array<{ id: string | number | null; file: string }>; runtime: string[]; errors: string[] }>;
}

interface ProfileState {
  name: Profile;
  generation: number;
  compiler: Any;
  watching: Any;
  virtual: Any;
  ready: Promise<void>;
  building: Promise<void> | null;
  finish: (() => void) | null;
  built: Snapshot | null;
  written: Map<string, string>;
}

let vueRedirected = false;
/**
 * Vue Loader requires Vue's SFC compiler from beside itself. Workbench ships
 * `@vue/compiler-sfc`, so Vue Loader is pointed at it; the project need not
 * install the compiler.
 */
function redirectVueCompiler(): void {
  if (vueRedirected) return;
  vueRedirected = true;
  const sfc = require.resolve('@vue/compiler-sfc');
  const original = (Module as Any)._resolveFilename;
  (Module as Any)._resolveFilename = function (request: string, parent: Any, ...rest: unknown[]) {
    if ((request === 'vue/compiler-sfc' || request === '@vue/compiler-sfc') && parent && /[\\/]vue-loader[\\/]/.test(parent.filename || '')) return sfc;
    return original.call(this, request, parent, ...rest);
  };
}

export class Pipeline {
  readonly root: string;
  readonly cacheDir: string;
  readonly virtualDir: string;
  /** Each set of compilers writes to its own folder and URL prefix. */
  generation = 1;
  prewarmed: { ms: number; entries: number } | null = null;
  warming: Promise<void> | null = null;
  private memory: Any;
  private profiles = new Map<Profile, ProfileState>();
  private entries = new Map<string, Entry>();
  private clients = new Set<ServerResponse>();
  private astro = new Set<string>();
  private watchers = new Map<string, { watcher: fs.FSWatcher; handlers: Set<() => void> }>();
  private configWatched = false;
  private docsWatched = false;
  private reloading: Promise<void> = Promise.resolve();
  private log: (event: string, details: Record<string, unknown>) => void;
  private compiler: ProjectCompiler;
  private helpers: PipelineHelpers;
  private previewDir: string;
  private rspack: Any;

  constructor(compiler: ProjectCompiler, helpers: PipelineHelpers, options: PipelineOptions) {
    this.compiler = compiler;
    this.helpers = helpers;
    this.root = compiler.root;
    this.previewDir = options.previewDir;
    this.log = options.log || (() => {});
    this.virtualDir = path.join(this.root, '.workbench-rspack');
    this.cacheDir = path.join(options.cacheRoot || os.tmpdir(), 'canonic-workbench-rspack', digest(this.root));
    const { createFsFromVolume, Volume } = require('memfs');
    this.memory = createFsFromVolume(new Volume());
    redirectVueCompiler();
    this.rspack = require('../../../preview/rspack.cjs').rspack;
  }

  /** Problems with the project's plugin config, reported with the catalog. */
  problems(): string[] {
    const config = this.compiler.config || {};
    return Array.from(new Set([...projectExtensions(config, 'web').problems, ...projectExtensions(config, 'react-native-web').problems]));
  }

  // ------------------------------------------------------------ compilers

  private hotClientFile(profile: Profile): string { return path.join(this.virtualDir, 'hot-client-' + profile + '.js'); }

  private profile(name: Profile): ProfileState {
    const existing = this.profiles.get(name);
    if (existing) return existing;
    const rspack = this.rspack;
    const { ReactRefreshRspackPlugin } = require('@rspack/plugin-react-refresh');
    const { VueLoaderPlugin } = require('vue-loader');
    const config = this.compiler.config || {};
    const virtual = new rspack.experiments.VirtualModulesPlugin({ [this.hotClientFile(name)]: hotClientSource(name, this.generation) });
    const generation = this.generation;
    const options = compilerOptions({
      profile: name, generation, root: this.root, config, previewDir: this.previewDir, cacheDir: this.cacheDir, development: true,
      entry: () => {
        const entries: Record<string, string[]> = {};
        for (const entry of this.entries.values()) if (entry.profile === name) entries[entry.slug] = [this.hotClientFile(name), entry.file];
        // A compiler needs an entry to start; the hot client alone is harmless.
        if (!Object.keys(entries).length) entries.__workbench_idle = [this.hotClientFile(name)];
        return entries;
      },
      plugins: [
        virtual,
        new rspack.DefinePlugin({ 'process.env.NODE_ENV': JSON.stringify('development'), __VUE_OPTIONS_API__: 'true', __VUE_PROD_DEVTOOLS__: 'false',
          __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: 'false', ...config.define }),
        new rspack.HotModuleReplacementPlugin(),
        new ReactRefreshRspackPlugin({ exclude: [/node_modules/, /\.workbench-rspack[\\/]/] }),
        new VueLoaderPlugin(),
      ],
      loaders: { vue: require.resolve('vue-loader'), style: require.resolve('style-loader'), css: require.resolve('css-loader'), html: path.join(HERE, "./html-loader.ts"), htmlPart: path.join(HERE, "./html-part-loader.ts") },
    });
    const compiler = rspack(options);
    compiler.outputFileSystem = this.memory;
    let resolveFirst: () => void = () => {};
    let rejectFirst: (error: unknown) => void = () => {};
    const state: ProfileState = { name, generation, compiler, watching: null, virtual, built: null, written: new Map(), building: null, finish: null,
      ready: new Promise<void>((resolve, reject) => { resolveFirst = resolve; rejectFirst = reject; }) };
    state.ready.catch(() => {});
    state.building = new Promise<void>(resolve => { state.finish = resolve; });
    compiler.hooks.invalid.tap('workbench', () => {
      if (!state.building) state.building = new Promise<void>(resolve => { state.finish = resolve; });
    });
    const started = Date.now();
    state.watching = compiler.watch(options.watchOptions, (error: unknown, stats: Any) => {
      if (error) { rejectFirst(error); this.log('preview.pipeline.failed', { profile: name, message: String((error as Error).message || error) }); return; }
      // A compiler replaced by a config change may still finish a build.
      if (this.profiles.get(name) !== state) return;
      state.built = snapshot(stats.compilation, stats.hash);
      const finish = state.finish;
      state.building = null; state.finish = null;
      if (finish) finish();
      resolveFirst();
      const errorsBySlug: Record<string, string[]> = {};
      for (const [slug, point] of state.built.entrypoints) if (point.errors.length) errorsBySlug[slug] = point.errors.slice(0, 3);
      this.log('preview.pipeline.built', { profile: name, generation, errors: Object.keys(errorsBySlug).length, ms: Date.now() - started });
      this.broadcast({ type: 'built', profile: name, generation, hash: stats.hash, errors: errorsBySlug, time: Date.now() });
    });
    this.profiles.set(name, state);
    return state;
  }

  private async settled(profile: ProfileState): Promise<Snapshot | null> {
    await profile.ready;
    while (profile.building) await profile.building;
    return profile.built;
  }

  /** Waits for a finished build that contains the entry. */
  private async contains(profile: ProfileState, slug: string): Promise<Snapshot> {
    for (let attempt = 0; attempt < 6000; attempt++) {
      const built = await this.settled(profile);
      if (built && built.entrypoints.has(slug)) return built;
      await new Promise(resolve => setTimeout(resolve, 10));
    }
    throw new Error('The preview compiler did not build ' + slug);
  }

  // --------------------------------------------------------------- entries

  async previewEntry(relative: string): Promise<Entry> {
    await this.compiler.settings();
    const file = this.compiler.declared(relative);
    const definition = this.helpers.validate((await this.compiler.definition(file)).value, file, this.root);
    const config = this.compiler.config || {};
    const astro = builtInAstro(config, definition.adapter);
    const slug = entrySlug(definition.id);
    const sources: SourceImport[] = [];
    if (astro) await this.renderAstro(slug, file, definition);
    for (const [state, spec] of Object.entries({ default: { source: definition.source }, ...definition.states }) as Array<[string, Any]>) {
      const source = spec.source || definition.source;
      if (astro) {
        sources.push({ state, kind: 'astro', module: path.join(this.virtualDir, 'astro', slug + '-' + state + '.js'),
          renderUrl: '/_workbench/previews/render?file=' + encodeURIComponent(slash(path.relative(this.root, file))) });
      } else sources.push({ state, kind: 'module', file: path.resolve(path.dirname(file), source.entry), exportName: source.export || 'default' });
    }
    const source = previewEntrySource({ slug, definition: file, adapter: adapterImport(this.compiler.config || {}, this.root, this.previewDir, definition.adapter), sources,
      projectEnvironment: projectEnvironment(this.compiler.config || {}, this.root, definition.adapter),
      ownEnvironment: definition.environment ? path.resolve(path.dirname(file), definition.environment) : null,
      styles: (definition.styles || []).map((style: string) => path.resolve(path.dirname(file), style)) });
    return { slug, kind: 'preview', key: slash(path.relative(this.root, file)), profile: profileOf(definition.adapter), title: definition.title || definition.id,
      file: path.join(this.virtualDir, slug + '.js'), source };
  }

  async docsEntry(raw: unknown): Promise<Entry> {
    await this.compiler.settings();
    const request = this.helpers.docsRequest(raw, this.root);
    const config = this.compiler.config || {};
    const astro = builtInAstro(config, request.adapter);
    if (astro && !request.folder) throw new Error('Astro examples are one component per file: point the lens at a folder of .astro files.');
    const listed = await this.compiler.docsExamples(request);
    const slug = docsSlug(raw);
    let examples: DocsExamples;
    if (astro) {
      const modules: Array<{ id: string; module: string }> = [];
      for (const example of listed.examples) {
        const renderer = await this.helpers.createAstro(this.root, example.file, { ...config,
          define: { 'import.meta.env.DEV': 'true', 'import.meta.env.PROD': 'false', ...config.define } });
        const key = slug + '-' + example.id;
        await this.writeAstro(key, 'default', example.file, await renderer.render({}, 'default'), renderer.resources, {}, renderer.publicDir);
        modules.push({ id: example.id, module: path.join(this.virtualDir, 'astro', key + '-default.js') });
        this.watchFiles(Array.from(renderer.dependencies).filter(dependency => !/[\\/]node_modules[\\/]/.test(dependency) && fs.existsSync(dependency)),
          () => { this.docsEntry(raw).catch(error => this.log('preview.pipeline.astro.failed', { slug, message: String(error.message || error) })); });
      }
      examples = { kind: 'astro', examples: modules };
    } else if (request.folder) examples = { kind: 'folder', examples: listed.examples.map(example => ({ id: example.id, file: example.file })) };
    else examples = { kind: 'exports', file: request.examples, examples: listed.examples.map(example => ({ id: example.id, exportName: example.export })) };
    const source = docsEntrySource({ slug, adapter: adapterImport(this.compiler.config || {}, this.root, this.previewDir, request.adapter), projectEnvironment: projectEnvironment(this.compiler.config || {}, this.root, request.adapter),
      lensEnvironment: request.environment, styles: request.styles, examples });
    return { slug, kind: 'docs', key: 'docs:' + JSON.stringify(raw), profile: profileOf(request.adapter), title: slug,
      file: path.join(this.virtualDir, slug + '.js'), source,
      examples: listed.examples.map(example => ({ id: example.id, file: slash(path.relative(this.root, example.file)), export: example.export })), problems: listed.problems };
  }

  /** Adds or refreshes an entry and waits for a build that includes it. */
  private async ensure(prepared: Entry): Promise<Entry> {
    const existing = this.entries.get(prepared.slug);
    const profile = this.profile(prepared.profile);
    // Rspack creates its virtual file store during the first build.
    await profile.ready;
    if (!existing || existing.source !== prepared.source) {
      this.entries.set(prepared.slug, prepared);
      profile.virtual.writeModule(prepared.file, prepared.source);
      if (!existing) profile.watching.invalidate();
    }
    await this.contains(profile, prepared.slug);
    return this.entries.get(prepared.slug)!;
  }

  /** A preview's entry. Requests wait for pre-warming, so they find the full entry set. */
  async preview(relative: string): Promise<Entry> {
    if (!this.warming) this.warming = this.prewarm();
    await this.warming;
    return this.ensure(await this.previewEntry(relative));
  }

  async docs(raw: unknown): Promise<Entry> {
    if (!this.warming) this.warming = this.prewarm();
    await this.warming;
    return this.ensure(await this.docsEntry(raw));
  }

  // ----------------------------------------------------------------- Astro

  /**
   * Astro frontmatter renders in Node with Workbench's renderer. Each state's
   * HTML becomes a virtual module; an edit to anything the render read
   * renders again and rewrites those modules, which Rspack hot-updates.
   */
  private async renderAstro(slug: string, file: string, definition: Any): Promise<void> {
    const config = this.compiler.config || {};
    const renderers = new Map<string, AstroRenderer>();
    const dependencies = new Set<string>();
    for (const [state, spec] of Object.entries({ default: { source: definition.source }, ...definition.states }) as Array<[string, Any]>) {
      const source = spec.source || definition.source;
      const target = path.resolve(path.dirname(file), source.entry);
      let renderer = renderers.get(target);
      if (!renderer) {
        renderer = await this.helpers.createAstro(this.root, target, { ...config,
          define: { 'import.meta.env.DEV': 'true', 'import.meta.env.PROD': 'false', ...config.define } });
        renderers.set(target, renderer);
        for (const dependency of renderer.dependencies) if (!/[\\/]node_modules[\\/]/.test(dependency) && fs.existsSync(dependency)) dependencies.add(dependency);
      }
      const inputs = { ...definition.inputs, ...spec.inputs };
      const active = renderer;
      const html = await this.helpers.mockFetches(definition, state, inputs, () => active.render(inputs, source.export));
      await this.writeAstro(slug, state, target, html, renderer.resources, inputs, renderer.publicDir);
    }
    if (!this.astro.has(slug)) {
      this.astro.add(slug);
      this.watchFiles(dependencies, () => { this.refreshAstro(slug, file).catch(() => {}); });
    }
  }

  private async writeAstro(slug: string, state: string, component: string, html: string, resources: Map<string, { contents: string; resolveDir?: string }>, inputs: unknown, publicDir: string | null): Promise<void> {
    const profile = this.profile('web');
    await profile.ready;
    astroModules(this.virtualDir, slug + '-' + state, component, html, resources, inputs, publicDir, (file, contents) => this.writeVirtual(profile, file, contents));
  }

  /** A virtual module rewritten with the same content would still start a build. */
  private writeVirtual(profile: ProfileState, file: string, contents: string): void {
    if (profile.written.get(file) === contents) return;
    profile.written.set(file, contents);
    profile.virtual.writeModule(file, contents);
  }

  private async refreshAstro(slug: string, file: string): Promise<void> {
    try {
      await this.renderAstro(slug, file, this.helpers.validate((await this.compiler.definition(file)).value, file, this.root));
    } catch (error) {
      this.broadcast({ type: 'built', profile: 'web', generation: this.generation, hash: 'astro-error-' + Date.now(), errors: { [slug]: [String((error as Error).message || error)] } });
    }
  }

  // -------------------------------------------------------------- watching

  private watchFiles(files: Iterable<string>, onChange: () => void): void {
    for (const file of files) {
      const existing = this.watchers.get(file);
      if (existing) { existing.handlers.add(onChange); continue; }
      let timer: NodeJS.Timeout | undefined;
      const handlers = new Set([onChange]);
      const changed = () => { clearTimeout(timer); timer = setTimeout(() => handlers.forEach(handler => handler()), 20); };
      try {
        // A file that doesn't exist yet, such as a config the project may add,
        // is watched through its folder until it appears.
        const watcher = fs.existsSync(file) ? fs.watch(file, changed)
          : fs.watch(path.dirname(file), (_event, name) => { if (name && String(name) === path.basename(file)) changed(); });
        this.watchers.set(file, { watcher, handlers });
      } catch (error) { this.log('preview.pipeline.watch.failed', { file: slash(path.relative(this.root, file)), message: String((error as Error).message || error) }); }
    }
  }

  /** Docs Markdown renders on the server; open docs pages hear that it changed. */
  private async watchDocs(index: { docs: Array<{ src: string }> }): Promise<void> {
    if (this.docsWatched) return;
    this.docsWatched = true;
    this.watchFiles(new Set(index.docs.map(page => path.resolve(this.root, page.src))), () => this.broadcast({ type: 'docs-changed', time: Date.now() }));
  }

  /**
   * `workbench.config.ts` sets the compilers' options: a change replaces
   * them, and every open preview reloads into the new ones. Changes apply one
   * at a time.
   */
  private watchConfig(): void {
    if (this.configWatched) return;
    this.configWatched = true;
    this.watchFiles(new Set(this.compiler.configFiles.keys()), () => { this.reloading = this.reloading.then(() => this.reloadConfig()); });
  }

  private async reloadConfig(): Promise<void> {
    try {
      await this.compiler.settings();
      const profiles = Array.from(this.profiles.values());
      this.profiles.clear();
      this.generation++;
      this.entries.clear();
      await Promise.all(profiles.map(profile => new Promise(resolve => profile.watching.close(resolve))));
      this.warming = this.prewarm();
      await this.warming;
      this.broadcast({ type: 'reload-all', time: Date.now() });
      this.log('preview.pipeline.config.reloaded', { generation: this.generation });
    } catch (error) { this.log('preview.pipeline.config.failed', { message: String((error as Error).message || error) }); }
  }

  /**
   * Declares every discovered preview and docs lens as an entry before the
   * first build, then builds them all. Rspack's hot updates do not follow a
   * module that moves to another chunk, and adding entries while pages are
   * open can move shared modules, so the entry set stays fixed.
   */
  async prewarm(): Promise<void> {
    const started = Date.now();
    try {
      const index = await this.compiler.index();
      this.watchConfig();
      await this.watchDocs(index);
      const prepared: Entry[] = [];
      for (const preview of index.previews) {
        try { prepared.push(await this.previewEntry(preview.file)); }
        catch (error) { this.log('preview.pipeline.entry.skipped', { file: preview.file, message: String((error as Error).message || error).slice(0, 300) }); }
      }
      for (const page of index.docs) for (const lens of page.lenses) {
        const raw = { adapter: lens.adapter, examples: lens.examples, styles: lens.styles, ...(lens.environment ? { environment: lens.environment } : {}) };
        try { prepared.push(await this.docsEntry(raw)); }
        catch (error) { this.log('preview.pipeline.entry.skipped', { docs: page.id, message: String((error as Error).message || error).slice(0, 300) }); }
      }
      const byProfile = new Map<Profile, Entry[]>();
      for (const entry of prepared) { if (!byProfile.has(entry.profile)) byProfile.set(entry.profile, []); byProfile.get(entry.profile)!.push(entry); }
      await Promise.all(Array.from(byProfile, async ([name, entries]) => {
        const profile = this.profile(name);
        await profile.ready;
        for (const entry of entries) { this.entries.set(entry.slug, entry); profile.virtual.writeModule(entry.file, entry.source); }
        profile.watching.invalidate();
        await this.contains(profile, entries[entries.length - 1].slug);
      }));
      this.prewarmed = { ms: Date.now() - started, entries: prepared.length };
      this.log('preview.pipeline.prewarmed', this.prewarmed);
    } catch (error) { this.log('preview.pipeline.prewarm.failed', { message: String((error as Error).message || error) }); }
  }

  // ---------------------------------------------------------------- serving

  private assets(entry: Entry): EntryAssets {
    const profile = this.profiles.get(entry.profile);
    const point = profile && profile.built && profile.built.entrypoints.get(entry.slug);
    const uniqueName = 'wb_' + entry.profile.replace(/-/g, '_');
    if (!profile || !point) return { hash: null, runtime: [], scripts: [], styles: [], uniqueName, errors: ['The preview compiler has not built ' + entry.slug] };
    const url = (name: string) => BASE + entry.profile + '/' + profile.generation + '/' + name;
    return { hash: profile.built!.hash, runtime: point.runtime.map(url), scripts: point.files.filter(name => name.endsWith('.js')).map(url),
      styles: point.cssChunks.map(chunk => ({ href: url(chunk.file), id: chunk.id })), uniqueName, errors: point.errors };
  }

  /** Serves preview modules and build output. Returns false for other routes. */
  async serve(pathname: string, res: ServerResponse): Promise<boolean> {
    if (!pathname.startsWith('/rspack/')) return false;
    const rest = decodeURIComponent(pathname.slice('/rspack/'.length));
    const module = /^module\/([^/]+)\.js$/.exec(rest);
    if (module) {
      const entry = this.entries.get(module[1]);
      if (!entry) { res.statusCode = 404; res.end('Unknown preview'); return true; }
      await this.contains(this.profile(entry.profile), entry.slug);
      res.setHeader('Content-Type', 'text/javascript');
      res.setHeader('Cache-Control', 'no-store');
      res.end(previewModuleSource(entry, this.assets(entry)));
      return true;
    }
    const [name, ...parts] = rest.split('/');
    const profile = this.profiles.get(name as Profile);
    if (!profile) { res.statusCode = 404; res.end('Unknown preview profile'); return true; }
    const hot = /(^|\/)hot\//.test(parts.join('/'));
    if (!hot) await this.settled(profile);
    const file = '/out/' + name + '/' + parts.join('/');
    let body: Buffer;
    try { body = this.memory.readFileSync(file); } catch (_) { res.statusCode = 404; res.end('Not found'); return true; }
    res.setHeader('Content-Type', MIME[path.extname(file)] || 'application/octet-stream');
    // Scripts and assets have content-hashed names; stylesheets keep stable
    // names for CSS hot updates, and hot-update files change every build.
    res.setHeader('Cache-Control', hot || file.endsWith('.css') ? 'no-cache' : 'public, max-age=31536000, immutable');
    res.end(body);
    return true;
  }

  options(): Record<string, unknown> { return { development: true, push: true }; }

  descriptor(entry: Entry): Record<string, unknown> {
    return { base: BASE + entry.profile + '/', module: BASE + 'module/' + entry.slug + '.js?load=' + Date.now(), stylesheet: null, title: entry.title, options: this.options() };
  }

  /** A standalone document for a preview, for screenshots and "Open on its own". */
  page(entry: Entry): string {
    return '<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">' +
      '<title>' + String(entry.title).replace(/[&<>"']/g, char => '&#' + char.charCodeAt(0) + ';') + '</title>' +
      '<style>html,body{margin:0;min-height:100%}#workbench-preview{width:100%;min-width:0;box-sizing:border-box;min-height:100vh}</style></head><body>' +
      '<script>window.__workbenchOptions=' + JSON.stringify(this.options()) + '</script>' +
      '<script type="module">import { mount } from ' + JSON.stringify(BASE + 'module/' + entry.slug + '.js') + '; mount(window.__workbenchOptions);</script></body></html>';
  }

  private broadcast(message: Record<string, unknown>): void {
    const data = 'data: ' + JSON.stringify(message) + '\n\n';
    for (const client of this.clients) client.write(data);
  }

  /** One event stream per open canvas. */
  events(req: IncomingMessage, res: ServerResponse): void {
    res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-store', Connection: 'keep-alive' });
    res.write(': connected\n\n');
    this.clients.add(res);
    const heartbeat = setInterval(() => res.write(': heartbeat\n\n'), 15000);
    req.on('close', () => { clearInterval(heartbeat); this.clients.delete(res); });
  }

  async close(): Promise<void> {
    for (const { watcher } of this.watchers.values()) watcher.close();
    for (const client of this.clients) client.end();
    await Promise.all(Array.from(this.profiles.values(), profile => new Promise(resolve => profile.watching.close(resolve))));
  }
}

/**
 * Rspack frees a compilation once the next build starts, so what serving
 * needs is copied out when each build finishes: each entry's files, its
 * runtime files and stylesheet chunks, and the errors in its module graph.
 */
export function snapshot(compilation: Any, hash: string): Snapshot {
  const entrypoints: Snapshot['entrypoints'] = new Map();
  const errors = compilation.errors.map((error: Any) => ({ message: String(error.message || error), module: error.module ? error.module.identifier() : null }));
  for (const [slug, point] of compilation.entrypoints as Map<string, Any>) {
    const runtimeChunk = point.getRuntimeChunk();
    const runtimeFiles = new Set<string>(runtimeChunk ? Array.from(runtimeChunk.files as Iterable<string>) : []);
    const modules = new Set<string>();
    for (const chunk of point.chunks) for (const module of compilation.chunkGraph.getChunkModules(chunk)) modules.add(module.identifier());
    // Rspack also lists an entry's hot-update files; only hot updates load those.
    const files = (point.getFiles() as string[]).filter(name => !name.startsWith('hot/') && !name.includes('.hot-update.'));
    const cssChunks: Array<{ id: string | number | null; file: string }> = [];
    for (const chunk of point.chunks) for (const file of chunk.files) if (file.endsWith('.css')) cssChunks.push({ id: chunk.id, file });
    entrypoints.set(slug, { files, cssChunks, runtime: files.filter(name => runtimeFiles.has(name)),
      errors: errors.filter((error: Any) => !error.module || modules.has(error.module)).map((error: Any) => error.message) });
  }
  return { hash, entrypoints };
}
