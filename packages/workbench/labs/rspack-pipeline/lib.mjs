// Shared pieces of the Rspack pipeline lab's stage 2 runner: snapshots per
// variant, servers, browsers and the canvas helpers. See README.md.
import { spawn, spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

export const here = path.dirname(fileURLToPath(import.meta.url));
export const workbench = path.resolve(here, '../..');
export const { Browser } = createRequire(import.meta.url)(path.join(workbench, 'scripts', 'chrome.cjs'));
export const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
export const percentile = (values, p) => { const sorted = [...values].sort((a, b) => a - b); return sorted[Math.max(0, Math.ceil(p / 100 * sorted.length) - 1)]; };

// Variant 1 is the control with only the readiness files of the overlay.
const READINESS_FILES = ['preview/browser.js', 'workbench/workbench.css'];
const SNAPSHOT_SKIP = new Set(['node_modules', 'dist', 'labs', '.DS_Store']);

export function snapshot(runDir, kind) {
  const target = path.join(runDir, 'snapshots', 'variant-' + kind);
  if (fs.existsSync(target)) return target;
  const copy = (from, to) => {
    fs.mkdirSync(to, { recursive: true });
    for (const item of fs.readdirSync(from, { withFileTypes: true })) {
      if (SNAPSHOT_SKIP.has(item.name) || item.name.endsWith('.vsix')) continue;
      const source = path.join(from, item.name);
      if (item.isDirectory()) copy(source, path.join(to, item.name));
      else if (item.isFile()) fs.copyFileSync(source, path.join(to, item.name));
    }
  };
  copy(workbench, target);
  fs.symlinkSync(path.join(workbench, 'node_modules'), path.join(target, 'node_modules'), 'junction');
  const overlay = path.join(here, 'overlay');
  const files = [];
  const walk = (directory, relative = '') => {
    for (const item of fs.readdirSync(directory, { withFileTypes: true })) {
      const rel = path.join(relative, item.name);
      if (item.isDirectory()) walk(path.join(directory, item.name), rel); else files.push(rel);
    }
  };
  walk(overlay);
  for (const rel of files) {
    if (kind === '0' || (kind === '1' && !READINESS_FILES.includes(rel.split(path.sep).join('/')))) continue;
    fs.mkdirSync(path.dirname(path.join(target, rel)), { recursive: true });
    fs.copyFileSync(path.join(overlay, rel), path.join(target, rel));
  }
  return target;
}

// Hashes of the measured sources outside Git: the overlay and this lab's runners.
export function measuredFiles() {
  const out = {};
  const walk = directory => {
    for (const item of fs.readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, item.name);
      if (item.isDirectory()) { if (!['results', 'node_modules'].includes(item.name)) walk(file); continue; }
      out[path.relative(workbench, file)] = crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex').slice(0, 16);
    }
  };
  walk(here);
  walk(path.join(here, '..', 'fixtures'));
  return out;
}

export function treeDigest(root) {
  const hash = crypto.createHash('sha256');
  const walk = directory => {
    for (const item of fs.readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const file = path.join(directory, item.name);
      if (item.isSymbolicLink()) { hash.update('L' + path.relative(root, file)); continue; }
      if (item.isDirectory()) { hash.update('D' + path.relative(root, file)); walk(file); continue; }
      const stat = fs.statSync(file);
      hash.update('F' + path.relative(root, file) + ':' + (/node_modules/.test(file) ? stat.size + ':' + stat.mtimeMs : crypto.createHash('sha1').update(fs.readFileSync(file)).digest('hex')));
    }
  };
  walk(root);
  return hash.digest('hex');
}

export function directorySize(directory) {
  let total = 0;
  const walk = current => {
    let items; try { items = fs.readdirSync(current, { withFileTypes: true }); } catch (_) { return; }
    for (const item of items) {
      const file = path.join(current, item.name);
      if (item.isDirectory()) walk(file); else if (item.isFile()) total += fs.statSync(file).size;
    }
  };
  walk(directory);
  return total;
}

// Resident memory of a process and its descendants, in bytes.
export function treeRss(pid) {
  const out = spawnSync('ps', ['-A', '-o', 'pid=,ppid=,rss='], { encoding: 'utf8' }).stdout || '';
  const rows = out.trim().split('\n').map(line => line.trim().split(/\s+/).map(Number));
  const own = new Set([pid]);
  for (let changed = true; changed;) { changed = false; for (const [child, parent] of rows) if (own.has(parent) && !own.has(child)) { own.add(child); changed = true; } }
  return rows.filter(([child]) => own.has(child)).reduce((sum, row) => sum + row[2] * 1024, 0);
}

export async function startServer({ snapshotDir, root, cache, env = {}, log = () => {} }) {
  fs.mkdirSync(cache, { recursive: true });
  const spawned = Date.now();
  const child = spawn(process.execPath, [path.join(here, '..', 'preview-loading', 'serve.cjs'), snapshotDir, root], {
    stdio: ['pipe', 'pipe', 'pipe'], env: { ...process.env, TMPDIR: cache, ...env },
  });
  const logs = [];
  let buffer = '';
  const ready = new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Server did not start')), 120000);
    child.stdout.on('data', chunk => {
      buffer += chunk;
      let index;
      while ((index = buffer.indexOf('\n')) > -1) {
        const line = buffer.slice(0, index); buffer = buffer.slice(index + 1);
        let message; try { message = JSON.parse(line); } catch (_) { continue; }
        if (message.url) { clearTimeout(timer); resolve(message.url); }
        else if (message.error) { clearTimeout(timer); reject(new Error(message.error)); }
        else if (message.log) logs.push(message);
      }
    });
    child.on('exit', code => { clearTimeout(timer); reject(new Error('Server exited ' + code)); });
  });
  let stderr = '';
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-8000); });
  const url = await ready;
  const urlAt = Date.now();
  let peakRss = 0;
  const sampler = setInterval(() => { try { peakRss = Math.max(peakRss, treeRss(child.pid)); } catch (_) {} }, 500);
  return {
    url, logs, spawned, urlAt, pid: child.pid, stderr: () => stderr, peakRss: () => peakRss,
    close: () => new Promise(resolve => {
      clearInterval(sampler);
      if (child.exitCode !== null) { resolve(); return; }
      const timer = setTimeout(() => { child.kill('SIGKILL'); log('server did not stop; killed'); resolve(); }, 8000);
      child.once('exit', () => { clearTimeout(timer); resolve(); });
      child.kill('SIGTERM');
    }),
  };
}

export async function launchBrowser(profile) {
  const browser = new Browser(profile ? { profile } : {});
  await browser.launch();
  return browser;
}

// Installed in the canvas document. __labSelect changes the address and
// resolves once the active frame is ready and shows `check`, two animation
// frames later, so the result has been painted.
const CANVAS_SCRIPT = `if (window.top === window) {
  window.__labActive = () => document.querySelector('iframe.is-active');
  window.__labWin = () => { try { return __labActive().contentWindow; } catch (_) { return null; } };
}`;

// An error ends the wait only when the caller expects one (a broken page);
// otherwise the previous page's error must not end it.
export const visibleWhen = (check, allowError = false) => `(() => { const frame = __labActive(); let win = null; try { win = frame && frame.contentWindow; } catch (_) {}
  if (!win || !win.document || !win.document.body) return null;
  if (win.__workbenchError) return ${allowError ? '{ error: String(win.__workbenchError).slice(0, 300) }' : 'null'};
  const doc = win.document;
  return win.__workbenchReady === true && (${check}) ? { ok: true } : null; })()`;

// Facts read at the moment a page became visible.
const AT_VISIBLE = `(() => { const win = __labWin(); const doc = win.document;
  const images = Array.from(doc.images).filter(image => { const rect = image.getBoundingClientRect(); return rect.bottom > 0 && rect.top < win.innerHeight && rect.width > 0; });
  return { fonts: doc.fonts ? doc.fonts.status : 'unavailable', imagesVisible: images.length, imagesIncomplete: images.filter(image => !image.complete).length }; })()`;

// With fromNavigation, the time counts from the canvas document's navigation.
export async function select(target, hash, check, { timeout = 60000, action = null, fromNavigation = false, allowError = false } = {}) {
  const expression = `new Promise(resolve => {
    const start = ${fromNavigation ? '0' : 'performance.now()'};
    const probe = () => { try { return ${visibleWhen(check, allowError)}; } catch (_) { return null; } };
    const tick = () => {
      const seen = probe();
      if (seen && seen.error) { resolve({ ms: performance.now() - start, error: seen.error }); return; }
      if (seen) { requestAnimationFrame(() => requestAnimationFrame(() => resolve({ ms: performance.now() - start, at: ${AT_VISIBLE} }))); return; }
      if (performance.now() - start > ${timeout}) { resolve({ ms: null, error: 'not visible within ${timeout} ms' }); return; }
      setTimeout(tick, 4);
    };
    ${action ? action : `location.hash = ${JSON.stringify(hash)};`}
    setTimeout(tick, 0);
  })`;
  return evaluate(target, expression, timeout + 10000);
}

export async function evaluate(target, expression, timeout = 90000) {
  const result = await target.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }, { timeout });
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text || 'The page threw');
  return result.result?.value;
}

export async function until(target, expression, timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs;
  let last;
  while (Date.now() < deadline) {
    try { last = await evaluate(target, expression); if (last) return last; } catch (error) { last = String(error.message || error); }
    await sleep(20);
  }
  throw new Error('Timed out waiting for ' + expression.slice(0, 160) + (last ? ' (last: ' + String(last).slice(0, 160) + ')' : ''));
}

// Opens the canvas and records console entries, failed requests and the
// network traffic, with optional added latency for fonts and images.
export async function openCanvas(browser, url, { query = '', hash = '', latencyMs = 0 } = {}) {
  // Creating a target occasionally times out right after launch; retry once.
  const target = await browser.createTarget().catch(async () => { await sleep(1000); return browser.createTarget(); });
  const consoleEntries = [];
  const failures = [];
  const network = [];
  const byId = new Map();
  target.on('Runtime.exceptionThrown', event => consoleEntries.push({ level: 'exception', text: (event.exceptionDetails?.exception?.description || event.exceptionDetails?.text || '').slice(0, 300) }));
  target.on('Runtime.consoleAPICalled', event => {
    if (['error', 'warning', 'assert'].includes(event.type)) consoleEntries.push({ level: event.type, text: event.args.map(arg => arg.value ?? arg.description ?? '').join(' ').slice(0, 300) });
  });
  target.on('Network.requestWillBeSent', event => {
    const item = { id: event.requestId, url: event.request.url, type: event.type, bytes: 0, cached: false, done: false };
    byId.set(event.requestId, item); network.push(item);
  });
  target.on('Network.responseReceived', event => {
    const item = byId.get(event.requestId);
    if (item) { item.status = event.response.status; item.cached = !!(event.response.fromDiskCache || event.response.fromMemoryCache || event.response.fromPrefetchCache); item.mime = event.response.mimeType; }
    if (event.response.status >= 400) failures.push(event.response.status + ' ' + event.response.url.replace(/^https?:\/\/[^/]+/, '').slice(0, 160));
  });
  target.on('Network.requestServedFromCache', event => { const item = byId.get(event.requestId); if (item) item.cached = true; });
  target.on('Network.loadingFinished', event => { const item = byId.get(event.requestId); if (item) { item.bytes = event.encodedDataLength; item.done = true; } });
  if (latencyMs) {
    target.on('Fetch.requestPaused', event => { setTimeout(() => target.send('Fetch.continueRequest', { requestId: event.requestId }).catch(() => {}), latencyMs); });
    await target.send('Fetch.enable', { patterns: ['*.woff2*', '*.woff?*', '*.png*', '*.jpg*', '*.svg*'].map(urlPattern => ({ urlPattern })) });
  }
  await target.send('Runtime.enable');
  await target.send('Network.enable');
  await target.send('Emulation.setDeviceMetricsOverride', { width: 1600, height: 1000, deviceScaleFactor: 1, mobile: false });
  await target.send('Page.addScriptToEvaluateOnNewDocument', { source: CANVAS_SCRIPT });
  const navigated = Date.now();
  await target.navigate(url + 'index.html' + query + (hash ? '#' + hash : ''));
  return { target, consoleEntries, failures, network, navigated };
}

// Bytes and counts of what loaded since `mark` (an index into network).
export function traffic(canvas, mark) {
  const items = canvas.network.slice(mark).filter(item => !/\/(events|status)(\?|$)/.test(item.url));
  const kind = item => /\.woff2?(\?|$)|font/.test(item.url + ' ' + (item.mime || '')) ? 'font' : /image|\.(png|svg|jpe?g)(\?|$)/.test((item.mime || '') + ' ' + item.url) ? 'image' : item.type === 'Script' || /javascript/.test(item.mime || '') ? 'script' : item.type === 'Stylesheet' || /css/.test(item.mime || '') ? 'style' : 'other';
  const out = { requests: items.length, bytes: 0, fonts: 0, images: 0, scripts: 0, cached: 0 };
  for (const item of items) { out.bytes += item.bytes || 0; if (item.cached) out.cached++; const k = kind(item); if (k === 'font') out.fonts++; else if (k === 'image') out.images++; else if (k === 'script') out.scripts++; }
  return out;
}

export const address = (file, state = 'default', size = 'laptop') => `${file}:${state}@${size}`;
