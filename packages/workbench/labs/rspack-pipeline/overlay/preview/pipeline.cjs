/* Rspack pipeline lab: one Rspack development compiler per project and
   adapter profile, with shared chunks, HMR, React Refresh, the Vue loader and
   an HTML loader. Entries are virtual modules added the first time a preview
   or docs lens is requested (or pre-warmed); output lives in memory; the
   persistent cache lives in the system temporary folder. Builds are pushed to
   open frames over one event stream. Astro previews keep their Node render
   and enter the build as rendered HTML. */
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const Module = require('node:module');
const { createRequire } = Module;
const { validate, docsRequest } = require('./compiler.cjs');

const BASE = '/_workbench/previews/rspack/';
const MIME = { '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.woff2': 'font/woff2', '.woff': 'font/woff', '.map': 'application/json' };
const BUILTINS = { html: 'html.js', react: 'react.js', vue: 'vue.js', 'react-native-web': 'react.js', astro: 'astro.js' };
const digest = value => crypto.createHash('sha256').update(value).digest('hex').slice(0, 12);
const slash = file => file.split(path.sep).join('/');

// vue-loader requires Vue's SFC compiler from its own folder. Workbench ships
// @vue/compiler-sfc, as the esbuild path uses, so point vue-loader at it.
const workbenchSfc = require.resolve('@vue/compiler-sfc');
const resolveFilename = Module._resolveFilename;
Module._resolveFilename = function (request, parent, ...rest) {
  if ((request === 'vue/compiler-sfc' || request === '@vue/compiler-sfc') && parent && /[\\/]vue-loader[\\/]/.test(parent.filename || '')) return workbenchSfc;
  return resolveFilename.call(this, request, parent, ...rest);
};

class Pipeline {
  constructor(compiler, { tools, log = () => {} }) {
    this.compiler = compiler;
    this.root = compiler.root;
    this.toolRequire = createRequire(path.join(tools, 'package.json'));
    this.log = log;
    this.virtualDir = path.join(this.root, '.workbench-rspack');
    this.cacheDir = path.join(os.tmpdir(), 'workbench-rspack-cache', digest(this.root));
    const { createFsFromVolume, Volume } = this.toolRequire('memfs');
    this.memory = createFsFromVolume(new Volume());
    this.profiles = new Map();
    this.entries = new Map();   // slug -> entry
    this.byKey = new Map();     // file or docs key -> slug
    this.clients = new Set();
    this.astro = new Map();     // slug -> { target, watchers }
    this.watchers = new Map();  // watched file -> watcher
    // Each set of compilers (replaced when the config changes) writes to its
    // own folder, so a document from an older set never mixes in newer files.
    this.generation = 1;
    // Rspack's lazy compilation, as an option of the lab: every preview stays
    // an entry, and only entries a frame asks for are compiled.
    this.lazy = !!process.env.WORKBENCH_LAB_LAZY;
  }

  // ----------------------------------------------------------- profiles

  profileName(adapter) { return adapter === 'react-native-web' ? 'react-native-web' : 'web'; }

  profile(name) {
    let profile = this.profiles.get(name);
    if (profile) return profile;
    const { rspack } = this.toolRequire('@rspack/core');
    const { ReactRefreshRspackPlugin } = this.toolRequire('@rspack/plugin-react-refresh');
    const { VueLoaderPlugin } = this.toolRequire('vue-loader');
    const config = this.compiler.config;
    const native = name === 'react-native-web';
    const virtual = new rspack.experiments.VirtualModulesPlugin({ [this.hotClientFile(name)]: this.hotClientSource(name, this.generation) });
    const aliases = { '@canonic2/workbench': path.join(__dirname, 'api.js'), ...(native ? { 'react-native$': 'react-native-web' } : {}) };
    for (const [key, value] of Object.entries(config.aliases || {})) {
      aliases[key + '$'] = value.startsWith('.') || path.isAbsolute(value) ? path.resolve(this.root, value) : value;
    }
    // Deduplicated packages resolve from the project root, as esbuild's build.resolve does.
    const projectRequire = createRequire(path.join(this.root, 'package.json'));
    for (const name of config.dedupe || []) {
      try { aliases[name] = path.dirname(projectRequire.resolve(name + '/package.json')); } catch (_) { /* Unresolvable packages stay as written. */ }
    }
    const extensions = config.resolveExtensions || (native ? ['.web.tsx', '.web.ts', '.web.jsx', '.web.js'] : []).concat(['.tsx', '.ts', '.jsx', '.js', '.mjs', '.cjs', '.json', '.vue', '.html']);
    const rules = [
      { test: /\.[cm]?[jt]sx?$/, exclude: /node_modules/, loader: 'builtin:swc-loader', type: 'javascript/auto',
        options: { jsc: { parser: { syntax: 'typescript', tsx: true }, transform: { react: { runtime: 'automatic', development: true, refresh: true } }, target: 'es2020' } } },
      { test: /\.vue$/, loader: this.toolRequire.resolve('vue-loader'), options: { hotReload: true } },
      { test: /\.html?$/, loader: path.join(__dirname, 'html-loader.cjs') },
      { test: /\.css$/, oneOf: [
        // Vue Loader routes each style block through a JavaScript module, which
        // Rspack's native CSS type cannot take; these use the classic loaders.
        { resourceQuery: /vue/, use: [this.toolRequire.resolve('style-loader'), this.toolRequire.resolve('css-loader')], type: 'javascript/auto' },
        // Projects import CSS modules as a default object, as esbuild provides.
        { test: /\.module\.css$/, type: 'css/module', parser: { namedExports: false } },
        { type: 'css' },
      ] },
      { test: /\.(svg|png|jpe?g|gif|webp|avif|woff2?|ttf|mp4|mp3)$/, type: 'asset/resource' },
      ...((config.rspack && config.rspack.rules) || []),
    ];
    const addonPlugins = Object.values(config.adapters || {}).flatMap(addon => (addon && addon.rspack && addon.rspack.plugins) || []);
    // A selected project's Rspack plugins, supplied at run time from outside
    // the repository, in place of its config's esbuild plugins.
    const supplied = process.env.WORKBENCH_LAB_RSPACK_CONFIG ? require(process.env.WORKBENCH_LAB_RSPACK_CONFIG)({ root: this.root, rspack, profile: name }) : {};
    rules.push(...(supplied.rules || []));
    const compiler = rspack({
      name, mode: 'development', devtool: false, context: this.root, target: 'web',
      entry: () => {
        const entries = {};
        for (const entry of this.entries.values()) if (entry.profile === name) entries[entry.slug] = [this.hotClientFile(name), entry.file];
        // A compiler needs an entry to start; the hot client alone is harmless.
        if (!Object.keys(entries).length) entries.__workbench_idle = [this.hotClientFile(name)];
        return entries;
      },
      output: { path: '/out/' + name + '/' + this.generation, publicPath: BASE + name + '/' + this.generation + '/', filename: '[name].[contenthash:8].js', chunkFilename: 'chunks/[name].[contenthash:8].js',
        // Stylesheets keep stable names: Rspack's CSS hot update fetches a
        // chunk's stylesheet by the name its runtime already knows.
        cssFilename: '[name].css', cssChunkFilename: 'chunks/[name].css', assetModuleFilename: 'assets/[name].[hash:8][ext]',
        uniqueName: 'wb_' + name.replace(/-/g, '_'), hotUpdateChunkFilename: 'hot/[id].[fullhash].hot-update.js', hotUpdateMainFilename: 'hot/[runtime].[fullhash].hot-update.json' },
      externalsType: 'window',
      externals: { '/_workbench/preview-runtime.js': '__workbenchRuntime' },
      resolve: { alias: aliases, extensions, modules: [path.join(this.root, 'node_modules'), 'node_modules'] },
      module: { rules },
      plugins: [
        virtual,
        new rspack.DefinePlugin({ 'process.env.NODE_ENV': JSON.stringify('development'), __VUE_OPTIONS_API__: 'true', __VUE_PROD_DEVTOOLS__: 'false',
          __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: 'false', ...config.define }),
        new rspack.HotModuleReplacementPlugin(),
        new ReactRefreshRspackPlugin({ exclude: [/node_modules/, /\.workbench-rspack[\\/]/] }),
        new VueLoaderPlugin(),
        ...((config.rspack && config.rspack.plugins) || []),
        ...addonPlugins,
        ...(supplied.plugins || []),
      ],
      optimization: { splitChunks: { chunks: 'all' }, runtimeChunk: 'single', minimize: false, moduleIds: 'named' },
      // The default location is node_modules/.cache inside the project.
      cache: process.env.WORKBENCH_LAB_NO_PERSISTENT_CACHE ? true : { type: 'persistent', name, storage: { type: 'filesystem', directory: this.cacheDir } },
      infrastructureLogging: { level: 'error' }, stats: 'errors-only',
      watchOptions: { ignored: ['**/node_modules/**', '**/.git/**'], aggregateTimeout: 5 },
      ...(this.lazy ? { lazyCompilation: { entries: true, imports: false, client: this.lazyClient(), serverUrl: '/_workbench/previews', prefix: '/rspack/lazy' } } : {}),
    });
    const lazyMiddleware = this.lazy ? this.toolRequire('@rspack/core').lazyCompilationMiddleware(compiler) : null;
    compiler.outputFileSystem = this.memory;
    profile = { name, generation: this.generation, compiler, lazyMiddleware, virtual, compilation: null, hash: null, building: null, finish: null, ready: null };
    let resolveFirst, rejectFirst;
    profile.ready = new Promise((resolve, reject) => { resolveFirst = resolve; rejectFirst = reject; });
    compiler.hooks.invalid.tap('workbench', () => {
      if (!profile.building) profile.building = new Promise(resolve => { profile.finish = resolve; });
    });
    profile.building = new Promise(resolve => { profile.finish = resolve; });
    const started = Date.now();
    profile.watching = compiler.watch(compiler.options.watchOptions, (error, stats) => {
      if (error) { rejectFirst(error); this.log('pipeline.error', { profile: name, message: String(error.message || error) }); return; }
      // A compiler replaced by a config change may still finish a build.
      if (this.profiles.get(name) !== profile) return;
      const previous = profile.hash;
      profile.built = this.snapshot(stats.compilation, stats.hash);
      profile.hash = stats.hash;
      if (previous) { let present = false; try { present = this.memory.existsSync('/out/' + name + '/' + profile.generation + '/hot/runtime.' + previous + '.hot-update.json'); } catch (_) {} if (present && process.env.WORKBENCH_LAB_TRACE) { try { const body = JSON.parse(this.memory.readFileSync('/out/' + name + '/' + profile.generation + '/hot/runtime.' + previous + '.hot-update.json', 'utf8')); this.log('pipeline.manifest', { profile: name, hash: stats.hash, chunks: body.c, removedChunks: body.r, removedModules: body.m, modified: Array.from(compiler.modifiedFiles || []).map(file => path.basename(file)) }); } catch (_) {} }
        if (!present) this.log('pipeline.manifest.missing', { profile: name, previous, hash: stats.hash, modified: Array.from(compiler.modifiedFiles || []).slice(0, 5) }); }
      const errors = profile.built.errors;
      const finish = profile.finish;
      profile.building = null; profile.finish = null;
      if (finish) finish();
      resolveFirst();
      this.log('pipeline.built', { profile: name, hash: stats.hash, errors: errors.length, ms: Date.now() - started });
      // Errors go to the previews whose graph they are in, not to every page.
      const errorsBySlug = {};
      for (const [slug, point] of profile.built.entrypoints) if (point.errors.length) errorsBySlug[slug] = point.errors.slice(0, 3);
      this.broadcast({ type: 'built', profile: name, generation: profile.generation, hash: stats.hash, errors: errorsBySlug, time: Date.now() });
    });
    this.profiles.set(name, profile);
    return profile;
  }

  // Workbench's server forwards only GET requests to the worker, so the lab's
  // copy of Rspack's lazy-compilation client asks with GET.
  lazyClient() {
    const source = fs.readFileSync(this.toolRequire.resolve('@rspack/core/hot/lazy-compilation-web.js'), 'utf8')
      .replace("xhr.open('POST', urlBase, true);", "xhr.open('GET', urlBase + '?modules=' + encodeURIComponent(data), true);")
      .replace("xhr.setRequestHeader('Content-Type', 'text/plain');", '').replace('xhr.send(data);', 'xhr.send();');
    if (!source.includes("xhr.open('GET'")) throw new Error('The lazy-compilation client changed shape');
    const file = path.join(this.cacheDir, 'lazy-client.js');
    fs.mkdirSync(this.cacheDir, { recursive: true });
    fs.writeFileSync(file, source);
    return file;
  }

  // Hands a GET from the lab's client to Rspack's middleware as the POST it expects.
  async lazyRequest(search, res) {
    const modules = (new URLSearchParams(search).get('modules') || '').split('\n').filter(Boolean);
    const profile = Array.from(this.profiles.values()).find(item => item.lazyMiddleware);
    if (!profile) { res.statusCode = 404; res.end(); return; }
    await new Promise(resolve => { let done = false; const next = () => { if (!done) { done = true; res.statusCode = 404; res.end(); resolve(); } };
      res.once('finish', () => { done = true; resolve(); });
      Promise.all(Array.from(this.profiles.values()).filter(item => item.lazyMiddleware).map(item => item.lazyMiddleware({ method: 'POST', url: '/rspack/lazy', body: modules }, res, () => {}))).then(() => setTimeout(next, 50), next); });
  }

  async settled(profile) {
    await profile.ready;
    while (profile.building) await profile.building;
    return profile.built;
  }

  // Waits for a build that includes the entry: an invalidation lands after
  // the call that asked for it, and other entries may be building meanwhile.
  async contains(profile, slug) {
    for (let attempt = 0; attempt < 600; attempt++) {
      const built = await this.settled(profile);
      if (built && built.entrypoints.has(slug)) return built;
      await new Promise(resolve => setTimeout(resolve, 10));
    }
    throw new Error('Rspack did not build ' + slug);
  }

  hotClientFile(profile) { return path.join(this.virtualDir, 'hot-client-' + profile + '.js'); }

  // Runs once per document and profile runtime. Applies hot updates only in
  // the visible frame; a hidden retained frame marks itself stale and applies
  // the update when it is shown. An update nothing accepts reloads the preview
  // in its warm host, keeping state, size and inputs in its address.
  hotClientSource(profile, generation) {
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
  let stale = false, failed = false, latest = null, queue = Promise.resolve();
  const active = () => { try { return !window.frameElement || window.frameElement.classList.contains('is-active'); } catch (_) { return true; } };
  const reload = () => {
    if (window.__workbenchHost && window.wbPreviewHost) window.wbPreviewHost.load(location.href);
    else location.reload();
  };
  // Each build's update starts from the build before it, so a document
  // applies updates one at a time until it reaches the target build.
  const syncTo = target => (queue = queue.catch(() => {}).then(async () => {
    for (let step = 0; step < 50 && target && __webpack_hash__ !== target; step++) {
      for (let wait = 0; module.hot.status() !== 'idle'; wait++) {
        if (/^(abort|fail)$/.test(module.hot.status()) || wait > 500) throw new Error('Hot updates stopped: ' + module.hot.status());
        await new Promise(resolve => setTimeout(resolve, 10));
      }
      const updated = await module.hot.check(true);
      if (hub.__wbTrace) console.info('[workbench] trace step', location.pathname, 'now', __webpack_hash__, 'updated', updated && updated.join(',').slice(0, 160));
      if (!updated) break;
      if (updated.length) window.dispatchEvent(new CustomEvent('workbench:hot-updated', { detail: { modules: updated.length } }));
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
    // New compilers: this document's runtime cannot follow them.
    // A warm host becomes a fresh host, which the canvas fills again.
    if (data.type === 'reload-all') { if (window.__workbenchHost) location.replace('/_workbench/preview-host.html?actions=off'); else location.reload(); return; }
    // Builds from a newer set of compilers are for documents that set loads.
    if (data.type !== 'built' || data.profile !== profile || (data.generation && data.generation !== generation)) return;
    latest = data.hash;
    const own = window.__wbCurrent && data.errors && data.errors[window.__wbCurrent];
    if (own && own.length) {
      failed = true;
      if (active() && window.workbench && window.workbench.error) window.workbench.error(new Error(own.join('\\n\\n')));
      return;
    }
    if (hub.__wbTrace) console.info('[workbench] trace built', data.hash, 'doc', __webpack_hash__, 'active', active(), location.pathname);
    if (data.hash === __webpack_hash__) return;
    // Hidden retained frames apply updates too, so a page is current when it is
    // shown again. Holding updates until then lost edits after the compilers
    // were replaced (stage 1).
    update();
  };
  const onShown = () => { if (hub.__wbTrace) console.info('[workbench] trace shown', stale, active(), location.pathname); if (stale && active()) { stale = false; update(); } };
  source.addEventListener('message', onMessage);
  hub.addEventListener('wb-frame-change', onShown);
  window.addEventListener('pagehide', () => { source.removeEventListener('message', onMessage); hub.removeEventListener('wb-frame-change', onShown); });
}
`;
  }

  // ------------------------------------------------------------- entries

  // Generates the virtual entry for a preview definition: the same imports as
  // the esbuild entry, with Workbench's runtime external and sources read at
  // call time so a hot update re-renders with the new module.
  async previewEntry(relative) {
    await this.compiler.settings();
    const file = path.resolve(this.root, relative);
    const evaluated = await this.compiler.definition(file);
    const definition = validate(evaluated.value, file, this.root);
    const config = this.compiler.config;
    const addon = config.adapters && config.adapters[definition.adapter];
    const runtime = addon && addon.runtime ? path.resolve(this.root, addon.runtime) : BUILTINS[definition.adapter] && path.join(__dirname, BUILTINS[definition.adapter]);
    if (!runtime) throw new Error('Unknown adapter “' + definition.adapter + '”. Register it in workbench.config.ts.');
    const slug = definition.id.replace(/\//g, '--') + '-' + digest(definition.id);
    const lines = ['import definition from ' + JSON.stringify(file) + ';'];
    if (definition.adapter === 'react-native-web' && !(addon && addon.runtime)) {
      lines.push('import ' + JSON.stringify(path.join(__dirname, 'react-native-web.css')) + ';', 'import { mountNative } from ' + JSON.stringify(runtime) + ';', 'const adapter = { mount: mountNative };');
    } else lines.push('import * as adapter from ' + JSON.stringify(runtime) + ';');
    lines.push("import { boot, combine } from '/_workbench/preview-runtime.js';");
    const accepted = [];
    const sources = [];
    const astro = definition.adapter === 'astro' && !addon;
    if (astro) await this.renderAstro(slug, file, definition);
    for (const [state, spec] of Object.entries({ default: { source: definition.source }, ...definition.states })) {
      const source = spec.source || definition.source;
      const name = 'source' + sources.length;
      if (astro) {
        const module = path.join(this.virtualDir, 'astro', slug + '-' + state + '.js');
        lines.push('import * as ' + name + ' from ' + JSON.stringify(module) + ';');
        accepted.push(module);
        sources.push(JSON.stringify(state) + ': { html: ' + name + '.html, inputs: ' + name + '.inputs, renderUrl: ' +
          JSON.stringify('/_workbench/previews/render?file=' + encodeURIComponent(slash(path.relative(this.root, file)))) + ' }');
      } else {
        const target = path.resolve(path.dirname(file), source.entry);
        lines.push('import * as ' + name + ' from ' + JSON.stringify(target) + ';');
        accepted.push(target);
        sources.push(JSON.stringify(state) + ': ' + name + '[' + JSON.stringify(source.export || 'default') + ']');
      }
    }
    const environments = this.environments(definition.adapter, lines, definition.environment && path.resolve(path.dirname(file), definition.environment));
    for (const style of definition.styles || []) lines.push('import ' + JSON.stringify(path.resolve(path.dirname(file), style)) + ';');
    lines.push('const sources = () => ({' + sources.join(', ') + '});',
      'const environment = ' + (environments.length === 2 ? 'combine(' + environments.join(', ') + ')' : environments[0] || '{}') + ';',
      'function mount(options) { return boot(definition, sources(), adapter, environment, options); }',
      '(window.__wbRspack = window.__wbRspack || {})[' + JSON.stringify(slug) + '] = { mount, moduleId: module.id };',
      // Source modules that refresh themselves (React components) stop the
      // update there. Anything else bubbles here and re-renders in place.
      'if (module.hot) module.hot.accept(' + JSON.stringify(accepted) + ', () => { if (window.__workbenchRerender) window.__workbenchRerender(sources()); });');
    return { slug, kind: 'preview', key: slash(path.relative(this.root, file)), profile: this.profileName(definition.adapter), title: definition.title || definition.id,
      file: path.join(this.virtualDir, slug + '.js'), source: lines.join('\n') + '\n' };
  }

  environments(adapter, lines, own) {
    const config = this.compiler.config;
    const environments = [];
    const project = typeof config.environment === 'string' ? config.environment
      : config.environment && typeof config.environment === 'object' ? config.environment[adapter] : undefined;
    if (project) {
      lines.push('import * as projectEnvironment from ' + JSON.stringify(path.resolve(this.root, project)) + ';');
      environments.push('projectEnvironment');
    }
    if (own) {
      lines.push('import * as ownEnvironment from ' + JSON.stringify(own) + ';');
      environments.push('ownEnvironment');
    }
    return environments;
  }

  async docsEntry(raw) {
    await this.compiler.settings();
    const request = docsRequest(raw, this.root);
    const config = this.compiler.config;
    const addon = config.adapters && config.adapters[request.adapter];
    if (request.adapter === 'astro' && !addon) throw new Error('Astro docs lenses are not part of this lab prototype.');
    const runtime = addon && addon.runtime ? path.resolve(this.root, addon.runtime) : BUILTINS[request.adapter] && path.join(__dirname, BUILTINS[request.adapter]);
    if (!runtime) throw new Error('Unknown adapter “' + request.adapter + '”. Register it in workbench.config.ts.');
    const listed = await this.compiler.docsExamples(request);
    const slug = 'docs-' + digest(JSON.stringify(raw));
    const lines = [];
    if (request.adapter === 'react-native-web' && !(addon && addon.runtime)) {
      lines.push('import ' + JSON.stringify(path.join(__dirname, 'react-native-web.css')) + ';', 'import { mountNative } from ' + JSON.stringify(runtime) + ';', 'const adapter = { mount: mountNative };');
    } else lines.push('import * as adapter from ' + JSON.stringify(runtime) + ';');
    lines.push("import { combine } from '/_workbench/preview-runtime.js';");
    const environments = this.environments(request.adapter, lines, request.environment);
    for (const style of request.styles) lines.push('import ' + JSON.stringify(style) + ';');
    const entries = [];
    if (request.folder) {
      listed.examples.forEach((example, index) => {
        lines.push('import * as example' + index + ' from ' + JSON.stringify(example.file) + ';');
        entries.push(JSON.stringify(example.id) + ': example' + index + '.default');
      });
    } else {
      lines.push('import * as source from ' + JSON.stringify(request.examples) + ';');
      for (const example of listed.examples) entries.push(JSON.stringify(example.id) + ': source[' + JSON.stringify(example.export) + ']');
    }
    lines.push('const environment = ' + (environments.length === 2 ? 'combine(' + environments.join(', ') + ')' : environments[0] || '{}') + ';',
      '(window.__wbRspack = window.__wbRspack || {})[' + JSON.stringify(slug) + '] = { adapter, environment, examples: {' + entries.join(', ') + '}, moduleId: module.id };');
    return { slug, kind: 'docs', key: 'docs:' + JSON.stringify(raw), profile: this.profileName(request.adapter), title: slug,
      file: path.join(this.virtualDir, slug + '.js'), source: lines.join('\n') + '\n',
      examples: listed.examples.map(example => ({ id: example.id, file: slash(path.relative(this.root, example.file)), export: example.export })), problems: listed.problems };
  }

  // Adds or refreshes an entry and waits for a build that includes it.
  async ensure(prepared) {
    const existing = this.entries.get(prepared.slug);
    const profile = this.profile(prepared.profile);
    // Rspack creates its virtual file store during the first build.
    await profile.ready;
    if (!existing || existing.source !== prepared.source) {
      this.entries.set(prepared.slug, prepared);
      this.byKey.set(prepared.key, prepared.slug);
      profile.virtual.writeModule(prepared.file, prepared.source);
      if (!existing) profile.watching.invalidate();
    }
    await this.contains(profile, prepared.slug);
    return this.entries.get(prepared.slug);
  }

  // Requests wait for pre-warming, so they find the full entry set.
  // Without pre-warming (lab control D2), the first request starts the same build.
  async preview(relative) { if (!this.warming) this.warming = this.prewarm(); await this.warming; return this.ensure(await this.previewEntry(relative)); }
  async docs(raw) { if (!this.warming) this.warming = this.prewarm(); await this.warming; return this.ensure(await this.docsEntry(raw)); }

  // ---------------------------------------------------------------- Astro

  // Astro frontmatter renders in Node with Workbench's own renderer. Each
  // state's HTML becomes a virtual module; an edit to anything the render read
  // renders again and rewrites those modules, which Rspack hot-updates.
  async renderAstro(slug, file, definition) {
    const config = this.compiler.config;
    const { mockFetches } = require('./compiler.cjs');
    const renderers = new Map();
    const dependencies = new Set();
    for (const [state, spec] of Object.entries({ default: { source: definition.source }, ...definition.states })) {
      const source = spec.source || definition.source;
      const target = path.resolve(path.dirname(file), source.entry);
      let renderer = renderers.get(target);
      if (!renderer) {
        renderer = await require('./astro.cjs').create(this.root, target, { ...config,
          define: { 'import.meta.env.DEV': 'true', 'import.meta.env.PROD': 'false', ...config.define } });
        renderers.set(target, renderer);
        for (const dependency of renderer.dependencies) if (!/[\\/]node_modules[\\/]/.test(dependency) && fs.existsSync(dependency)) dependencies.add(dependency);
      }
      const inputs = { ...definition.inputs, ...spec.inputs };
      const html = mockFetches ? await mockFetches(definition, state, inputs, () => renderer.render(inputs, source.export)) : await renderer.render(inputs, source.export);
      await this.writeAstro(slug, state, html, renderer.resources, inputs, renderer.publicDir);
    }
    if (!this.astro.has(slug)) { this.astro.set(slug, true); this.watch(dependencies, () => this.refreshAstro(slug, file)); }
  }

  async writeAstro(slug, state, html, resources, inputs, publicDir) {
    const profile = this.profile('web');
    await profile.ready;
    for (const [id, resource] of resources) {
      const extension = id.endsWith('.css') ? '.css' : '.ts';
      const virtualFile = path.join(this.virtualDir, 'astro', 'resources', digest(id) + extension);
      this.writeVirtual(profile, virtualFile, resource.contents);
      html = html.split(id).join('wbfile:' + virtualFile);
    }
    // Root-relative URLs in an Astro page name files in its public folder.
    if (publicDir) html = html.replace(/\b(src|href|poster)=(["'])(\/[^"'?#]+)/g, (match, attribute, quote, url) =>
      fs.existsSync(path.join(publicDir, url.slice(1))) ? attribute + '=' + quote + 'wbfile:' + path.join(publicDir, url.slice(1)) : match);
    const htmlFile = path.join(this.virtualDir, 'astro', slug + '-' + state + '.html');
    this.writeVirtual(profile, htmlFile, html);
    this.writeVirtual(profile, path.join(this.virtualDir, 'astro', slug + '-' + state + '.js'),
      'export { default as html } from ' + JSON.stringify(htmlFile) + ';\nexport const inputs = ' + JSON.stringify(inputs) + ';\n');
  }

  // A virtual module rewritten with the same content still triggers a build.
  writeVirtual(profile, file, contents) {
    profile.written = profile.written || new Map();
    if (profile.written.get(file) === contents) return;
    profile.written.set(file, contents);
    profile.virtual.writeModule(file, contents);
  }

  async refreshAstro(slug, file) {
    try {
      const evaluated = await this.compiler.definition(file);
      await this.renderAstro(slug, file, validate(evaluated.value, file, this.root));
      this.log('pipeline.astro.rendered', { slug });
    } catch (error) {
      this.broadcast({ type: 'built', profile: 'web', hash: 'astro-error-' + Date.now(), errors: { [slug]: [String(error.message || error)] } });
    }
  }

  watch(files, onChange) {
    for (const file of files) {
      if (this.watchers.has(file)) { this.watchers.get(file).handlers.add(onChange); continue; }
      let timer;
      const handlers = new Set([onChange]);
      const watcher = fs.watch(file, () => { clearTimeout(timer); timer = setTimeout(() => handlers.forEach(handler => handler()), 20); });
      this.watchers.set(file, { watcher, handlers });
    }
  }

  // Docs Markdown is rendered by the server, not built. Pushing its changes
  // lets an open docs page check its revision once instead of polling.
  async watchDocs() {
    if (this.docsWatched) return;
    this.docsWatched = true;
    const index = await this.compiler.index();
    const files = new Set(index.docs.map(page => path.resolve(this.root, page.src)));
    this.watch(files, () => this.broadcast({ type: 'docs-changed', time: Date.now() }));
  }

  // ------------------------------------------------------------ serving

  // Rspack frees a compilation once the next build starts, so what serving
  // needs is copied out when each build finishes.
  snapshot(compilation, hash) {
    const entrypoints = new Map();
    const errors = compilation.errors.map(error => ({ message: String(error.message || error), module: error.module ? error.module.identifier() : null }));
    for (const [slug, point] of compilation.entrypoints) {
      const runtimeChunk = point.getRuntimeChunk();
      const runtimeFiles = new Set(runtimeChunk ? Array.from(runtimeChunk.files) : []);
      const modules = new Set();
      for (const chunk of point.chunks) for (const module of compilation.chunkGraph.getChunkModules(chunk)) modules.add(module.identifier());
      // Rspack also lists an entry's hot-update files; only HMR loads those.
      const files = point.getFiles().filter(name => !name.startsWith('hot/') && !name.includes('.hot-update.'));
      const cssChunks = [];
      for (const chunk of point.chunks) for (const file of chunk.files) if (file.endsWith('.css')) cssChunks.push({ id: chunk.id, file });
      entrypoints.set(slug, { files, cssChunks, runtime: files.filter(name => runtimeFiles.has(name)),
        errors: errors.filter(error => !error.module || modules.has(error.module)).map(error => error.message) });
    }
    return { hash, entrypoints, errors: errors.map(error => error.message) };
  }

  entryAssets(entry) {
    const profile = this.profiles.get(entry.profile);
    const point = profile.built && profile.built.entrypoints.get(entry.slug);
    if (!point) return { hash: null, runtime: [], scripts: [], styles: [], errors: ['Rspack has not built ' + entry.slug] };
    const url = name => BASE + entry.profile + '/' + profile.generation + '/' + name;
    return { hash: profile.built.hash, runtime: point.runtime.map(url), scripts: point.files.filter(name => name.endsWith('.js')).map(url),
      styles: point.cssChunks.map(chunk => ({ href: url(chunk.file), id: chunk.id })), uniqueName: 'wb_' + entry.profile.replace(/-/g, '_'), errors: point.errors };
  }

  // The module a warm host imports: Workbench's runtime first (request mocks
  // before project code), then the entry's chunks as classic scripts. A
  // document that already ran this profile's runtime keeps its dependency
  // modules; the previous preview's project modules are dropped first.
  shim(entry) {
    const assets = this.entryAssets(entry);
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
  const link = document.createElement('link'); link.rel = 'stylesheet'; link.href = style.href;
  link.setAttribute('data-rspack', ${JSON.stringify(assets.uniqueName)} + ':' + style.id);
  await new Promise(resolve => { link.onload = link.onerror = resolve; document.head.append(link); });
}
// A document that already runs this profile's runtime first catches up with
// the build these chunks come from, then keeps its dependency modules
// (React, libraries) but evaluates every project module again.
const profileState = window.__wbProfiles && window.__wbProfiles[${JSON.stringify(entry.profile)}];
const runtimeFiles = new Set(${JSON.stringify(assets.runtime)});
if (profileState && profileState.require) {
  try { if (profileState.sync) await profileState.sync(${JSON.stringify(assets.hash)}); }
  catch (error) { if (window.__workbenchHost) location.replace('/_workbench/preview-host.html?actions=off'); else location.reload(); await new Promise(() => {}); }
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
// A lazily compiled entry registers after Rspack builds it and the hot update lands.
for (let waited = 0; !(window.__wbRspack && window.__wbRspack[slug] && window.__wbRspack[slug] !== before) && waited < 60000 && ${JSON.stringify(this.lazy)}; waited += 20) await new Promise(resolve => setTimeout(resolve, 20));
const entry = window.__wbRspack && window.__wbRspack[slug];
if (!entry) throw new Error('The preview module did not register: ' + slug);
window.__wbCurrent = slug;
${exports}`;
  }

  // Serves output, shims and the event stream. Returns false for other routes.
  async serve(pathname, search, res) {
    if (!pathname.startsWith('/rspack/')) return false;
    const rest = decodeURIComponent(pathname.slice('/rspack/'.length));
    const shim = /^shim\/([^/]+)\.js$/.exec(rest);
    if (shim) {
      const entry = this.entries.get(shim[1]);
      if (!entry) { res.statusCode = 404; res.end('Unknown entry'); return true; }
      await this.contains(this.profiles.get(entry.profile), entry.slug);
      res.setHeader('Content-Type', 'text/javascript'); res.setHeader('Cache-Control', 'no-store');
      res.end(this.shim(entry));
      return true;
    }
    const [name, ...parts] = rest.split('/');
    const profile = this.profiles.get(name);
    if (!profile) { res.statusCode = 404; res.end('Unknown profile'); return true; }
    if (!/(^|\/)hot\//.test(parts.join('/'))) await this.settled(profile);
    const file = '/out/' + name + '/' + parts.join('/');
    let body;
    try { body = this.memory.readFileSync(file); } catch (_) { res.statusCode = 404; res.end('Not found'); return true; }
    res.setHeader('Content-Type', MIME[path.extname(file)] || 'application/octet-stream');
    res.setHeader('Cache-Control', /(^|\/)hot\//.test(parts.join('/')) || file.endsWith('.css') ? 'no-cache' : 'public, max-age=31536000, immutable');
    res.end(body);
    return true;
  }

  options() {
    return { development: true, revision: 'rspack', push: true, ...(process.env.WORKBENCH_LAB_READY_ON_MOUNT ? { labReadyOnMount: true } : {}) };
  }

  descriptor(entry) {
    return { base: BASE + entry.profile + '/', module: BASE + 'shim/' + entry.slug + '.js?load=' + Date.now(), stylesheet: null, title: entry.title,
      options: this.options() };
  }

  // A standalone document for a preview (screenshots, "open on its own").
  page(entry) {
    return '<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">' +
      '<title>' + String(entry.title).replace(/[&<>"']/g, char => '&#' + char.charCodeAt(0) + ';') + '</title>' +
      '<style>html,body{margin:0;min-height:100%}#workbench-preview{width:100%;min-width:0;box-sizing:border-box;min-height:100vh}</style></head><body>' +
      '<script>window.__workbenchOptions=' + JSON.stringify(this.options()) + '</script>' +
      '<script type="module">import { mount } from ' + JSON.stringify(BASE + 'shim/' + entry.slug + '.js') + '; mount(window.__workbenchOptions);</script></body></html>';
  }

  broadcast(message) {
    const data = 'data: ' + JSON.stringify(message) + '\n\n';
    for (const client of this.clients) client.write(data);
  }

  events(req, res) {
    res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-store', Connection: 'keep-alive' });
    res.write(': connected\n\n');
    this.clients.add(res);
    const heartbeat = setInterval(() => res.write(': heartbeat\n\n'), 15000);
    req.on('close', () => { clearInterval(heartbeat); this.clients.delete(res); });
  }

  // Starts the web compiler and builds the first discovered preview before
  // anything asks for it. Requests never wait for this beyond the build they
  // need themselves.
  // workbench.config.ts sets aliases, defines, plugins and environments that
  // the compilers were created with: a change replaces them, and every open
  // preview reloads into the new ones.
  watchConfig() {
    if (this.configWatched) return;
    this.configWatched = true;
    let reloading = Promise.resolve();
    this.watch(new Set(this.compiler.configFiles.keys()), () => { reloading = reloading.then(() => this.reloadConfig()); });
  }

  // One at a time: two quick saves must not leave an old compiler running.
  async reloadConfig() {
    {
      try {
        await this.compiler.settings();
        const profiles = Array.from(this.profiles.values());
        this.profiles.clear();
        this.generation++;
        this.entries.clear();
        this.byKey.clear();
        await Promise.all(profiles.map(profile => new Promise(resolve => profile.watching.close(resolve))));
        this.warming = this.prewarm();
        await this.warming;
        this.broadcast({ type: 'reload-all', time: Date.now() });
        this.log('pipeline.config.reloaded', {});
      } catch (error) { this.log('pipeline.config.failed', { message: String(error.message || error) }); }
    }
  }

  // Declares every discovered preview and docs lens as an entry before the
  // first build, then builds them. Rspack's hot updates do not follow a module
  // that moves to another chunk, and adding an entry while pages are open can
  // move shared modules (stage 1), so the entry set stays fixed while the
  // project's previews do; a preview added later is the exception.
  async prewarm() {
    const started = Date.now();
    try {
      const index = await this.compiler.index();
      this.watchConfig();
      await this.watchDocs();
      const prepared = [];
      for (const preview of index.previews) {
        try { prepared.push(await this.previewEntry(preview.file)); } catch (error) { this.log('pipeline.entry.skipped', { file: preview.file, message: String(error.message || error).slice(0, 200) }); }
      }
      for (const page of index.docs) for (const lens of page.lenses) {
        const raw = { adapter: lens.adapter, examples: lens.examples, styles: lens.styles, ...(lens.environment ? { environment: lens.environment } : {}) };
        try { prepared.push(await this.docsEntry(raw)); } catch (error) { this.log('pipeline.entry.skipped', { docs: page.id, message: String(error.message || error).slice(0, 200) }); }
      }
      const byProfile = new Map();
      for (const entry of prepared) { if (!byProfile.has(entry.profile)) byProfile.set(entry.profile, []); byProfile.get(entry.profile).push(entry); }
      await Promise.all(Array.from(byProfile, async ([name, entries]) => {
        const profile = this.profile(name);
        await profile.ready;
        for (const entry of entries) { this.entries.set(entry.slug, entry); this.byKey.set(entry.key, entry.slug); profile.virtual.writeModule(entry.file, entry.source); }
        profile.watching.invalidate();
        await this.contains(profile, entries[entries.length - 1].slug);
      }));
      this.prewarmed = { ms: Date.now() - started, entries: prepared.length };
      this.log('pipeline.prewarmed', this.prewarmed);
    } catch (error) { this.log('pipeline.prewarm.failed', { message: String(error.message || error) }); }
  }

  close() {
    for (const { watcher } of this.watchers.values()) watcher.close();
    for (const client of this.clients) client.end();
    return Promise.all(Array.from(this.profiles.values(), profile => new Promise(resolve => profile.watching.close(resolve))));
  }
}

module.exports = { Pipeline, BASE };
