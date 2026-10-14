// Runs under the bundled Electron's Node mode, or Node for a source checkout.
// Neither the editor extension host nor the screenshot renderer loads projects.
const http = require('node:http');
const path = require('node:path');
const { Compiler, docsRequest, validate, mockFetches } = require('./compiler.cjs');
const { Pipeline } = require('../src/modules/preview-pipeline/index.ts');
const portable = require('./portable.cjs');
const { withPreviewScripts } = require('../preview-scripts');
const compiler = new Compiler(process.argv[2], JSON.parse(process.argv[3] || '{}'));
const diagnostic = (event, details) => { if (process.connected) process.send({ type: 'diagnostic', event, details }); };
// Previews and docs lenses build through the Rspack pipeline; esbuild still
// evaluates definitions and builds Astro input edits and portable exports.
const pipeline = new Pipeline(compiler, { validate, docsRequest, mockFetches, createAstro: (root, target, config) => require('./astro.cjs').create(root, target, config) },
  { previewDir: __dirname, log: diagnostic });
const mime = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.woff2': 'font/woff2' };
const variants = new Map();
let queue = Promise.resolve();
const serial = work => { const result = queue.then(work); queue = result.catch(() => {}); return result; };
const json = (res, value) => { res.setHeader('Content-Type', 'application/json'); res.end(JSON.stringify(value)); };
const server = http.createServer((req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  const requested = Date.now();
  let started;
  const traced = ['/index', '/page', '/descriptor', '/render', '/export', '/docs/bundle'].includes(url.pathname);
  res.once('finish', () => {
    const elapsedMs = Date.now() - requested;
    if (traced || elapsedMs >= 500) diagnostic('preview.request.completed', {
      route: url.pathname, file: url.searchParams.get('file'), status: res.statusCode,
      queueMs: started === undefined ? 0 : started - requested,
      workMs: started === undefined ? 0 : Date.now() - started, elapsedMs,
    });
  });
  res.setHeader('Cache-Control', 'no-store');
  if (req.method !== 'GET') { res.statusCode = 405; res.end('GET only'); return; }
  // The event stream stays open and built output is read from memory, so
  // neither waits in the serial queue.
  if (url.pathname === '/rspack/events') { pipeline.events(req, res); return; }
  // First page lab: the start timeline, and lazy-compilation requests (variant 2).
  if (url.pathname === '/rspack/lab') { json(res, { timeline: pipeline.timeline, prewarmed: pipeline.prewarmed, modules: pipeline.chunkOf.size, moved: pipeline.moved }); return; }
  if (url.pathname === '/rspack/lazy') { pipeline.lazyRequest(url.search, res).catch(error => { res.statusCode = 500; res.end(String(error.message || error)); }); return; }
  if (url.pathname.startsWith('/rspack/')) {
    pipeline.serve(url.pathname, res).catch(error => {
      diagnostic('preview.pipeline.serve.failed', { path: url.pathname, message: String(error.message || error).slice(0, 500) });
      if (!res.headersSent) { res.statusCode = 500; json(res, { error: error.message }); } else res.end();
    });
    return;
  }
  serial(async () => {
    started = Date.now();
    if (traced) diagnostic('preview.request.started', {
      route: url.pathname, file: url.searchParams.get('file'), queueMs: started - requested,
    });
    if (url.pathname === '/index') {
      const listing = Date.now();
      const index = await compiler.index();
      pipeline.timeline.push({ event: 'worker.index.answered', at: Date.now(), ms: Date.now() - listing, previews: index.previews.length });
      json(res, { ...index, errors: [...index.errors, ...pipeline.problems()] });
      return;
    }
    if (url.pathname === '/render') {
      const inputs = JSON.parse(url.searchParams.get('inputs') || '{}');
      if (!inputs || typeof inputs !== 'object' || Array.isArray(inputs)) throw new Error('Astro inputs must be an object.');
      const state = url.searchParams.get('state');
      const result = await compiler.compile(url.searchParams.get('file'), true, { state, inputs });
      if (!result.rendered[state]) throw new Error('This preview does not use the built-in Astro adapter.');
      variants.set(result.slug, result);
      while (variants.size > 32) variants.delete(variants.keys().next().value);
      json(res, { html: result.rendered[state].html, base: '/_workbench/previews/preview/' + result.slug + '/' });
      return;
    }
    if (url.pathname === '/descriptor') { json(res, pipeline.descriptor(await pipeline.preview(url.searchParams.get('file')))); return; }
    if (url.pathname === '/page') {
      res.setHeader('Content-Type', mime['.html']);
      // The outer URL owns state; the compatibility bundle reads it here.
      res.end(withPreviewScripts(pipeline.page(await pipeline.preview(url.searchParams.get('file')))));
      return;
    }
    // A docs page lens: what it offers, for problems, or its bundle to mount.
    if (url.pathname === '/docs/index') {
      await compiler.settings();
      const listed = await compiler.docsExamples(docsRequest(JSON.parse(url.searchParams.get('request') || 'null'), compiler.root));
      json(res, { problems: listed.problems, examples: listed.examples.map(example =>
        ({ ...example, file: path.relative(compiler.root, example.file).split(path.sep).join('/') })) });
      return;
    }
    if (url.pathname === '/docs/bundle') {
      const request = JSON.parse(url.searchParams.get('request') || 'null');
      const entry = await pipeline.docs(request);
      const descriptor = pipeline.descriptor(entry);
      // Examples update in place, so the bundle's revision stays the same.
      json(res, { base: descriptor.base, module: descriptor.module, revision: 'rspack', stylesheet: null, examples: entry.examples, problems: entry.problems });
      return;
    }
    if (url.pathname === '/export') {
      const config = require('../config').read(compiler.root);
      var selection = url.searchParams.has('selection') ? JSON.parse(url.searchParams.get('selection')) : undefined;
      if (selection && (!Array.isArray(selection.pages) || selection.pages.some(file => typeof file !== 'string') ||
          !Array.isArray(selection.states) || selection.states.some(state => typeof state !== 'string') ||
          !Array.isArray(selection.sizes) || selection.sizes.some(size => typeof size !== 'string'))) throw new Error('Invalid portable export selection');
      json(res, await portable.create(compiler, { name: config && config.name, selection: selection }));
      return;
    }
    const match = /^\/preview\/([^/]+)\/(.+)$/.exec(url.pathname);
    // Astro input edits render through esbuild, with their own assets.
    if (match && variants.has(match[1])) {
      const result = variants.get(match[1]);
      const name = decodeURIComponent(match[2]);
      const body = result.outputs.get(name);
      if (body) {
        const etag = '"' + result.revision + ':' + name + '"';
        res.setHeader('Cache-Control', 'no-cache');
        res.setHeader('ETag', etag);
        if (req.headers['if-none-match'] === etag) { res.statusCode = 304; res.end(); return; }
        res.setHeader('Content-Type', mime[path.extname(name)] || 'application/octet-stream');
        res.end(body);
        return;
      }
    }
    res.statusCode = 404;
    res.end('Preview asset not found');
  }).catch(error => { res.statusCode = 500; json(res, { error: error.message }); });
});
server.listen(0, '127.0.0.1', () => {
  if (process.send) process.send({ port: server.address().port });
  // Pre-warm: build every preview and docs lens before anything asks.
  pipeline.warming = pipeline.prewarm();
});
const stop = () => pipeline.close().catch(() => {}).finally(() => server.close(() => process.exit(0)));
process.on('disconnect', stop);
process.on('message', message => { if (message === 'close') stop(); });
process.on('SIGTERM', stop);
