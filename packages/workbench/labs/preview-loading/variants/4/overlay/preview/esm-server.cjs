/* Lab variant 4: unbundled development serving with hot module replacement
   and React Fast Refresh. Project modules are transformed one at a time and
   served as native ES modules with versioned URLs; npm dependencies are
   bundled once per project into shared chunks. A file watcher pushes updates
   over server-sent events; modules that only export React components accept
   their own updates, and anything else re-imports the nearest accepting
   modules or reloads the preview. */
const fs = require('node:fs');
const path = require('node:path');
const { createRequire } = require('node:module');
const { pathToFileURL } = require('node:url');
const esbuild = require('./engine.cjs');

const BASE = '/_workbench/previews/esm/';
const ASSETS = new Set(['.svg', '.png', '.jpg', '.jpeg', '.gif', '.webp', '.avif', '.woff', '.woff2', '.ttf', '.mp4', '.mp3']);
const SOURCE = /\.(?:[cm]?[jt]sx?)$/;
const MIME = { '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.woff2': 'font/woff2', '.woff': 'font/woff', '.ttf': 'font/ttf' };
const slash = file => file.split(path.sep).join('/');

// es-module-lexer 3 reports { type, specifier, start, end }; a dynamic
// import's range includes its quotes. Normalized to the specifier's own range.
function importsOf(lexer, code) {
  const [imports, exports] = lexer.parse(code);
  return {
    imports: imports.filter(item => item.type !== 'import-meta' && typeof item.specifier === 'string')
      .map(item => item.type === 'dynamic' ? { n: item.specifier, s: item.start + 1, e: item.end - 1 } : { n: item.specifier, s: item.start, e: item.end }),
    exports: exports.map(item => item.name),
  };
}

class EsmServer {
  constructor(compiler, project) {
    const tools = process.env.WORKBENCH_LAB_TOOLS;
    if (!tools) throw new Error('Variant 4 needs WORKBENCH_LAB_TOOLS');
    this.toolRequire = createRequire(path.join(tools, 'package.json'));
    this.oxc = this.toolRequire('oxc-transform');
    this.lexer = this.toolRequire('es-module-lexer');
    this.compiler = compiler;
    this.project = project;
    this.root = compiler.root;
    this.versions = new Map();
    this.importers = new Map();
    this.boundaries = new Set();
    this.transforms = new Map();
    this.entryFiles = new Map();
    this.entryVersion = new Map();
    this.clients = new Set();
    this.deps = null;
    this.watching = false;
  }

  version(file) { return this.versions.get(file) || 0; }
  bump(file) { this.versions.set(file, this.version(file) + 1); this.transforms.delete(file); }

  urlFor(file) {
    const relative = slash(path.relative(this.root, file));
    if (relative.startsWith('..')) return BASE + 'wb/' + path.basename(file) + '?v=' + this.version(file);
    return BASE + 'm/' + relative + '?v=' + this.version(file);
  }
  fileFor(kind, rest) {
    if (kind === 'wb') return path.join(__dirname, path.basename(rest));
    const file = path.resolve(this.root, decodeURIComponent(rest));
    if (path.relative(this.root, file).startsWith('..')) throw new Error('Outside the project: ' + rest);
    return file;
  }

  resolveFile(from, specifier) {
    const base = path.resolve(path.dirname(from), specifier);
    const extensions = this.compiler.config.resolveExtensions || ['.tsx', '.ts', '.jsx', '.js', '.mjs', '.cjs', '.json'];
    for (const candidate of [base, ...extensions.map(extension => base + extension), ...extensions.map(extension => path.join(base, 'index' + extension))]) {
      if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) return candidate;
    }
    throw new Error(`Cannot resolve ${specifier} from ${from}`);
  }

  // A bare specifier that names a file rather than a JavaScript package
  // entry: CSS, assets and JSON are served as project modules.
  bareFile(from, specifier) {
    if (!/\.(?:css|json|svg|png|jpe?g|gif|webp|avif|woff2?|ttf)$/.test(specifier)) return null;
    return createRequire(from).resolve(specifier);
  }

  depName(specifier) { return specifier.replace(/[^a-zA-Z0-9]/g, '_'); }

  async transform(file) {
    const cached = this.transforms.get(file);
    if (cached) return cached;
    const extension = path.extname(file);
    let code;
    if (extension === '.css') code = this.cssModule(file);
    else if (ASSETS.has(extension)) code = `export default ${JSON.stringify(BASE + 'raw/' + slash(path.relative(this.root, file)))};\n`;
    else if (extension === '.json') code = 'export default ' + fs.readFileSync(file, 'utf8') + ';\n';
    else code = await this.sourceModule(file);
    this.transforms.set(file, code);
    return code;
  }

  cssModule(file) {
    const css = fs.readFileSync(file, 'utf8').replace(/url\((['"]?)([^'")]+)\1\)/g, (match, quote, reference) => {
      if (/^(?:data:|https?:|#|\/)/.test(reference)) return match;
      const target = path.resolve(path.dirname(file), reference.split(/[?#]/)[0]);
      return `url(${BASE}raw/${slash(path.relative(this.root, target))})`;
    });
    this.boundaries.add(file);
    const id = JSON.stringify(slash(path.relative(this.root, file)));
    return `import { __labHot } from '${BASE}client.js';\nimport.meta.hot = __labHot(import.meta.url);\n` +
      `let style = document.querySelector('style[data-lab-css=' + JSON.stringify(${id}) + ']');\n` +
      `if (!style) { style = document.createElement('style'); style.dataset.labCss = ${id}; document.head.append(style); }\n` +
      `style.textContent = ${JSON.stringify(css)};\nimport.meta.hot.accept();\n`;
  }

  async sourceModule(file) {
    const project = !slash(path.relative(this.root, file)).split('/').includes('node_modules') && !path.relative(this.root, file).startsWith('..');
    const source = fs.readFileSync(file, 'utf8');
    const transform = this.oxc.transformSync || this.oxc.transform;
    const result = transform(file, source, {
      sourceType: 'module', target: 'es2020',
      typescript: { onlyRemoveTypeImports: false },
      jsx: { runtime: 'automatic', development: true, refresh: project && /\.[jt]sx$/.test(file) },
    });
    if (result.errors && result.errors.length) throw new Error(result.errors.map(error => error.message || String(error)).join('\n'));
    let code = result.code;
    for (const [key, value] of Object.entries({ 'process.env.NODE_ENV': '"development"', ...(this.compiler.config.define || {}) })) code = code.split(key).join(value);
    await this.lexer.init;
    const { imports, exports } = importsOf(this.lexer, code);
    let output = '';
    let last = 0;
    for (const item of imports) {
      const replacement = this.rewrite(file, item.n);
      if (replacement === item.n) continue;
      output += code.slice(last, item.s) + replacement;
      last = item.e;
    }
    code = output + code.slice(last);
    if (code.includes('$RefreshReg$') || code.includes('$RefreshSig$')) {
      const id = JSON.stringify(slash(path.relative(this.root, file)));
      const names = exports;
      const componentsOnly = names.length > 0 && names.every(name => /^[A-Z]/.test(name));
      code = `import { __labRefresh, __labHot } from '${BASE}client.js';\nimport.meta.hot = __labHot(import.meta.url);\n` +
        `const $RefreshReg$ = (type, id) => __labRefresh.register(type, ${id} + ' ' + id);\nconst $RefreshSig$ = __labRefresh.sig;\n` + code +
        (componentsOnly ? '\nimport.meta.hot.accept();\n__labRefresh.schedule();\n' : '');
      if (componentsOnly) this.boundaries.add(file); else this.boundaries.delete(file);
    }
    return code;
  }

  rewrite(from, specifier) {
    if (specifier.startsWith('/_workbench/')) return specifier;
    if (specifier === '@canonic2/workbench') return BASE + 'wb/api.js';
    const aliases = this.compiler.config.aliases || {};
    if (Object.prototype.hasOwnProperty.call(aliases, specifier)) specifier = aliases[specifier].startsWith('.') ? path.resolve(this.root, aliases[specifier]) : aliases[specifier];
    let target;
    if (specifier.startsWith('.') || path.isAbsolute(specifier)) target = this.resolveFile(from, specifier);
    else target = this.bareFile(from, specifier);
    if (target) {
      if (!this.importers.has(target)) this.importers.set(target, new Set());
      this.importers.get(target).add(from);
      return this.urlFor(target);
    }
    if (this.deps && !this.deps.names.has(specifier)) this.deps.stale = true;
    return BASE + 'deps/' + this.depName(specifier) + '.js';
  }

  // Bare JavaScript specifiers reachable from every entry, bundled once into
  // shared chunks so the page has one copy of each package (one React).
  // Concurrent requests share one dependency build.
  ensureDeps() {
    if (this.deps && !this.deps.stale) return Promise.resolve(this.deps);
    if (!this.depsBuilding) this.depsBuilding = this.buildDeps().finally(() => { this.depsBuilding = null; });
    return this.depsBuilding;
  }

  // Pre-bundled dependencies persist across worker restarts, keyed by the
  // project's manifest and lockfile, as dev servers keep them.
  depsKey() {
    const read = name => { try { return fs.readFileSync(path.join(this.root, name), 'utf8'); } catch (_) { return ''; } };
    return 'esm-deps:' + require('node:crypto').createHash('sha256')
      .update(read('package.json') + read('package-lock.json') + read('pnpm-lock.yaml') + read('yarn.lock') + JSON.stringify(this.compiler.config.define || {}))
      .digest('hex').slice(0, 16);
  }

  async buildDeps() {
    if (!this.deps) {
      const stored = this.compiler.diskCache.read(this.depsKey());
      if (stored && stored.outputs instanceof Map && stored.names instanceof Map) { this.deps = { ...stored, stale: false }; return this.deps; }
    }
    const built = await this.bundleDeps();
    this.compiler.diskCache.write(this.depsKey(), { outputs: built.outputs, names: built.names });
    return built;
  }

  async bundleDeps() {
    const specifiers = new Set(['react', 'react-dom/client', 'react/jsx-dev-runtime']);
    const seen = new Set();
    const queue = Array.from(this.entryFiles.values()).flatMap(entry => entry.files);
    while (queue.length) {
      const file = queue.pop();
      if (seen.has(file) || !SOURCE.test(file)) continue;
      seen.add(file);
      const source = fs.readFileSync(file, 'utf8');
      const transform = this.oxc.transformSync || this.oxc.transform;
      const { code } = transform(file, source, { sourceType: 'module', jsx: { runtime: 'automatic', development: true } });
      await this.lexer.init;
      for (const item of importsOf(this.lexer, code).imports) {
        const specifier = item.n;
        if (!specifier || specifier.startsWith('/_workbench/') || specifier === '@canonic2/workbench') continue;
        if (specifier.startsWith('.') || path.isAbsolute(specifier)) { try { queue.push(this.resolveFile(file, specifier)); } catch (_) {} continue; }
        if (this.bareFile(file, specifier)) continue;
        specifiers.add(specifier);
      }
    }
    if (this.deps) for (const name of this.deps.names.keys()) specifiers.add(name);
    const projectRequire = createRequire(path.join(this.root, 'package.json'));
    const define = { 'process.env.NODE_ENV': '"development"', ...(this.compiler.config.define || {}) };
    // First pass: what each package exports when esbuild resolves it for
    // the browser. ES modules list their names; CommonJS shows only default.
    const probe = await esbuild.build({
      absWorkingDir: this.root, bundle: true, write: false, metafile: true, format: 'esm', platform: 'browser', logLevel: 'silent', define,
      entryPoints: Object.fromEntries(Array.from(specifiers, specifier => [this.depName(specifier), specifier])),
      outdir: path.join(this.root, '__workbench_deps_probe__'), entryNames: '[name]', splitting: true, chunkNames: 'chunk-[hash]',
    });
    const exported = new Map();
    for (const output of Object.values(probe.metafile.outputs)) if (output.entryPoint) exported.set(output.entryPoint, output.exports || []);
    const wrappers = new Map();
    for (const specifier of specifiers) {
      const name = this.depName(specifier);
      const entryKey = Object.keys(probe.metafile.outputs).find(key => path.basename(key, '.js') === name);
      const names = entryKey ? probe.metafile.outputs[entryKey].exports || [] : [];
      let named = names.filter(item => item !== 'default');
      if (!named.length && names.includes('default')) {
        // CommonJS: named exports as Node reads them.
        try { named = Object.keys(await import(pathToFileURL(projectRequire.resolve(specifier)).href)).filter(item => item !== 'default' && item !== '__esModule' && /^[A-Za-z_$][\w$]*$/.test(item)); } catch (_) {}
      }
      wrappers.set(name, (named.length ? `export { ${named.join(', ')} } from ${JSON.stringify(specifier)};\n` : '') +
        (names.includes('default') ? `export { default } from ${JSON.stringify(specifier)};\n` : ''));
    }
    const root = this.root;
    const virtual = { name: 'lab-dep-entries', setup(build) {
      build.onResolve({ filter: /^lab-dep:/ }, args => ({ path: args.path.slice(8), namespace: 'lab-dep' }));
      build.onLoad({ filter: /.*/, namespace: 'lab-dep' }, args => ({ contents: wrappers.get(args.path), loader: 'js', resolveDir: root }));
    } };
    const result = await esbuild.build({
      absWorkingDir: this.root, bundle: true, write: false, splitting: true, format: 'esm', platform: 'browser', target: 'es2020',
      entryPoints: Object.fromEntries(Array.from(wrappers.keys(), name => [name, 'lab-dep:' + name])), plugins: [virtual],
      outdir: path.join(this.root, '__workbench_deps__'), entryNames: '[name]', chunkNames: 'chunk-[hash]', logLevel: 'silent', define,
    });
    const outputs = new Map(result.outputFiles.map(output => [slash(path.relative(path.join(this.root, '__workbench_deps__'), output.path)), Buffer.from(output.contents)]));
    this.deps = { outputs, names: new Map(Array.from(specifiers, specifier => [specifier, this.depName(specifier)])), stale: false };
    return this.deps;
  }

  async refreshRuntime() {
    if (this.refreshCode) return this.refreshCode;
    const runtime = this.toolRequire.resolve('react-refresh/runtime');
    const result = await esbuild.build({ stdin: { contents: `import runtime from ${JSON.stringify(runtime)};\nexport default runtime;\n`, resolveDir: path.dirname(runtime), loader: 'js' },
      bundle: true, write: false, format: 'esm', platform: 'browser', define: { 'process.env.NODE_ENV': '"development"' }, logLevel: 'silent' });
    this.refreshCode = result.outputFiles[0].text;
    return this.refreshCode;
  }

  client() {
    return `import RefreshRuntime from '${BASE}refresh.js';
if (!window.__labRefreshInstalled) { RefreshRuntime.injectIntoGlobalHook(window); window.__labRefreshInstalled = true; window.$RefreshReg$ = () => {}; window.$RefreshSig$ = () => type => type; }
let timer;
export const __labRefresh = {
  register: (type, id) => RefreshRuntime.register(type, id),
  sig: RefreshRuntime.createSignatureFunctionForTransform,
  schedule() { clearTimeout(timer); timer = setTimeout(() => RefreshRuntime.performReactRefresh(), 16); },
};
const accepted = new Set();
export function __labHot(url) {
  const key = url.split('?')[0];
  return { accept() { accepted.add(key); }, dispose() {}, data: {} };
}
if (!window.__labEvents) {
  window.__labEvents = new EventSource('${BASE}events');
  window.__labEvents.onmessage = async event => {
    const message = JSON.parse(event.data);
    if (message.type === 'reload') {
      if (window.wbPreviewHost) window.wbPreviewHost.load(location.href); else location.reload();
      return;
    }
    try {
      for (const url of message.updates) await import(url + '&t=' + message.time);
      __labRefresh.schedule();
    } catch (error) {
      window.__workbenchError = String(error && error.stack || error);
      console.error('[workbench] hot update failed', error);
    }
  };
}
`;
  }

  async entry(slug) {
    const known = this.entryFiles.get(slug);
    if (!known) throw new Error('Unknown preview ' + slug);
    const prepared = await this.compiler.prepareEntry(known.definition);
    let code = prepared.entry;
    await this.lexer.init;
    const { imports } = importsOf(this.lexer, code);
    let output = '', last = 0;
    for (const item of imports) {
      const replacement = this.rewrite(path.join(prepared.resolveDir, '__entry__.js'), item.n);
      output += code.slice(last, item.s) + replacement;
      last = item.e;
    }
    // The refresh runtime must hook React before react-dom evaluates.
    return `import '${BASE}client.js';\n` + output + code.slice(last);
  }

  // Descriptor fields for a preview, or null when it stays on the shipping
  // path (Astro, custom adapters, declared assets, docs definitions).
  async descriptor(file) {
    const absolute = path.resolve(this.root, file);
    let prepared;
    try { prepared = await this.compiler.prepareEntry(absolute); } catch (_) { return null; }
    if (!prepared.projectBuildable) return null;
    const imports = Array.from(prepared.entry.matchAll(/from "(\/[^"]+)"|import "(\/[^"]+)"/g), match => match[1] || match[2]).filter(item => !item.startsWith('/_workbench/'));
    this.entryFiles.set(prepared.slug, { definition: absolute, files: imports.filter(item => fs.existsSync(item)) });
    for (const item of imports) this.entryImports(item, prepared.slug);
    this.watch();
    await this.ensureDeps();
    const version = this.entryVersion.get(prepared.slug) || 0;
    return { slug: prepared.slug, file: prepared.file, title: prepared.title, module: BASE + 'entry/' + prepared.slug + '.js?v=' + version, version };
  }
  entryImports(file, slug) {
    if (!this.importers.has(file)) this.importers.set(file, new Set());
    this.importers.get(file).add('entry:' + slug);
  }

  watch() {
    if (this.watching) return;
    this.watching = true;
    const pending = new Map();
    fs.watch(this.root, { recursive: true }, (event, name) => {
      if (!name || /(^|[\\/])(node_modules|\.lab|\.git|__workbench)/.test(name)) return;
      const file = path.join(this.root, name);
      clearTimeout(pending.get(file));
      pending.set(file, setTimeout(() => { pending.delete(file); this.changed(file); }, 10));
    });
  }

  changed(file) {
    if (!this.importers.has(file) && !this.transforms.has(file)) return;
    const visited = new Set();
    const accepting = new Set();
    const reload = new Set();
    const queue = [file];
    while (queue.length) {
      const current = queue.shift();
      if (visited.has(current)) continue;
      visited.add(current);
      if (current.startsWith('entry:')) { reload.add(current.slice(6)); continue; }
      if (this.boundaries.has(current)) { accepting.add(current); continue; }
      const importers = this.importers.get(current);
      if (!importers || !importers.size) continue;
      for (const importer of importers) queue.push(importer);
    }
    for (const item of visited) if (!item.startsWith('entry:')) this.bump(item);
    const time = Date.now();
    if (reload.size) {
      for (const slug of reload) this.entryVersion.set(slug, (this.entryVersion.get(slug) || 0) + 1);
      this.broadcast({ type: 'reload', previews: Array.from(reload), time });
    } else if (accepting.size) {
      this.broadcast({ type: 'update', updates: Array.from(accepting, item => this.urlFor(item)), time });
    }
  }

  broadcast(message) {
    const data = 'data: ' + JSON.stringify(message) + '\n\n';
    for (const client of this.clients) client.write(data);
  }

  events(req, res) {
    res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-store', Connection: 'keep-alive' });
    res.write(': connected\n\n');
    this.clients.add(res);
    // The server's proxy drops requests idle for two minutes.
    const heartbeat = setInterval(() => res.write(': heartbeat\n\n'), 30000);
    req.on('close', () => { clearInterval(heartbeat); this.clients.delete(res); });
  }

  // Serves /esm/... paths; returns false when the path is not one of them.
  async serve(pathname, res) {
    const match = /^\/esm\/([^/]+)\/?(.*)$/.exec(pathname);
    if (!match) return false;
    const [, kind, rest] = match;
    const send = (type, body, cache = 'no-cache') => { res.setHeader('Content-Type', type); res.setHeader('Cache-Control', cache); res.end(body); };
    if (kind === 'client.js') { send('text/javascript', this.client()); return true; }
    if (kind === 'refresh.js') { send('text/javascript', await this.refreshRuntime()); return true; }
    if (kind === 'entry') { send('text/javascript', await this.entry(rest.replace(/\.js$/, ''))); return true; }
    if (kind === 'deps') {
      const deps = await this.ensureDeps();
      const body = deps.outputs.get(rest);
      if (!body) { res.statusCode = 404; res.end('No dependency ' + rest); return true; }
      send('text/javascript', body, rest.startsWith('chunk-') ? 'public, max-age=31536000, immutable' : 'no-cache');
      return true;
    }
    if (kind === 'raw') {
      const file = this.fileFor('m', rest);
      send(MIME[path.extname(file)] || 'application/octet-stream', fs.readFileSync(file), 'no-cache');
      return true;
    }
    if (kind === 'm' || kind === 'wb') {
      const file = this.fileFor(kind, rest);
      send('text/javascript', await this.transform(file));
      return true;
    }
    return false;
  }
}

module.exports = { EsmServer, BASE };
