// One builder per compiler, all driven by the spec capture-entries.cjs wrote.
// Each returns { build(), rebuild(), close() } for a set of entries. Bun runs
// in its own runtime; see bun-builder.mjs. Tools load from the run's tools
// directory, passed in by the caller, never from the package.
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';

const ASSET_NAMES = 'assets/[name]-[hash]';
const CHUNK_NAMES = 'chunks/[name]-[hash]';

const toolImport = (tools, name) => import(pathToFileURL(createRequire(path.join(tools, 'package.json')).resolve(name)).href);
const inputs = entries => Object.fromEntries(entries.map(entry => [entry.slug, entry.file]));

// B6: development builds with HMR and React Fast Refresh, for tools whose HMR
// can be driven from code. Each returns { start(), update(marker), close() }:
// start() resolves after the initial build; update() resolves with the hot
// update the tool produced for the next edit.
export const hmrBuilders = {
  async rspack({ spec, entries, outdir, tools }) {
    const { rspack } = await toolImport(tools, '@rspack/core');
    const { ReactRefreshRspackPlugin } = await toolImport(tools, '@rspack/plugin-react-refresh');
    const define = { ...spec.define };
    delete define['process.env.NODE_ENV'];
    const compiler = rspack({
      mode: 'development', devtool: false, context: spec.fixture, target: 'web',
      entry: Object.fromEntries(entries.map(entry => [entry.slug, entry.file])),
      output: { path: outdir, filename: '[name].js', chunkFilename: CHUNK_NAMES + '.js', assetModuleFilename: ASSET_NAMES + '[ext]', cssFilename: '[name].css' },
      experiments: { css: true },
      // Classic chunks for the HMR runtime; the URL externals load with import().
      externalsType: 'import',
      externals: Object.fromEntries(spec.external.map(request => [request, request])),
      resolve: { alias: spec.alias, extensions: spec.resolveExtensions },
      module: { rules: [
        { test: /\.[cm]?[jt]sx?$/, exclude: /node_modules/, loader: 'builtin:swc-loader', type: 'javascript/auto',
          options: { jsc: { parser: { syntax: 'typescript', tsx: true }, transform: { react: { runtime: 'automatic', development: true, refresh: true } }, target: spec.target } } },
        { test: new RegExp('(' + spec.fileLoaders.map(extension => extension.replace('.', '\\.')).join('|') + ')$'), type: 'asset/resource' },
        { test: /\.css$/, type: 'css' },
      ] },
      optimization: { minimize: false, splitChunks: { chunks: 'all' }, runtimeChunk: 'single' },
      plugins: [new rspack.DefinePlugin(define), new rspack.HotModuleReplacementPlugin(), new ReactRefreshRspackPlugin()],
      cache: true, infrastructureLogging: { level: 'error' }, stats: 'errors-only',
    });
    let watching, waiting;
    return {
      start: () => new Promise((resolve, reject) => {
        let first = true;
        watching = compiler.watch({ aggregateTimeout: 0 }, (error, stats) => {
          if (error || stats.hasErrors()) { (waiting?.reject || reject)(error || new Error(stats.toString({ all: false, errors: true }))); return; }
          if (first) { first = false; resolve(); return; }
          const updates = Object.keys(stats.compilation.assets).filter(name => name.includes('.hot-update.'));
          if (waiting && updates.length) {
            const text = updates.map(name => stats.compilation.assets[name].source().toString()).join('\n');
            waiting.resolve({ updates: updates.length, text });
            waiting = null;
          }
        });
      }),
      update: () => new Promise((resolve, reject) => { waiting = { resolve, reject }; }),
      close: () => new Promise(resolve => watching ? watching.close(resolve) : resolve()),
    };
  },

  // Rolldown's dev engine is marked experimental and hidden from its public
  // documentation; it is the HMR path Vite's bundled dev mode uses.
  async rolldown({ spec, entries, outdir, tools }) {
    const experimental = await toolImport(tools, 'rolldown/experimental');
    let waiting;
    const engine = await experimental.dev({
      cwd: spec.fixture, input: inputs(entries), platform: 'browser', external: spec.external,
      resolve: { alias: spec.alias, extensions: spec.resolveExtensions },
      transform: { define: spec.define, target: spec.target, jsx: { runtime: 'automatic', development: true, refresh: true } },
      moduleTypes: Object.fromEntries(spec.fileLoaders.map(extension => [extension, 'asset'])),
      plugins: [{ name: 'lab-css-empty', load: { filter: { id: /\.css$/ }, handler: () => ({ code: '', moduleType: 'js' }) } }],
      experimental: { devMode: true }, logLevel: 'silent',
    }, { dir: outdir, format: 'esm', entryFileNames: '[name].js', chunkFileNames: CHUNK_NAMES + '.js' }, {
      watch: { skipWrite: false },
      onHmrUpdates: result => {
        if (!waiting) return;
        const updates = (result?.updates ?? result ?? []).map(item => item.update ?? item);
        const text = JSON.stringify(updates);
        waiting.resolve({ updates: updates.length, text });
        waiting = null;
      },
    });
    return {
      async start() { await engine.run(); await engine.ensureCurrentBuildFinish(); await engine.registerClient('lab'); },
      update: () => new Promise((resolve, reject) => { waiting = { resolve, reject }; }),
      close: () => engine.close?.(),
    };
  },
};

export const builders = {
  // Variant 1: esbuild with the lab's build specification.
  async esbuild({ spec, entries, split, outdir, tools }) {
    const esbuild = await toolImport(tools, 'esbuild');
    const options = {
      absWorkingDir: spec.fixture, entryPoints: inputs(entries), bundle: true, write: true, outdir,
      format: 'esm', platform: 'browser', target: spec.target, jsx: spec.jsx, define: spec.define,
      resolveExtensions: spec.resolveExtensions, alias: spec.alias, external: spec.external,
      loader: Object.fromEntries(spec.fileLoaders.map(extension => [extension, 'file'])),
      assetNames: ASSET_NAMES, chunkNames: CHUNK_NAMES, splitting: split, logLevel: 'silent', metafile: false,
    };
    let context;
    return {
      incremental: 'esbuild build context',
      async build() { await esbuild.build(options); },
      async rebuild() { context ||= await esbuild.context(options); return context.rebuild(); },
      async prepareRebuild() { context = await esbuild.context(options); await context.rebuild(); },
      async close() { await context?.dispose(); },
    };
  },

  async rolldown({ spec, entries, split, outdir, tools }) {
    const { rolldown, watch } = await toolImport(tools, 'rolldown');
    // Rolldown has no built-in CSS bundling (Vite supplies it), so CSS imports
    // become modules that inject their stylesheet, with url() assets emitted.
    const cssPlugin = {
      name: 'lab-css',
      load: {
        filter: { id: /\.css$/ },
        async handler(id) {
          let css = fs.readFileSync(id, 'utf8');
          const references = [];
          css = css.replace(/url\((['"]?)([^'")]+)\1\)/g, (match, quote, reference) => {
            if (/^(?:data:|https?:|#)/.test(reference)) return match;
            const file = path.resolve(path.dirname(id), reference.split(/[?#]/)[0]);
            const handle = this.emitFile({ type: 'asset', name: path.basename(file), source: fs.readFileSync(file) });
            references.push(handle);
            return 'url(__LAB_ASSET_' + (references.length - 1) + '__)';
          });
          const urls = references.map(handle => `new URL(import.meta.ROLLUP_FILE_URL_${handle}).href`);
          return {
            code: `const css = ${JSON.stringify(css)}.replace(/__LAB_ASSET_(\\d+)__/g, (m, i) => [${urls.join(', ')}][i]);\n` +
              'const style = document.createElement("style"); style.textContent = css; document.head.append(style);\n',
            moduleType: 'js',
          };
        },
      },
    };
    const input = {
      cwd: spec.fixture, input: inputs(entries), platform: 'browser', external: spec.external,
      resolve: { alias: spec.alias, extensions: spec.resolveExtensions },
      transform: { define: spec.define, target: spec.target, jsx: 'react-jsx' },
      moduleTypes: Object.fromEntries(spec.fileLoaders.map(extension => [extension, 'asset'])),
      plugins: [cssPlugin], logLevel: 'silent',
    };
    const output = {
      dir: outdir, format: 'esm', entryFileNames: '[name].js', chunkFileNames: CHUNK_NAMES + '.js',
      assetFileNames: ASSET_NAMES + '[extname]', codeSplitting: split,
    };
    let watcher;
    let resolveBuild;
    return {
      incremental: 'watch with experimental.incrementalBuild',
      async build() { const bundle = await rolldown(input); await bundle.write(output); await bundle.close(); },
      async prepareRebuild() {
        await new Promise((resolve, reject) => {
          watcher = watch({ ...input, output, experimental: { incrementalBuild: true }, watch: { buildDelay: 0 } });
          watcher.on('event', event => {
            if (event.code === 'BUNDLE_END') { event.result?.close(); (resolveBuild || resolve)(); resolveBuild = null; }
            if (event.code === 'ERROR') { event.result?.close(); reject(event.error); }
          });
        });
      },
      // The watcher notices the edit itself; this waits for the next bundle.
      rebuild() { return new Promise(resolve => { resolveBuild = resolve; }); },
      watches: true,
      async close() { await watcher?.close(); },
    };
  },

  async rspack({ spec, entries, split, outdir, tools, persistentCache }) {
    const { rspack } = await toolImport(tools, '@rspack/core');
    // Development mode defines process.env.NODE_ENV itself.
    const define = { ...spec.define };
    delete define['process.env.NODE_ENV'];
    const config = {
      mode: 'development', devtool: false, context: spec.fixture, target: 'web',
      entry: Object.fromEntries(entries.map(entry => [entry.slug, entry.file])),
      output: {
        path: outdir, module: true, chunkFormat: 'module', chunkLoading: 'import', library: { type: 'module' },
        filename: '[name].js', chunkFilename: CHUNK_NAMES + '.js', cssFilename: '[name].css',
        cssChunkFilename: CHUNK_NAMES + '.css', assetModuleFilename: ASSET_NAMES + '[ext]',
        // Output is written first-time into an empty directory per sample.
        clean: false, asyncChunks: split,
      },
      experiments: { outputModule: true, css: true },
      externalsType: 'module',
      externals: Object.fromEntries(spec.external.map(request => [request, request])),
      resolve: { alias: spec.alias, extensions: spec.resolveExtensions },
      module: {
        rules: [
          { test: /\.[cm]?[jt]sx?$/, exclude: /node_modules/, loader: 'builtin:swc-loader', type: 'javascript/auto',
            options: { jsc: { parser: { syntax: 'typescript', tsx: true }, transform: { react: { runtime: 'automatic' } }, target: spec.target } } },
          { test: new RegExp('(' + spec.fileLoaders.map(extension => extension.replace('.', '\\.')).join('|') + ')$'), type: 'asset/resource' },
          { test: /\.css$/, type: 'css' },
        ],
      },
      optimization: { minimize: false, splitChunks: split ? { chunks: 'all' } : false, runtimeChunk: split ? 'single' : false },
      plugins: [new rspack.DefinePlugin(define)],
      cache: persistentCache ? { type: 'persistent', storage: { type: 'filesystem', directory: persistentCache } } : true,
      infrastructureLogging: { level: 'error' }, stats: 'errors-only',
    };
    const compiler = rspack(config);
    const run = () => new Promise((resolve, reject) => compiler.run((error, stats) => {
      if (error) reject(error);
      else if (stats.hasErrors()) reject(new Error(stats.toString({ all: false, errors: true })));
      else resolve(stats);
    }));
    // A second run() reuses cached file reads and misses the edit, so
    // rebuilds use watch mode, as Rspack rebuilds in development.
    let watching, waiting;
    return {
      incremental: 'watch mode with its memory cache',
      build: run,
      prepareRebuild: () => new Promise((resolve, reject) => {
        let first = true;
        watching = compiler.watch({ aggregateTimeout: 0 }, (error, stats) => {
          const failure = error || (stats.hasErrors() ? new Error(stats.toString({ all: false, errors: true })) : null);
          if (first) { first = false; failure ? reject(failure) : resolve(); return; }
          if (waiting) { failure ? waiting.reject(failure) : waiting.resolve(); waiting = null; }
        });
      }),
      rebuild: () => new Promise((resolve, reject) => { waiting = { resolve, reject }; }),
      close: () => new Promise(resolve => watching ? watching.close(() => compiler.close(resolve)) : compiler.close(resolve)),
    };
  },

  async utoo({ spec, entries, split, outdir, tools, persistentCache, publicPath }) {
    const { build } = await toolImport(tools, '@utoo/pack');
    // Turbopack refuses output outside the project root, so Utoo writes into
    // the lab-owned fixture rather than the run's output directory.
    outdir = path.join(spec.fixture, '.lab', 'utoo-out', path.basename(outdir));
    // Turbopack resolves "/…" imports as files and does not apply externals
    // to aliased requests, so Utoo builds copies of the entries whose runtime
    // imports use bare names; the build work is otherwise identical.
    const entryDir = path.join(spec.fixture, '.lab', 'utoo-entries');
    fs.mkdirSync(entryDir, { recursive: true });
    const utooEntries = entries.map(entry => {
      let source = fs.readFileSync(entry.file, 'utf8');
      spec.external.forEach((request, index) => { source = source.split(JSON.stringify(request)).join(JSON.stringify('lab-external-' + index)); });
      // Turbopack reads "/…" as relative to the project root, as Next.js
      // does, so absolute module paths become relative ones.
      source = source.replace(/from "(\/[^"]+)"|import "(\/[^"]+)"/g, (match, from, bare) => {
        const relative = path.relative(entryDir, from || bare).split(path.sep).join('/');
        return from ? `from "${relative.startsWith('.') ? relative : './' + relative}"` : `import "${relative.startsWith('.') ? relative : './' + relative}"`;
      });
      const file = path.join(entryDir, entry.slug + '.js');
      fs.writeFileSync(file, source);
      return { name: entry.slug, import: file };
    });
    const config = {
      config: {
        mode: 'development',
        entry: utooEntries,
        // Turbopack names chunks with [contenthash:N]. Its classic-script
        // runtime finds chunks through the public path when loaded by parity.
        output: { path: outdir, type: 'standalone', filename: '[name].js', chunkFilename: 'chunks/[name]-[contenthash:8].js', clean: false,
          ...(publicPath ? { publicPath } : {}) },
        externals: Object.fromEntries(spec.external.map((request, index) => ['lab-external-' + index, { type: 'esm', root: request }])),
        // Alias targets are read relative to the project as well.
        resolve: {
          alias: Object.fromEntries(Object.entries(spec.alias).map(([name, target]) => [name, './' + path.relative(spec.fixture, target).split(path.sep).join('/')])),
          extensions: spec.resolveExtensions,
        },
        // `target` takes a browserslist query here, not a platform name.
        define: spec.define, target: 'chrome >= 109', sourceMaps: false, react: { runtime: 'automatic' },
        optimization: { minify: false, ...(split ? {} : { splitChunks: undefined }) },
        persistentCaching: !!persistentCache, ...(persistentCache ? { cacheDirectory: persistentCache } : {}),
      },
      processEnv: { NODE_ENV: 'development' },
    };
    return {
      outputDir: outdir,
      incremental: 'none used: full rebuild (its watch mode is not exposed as an awaitable rebuild)',
      build: () => build(config, spec.fixture, spec.fixture),
      prepareRebuild: () => build(config, spec.fixture, spec.fixture),
      rebuild: () => build(config, spec.fixture, spec.fixture),
      async close() {},
    };
  },
};
