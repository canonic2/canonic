// The @rspack/core side of stage 1's B cells: the Rust host's line protocol
// (see host/src/compile.rs) over @rspack/core 2.2.7, with the same options,
// so the two differ only in the host.
//
//   node labs/instant-first-page/node-host.mjs
//
// One JSON request per line on stdin, one JSON answer per line on stdout.
import { createRequire } from 'node:module';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const workbench = path.resolve(here, '..', '..');
const require = createRequire(path.join(workbench, 'package.json'));
const { rspack } = require('@rspack/core');
const { ReactRefreshRspackPlugin } = require('@rspack/plugin-react-refresh');
const { createFsFromVolume, Volume } = require('memfs');

const entries = new Map();
let compiler = null;
let watching = null;
let virtual = null;
let hotClient = null;
let pending = null;
let lastCompilation = null;
let started = 0;

const clean = name => name.replace(/[^a-z0-9-]/gi, '-');
function stableChunks(root) {
  const name = module => {
    const file = module.nameForCondition && module.nameForCondition() || '';
    const at = file.lastIndexOf('/node_modules/');
    if (at !== -1) {
      const rest = file.slice(at + '/node_modules/'.length).split('/');
      return 'pkg-' + clean(rest[0].startsWith('@') && rest.length > 1 ? rest[0] + '-' + rest[1] : rest[0]);
    }
    const folder = path.relative(root, path.dirname(file)).split(path.sep).join('-');
    return 'dir-' + clean(folder || 'root');
  };
  return {
    chunks: 'all', minSize: 0, maxAsyncRequests: Infinity, maxInitialRequests: Infinity,
    cacheGroups: {
      default: false, defaultVendors: false,
      stable: { test: module => { const file = module.nameForCondition && module.nameForCondition() || ''; return !!file && !/\/\.workbench-(rspack|astro)/.test(file); },
        name, enforce: true, chunks: 'all', reuseExistingChunk: false },
    },
  };
}

function options(job, memory) {
  const swc = { loader: 'builtin:swc-loader', options: { jsc: { parser: { syntax: 'typescript', tsx: true }, transform: { react: { runtime: 'automatic', development: true, refresh: true } }, target: 'es2020' } } };
  return {
    name: 'web', mode: 'development', devtool: false, context: job.root, target: 'web',
    entry: () => {
      const out = { __workbench_idle: [job.hot_client.file] };
      for (const entry of entries.values()) out[entry.slug] = [job.hot_client.file, entry.file];
      return out;
    },
    output: { path: '/out/web/' + job.generation, publicPath: '/_workbench/previews/rspack/web/' + job.generation + '/', filename: '[name].[contenthash:8].js',
      chunkFilename: 'chunks/[name].[contenthash:8].js', cssFilename: '[name].css', cssChunkFilename: 'chunks/[name].css', assetModuleFilename: 'assets/[name].[hash:8][ext]',
      uniqueName: 'wb_web', hotUpdateChunkFilename: 'hot/[id].[fullhash].hot-update.js', hotUpdateMainFilename: 'hot/[runtime].[fullhash].hot-update.json' },
    externalsType: 'window',
    externals: { '/_workbench/preview-runtime.js': '__workbenchRuntime' },
    resolve: { alias: { '@canonic2/workbench$': path.join(job.preview_dir, 'api.js') }, extensions: ['.tsx', '.ts', '.jsx', '.js', '.mjs', '.cjs', '.json', '.vue', '.html'],
      modules: [path.join(job.root, 'node_modules'), 'node_modules'] },
    module: { rules: [
      { test: /\.[cm]?[jt]sx?$/, exclude: /node_modules/, type: 'javascript/auto', use: [swc] },
      // Projects import CSS modules as a default object.
      { test: /\.module\.css$/, type: 'css/module', parser: { namedExports: false } },
      { test: /\.css$/, exclude: /\.module\.css$/, type: 'css' },
      { test: /\.(svg|png|jpe?g|gif|webp|avif|ico|bmp|woff2?|ttf|otf|eot|mp4|webm|mov|mp3|ogg|wav|m4a)$/, type: 'asset/resource' },
    ] },
    experiments: { css: true },
    plugins: [
      virtual,
      new rspack.DefinePlugin({ 'process.env.NODE_ENV': JSON.stringify('development') }),
      new rspack.HotModuleReplacementPlugin(),
      new ReactRefreshRspackPlugin({ exclude: [/node_modules/] }),
    ],
    optimization: { splitChunks: job.stable_chunks ? stableChunks(job.root) : { chunks: 'all' }, runtimeChunk: 'single', minimize: false, moduleIds: 'named' },
    cache: { type: 'persistent', name: 'web', version: 'instant-first-page', storage: { type: 'filesystem', directory: job.cache_dir } },
    infrastructureLogging: { level: 'error' }, stats: 'errors-only',
    // Builds start only from the protocol's requests.
    watchOptions: { ignored: /./, aggregateTimeout: 0 },
  };
}

function answer(stats) {
  const ms = performance.now() - started;
  const compilation = stats.compilation;
  lastCompilation = compilation;
  const entrypoints = {};
  for (const [name, point] of compilation.entrypoints) entrypoints[name] = point.getFiles();
  const chunk_of = {};
  for (const chunk of compilation.chunks) {
    if (!chunk.name || !/^(dir|pkg)-/.test(chunk.name)) continue;
    for (const module of compilation.chunkGraph.getChunkModules(chunk)) {
      const id = module.identifier();
      if (!id.includes('node_modules')) chunk_of[id] = chunk.name;
    }
  }
  return { ok: true, ms, hash: stats.hash, errors: compilation.errors.map(error => String(error.message || error)), entrypoints,
    assets: Object.keys(compilation.assets).sort(), chunk_of, modules: compilation.modules.size };
}

function build(trigger) {
  return new Promise(resolve => { pending = resolve; started = performance.now(); trigger(); });
}

async function handle(request) {
  switch (request.op) {
    case 'start': {
      const created = performance.now();
      hotClient = request.hot_client;
      for (const entry of request.entries) entries.set(entry.slug, entry);
      virtual = new rspack.experiments.VirtualModulesPlugin(Object.fromEntries([[hotClient.file, hotClient.source], ...request.entries.map(entry => [entry.file, entry.source])]));
      compiler = rspack(options(request, null));
      compiler.outputFileSystem = createFsFromVolume(new Volume());
      const created_ms = performance.now() - created;
      const result = await build(() => {
        watching = compiler.watch(compiler.options.watchOptions, (error, stats) => {
          const done = pending; pending = null;
          if (done) done(error ? { ok: false, ms: performance.now() - started, error: String(error.message || error) } : answer(stats));
        });
      });
      return { ...result, created_ms };
    }
    case 'add': {
      for (const entry of request.entries) { entries.set(entry.slug, entry); virtual.writeModule(entry.file, entry.source); }
      return build(() => watching.invalidateWithChangesAndRemovals(new Set(request.entries.map(entry => entry.file)), new Set()));
    }
    case 'rebuild':
      return build(() => watching.invalidateWithChangesAndRemovals(new Set(request.changed), new Set(request.deleted)));
    case 'asset': {
      const asset = lastCompilation && lastCompilation.assets[request.name];
      return asset ? { ok: true, source: String(asset.source()) } : { ok: false, error: 'no such asset' };
    }
    case 'close':
      if (watching) await new Promise(resolve => watching.close(resolve));
      return { ok: true, closed: true };
    default:
      return { ok: false, error: 'unknown op' };
  }
}

let queue = Promise.resolve();
readline.createInterface({ input: process.stdin }).on('line', line => {
  queue = queue.then(async () => {
    let request;
    try { request = JSON.parse(line); } catch (error) { process.stdout.write(JSON.stringify({ ok: false, error: String(error) }) + '\n'); return; }
    let result;
    try { result = await handle(request); } catch (error) { result = { ok: false, error: String(error.stack || error) }; }
    process.stdout.write(JSON.stringify(result) + '\n');
    if (request.op === 'close') process.exit(0);
  });
});
