// Instant first page lab, stage 2 runner. See README.md, "Stage 2 plan".
//
//   node labs/instant-first-page/stage2.mjs --set <label> [--results <new directory>]
//     [--variants 0,R,RN,D2] [--fixtures small,large-system,giant,colossal,formats]
//     [--samples 10] [--warmups 3] [--session 5] [--quiet 8] [--smoke] [--keep]
//
// Every variant runs the shipping server and canvas from a snapshot of the
// package. Variant 0 is the snapshot as it is; R, RN and D2 add the overlay
// (make-overlay.mjs) that starts the Rust preview host instead of the Node
// worker. Fixtures, caches and browser profiles live in a new run directory
// under the system temporary folder.
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { cargo } from './cargo.mjs';
import { HOST, Host, entryMaker, fixture, job } from './lib.mjs';
import { writeOverlay } from './make-overlay.mjs';
import { acquireLabLock, hostSnapshot, waitForQuiet } from '../fixtures/host.mjs';
import { spawn } from 'node:child_process';
import { address, evaluate, launchBrowser as launchOnce, select, sleep, treeDigest, treeRss, until, workbench } from '../rspack-pipeline/lib.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf('--' + name); return index === -1 ? fallback : args[index + 1]; };
const flag = name => args.includes('--' + name);
const smoke = flag('smoke');
const settings = {
  set: option('set', smoke ? 'smoke' : null),
  variants: option('variants', '0,R,RN,D2').split(','),
  fixtures: option('fixtures', 'small,large-system,giant,colossal,formats').split(','),
  samples: Number(option('samples', smoke ? 1 : 10)),
  warmups: Number(option('warmups', smoke ? 0 : 3)),
  session: Number(option('session', smoke ? 1 : 5)),
  quiet: Number(option('quiet', 8)),
};
if (!settings.set) { console.error('Name the run set with --set <label>.'); process.exit(2); }
const resultsDir = option('results', null);
if (resultsDir && fs.existsSync(resultsDir)) { console.error('Refusing to overwrite ' + resultsDir); process.exit(2); }

const VARIANTS = {
  0: { snapshot: 'control', env: {} },
  R: { snapshot: 'rust', env: { WORKBENCH_LAB_PREVIEW: 'rust' } },
  RN: { snapshot: 'rust', env: { WORKBENCH_LAB_PREVIEW: 'neighbours', WORKBENCH_LAB_NEIGHBOURS: '8' } },
  D2: { snapshot: 'rust', env: { WORKBENCH_LAB_PREVIEW: 'cap', WORKBENCH_LAB_CAP: '20' } },
};
const rust = variant => variant !== '0';

const runParent = path.join(os.tmpdir(), 'canonic-workbench-labs', 'instant-first-page', 'runs');
fs.mkdirSync(runParent, { recursive: true });
const runDir = path.join(fs.realpathSync(runParent), settings.set + '-stage2-' + new Date().toISOString().replace(/[:.]/g, '-'));
fs.mkdirSync(path.join(runDir, 'fixtures'), { recursive: true });
const log = message => console.log(`[${new Date().toISOString().slice(11, 19)}] ${message}`);

const results = [];
const checks = [];
const record = entry => results.push({ ...entry, load: hostSnapshot().loadAverage[0] });
// A check's verdict is computed by the caller and spread last.
const check = entry => { checks.push(entry); if (!entry.ok) log(`  CHECK FAILED ${entry.check} ${entry.variant} ${entry.fixture}: ${JSON.stringify(entry).slice(0, 300)}`); };
const quiet = async entry => { const waited = await waitForQuiet(settings.quiet, 600000, log); entry.quietWaitMs = waited.waitedMs; if (!waited.quiet) entry.loud = true; return entry; };

function seeded(seedText) {
  let seed = crypto.createHash('sha256').update(seedText).digest().readUInt32LE(0);
  return () => { seed = (seed + 0x6D2B79F5) >>> 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
}
const shuffled = (items, seedText) => { const out = [...items]; const rand = seeded(seedText); for (let i = out.length - 1; i > 0; i--) { const j = Math.floor(rand() * (i + 1)); [out[i], out[j]] = [out[j], out[i]]; } return out; };
const working = fx => fx.manifest.previews.filter(page => !page.broken && (fx.size !== 'formats' || ['react', 'acme-canvas'].includes(page.adapter)));

// ---------------------------------------------------------------- snapshots

const SKIP = new Set(['node_modules', 'dist', 'labs', 'target', '.DS_Store']);
function snapshot(kind) {
  const target = path.join(runDir, 'snapshots', kind);
  if (fs.existsSync(target)) return target;
  const copy = (from, to) => {
    fs.mkdirSync(to, { recursive: true });
    for (const item of fs.readdirSync(from, { withFileTypes: true })) {
      if (SKIP.has(item.name) || item.name.endsWith('.vsix')) continue;
      const source = path.join(from, item.name);
      if (item.isDirectory()) copy(source, path.join(to, item.name));
      else if (item.isFile()) { fs.copyFileSync(source, path.join(to, item.name)); fs.chmodSync(path.join(to, item.name), fs.statSync(source).mode); }
    }
  };
  copy(workbench, target);
  fs.symlinkSync(path.join(workbench, 'node_modules'), path.join(target, 'node_modules'), 'junction');
  if (kind === 'rust') writeOverlay(target);
  return target;
}

function serverFor(variant, fx, cache) {
  const spec = VARIANTS[variant];
  return startServer({ snapshotDir: snapshot(spec.snapshot), root: fx.root, cache,
    env: { WORKBENCH_LAB_HOST: HOST, WORKBENCH_LAB_EVALUATOR: path.join(here, 'evaluate.mjs'), WORKBENCH_LAB_STORAGE: path.join(runDir, 'storage'), ...spec.env } });
}

// One server per cell, its system temporary folder (every cache the server and
// its children keep there) pointed at `cache`; the runtime stays in storage.
async function startServer({ snapshotDir, root, cache, env }) {
  fs.mkdirSync(cache, { recursive: true });
  const spawned = Date.now();
  const child = spawn(process.execPath, [path.join(here, 'serve.cjs'), snapshotDir, root], { stdio: ['pipe', 'pipe', 'pipe'], env: { ...process.env, TMPDIR: cache, ...env } });
  const logs = [];
  let buffer = '';
  let stderr = '';
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-8000); });
  const url = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Server did not start: ' + stderr.slice(-400))), 120000);
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
    child.on('exit', code => { clearTimeout(timer); reject(new Error('Server exited ' + code + ': ' + stderr.slice(-400))); });
  });
  const urlAt = Date.now();
  let peakRss = 0;
  const sampler = setInterval(() => { try { peakRss = Math.max(peakRss, treeRss(child.pid)); } catch (_) {} }, 500);
  return {
    url, logs, spawned, urlAt, stderr: () => stderr, peakRss: () => peakRss,
    close: () => new Promise(resolve => {
      clearInterval(sampler);
      if (child.exitCode !== null) { resolve(); return; }
      const timer = setTimeout(() => { child.kill('SIGKILL'); log('server did not stop; killed'); resolve(); }, 8000);
      child.once('exit', () => { clearTimeout(timer); resolve(); });
      child.kill('SIGTERM');
    }),
  };
}

// A first preview request starts the preview service in every variant. The
// control's answer waits for its pre-warming, which builds every page; R
// builds that page.
// The server answers a preview request within 120 s; a control that is still
// building every page is asked again until it finishes, for at most 15 minutes.
async function warmed(server, fx) {
  const page = working(fx)[0];
  const deadline = Date.now() + 900000;
  let last = null;
  while (Date.now() < deadline) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 300000);
    try {
      const answer = await fetch(new URL('/_workbench/previews/descriptor?file=' + encodeURIComponent(page.file), server.url), { signal: controller.signal });
      if (answer.status === 200) return;
      last = answer.status + ' ' + (await answer.text()).slice(0, 200);
      if (!/timed out/.test(last)) break;
    } catch (error) { last = String(error.message || error); }
    finally { clearTimeout(timer); }
  }
  throw new Error('Warming failed: ' + last);
}

// Launching the browser occasionally times out under load; retry once and
// record the retry.
async function launchBrowser(profile) {
  try { return await launchOnce(profile); }
  catch (error) { record({ group: 'runner', sequence: 'browser-retry', cell: 'launch', error: String(error.message || error).slice(0, 200) }); await sleep(2000); return launchOnce(profile); }
}

// ---------------------------------------------------------------- canvas

function pageCheck(page) {
  if (page.selector) return `!!doc.querySelector(${JSON.stringify(page.selector)})`;
  if (page.owner) return `doc.body.classList.contains(${JSON.stringify('acme-owner-' + page.owner)})`;
  return 'true';
}

// Records in the canvas document when the page list holds every expected
// page, on the document's own clock (performance.now() from navigation).
const listProbe = expected => `(() => {
  if (window.top !== window) return;
  const count = () => { const list = document.getElementById('pageList'); if (!list) return 0; const seen = new Set(); for (const node of list.querySelectorAll('[data-src]')) seen.add(node.dataset.src); return seen.size; };
  const done = () => requestAnimationFrame(() => requestAnimationFrame(() => { window.__labPageList = performance.now(); }));
  const observer = new MutationObserver(() => { if (window.__labPageList === undefined && count() >= ${expected}) { window.__labPageList = null; observer.disconnect(); done(); } });
  document.addEventListener('DOMContentLoaded', () => observer.observe(document.body, { childList: true, subtree: true }));
})();`;

async function openCanvas(browser, url, { hash = '', expected = 0 } = {}) {
  let target;
  for (let attempt = 1; !target; attempt++) {
    try { target = await browser.createTarget(); } catch (error) { if (attempt >= 3) throw error; await sleep(1000 * attempt); }
  }
  const consoleEntries = [];
  const failures = [];
  const timings = [];
  target.on('Runtime.exceptionThrown', event => consoleEntries.push({ level: 'exception', text: (event.exceptionDetails?.exception?.description || event.exceptionDetails?.text || '').slice(0, 300) }));
  target.on('Runtime.consoleAPICalled', event => { if (['error', 'warning', 'assert'].includes(event.type)) consoleEntries.push({ level: event.type, text: event.args.map(arg => arg.value ?? arg.description ?? '').join(' ').slice(0, 300) }); });
  target.on('Network.responseReceived', event => {
    if (event.response.status >= 400) failures.push(event.response.status + ' ' + event.response.url.replace(/^https?:\/\/[^/]+/, '').slice(0, 160));
    if (/\/_workbench\/(config|previews\/descriptor)/.test(event.response.url) && event.response.timing) timings.push({ url: event.response.url.replace(/^https?:\/\/[^/]+/, '').slice(0, 160), waitMs: event.response.timing.receiveHeadersEnd - event.response.timing.sendEnd });
  });
  await target.send('Runtime.enable');
  await target.send('Network.enable');
  await target.send('Emulation.setDeviceMetricsOverride', { width: 1600, height: 1000, deviceScaleFactor: 1, mobile: false });
  await target.send('Page.addScriptToEvaluateOnNewDocument', { source: `if (window.top === window) { window.__labActive = () => document.querySelector('iframe.is-active'); window.__labWin = () => { try { return __labActive().contentWindow; } catch (_) { return null; } }; }` });
  if (expected) await target.send('Page.addScriptToEvaluateOnNewDocument', { source: listProbe(expected) });
  const navigated = Date.now();
  await target.navigate(url + 'index.html' + (hash ? '#' + hash : ''));
  return { target, consoleEntries, failures, timings, navigated };
}

async function pageListMs(canvas, timeout = 120000) {
  try { return await until(canvas.target, 'typeof window.__labPageList === "number" ? window.__labPageList : null', timeout); }
  catch (_) { return null; }
}

async function hostState(server) {
  try { return await (await fetch(new URL('/_workbench/previews/lab/state', server.url))).json(); } catch (_) { return null; }
}

// ---------------------------------------------------------------- starts (S1–S3, S2i, S4)

async function first(browser, server, fx, page) {
  const canvas = await openCanvas(browser, server.url, { hash: address(page.file), expected: fx.manifest.previews.length });
  // A start has no previous page, so the page's own error ends the wait; it is
  // recorded as an error with no time, never as a latency.
  const shown = await select(canvas.target, null, pageCheck(page), { timeout: 600000, action: '/* navigating */', fromNavigation: true, allowError: true });
  if (shown.error) shown.ms = null;
  const list = await pageListMs(canvas);
  return { canvas, shown, list };
}

async function startGroup(fx, variant) {
  const pages = working(fx);
  const order = shuffled(pages, 'stage2' + fx.size);
  const total = settings.warmups + settings.samples;
  const base = { group: 'start', fixture: fx.size, variant };
  const warmCache = path.join(runDir, 'start', variant + '-' + fx.size + '-warm');
  // Warm the cache with every page the samples open, as earlier sessions would.
  {
    const server = await serverFor(variant, fx, warmCache);
    const browser = await launchBrowser();
    try {
      await warmed(server, fx);
      const opened = await first(browser, server, fx, order[0]);
      for (let index = 1; index <= total && rust(variant); index++) await select(opened.canvas.target, address(order[index % order.length].file), pageCheck(order[index % order.length]), { timeout: 600000 });
    } finally { await browser.close().catch(() => {}); await server.close(); }
  }
  for (let index = 0; index < total; index++) {
    const warmup = index < settings.warmups;
    const page = order[index % order.length];
    const second = order[(index + total + 1) % order.length];
    const coldCache = path.join(runDir, 'start', variant + '-' + fx.size + '-cold-' + index);
    const profile = path.join(runDir, 'profiles', variant + '-' + fx.size + '-' + index);
    for (const sequence of ['S1', 'S2', 'S2i', 'S3']) {
      const entry = await quiet({ ...base, sequence, sample: index, warmup, page: page.id });
      let retitled = [];
      if (sequence === 'S2i') retitled = retitle(fx, index);
      const server = await serverFor(variant, fx, sequence === 'S1' ? coldCache : warmCache);
      const browser = await launchBrowser(profile);
      try {
        if (sequence === 'S3') await sleep(2000);
        const { canvas, shown, list } = await first(browser, server, fx, page);
        record({ ...entry, cell: 'first page', ms: shown.ms, error: shown.error || null, fromUrl: shown.ms == null ? null : canvas.navigated - server.urlAt + shown.ms, fromServerStart: shown.ms == null ? null : canvas.navigated - server.spawned + shown.ms });
        record({ ...entry, cell: 'page list', ms: list, timings: canvas.timings });
        if (sequence === 'S2i') {
          const listed = await evaluate(canvas.target, `(() => { let n = 0; for (const file of ${JSON.stringify(retitled.map(item => item.file))}) { const item = window.wbItem(file); if (item && / Retitled$/.test(item.label)) n++; } return n; })()`).catch(() => -1);
          check({ check: 'S2i-retitled-listed', variant, fixture: fx.size, sample: index, listed, expected: retitled.length, ok: listed === retitled.length });
        }
        if (sequence === 'S2') {
          const next = await select(canvas.target, address(second.file), pageCheck(second), { timeout: 600000 });
          record({ ...base, sequence: 'S4', sample: index, warmup, cell: 'page not yet opened', ms: next.ms, error: next.error || null });
        }
        if (rust(variant)) record({ ...base, sequence: sequence + '-host', sample: index, warmup, cell: 'host timeline', host: await hostState(server) });
      } catch (error) { record({ ...entry, cell: 'first page', ms: null, error: String(error.message || error).slice(0, 300) }); }
      finally {
        await browser.close().catch(() => {});
        await server.close();
        restore(retitled);
      }
    }
    // The page's own build: a one-entry Rust host with an empty cache, beside S1.
    if (rust(variant)) {
      const own = await quiet({ ...base, sequence: 'own', sample: index, warmup, cell: 'own build' });
      const host = new Host('rust');
      try {
        const maker = entryMaker(fx.root);
        const built = await host.request(job(fx.root, path.join(runDir, 'own', variant + '-' + fx.size + '-' + index), maker, await maker.entries([page.file])));
        record({ ...own, ms: built.ms ?? null, error: built.ok ? null : built.error });
      } finally { await host.close(); }
    }
    fs.rmSync(profile, { recursive: true, force: true });
    fs.rmSync(coldCache, { recursive: true, force: true });
  }
  fs.rmSync(warmCache, { recursive: true, force: true });
}

// 1% of the literal definitions retitled while the server is stopped.
function retitle(fx, index) {
  const literal = working(fx).filter(page => !page.computed);
  const count = Math.max(1, Math.round(fx.manifest.previews.length / 100));
  return shuffled(literal, 'S2i' + fx.size + index).slice(0, count).map(page => {
    const file = path.join(fx.root, page.file);
    const text = fs.readFileSync(file, 'utf8');
    fs.writeFileSync(file, text.replace(/title: '([^']*)'/, "title: '$1 Retitled'"));
    return { file: page.file, path: file, text };
  });
}
function restore(items) { for (const item of items) fs.writeFileSync(item.path, item.text); }

// ---------------------------------------------------------------- sessions (S5–S9)

async function timedEdit(target, file, next, checkExpression, timeout = 30000) {
  const waiting = evaluate(target, `new Promise(resolve => { const start = Date.now();
    const tick = () => { let ok = false; try { const win = __labWin(); const doc = win.document; ok = win.__workbenchReady !== false && (${checkExpression}); } catch (_) {}
      if (ok) { requestAnimationFrame(() => requestAnimationFrame(() => resolve(Date.now()))); return; }
      if (Date.now() - start > ${timeout + 2000}) { resolve(null); return; } setTimeout(tick, 4); }; tick(); })`, timeout + 10000);
  await sleep(30);
  const written = Date.now();
  fs.writeFileSync(file, next);
  const visibleAt = await waiting;
  return visibleAt == null ? null : visibleAt - written;
}

const counter = `(() => { const button = __labWin().document.querySelector('[data-acme="counter"]'); return button ? button.textContent : null; })()`;

async function readSentinels(target, page) {
  try {
    return await until(target, `(() => { const html = __labWin().document.documentElement; return html.dataset.acmeProbed === ${JSON.stringify(page.id)} ? { leaks: html.dataset.acmeLeaks || '' } : null; })()`, 3000);
  } catch (_) { return { error: 'no probe result' }; }
}

async function sessionGroup(fx, variant) {
  const base = { group: 'session', fixture: fx.size, variant };
  const cache = path.join(runDir, 'session', variant + '-' + fx.size);
  const pages = working(fx);
  const order = shuffled(pages, 'session' + fx.size);
  { const warm = await serverFor(variant, fx, cache); try { await warmed(warm, fx); } finally { await warm.close(); } }
  await quiet({ ...base });
  const server = await serverFor(variant, fx, cache);
  const browser = await launchBrowser();
  const opened = [];
  try {
    await warmed(server, fx);
    const start = order[0];
    const { canvas, shown } = await first(browser, server, fx, start);
    if (shown.error) throw new Error('first page: ' + shown.error);
    opened.push(start);
    const visit = async (page, sequence, extra = {}) => {
      const entry = await quiet({ ...base, sequence, page: page.id, ...extra });
      const r = await select(canvas.target, address(page.file), pageCheck(page), { timeout: 120000 });
      record({ ...entry, cell: 'selection to visible', ms: r.ms, error: r.error || null });
      if (!r.error && fx.size !== 'small') {
        const sentinels = await readSentinels(canvas.target, page);
        check({ check: 'isolation', variant, fixture: fx.size, page: page.id, sentinels, ok: !sentinels.error && !sentinels.leaks });
      }
      opened.push(page);
      return r;
    };
    // S5: pages not yet opened, 1 s apart.
    for (let index = 0; index < settings.session; index++) { await sleep(1000); await visit(order[1 + index], 'S5', { sample: index }); }
    // S5r: back to pages opened earlier.
    for (let index = 0; index < settings.session; index++) { await sleep(1000); await visit(opened[index], 'S5r', { sample: index }); }
    // S6: edits, with React state kept.
    const later = order[1];
    const edits = [
      { name: 'component, first page', page: start, file: path.join(fx.root, start.edit.component), apply: (text, m) => text.replace(/Revision [^<]*/, 'Revision ' + m) },
      { name: 'component, page opened later', page: later, file: path.join(fx.root, later.edit.component), apply: (text, m) => text.replace(/Revision [^<]*/, 'Revision ' + m) },
      ...(fs.existsSync(path.join(fx.root, 'src/app/Shell.tsx')) ? [
        { name: 'shared module, first page', page: start, file: path.join(fx.root, 'src/app/Shell.tsx'), apply: (text, m) => text.replace('<nav aria-label="Features">', `<nav aria-label="Features"><span>${m}</span>`) },
      ] : []),
    ];
    const editOnce = async (edit, sequence, index) => {
      const r = await select(canvas.target, address(edit.page.file), pageCheck(edit.page), { timeout: 120000 });
      if (r.error) { check({ check: 'hot-update', variant, fixture: fx.size, edit: edit.name, ok: false, error: r.error }); return; }
      await evaluate(canvas.target, `(() => { const button = __labWin().document.querySelector('[data-acme="counter"]'); if (button) { button.click(); button.click(); } __labWin().__labToken = 'edit'; })()`);
      await sleep(200);
      const before = await evaluate(canvas.target, counter).catch(() => null);
      const original = fs.readFileSync(edit.file, 'utf8');
      const mark = 'lab' + crypto.randomBytes(3).toString('hex');
      const entry = await quiet({ ...base, sequence, cell: edit.name, sample: index });
      let ms = null;
      try { ms = await timedEdit(canvas.target, edit.file, edit.apply(original, mark), `doc.body.innerText.includes(${JSON.stringify(mark)})`); }
      finally {
        const sameDocument = await evaluate(canvas.target, `__labWin().__labToken === 'edit'`).catch(() => false);
        const after = await evaluate(canvas.target, counter).catch(() => null);
        record({ ...entry, ms, sameDocument });
        check({ check: 'hot-update', variant, fixture: fx.size, edit: edit.name, sequence, ms, sameDocument, before, after,
          ok: ms != null && sameDocument && (edit.name.startsWith('shared') || before === after) });
        fs.writeFileSync(edit.file, original);
        await until(canvas.target, `(() => { const w = __labWin(); return w && w.__workbenchReady === true && !w.document.body.innerText.includes(${JSON.stringify(mark)}); })()`, 60000).catch(() => {});
      }
    };
    for (const edit of edits) for (let index = 0; index < settings.session; index++) await editOnce(edit, 'S6', index);
    // A page that fails to compile, opened after start, shows its error.
    const broken = fx.manifest.previews.find(page => page.broken === 'compile');
    if (broken) {
      const r = await select(canvas.target, address(broken.file), 'false', { timeout: 60000, allowError: true });
      check({ check: 'error-shown', variant, fixture: fx.size, ok: !!r.error, error: r.error });
      const back = await select(canvas.target, address(start.file), pageCheck(start), { timeout: 60000 });
      check({ check: 'error-contained', variant, fixture: fx.size, ok: !back.error, error: back.error || null });
    }
    // S7: config edits; the page comes back.
    const config = path.join(fx.root, 'workbench.config.ts');
    const configText = fs.readFileSync(config, 'utf8');
    await select(canvas.target, address(start.file), pageCheck(start), { timeout: 120000 });
    for (let index = 0; index < settings.session; index++) {
      const entry = await quiet({ ...base, sequence: 'S7', cell: 'config edit', sample: index });
      await evaluate(canvas.target, "__labWin().__labToken = 'config'");
      const before = await hostState(server);
      fs.writeFileSync(config, configText + '\n// lab ' + Date.now() + '\n');
      const started = Date.now();
      let returned = false;
      try { await until(canvas.target, `(() => { const w = __labWin(); return !!w && w.__labToken !== 'config' && w.__workbenchReady === true && (() => { const doc = w.document; return ${pageCheck(start)}; })(); })()`, 600000); returned = true; } catch (_) {}
      const ms = returned ? Date.now() - started : null;
      record({ ...entry, ms, timeline: ms > 10000 && rust(variant) ? (await hostState(server))?.timeline.slice((before?.timeline || []).length) : undefined });
      check({ check: 'config-edit', variant, fixture: fx.size, sample: index, ok: returned });
      await evaluate(canvas.target, "__labWin().__labToken = 'config'").catch(() => {});
      fs.writeFileSync(config, configText);
      await until(canvas.target, `(() => { const w = __labWin(); return !!w && w.__labToken !== 'config' && w.__workbenchReady === true; })()`, 600000).catch(() => {});
    }
    // S8: a definition added, renamed and retitled while running.
    await definitionEdits(fx, variant, server, base);
    // S9: open up to 100 pages in turn, then a page not yet opened and an edit.
    const many = order.slice(1, Math.min(order.length - 2, 101));
    for (const page of many) {
      const r = await select(canvas.target, address(page.file), pageCheck(page), { timeout: 120000 });
      if (r.error) { check({ check: 'S9-open', variant, fixture: fx.size, page: page.id, ok: false, error: r.error }); break; }
    }
    record({ ...base, sequence: 'S9', cell: 'opened', value: many.length });
    const fresh = order[Math.min(order.length - 1, 102)];
    if (fresh && !many.includes(fresh)) await visit(fresh, 'S9-S4');
    for (let index = 0; index < settings.session; index++) await editOnce(edits[0], 'S9-S6', index);
    const state = rust(variant) ? await hostState(server) : null;
    if (state) check({ check: 'no-moved-modules', variant, fixture: fx.size, ok: state.moved.length === 0, moved: state.moved.slice(0, 5) });
    check({ check: 'console', variant, fixture: fx.size, ok: true, entries: canvas.consoleEntries.slice(0, 40), failures: canvas.failures.slice(0, 40) });
    record({ ...base, sequence: 'session-host', cell: 'host timeline', host: state });
  } catch (error) {
    check({ check: 'session', variant, fixture: fx.size, ok: false, error: String(error.stack || error).slice(0, 500) });
  } finally {
    await browser.close().catch(() => {});
    record({ ...base, sequence: 'memory', cell: 'peak RSS', value: server.peakRss() });
    await server.close();
    fs.rmSync(cache, { recursive: true, force: true });
  }
}

// S8: the page list after each change, read from the config answer.
async function definitionEdits(fx, variant, server, base) {
  const template = working(fx)[0];
  const source = path.join(fx.root, template.file);
  const dir = path.dirname(source);
  const added = path.join(dir, 'lab-added.workbench.tsx');
  const renamed = path.join(dir, 'lab-renamed.workbench.tsx');
  const text = fs.readFileSync(source, 'utf8').replace(/id: '([^']*)'/, "id: '$1-lab'").replace(/title: '([^']*)'/, "title: '$1 Lab'");
  const listed = async predicate => {
    const started = Date.now();
    const deadline = started + 60000;
    while (Date.now() < deadline) {
      try { const config = await (await fetch(new URL('/_workbench/config', server.url))).json(); if (predicate(JSON.stringify(config))) return Date.now() - started; } catch (_) {}
      await sleep(50);
    }
    return null;
  };
  try {
    for (const [cell, apply, predicate] of [
      ['added', () => fs.writeFileSync(added, text), json => json.includes('lab-added.workbench.tsx')],
      ['renamed', () => fs.renameSync(added, renamed), json => json.includes('lab-renamed.workbench.tsx') && !json.includes('lab-added.workbench.tsx')],
      ['retitled', () => fs.writeFileSync(renamed, text.replace(/ Lab'/, " Lab Again'")), json => json.includes(' Lab Again')],
    ]) {
      const entry = await quiet({ ...base, sequence: 'S8', cell });
      apply();
      const ms = await listed(predicate);
      record({ ...entry, ms });
      check({ check: 'S8-' + cell, variant, fixture: fx.size, ok: ms != null });
    }
  } finally { fs.rmSync(added, { force: true }); fs.rmSync(renamed, { force: true }); }
}

// ---------------------------------------------------------------- formats and parity

const SIGNATURE = `(() => { const root = __labWin().document.getElementById('workbench-preview') || __labWin().document.body;
  const tags = []; const walk = node => { for (const child of node.children) { if (/^(STYLE|LINK|SCRIPT|META|TITLE|BASE)$/.test(child.tagName)) continue; tags.push(child.tagName); walk(child); } }; walk(root);
  return { text: root.innerText.replace(/\\s+/g, ' ').trim(), tags: tags.join(','), images: root.querySelectorAll('img').length }; })()`;
const loadedCheck = fx => fx.size === 'small' || fx.size === 'formats' ? 'true' : "!doc.querySelector('[data-acme=\"empty\"]') && !doc.body.innerText.includes('Loading')";

async function parityGroup(fx, variant, renders) {
  const cache = path.join(runDir, 'parity', variant + '-' + fx.size);
  const server = await serverFor(variant, fx, cache);
  const browser = await launchBrowser();
  const out = [];
  try {
    await warmed(server, fx);
    const pages = working(fx).slice(0, 10);
    const { canvas } = await first(browser, server, fx, pages[0]);
    for (const page of pages) for (const size of ['laptop', 'mobile']) {
      const sizeCheck = size === 'mobile' ? 'win.innerWidth < 700' : 'win.innerWidth > 900';
      const r = await select(canvas.target, address(page.file, 'default', size), `${pageCheck(page)} && ${loadedCheck(fx)} && ${sizeCheck}`, { timeout: 120000 });
      await sleep(400);
      out.push({ key: `${page.id}|${size}`, error: r.error || null, signature: r.error ? null : await evaluate(canvas.target, SIGNATURE).catch(() => null) });
    }
  } catch (error) { out.push({ key: 'session', error: String(error.message || error) }); }
  finally { await browser.close().catch(() => {}); await server.close(); fs.rmSync(cache, { recursive: true, force: true }); }
  (renders[variant] ||= {})[fx.size] = out;
}

function parityChecks(renders) {
  for (const [variant, bySize] of Object.entries(renders)) {
    if (variant === '0') continue;
    for (const [size, items] of Object.entries(bySize)) {
      const control = new Map((renders['0']?.[size] || []).map(item => [item.key, item]));
      const strip = value => value && JSON.stringify({ ...value, text: value.text.replace(/\d{4}-\d{2}-\d{2}/g, 'date') });
      const mismatches = items.filter(item => !item.signature || strip(item.signature) !== strip(control.get(item.key)?.signature)).map(item => ({ key: item.key, error: item.error, variant: item.signature?.text.slice(0, 120), control: control.get(item.key)?.signature?.text.slice(0, 120) }));
      check({ check: 'render-parity', variant, fixture: size, renders: items.length, mismatches: mismatches.slice(0, 10), ok: control.size > 0 && mismatches.length === 0 });
    }
  }
}

// Entry sources generated in Rust match the shipping previewEntry's, page by page.
async function entryParity(fx) {
  const server = await serverFor('R', fx, path.join(runDir, 'entries', fx.size));
  const maker = entryMaker(fx.root);
  const mismatches = [];
  let compared = 0;
  try {
    const pages = fx.size === 'colossal' ? shuffled(working(fx), 'entries').slice(0, 200).concat(working(fx).filter(page => page.computed).slice(0, 50)) : working(fx);
    for (const page of pages) {
      const [shipping] = await maker.entries([page.file]);
      const answer = await (await fetch(new URL('/_workbench/previews/lab/entry?file=' + encodeURIComponent(page.file), server.url))).json();
      compared++;
      // The only difference by design: the host imports Workbench's adapters
      // from the snapshot it runs in, the shipping maker from this package.
      const source = String(answer.source).split(snapshot('rust')).join(workbench);
      if (source !== shipping.source || answer.slug !== shipping.slug) mismatches.push({ page: page.id, error: answer.error || null });
    }
    const hot = await (await fetch(new URL('/_workbench/previews/lab/hot-client', server.url))).text();
    const { hotClientSource } = await import(path.join(workbench, 'server/node/src', 'modules', 'preview-pipeline', 'client-sources.ts'));
    check({ check: 'hot-client-parity', variant: 'R', fixture: fx.size, ok: hot === hotClientSource('web', 1) });
  } finally { await server.close(); }
  check({ check: 'entry-parity', variant: 'R', fixture: fx.size, compared, mismatches: mismatches.slice(0, 10), ok: mismatches.length === 0 });
}

// ---------------------------------------------------------------- report

function aggregate() {
  const rank = (values, p) => { const sorted = [...values].sort((a, b) => a - b); return sorted[Math.max(0, Math.ceil(p / 100 * sorted.length) - 1)]; };
  const cells = new Map();
  for (const item of results) {
    if (item.warmup || typeof item.ms !== 'number') continue;
    const key = [item.fixture, item.sequence, item.cell, item.variant].join('|');
    if (!cells.has(key)) cells.set(key, []);
    cells.get(key).push(item.ms);
  }
  return [...cells].map(([key, values]) => { const [fixture, sequence, cell, variant] = key.split('|'); return { fixture, sequence, cell, variant, samples: values.length, p50: rank(values, 50), p95: rank(values, 95), max: Math.max(...values) }; });
}

const release = acquireLabLock('instant-first-page stage 2 ' + settings.set);
const report = { plan: 'labs/instant-first-page/README.md#stage-2-plan', stage: 2, settings, smoke, host: { atStart: hostSnapshot(), platform: process.platform, arch: process.arch, node: process.version }, started: new Date().toISOString() };
let failure = null;
const fixtures = {};
try {
  const built = cargo(['build', '--release'], { stdio: ['ignore', 'ignore', 'inherit'] });
  if (built.status !== 0) throw new Error('The Rust host did not build.');
  log('Run directory: ' + runDir);
  for (const size of settings.fixtures) {
    const fx = await fixture(runDir, size);
    fixtures[size] = { ...fx, digest: treeDigest(fx.root) };
    if (settings.variants.some(rust)) await entryParity(fx);
    const renders = {};
    for (const variant of shuffled(settings.variants, settings.set + size)) {
      log(`${size}, variant ${variant}`);
      try {
        if (size === 'formats' || size === 'large-system') await parityGroup(fx, variant, renders);
        if (size !== 'formats') await startGroup(fx, variant);
        if (['large-system', 'giant', 'colossal'].includes(size)) await sessionGroup(fx, variant);
      } catch (error) {
        log(`  ${size} ${variant} failed: ${error.stack || error}`);
        record({ fixture: size, variant, sequence: 'group', cell: 'failed', ms: null, error: String(error.message || error).slice(0, 500) });
      }
    }
    parityChecks(renders);
  }
} catch (error) { failure = String(error.stack || error); log('Run failed: ' + failure); }
finally {
  for (const fx of Object.values(fixtures)) check({ check: 'no-project-writes', variant: 'all', fixture: fx.size, ok: treeDigest(fx.root) === fx.digest });
  Object.assign(report, { finished: new Date().toISOString(), failure, results, checks, aggregates: aggregate() });
  report.host.atEnd = hostSnapshot();
  const redact = text => text.split(runDir).join('<run>').split(workbench).join('<workbench>').split(os.homedir()).join('~');
  fs.writeFileSync(path.join(runDir, 'report.json'), redact(JSON.stringify(report, null, 2)));
  if (resultsDir) { fs.mkdirSync(resultsDir, { recursive: true }); fs.copyFileSync(path.join(runDir, 'report.json'), path.join(resultsDir, 'report.json')); }
  log('Report: ' + path.join(runDir, 'report.json'));
  if (!flag('keep')) for (const name of ['snapshots', 'fixtures', 'start', 'session', 'profiles', 'parity', 'own', 'entries', 'storage']) {
    try { fs.rmSync(path.join(runDir, name), { recursive: true, force: true }); } catch (error) { console.error('Cleanup failed for ' + name + ': ' + error.message); }
  }
  release();
}
