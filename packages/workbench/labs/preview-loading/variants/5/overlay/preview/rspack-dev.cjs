/* Lab variant 5: a bundler with built-in hot module replacement. Rspack's
   development compiler watches every project-buildable preview of an adapter
   as one build with shared chunks, its HMR runtime and React Refresh. The
   worker serves its output; an event stream tells open previews to check for
   a hot update after each rebuild. Rspack's HMR runtime loads chunks as
   classic scripts, so each preview's module is a small ES module that loads
   them and passes the preview's mount through. */
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { createRequire } = require('node:module');

const BASE = '/_workbench/previews/rspack/';
const MIME = { '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.woff': 'font/woff' };

class RspackDev {
  constructor(compiler) {
    const tools = process.env.WORKBENCH_LAB_TOOLS;
    if (!tools) throw new Error('Variant 5 needs WORKBENCH_LAB_TOOLS');
    this.toolRequire = createRequire(path.join(tools, 'package.json'));
    this.compiler = compiler;
    this.root = compiler.root;
    this.work = fs.mkdtempSync(path.join(os.tmpdir(), 'workbench-rspack-'));
    this.clients = new Set();
    this.entries = null;
    this.ready = null;
    this.stats = null;
    this.building = null;
  }

  async start() {
    if (this.ready) return this.ready;
    this.ready = (async () => {
      const { discover } = require('./compiler.cjs');
      await this.compiler.settings();
      const entries = [];
      for (const file of discover(this.root, this.compiler.options.include)) {
        let prepared;
        try { prepared = await this.compiler.prepareEntry(file); } catch (_) { continue; }
        if (!prepared.projectBuildable || prepared.adapter !== 'react') continue;
        const entryFile = path.join(this.work, 'entries', prepared.slug + '.js');
        fs.mkdirSync(path.dirname(entryFile), { recursive: true });
        // Expose mount for the shim instead of exporting it.
        fs.writeFileSync(entryFile, prepared.entry.replace(/^export function mount/m, 'function mount') +
          `\n(window.__labRspack = window.__labRspack || {})[${JSON.stringify(prepared.slug)}] = { mount };\n`);
        entries.push({ ...prepared, entryFile });
      }
      this.entries = new Map(entries.map(entry => [entry.file, entry]));
      const clientFile = path.join(this.work, 'hot-client.js');
      fs.writeFileSync(clientFile, `if (module.hot && !window.__labRspackEvents) {
  window.__labRspackEvents = new EventSource('${BASE}events');
  // A rebuild that lands while a check runs is checked after it, not lost.
  let checking = false, again = false;
  const check = () => {
    if (checking) { again = true; return; }
    checking = true;
    // A document that loaded Rspack's classic-script runtime cannot load a
    // fresh copy of it, so an update nothing accepts reloads the document.
    module.hot.check(true).catch(error => { console.warn('[workbench] hot update not accepted; reloading', error); location.reload(); })
      .finally(() => { checking = false; if (again) { again = false; check(); } });
  };
  window.__labRspackEvents.onmessage = event => {
    if (JSON.parse(event.data).type === 'built') check();
  };
}
`);
      const { rspack } = this.toolRequire('@rspack/core');
      const { ReactRefreshRspackPlugin } = this.toolRequire('@rspack/plugin-react-refresh');
      const config = this.compiler.config;
      const define = { ...(config.define || {}) };
      const compiler = rspack({
        mode: 'development', devtool: false, context: this.root, target: 'web',
        entry: Object.fromEntries(entries.map(entry => [entry.slug, [clientFile, entry.entryFile]])),
        output: { path: path.join(this.work, 'out'), publicPath: BASE, filename: '[name].js', chunkFilename: 'chunks/[name].[contenthash].js',
          cssFilename: '[name].css', cssChunkFilename: 'chunks/[name].[contenthash].css', assetModuleFilename: 'assets/[name].[hash][ext]' },
        experiments: { css: true },
        externalsType: 'import',
        externals: { '/_workbench/preview-runtime.js': '/_workbench/preview-runtime.js', '/_workbench/preview-requests.js': '/_workbench/preview-requests.js' },
        resolve: {
          alias: { '@canonic2/workbench': path.join(__dirname, 'api.js'), ...(config.aliases || {}) },
          extensions: config.resolveExtensions || ['.tsx', '.ts', '.jsx', '.js', '.mjs', '.cjs', '.json'],
          // Packages resolve from the project first, as Workbench dedupes them.
          modules: [path.join(this.root, 'node_modules'), 'node_modules'],
        },
        module: { rules: [
          { test: /\.[cm]?[jt]sx?$/, exclude: /node_modules/, loader: 'builtin:swc-loader', type: 'javascript/auto',
            options: { jsc: { parser: { syntax: 'typescript', tsx: true }, transform: { react: { runtime: 'automatic', development: true, refresh: true } }, target: 'es2020' } } },
          { test: /\.(svg|png|jpe?g|gif|webp|avif|woff2?|ttf|mp4|mp3)$/, type: 'asset/resource' },
          { test: /\.css$/, type: 'css' },
        ] },
        optimization: { minimize: false, splitChunks: { chunks: 'all' }, runtimeChunk: 'single' },
        plugins: [new rspack.DefinePlugin(define), new rspack.HotModuleReplacementPlugin(), new ReactRefreshRspackPlugin()],
        cache: true, infrastructureLogging: { level: 'error' }, stats: 'errors-only',
      });
      let resolveFirst, rejectFirst;
      const first = new Promise((resolve, reject) => { resolveFirst = resolve; rejectFirst = reject; });
      compiler.hooks.invalid.tap('lab', () => { this.building = new Promise(resolve => { this.finishBuilding = resolve; }); });
      this.watching = compiler.watch({ aggregateTimeout: 0, ignored: /node_modules|[\\/]\.lab[\\/]/ }, (error, stats) => {
        if (error) { rejectFirst(error); return; }
        this.stats = stats.toJson({ all: false, entrypoints: true, assets: false, errors: true, hash: true });
        if (this.finishBuilding) { this.finishBuilding(); this.finishBuilding = null; }
        resolveFirst();
        this.broadcast({ type: 'built', hash: this.stats.hash, time: Date.now() });
      });
      await first;
    })();
    return this.ready;
  }

  async current() {
    await this.start();
    if (this.building) await this.building;
    return this.stats;
  }

  async descriptor(file) {
    await this.start();
    const entry = this.entries.get(path.relative(this.root, path.resolve(this.root, file)).split(path.sep).join('/'));
    if (!entry) return null;
    return { slug: entry.slug, file: entry.file, title: entry.title };
  }

  // The preview's module: load the runtime and initial chunks as classic
  // scripts in order, then hand mount to the preview host.
  async shim(slug) {
    const stats = await this.current();
    const point = stats.entrypoints[slug];
    if (!point) throw new Error('No rspack entry for ' + slug + (stats.errors?.length ? ': ' + stats.errors[0].message : ''));
    const scripts = point.assets.map(asset => asset.name || asset).filter(name => name.endsWith('.js'));
    const styles = point.assets.map(asset => asset.name || asset).filter(name => name.endsWith('.css'));
    return `const load = src => new Promise((resolve, reject) => { const script = document.createElement('script'); script.src = src; script.onload = resolve; script.onerror = () => reject(new Error('Failed to load ' + src)); document.head.append(script); });
for (const href of ${JSON.stringify(styles.map(name => BASE + name))}) { const link = document.createElement('link'); link.rel = 'stylesheet'; link.href = href; document.head.append(link); }
for (const src of ${JSON.stringify(scripts.map(name => BASE + name))}) await load(src);
// The entry imports Workbench's runtime URLs with import(), so it is an
// async module: it registers mount after its script's load event.
const slug = ${JSON.stringify(slug)};
const deadline = Date.now() + 30000;
while (!window.__labRspack?.[slug]) {
  if (Date.now() > deadline) throw new Error('The rspack entry did not register ' + slug);
  await new Promise(resolve => setTimeout(resolve, 5));
}
export function mount(options) { return window.__labRspack[slug].mount(options); }
`;
  }

  broadcast(message) {
    const data = 'data: ' + JSON.stringify(message) + '\n\n';
    for (const client of this.clients) client.write(data);
  }

  events(req, res) {
    res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-store', Connection: 'keep-alive' });
    res.write(': connected\n\n');
    this.clients.add(res);
    const heartbeat = setInterval(() => res.write(': heartbeat\n\n'), 30000);
    req.on('close', () => { clearInterval(heartbeat); this.clients.delete(res); });
  }

  async serve(pathname, res) {
    const match = /^\/rspack\/(.+)$/.exec(pathname);
    if (!match) return false;
    const rest = decodeURIComponent(match[1]);
    if (rest.startsWith('shim/')) {
      res.setHeader('Content-Type', 'text/javascript'); res.setHeader('Cache-Control', 'no-cache');
      res.end(await this.shim(rest.slice(5).replace(/\.js$/, '')));
      return true;
    }
    await this.current();
    const file = path.join(this.work, 'out', rest);
    if (!file.startsWith(path.join(this.work, 'out') + path.sep) || !fs.existsSync(file)) { res.statusCode = 404; res.end('Not found'); return true; }
    res.setHeader('Content-Type', MIME[path.extname(file)] || 'application/octet-stream');
    res.setHeader('Cache-Control', /\.[0-9a-f]{8,}\./.test(rest) ? 'public, max-age=31536000, immutable' : 'no-cache');
    res.end(fs.readFileSync(file));
    return true;
  }
}

module.exports = { RspackDev, BASE };
