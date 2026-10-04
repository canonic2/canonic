// Runs under the bundled Electron's Node mode, or Node for a source checkout.
// Neither the editor extension host nor the screenshot renderer loads projects.
const http = require('node:http');
const path = require('node:path');
const { Compiler, docsRequest } = require('./compiler.cjs');
const portable = require('./portable.cjs');
const { withPreviewScripts } = require('../preview-scripts');
const compiler = new Compiler(process.argv[2], JSON.parse(process.argv[3] || '{}'));
const mime = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.woff2': 'font/woff2' };
const slugs = new Map();
const variants = new Map();
// Docs bundles by slug, to the request that builds them.
const docs = new Map();
const docsBase = slug => '/_workbench/previews/docs/' + slug + '/';
let queue = Promise.resolve();
const serial = work => { const result = queue.then(work); queue = result.catch(() => {}); return result; };
const json = (res, value) => { res.setHeader('Content-Type', 'application/json'); res.end(JSON.stringify(value)); };
const server = http.createServer((req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  const requested = Date.now();
  let started;
  const traced = ['/page', '/descriptor', '/render', '/export', '/docs/bundle'].includes(url.pathname);
  const diagnostic = (event, details) => {
    if (process.connected) process.send({ type: 'diagnostic', event, details });
  };
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
  serial(async () => {
    started = Date.now();
    if (traced) diagnostic('preview.request.started', {
      route: url.pathname, file: url.searchParams.get('file'), queueMs: started - requested,
    });
    if (url.pathname === '/index') { json(res, await compiler.index()); return; }
    if (url.pathname === '/revision') { json(res, { revision: (await compiler.compile(url.searchParams.get('file'))).revision }); return; }
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
    if (url.pathname === '/page' || url.pathname === '/descriptor') {
      const result = await compiler.compile(url.searchParams.get('file'));
      slugs.set(result.slug, result.file);
      if (url.pathname === '/descriptor') {
        const base = '/_workbench/previews/preview/' + result.slug + '/';
        json(res, { base, module: base + 'preview.js?revision=' + result.revision,
          stylesheet: result.outputs.has('preview.css') ? base + 'preview.css' : null,
          title: result.title, options: { development: true, revision: result.revision,
            revisionUrl: '/_workbench/previews/revision?file=' + encodeURIComponent(result.file) } });
        return;
      }
      res.setHeader('Content-Type', mime['.html']);
      // The outer URL owns state; the compatibility bundle reads it here.
      res.end(withPreviewScripts(result.outputs.get('index.html').toString()));
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
      const result = await compiler.compileDocs(request);
      docs.set(result.slug, request);
      const base = docsBase(result.slug);
      json(res, { base, module: base + 'examples.js?revision=' + result.revision, revision: result.revision,
        stylesheet: result.stylesheet ? base + 'examples.css?revision=' + result.revision : null,
        examples: result.examples, problems: result.problems });
      return;
    }
    const docsAsset = /^\/docs\/([^/]+)\/(.+)$/.exec(url.pathname);
    if (docsAsset && docs.has(docsAsset[1])) {
      const result = await compiler.compileDocs(docs.get(docsAsset[1]));
      const body = result.outputs.get(decodeURIComponent(docsAsset[2]));
      if (body) {
        res.setHeader('Content-Type', mime[path.extname(docsAsset[2])] || 'application/octet-stream');
        res.end(body);
        return;
      }
    }
    if (url.pathname === '/export') {
      const config = require('../config').read(compiler.root);
      json(res, await portable.create(compiler, { name: config && config.name }));
      return;
    }
    const match = /^\/preview\/([^/]+)\/(.+)$/.exec(url.pathname);
    if (match && (slugs.has(match[1]) || variants.has(match[1]))) {
      const result = variants.get(match[1]) || await compiler.compile(slugs.get(match[1]));
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
});
process.on('disconnect', () => server.close(() => process.exit(0)));
process.on('message', message => { if (message === 'close') server.close(() => process.exit(0)); });
process.on('SIGTERM', () => server.close(() => process.exit(0)));
