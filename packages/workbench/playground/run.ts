/** Canvas playground launcher: serves the playground shell, its scenarios and the read-only UI
 *  modules it reuses, then opens an owned Chrome window. Closing the window, SIGINT or SIGTERM
 *  stops the browser, the server and every resource this process created. */
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { Browser } = require('../scripts/chrome.cjs');

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.dirname(here);
const usage = `Usage: node playground/run.ts [--scenarios <dir>]... [--port <n>] [--no-open] [--paint-record]

  --scenarios <dir>  Also load *.scenario.js files from <dir> (repeatable; e.g. a local lab).
  --port <n>         Listen on a fixed loopback port (default: any free port).
  --no-open          Serve only; open the printed address in any Chromium browser.
  --paint-record     Launch Chrome with --enable-gpu-benchmarking --no-sandbox so scenarios can
                     read Chrome's paint records. Use only with trusted local scenarios.
  --verbose          Log each background render's phases and size.
  --renderers <n>    Background headless Chromes rendering in parallel (default: CPU count / 4, 1–4).`;

type Options = { scenarios: string[]; port: number; open: boolean; paintRecord: boolean; verbose: boolean; renderers: number };
function parse(argv: string[]): Options {
  const options: Options = { scenarios: [], port: 0, open: true, paintRecord: false, verbose: false, renderers: Math.max(1, Math.min(4, Math.floor(os.cpus().length / 4))) };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--scenarios') options.scenarios.push(path.resolve(argv[++i] ?? ''));
    else if (arg === '--port') options.port = Number(argv[++i]);
    else if (arg === '--no-open') options.open = false;
    else if (arg === '--paint-record') options.paintRecord = true;
    else if (arg === '--verbose') options.verbose = true;
    else if (arg === '--renderers') options.renderers = Number(argv[++i]);
    else if (arg === '--help' || arg === '-h') { console.log(usage); process.exit(0); }
    else throw Error(`Unknown argument ${arg}\n\n${usage}`);
  }
  for (const dir of options.scenarios) if (!fs.statSync(dir, { throwIfNoEntry: false })?.isDirectory()) throw Error(`Not a directory: ${dir}`);
  if (!Number.isInteger(options.port) || options.port < 0) throw Error('--port needs a number');
  if (!Number.isInteger(options.renderers) || options.renderers < 1 || options.renderers > 16) throw Error('--renderers needs 1–16');
  return options;
}

const options = parse(process.argv.slice(2));
const sources = [{ name: 'built-in', dir: path.join(here, 'scenarios'), base: '/playground/scenarios/' },
  ...options.scenarios.map((dir, i) => ({ name: path.basename(dir), dir, base: `/local/${i}/` }))];
// Read-only roots the browser may load: the shell, the UI modules it reuses and shared contracts.
const roots: [string, string][] = [['/playground/', here], ['/ui/src/', path.join(root, 'ui', 'src')], ['/contracts/src/', path.join(root, 'contracts', 'src')],
  ...sources.slice(1).map(s => [s.base, s.dir] as [string, string])];
const types: Record<string, string> = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.json': 'application/json',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.svg': 'image/svg+xml', '.ttf': 'font/ttf', '.woff2': 'font/woff2', '.wasm': 'application/wasm', '.skp': 'application/octet-stream' };

function resolve(urlPath: string) {
  for (const [prefix, dir] of roots) {
    if (!urlPath.startsWith(prefix)) continue;
    const file = path.resolve(dir, decodeURIComponent(urlPath.slice(prefix.length)));
    if (file === dir || file.startsWith(dir + path.sep)) return file;
  }
  return null;
}
function scenarios() {
  const list = [];
  for (const source of sources) {
    const walk = (dir: string, rel: string) => {
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        if (entry.name.startsWith('.') || entry.name === 'node_modules' || entry.name === 'results') continue;
        if (entry.isDirectory()) { if (rel.split('/').length < 3) walk(path.join(dir, entry.name), rel + entry.name + '/'); }
        else if (entry.name.endsWith('.scenario.js')) list.push({ url: source.base + rel + entry.name, source: source.name, file: rel + entry.name });
      }
    };
    if (fs.existsSync(source.dir)) walk(source.dir, '');
  }
  return list;
}

// Owned temporary directory for paint records; removed on exit.
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-playground-'));
let browser: InstanceType<typeof Browser> | null = null, target: any = null, records = 0;

/** Background renderer: headless Chromes, started with the server, that render HTML at an exact
 *  device scale without touching the visible window. Each pool page runs in its own headless Chrome,
 *  so pages composite and encode screenshots in parallel instead of queueing on one browser's GPU
 *  process; together they drain one shared FIFO queue. Each page stays on one blank server document
 *  and receives every job's HTML as its new content. */
type Region = { x: number, y: number, width: number, height: number };
type RenderRequest = { html: string, width: number, height: number, scale: number, clip?: Region };
type Phases = { queue: number, load: number, ready: number, screenshot: number };
type Job = { request: RenderRequest, resolve: (png: Buffer) => void, reject: (error: Error) => void, state: 'queued' | 'running' | 'cancelled', queued: number, phases?: Phases };
let renderer: { browsers: InstanceType<typeof Browser>[], pages: any[] } | null = null, starting: Promise<void> | null = null, renders = 0;
const queue: Job[] = [], idle: any[] = [], counts = { done: 0, cancelled: 0, discarded: 0, failed: 0 };
async function startRenderer() {
  const browsers = Array.from({ length: options.renderers }, () => {
    const headless = new Browser({}), base = headless.arguments.bind(headless);
    headless.arguments = () => { const args = base(); args.splice(args.length - 1, 0, '--force-color-profile=srgb', '--hide-scrollbars'); return args; };
    return headless;
  });
  renderer = { browsers, pages: [] };
  const pages = await Promise.all(browsers.map(async headless => {
    await headless.launch();
    const page = await headless.createTarget();
    page.pageUrl = null; await page.navigate(`${origin}api/snapshot/blank`);
    page.frameId = (await page.send('Page.getFrameTree')).frameTree.frame.id;
    return page;
  }));
  renderer.pages = pages; idle.push(...pages);
}
function pump() {
  while (idle.length && queue.length) {
    const job = queue.shift()!;
    if (job.state === 'cancelled') continue;
    const page = idle.pop(); job.state = 'running'; const wait = performance.now() - job.queued;
    draw(page, job.request, phases => { job.phases = { queue: wait, ...phases }; }).then(png => {
      if (job.state === 'cancelled') counts.discarded++; else { counts.done++; job.resolve(png); }
    }, error => { counts.failed++; job.reject(error); }).finally(() => { idle.push(page); pump(); });
  }
}
/** Queues a render. `cancel()` drops a queued job; a running one finishes and its result is discarded. */
function render(request: RenderRequest) {
  const job = { request, state: 'queued', queued: performance.now() } as Job;
  const promise = new Promise<Buffer>((resolve, reject) => { job.resolve = resolve; job.reject = reject; });
  promise.catch(() => {}); // a cancelled job's rejection is expected
  queue.push(job);
  starting ??= startRenderer();
  void starting.then(pump, error => { for (const j of queue.splice(0)) j.reject(error); });
  return { promise, job, cancel() { if (job.state === 'queued' || job.state === 'running') { counts.cancelled++; job.state = 'cancelled'; job.reject(Error('cancelled')); } } };
}
/** Renders `html` laid out at width × height CSS pixels. Without `clip` the whole page is captured at
 *  `scale` device pixels per CSS pixel. With `clip` only that CSS region is rendered, magnified by
 *  `scale`, so the cost follows the region's output size rather than the page's. */
async function draw(page: any, request: RenderRequest, report: (phases: Omit<Phases, 'queue'>) => void) {
  {
    const { width, height, scale, clip } = request, id = String(++renders);
    // Each pool page runs in its own browser context; the snapshot is served by this server either way.
    const out = clip ?? { width, height };
    if (![width, height, scale, out.width, out.height].every(v => Number.isFinite(v) && v > 0) || out.width * scale > 16384 || out.height * scale > 16384) throw Error('Render size out of range');
    if (clip && (clip.x < 0 || clip.y < 0 || clip.x + clip.width > width + 0.5 || clip.y + clip.height > height + 0.5)) throw Error('Render clip outside the page');
    try {
      const t0 = performance.now();
      const metrics = { width: Math.round(width), height: Math.round(height), deviceScaleFactor: clip ? 1 : scale, mobile: false }, signature = JSON.stringify(metrics);
      if (page.metrics !== signature) { await page.send('Emulation.setDeviceMetricsOverride', metrics); page.metrics = signature; }
      // The page stays on one server document; replacing its content skips navigation and a new
      // document load. Relative URLs resolve against /api/.
      if (!page.frameId) {
        page.pageUrl = null; await page.navigate(`${origin}api/snapshot/blank`);
        page.frameId = (await page.send('Page.getFrameTree')).frameTree.frame.id;
      }
      await page.send('Page.setDocumentContent', { frameId: page.frameId, html: request.html });
      const t1 = performance.now();
      // Wait for every srcdoc/src iframe's own document, then fonts and images in each document.
      await page.evaluate(`(async () => {
        const settle = async doc => {
          const frames = [...doc.querySelectorAll('iframe')];
          await Promise.all(frames.map(f => new Promise(resolve => {
            const done = () => { const d = f.contentDocument; return !d || (d.readyState === 'complete' && (d.URL !== 'about:blank' || (!f.srcdoc && !f.src))); };
            if (done()) return resolve();
            f.addEventListener('load', resolve, { once: true }); setTimeout(resolve, 10000);
          })));
          await doc.fonts.ready; await Promise.all([...doc.images].map(i => i.decode().catch(() => {})));
          await Promise.all(frames.map(f => f.contentDocument && settle(f.contentDocument)));
        };
        await settle(document); return true;
      })()`);
      const t2 = performance.now();
      // optimizeForSpeed keeps PNG lossless but compresses lightly; encoding dominates large renders.
      const shot = await page.send('Page.captureScreenshot', { format: 'png', optimizeForSpeed: true, fromSurface: true, captureBeyondViewport: false, ...(clip ? { clip: { ...clip, scale } } : {}) }, { timeout: 120000 });
      const png = Buffer.from(shot.data, 'base64');
      report({ load: t1 - t0, ready: t2 - t1, screenshot: performance.now() - t2 });
      if (options.verbose) console.log(`render ${id} ${clip ? 'clip ' + [clip.x, clip.y, clip.width, clip.height].map(Math.round).join(',') : 'page'} @${scale.toFixed(2)}×: set content ${(t1 - t0).toFixed(0)} ms, ready ${(t2 - t1).toFixed(0)} ms, screenshot ${(performance.now() - t2).toFixed(0)} ms, ${png.length} bytes`);
      return png;
    } catch (error) { page.frameId = null; page.metrics = null; throw error; }
  }
}
let origin = '';
const events = new Set<http.ServerResponse>();
const watchers = sources.filter(s => fs.existsSync(s.dir)).map(s => fs.watch(s.dir, { recursive: true }, (_kind, file) => {
  if (!file || /(^|\/)(results|node_modules)\//.test(String(file))) return;
  const message = `data: ${JSON.stringify({ url: s.base + String(file).split(path.sep).join('/') })}\n\n`;
  for (const client of events) client.write(message);
}));

async function json(req: http.IncomingMessage) { let body = ''; for await (const chunk of req) body += chunk; return body ? JSON.parse(body) : {}; }
function send(res: http.ServerResponse, status: number, body: unknown, type = 'application/json') {
  res.writeHead(status, { 'Content-Type': type, 'Cache-Control': 'no-store' });
  res.end(type === 'application/json' ? JSON.stringify(body) : body as Buffer);
}
async function forceFrame() { await target.send('Page.bringToFront'); await target.screenshot(); }
async function api(route: string, req: http.IncomingMessage, res: http.ServerResponse) {
  if (route === '/api/scenarios') return send(res, 200, scenarios());
  if (route === '/api/host') return send(res, 200, { owned: !!target, paintRecord: options.paintRecord && !!target, platform: process.platform, renderers: options.renderers });
  if (route === '/api/events') {
    res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-store', Connection: 'keep-alive' });
    res.write(': connected\n\n'); events.add(res); req.on('close', () => events.delete(res)); return;
  }
  if (route === '/api/snapshot/blank') return send(res, 200, Buffer.from('<!doctype html>'), 'text/html');
  if (route === '/api/render' && req.method === 'POST') {
    const began = performance.now(), job = render(await json(req));
    // A client that aborts (artboard no longer visible) cancels its job.
    res.on('close', () => { if (!res.writableFinished) job.cancel(); });
    const png = await job.promise.catch(error => { if (error.message === 'cancelled') return null; throw error; });
    if (!png) { if (!res.destroyed) send(res, 409, { error: 'cancelled' }); return; }
    res.setHeader('x-render-ms', String(performance.now() - began));
    if (job.job.phases) res.setHeader('x-render-phases', JSON.stringify(job.job.phases));
    return send(res, 200, png, 'image/png');
  }
  if (route === '/api/render-stats') return send(res, 200, { renderers: options.renderers, queued: queue.filter(j => j.state === 'queued').length, running: (renderer?.pages.length ?? 0) - idle.length, ...counts });
  if (!target) return send(res, 409, { error: 'This action needs the Chrome window opened by playground/run.ts.' });
  if (route === '/api/frame' && req.method === 'POST') { await forceFrame(); return send(res, 200, { ok: true }); }
  if (route === '/api/capture' && req.method === 'POST') {
    // Native screenshot of a CSS-pixel rectangle at `scale` × CSS pixels.
    const { x, y, width, height, scale = 0 } = await json(req), dpr = await target.evaluate('devicePixelRatio');
    const shot = await target.send('Page.captureScreenshot', { format: 'png', fromSurface: true, captureBeyondViewport: false,
      clip: { x, y, width, height, scale: scale ? scale / dpr : 1 } }, { timeout: 180000 });
    return send(res, 200, Buffer.from(shot.data, 'base64'), 'image/png');
  }
  if (route === '/api/paint-record' && req.method === 'POST') {
    if (!options.paintRecord) return send(res, 409, { error: 'Restart with --paint-record to read Chrome paint records.' });
    const id = String(++records), dir = path.join(scratch, id);
    await forceFrame();
    await target.evaluate(`chrome.gpuBenchmarking.printToSkPicture(${JSON.stringify(dir)})`);
    // Enabling LayerTree reports the current layer tree (ScreenSpaceTransform per layer) with the next frame.
    await target.send('LayerTree.disable').catch(() => {});
    const tree = target.waitFor('LayerTree.layerTreeDidChange'); await target.send('LayerTree.enable'); await target.screenshot();
    const layers = (await tree).layers ?? [];
    const files = fs.readdirSync(dir).filter(n => n.endsWith('.skp')).sort((a, b) => parseInt(a.slice(6)) - parseInt(b.slice(6)))
      .map(name => ({ name, size: fs.statSync(path.join(dir, name)).size, url: `/api/paint-record/${id}/${name}` }));
    return send(res, 200, { id, files, layers });
  }
  const record = /^\/api\/paint-record\/(\d+)\/(layer_\d+\.skp)$/.exec(route);
  if (record) return send(res, 200, fs.readFileSync(path.join(scratch, record[1], record[2])), 'application/octet-stream');
  return send(res, 404, { error: 'Unknown route' });
}

const server = http.createServer(async (req, res) => {
  try {
    const route = new URL(req.url ?? '/', 'http://localhost').pathname;
    if (route.startsWith('/api/')) return await api(route, req, res);
    const file = route === '/' ? path.join(here, 'index.html') : resolve(route);
    if (!file || !fs.statSync(file, { throwIfNoEntry: false })?.isFile()) return send(res, 404, 'Not found', 'text/plain');
    send(res, 200, fs.readFileSync(file), types[path.extname(file)] ?? 'application/octet-stream');
  } catch (error) { send(res, 500, { error: String((error as Error)?.stack ?? error) }); }
});

let closing: Promise<void> | null = null;
function close(code = 0) {
  closing ??= (async () => {
    for (const w of watchers) w.close();
    for (const client of events) client.end();
    await starting?.catch(() => {});
    try { await Promise.allSettled([browser?.close(), ...(renderer?.browsers ?? []).map(b => b.close())]); } finally {
      await new Promise(r => server.close(r));
      fs.rmSync(scratch, { recursive: true, force: true });
    }
  })().finally(() => process.exit(code));
  return closing;
}
for (const signal of ['SIGINT', 'SIGTERM'] as const) process.once(signal, () => void close(0));

await new Promise<void>(r => server.listen(options.port, '127.0.0.1', r));
const address = server.address();
const url = `http://127.0.0.1:${typeof address === 'object' && address ? address.port : options.port}/`;
origin = url;
starting = startRenderer(); // warm, so the first render does not wait for Chrome to launch
starting.catch(error => console.error(`Background renderer failed to start: ${error?.message ?? error}`));
console.log(`Canvas playground: ${url}`);
console.log(`Scenarios: ${sources.map(s => `${s.name} (${s.dir})`).join(', ')}`);
if (options.open) {
  browser = new Browser({ headless: false });
  const base = browser.arguments.bind(browser);
  browser.arguments = () => {
    const args = base();
    args.splice(args.length - 1, 0, '--window-size=1600,1000', '--force-color-profile=srgb',
      ...(options.paintRecord ? ['--enable-gpu-benchmarking', '--no-sandbox'] : []));
    return args;
  };
  await browser.launch();
  // The window is the playground: closing it ends the session.
  browser.child.once('exit', () => void close(0));
  target = await browser.createTarget();
  await target.navigate(url);
  await target.send('Page.bringToFront');
  console.log(options.paintRecord ? 'Chrome opened with paint-record access (--no-sandbox). Close the window or press Ctrl-C to stop.' : 'Chrome opened. Close the window or press Ctrl-C to stop.');
}
