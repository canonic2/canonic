// Preview loading lab runner. See README.md for the plan this implements.
//
//   node labs/preview-loading/run.mjs [--smoke] [--variants 0,1,2,3,4,5]
//     [--fixtures small,large] [--samples 10] [--warmups 3]
//     [--sequences S1,S2,S3,S4,S5,S6,S8,S9,S10] [--source-revision <commit>]
//     [--results <new directory>] [--keep]
//
// Each variant runs from a snapshot of this package with the variant's
// overlay files from variants/<id>/overlay copied over it. Product code is
// never changed. Everything generated lives in a new run directory under the
// system temporary folder.
import { spawn } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { generate } from '../fixtures/acme-app.mjs';
import { install } from '../fixtures/install.mjs';
import { acquireLabLock, hostSnapshot } from '../fixtures/host.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const workbench = path.resolve(here, '../..');
const { Browser } = createRequire(import.meta.url)(path.join(workbench, 'scripts', 'chrome.cjs'));
const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf('--' + name); return index === -1 ? fallback : args[index + 1]; };
const flag = name => args.includes('--' + name);
const smoke = flag('smoke');
const settings = {
  variants: option('variants', '0').split(','),
  fixtures: option('fixtures', 'small,large').split(','),
  samples: smoke ? 2 : Number(option('samples', 10)),
  warmups: smoke ? 0 : Number(option('warmups', 3)),
  sequences: option('sequences', 'S1,S2,S3,S4,S5,S6,S8,S9,S10').split(','),
  sourceRevision: option('source-revision', null),
};
const resultsDir = option('results', null);
if (resultsDir && fs.existsSync(resultsDir)) { console.error('Refusing to overwrite ' + resultsDir); process.exit(2); }

const runParent = path.join(os.tmpdir(), 'canonic-workbench-labs', 'preview-loading');
fs.mkdirSync(runParent, { recursive: true });
const runDir = path.join(fs.realpathSync(runParent), new Date().toISOString().replace(/[:.]/g, '-'));
fs.mkdirSync(runDir);
const log = message => console.log(`[${new Date().toISOString().slice(11, 19)}] ${message}`);
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const percentile = (values, p) => { const sorted = [...values].sort((a, b) => a - b); return sorted[Math.max(0, Math.ceil(p / 100 * sorted.length) - 1)]; };

// ---------------------------------------------------------------- preparation

const SNAPSHOT_SKIP = new Set(['node_modules', 'dist', 'labs', '.DS_Store']);
let labTools = null;
function snapshot(variant) {
  const target = path.join(runDir, 'snapshots', 'variant-' + variant);
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
  // A variant's variant.json may name a variant it extends; its overlay
  // applies first, then this one's.
  const chain = [];
  for (let current = variant; current && current !== '0';) {
    chain.unshift(current);
    const meta = path.join(here, 'variants', current, 'variant.json');
    current = fs.existsSync(meta) ? JSON.parse(fs.readFileSync(meta, 'utf8')).extends : null;
  }
  const applied = [];
  for (const id of chain) {
    const overlay = path.join(here, 'variants', id, 'overlay');
    if (!fs.existsSync(overlay)) throw new Error('Variant ' + id + ' has no overlay in ' + overlay);
    const walk = (directory, relative = '') => {
      for (const item of fs.readdirSync(directory, { withFileTypes: true })) {
        const rel = path.join(relative, item.name);
        if (item.isDirectory()) walk(path.join(directory, item.name), rel);
        else { fs.mkdirSync(path.dirname(path.join(target, rel)), { recursive: true }); fs.copyFileSync(path.join(directory, item.name), path.join(target, rel)); applied.push(id + ':' + rel); }
      }
    };
    walk(overlay);
  }
  return { dir: target, overlay: applied };
}

function prepare() {
  const fixtures = {};
  for (const size of settings.fixtures) {
    const root = path.join(runDir, 'fixtures', size);
    log(`Generating and installing the ${size} fixture`);
    const manifest = generate(root, { size, seed: 1 });
    install(root, path.join(here, '..', 'fixtures', 'locks', size + '.package-lock.json'));
    fixtures[size] = { root, manifest };
  }
  const snapshots = Object.fromEntries(settings.variants.map(variant => [variant, snapshot(variant)]));
  // Packages variants 4 and 5 need, installed into the run directory; the
  // snapshot's node_modules is the package's own and stays unchanged.
  let tools = null;
  if (settings.variants.some(variant => ['4', '5'].includes(variant))) {
    tools = path.join(runDir, 'tools');
    fs.mkdirSync(tools);
    fs.copyFileSync(path.join(here, 'tools', 'package.json'), path.join(tools, 'package.json'));
    log('Installing variant tools');
    install(tools, path.join(here, 'tools', 'package-lock.json'));
  }
  labTools = tools;
  return { fixtures, snapshots, tools };
}

// ----------------------------------------------------------- server and page

// Starts a server for one variant and fixture. `cache` names the preview disk
// cache directory (the worker's TMPDIR), so cold and disk-warm cells differ
// only in what it holds.
async function startServer(snapshotDir, fixture, cache) {
  fs.mkdirSync(cache, { recursive: true });
  const child = spawn(process.execPath, [path.join(here, 'serve.cjs'), snapshotDir, fixture], {
    stdio: ['pipe', 'pipe', 'pipe'], env: { ...process.env, TMPDIR: cache, ...(labTools ? { WORKBENCH_LAB_TOOLS: labTools } : {}) },
  });
  const logs = [];
  let buffer = '';
  const ready = new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Server did not start')), 60000);
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
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-4000); });
  const url = await ready;
  return {
    url, logs, stderr: () => stderr,
    close: () => new Promise(resolve => {
      if (flag('debug')) {
        for (const item of logs.filter(entry => entry.log.level !== 'info' || /worker/.test(entry.log.event))) console.log('[server]', JSON.stringify(item.log).slice(0, 1500));
        if (stderr) console.log('[server stderr]', stderr.slice(-1500));
      }
      const timer = setTimeout(() => { child.kill('SIGKILL'); resolve(); }, 8000);
      child.once('exit', () => { clearTimeout(timer); resolve(); });
      child.kill('SIGTERM');
    }),
  };
}

// Installed in the canvas document only: counts promoted frames and exposes
// timing helpers that measure inside the page.
const CANVAS_SCRIPT = `if (window.top === window) {
  window.__lab = { changes: 0 };
  addEventListener('wb-frame-change', () => { __lab.changes++; __lab.last = performance.now(); });
  // Selection to visible: the hash change starts it; the promoted frame's
  // next painted frame (two animation frames later) stops it.
  window.__labGo = hash => new Promise((resolve, reject) => {
    const start = performance.now();
    const timer = setTimeout(() => { removeEventListener('wb-frame-change', changed); reject(new Error('No frame change for ' + hash)); }, 60000);
    function changed() {
      removeEventListener('wb-frame-change', changed);
      requestAnimationFrame(() => requestAnimationFrame(() => { clearTimeout(timer); resolve(performance.now() - start); }));
    }
    addEventListener('wb-frame-change', changed);
    if (location.hash === '#' + hash) { clearTimeout(timer); removeEventListener('wb-frame-change', changed); resolve(0); return; }
    location.hash = hash;
  });
  window.__labActive = () => document.querySelector('iframe.is-active');
  // Resolves with Date.now() once the active frame's text contains marker.
  window.__labWaitText = (marker, timeoutMs) => new Promise((resolve, reject) => {
    const deadline = Date.now() + timeoutMs;
    (function check() {
      const frame = __labActive();
      let text = '';
      try { text = frame?.contentDocument?.body?.textContent || ''; } catch (_) {}
      if (text.includes(marker)) { requestAnimationFrame(() => resolve(Date.now())); return; }
      if (Date.now() > deadline) { reject(new Error('Marker not visible: ' + marker)); return; }
      setTimeout(check, 10);
    })();
  });
}`;

async function openCanvas(browser, url, address) {
  const target = await browser.createTarget();
  const exceptions = [];
  target.on('Runtime.exceptionThrown', event => exceptions.push(event.exceptionDetails?.exception?.description || event.exceptionDetails?.text));
  await target.send('Runtime.enable');
  await target.send('Emulation.setDeviceMetricsOverride', { width: 1900, height: 1100, deviceScaleFactor: 1, mobile: false });
  await target.send('Page.addScriptToEvaluateOnNewDocument', { source: CANVAS_SCRIPT });
  const started = Date.now();
  await target.navigate(url + 'index.html#' + address);
  await until(target, '__lab.changes > 0', 120000);
  const firstVisible = await evaluate(target,'__lab.last') ;
  return { target, exceptions, firstVisibleMs: firstVisible, wallMs: Date.now() - started };
}

// The driver's evaluate gives up after its default command timeout; waits
// for rebuilds and reloads can take longer.
async function evaluate(target, expression, timeout = 90000) {
  const result = await target.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }, { timeout });
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text || 'The page threw');
  return result.result?.value;
}

async function until(target, expression, timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await evaluate(target,expression)) return;
    await sleep(20);
  }
  throw new Error('Timed out waiting for ' + expression);
}

// Phase evidence from the active preview frame and the server's diagnostics.
async function framePhases(target) {
  return evaluate(target,`(() => {
    const frame = __labActive(); let win; try { win = frame.contentWindow; } catch (_) { return null; }
    if (!win) return null;
    const resources = win.performance.getEntriesByType('resource').filter(entry => /\\.(m?js|css)(\\?|$)/.test(entry.name));
    return {
      ready: win.__workbenchReady === true, error: win.__workbenchError || null,
      scripts: resources.filter(entry => /\\.m?js(\\?|$)/.test(entry.name)).length,
      transferBytes: resources.reduce((sum, entry) => sum + (entry.transferSize || 0), 0),
      decodedBytes: resources.reduce((sum, entry) => sum + (entry.decodedBodySize || 0), 0),
      failedRequests: win.performance.getEntriesByType('resource').filter(entry => entry.responseStatus >= 400).map(entry => entry.responseStatus + ' ' + entry.name).slice(0, 5),
      seenPreviews: win.document.documentElement.dataset.acmeSeenPreviews || null,
      listeners: win.document.documentElement.dataset.acmeListeners || null,
    };
  })()`);
}

const address = (file, state = 'default', size = 'laptop') => `${file}:${state}@${size}`;

// ------------------------------------------------------------------ edits

function edit(file, transform) {
  const original = fs.readFileSync(file, 'utf8');
  const next = transform(original);
  if (next === original) throw new Error('Edit did not apply to ' + file);
  const written = Date.now();
  fs.writeFileSync(file, next);
  return { written, restore: () => fs.writeFileSync(file, original) };
}

// ---------------------------------------------------------------- sequences

async function runSequences({ variant, size, fixture, snapshotDir, browserFactory, samples, warmups }) {
  const results = [];
  // Each result records the load average when recorded.
  const record = (sequence, cell, value) => { const snapshot = hostSnapshot(); results.push({ variant, fixture: size, sequence, cell, ...value, host: { loadAverage: snapshot.loadAverage } }); };
  const pages = fixture.manifest.previews.filter(preview => !preview.broken);
  const broken = fixture.manifest.previews.filter(preview => preview.broken);
  const cacheRoot = path.join(runDir, 'cache', `variant-${variant}-${size}`);
  const want = name => settings.sequences.includes(name);

  // S1: first preview after the server starts, cold then disk-warm.
  if (want('S1')) {
    for (const cache of ['cold', 'disk-warm']) {
      for (let index = 0; index < warmups + samples; index++) {
        const cacheDir = path.join(cacheRoot, cache === 'cold' ? 'cold-' + index : 'shared');
        if (cache === 'disk-warm' && index === 0) {
          // Populate the shared cache once, outside the samples.
          const primer = await startServer(snapshotDir, fixture.root, cacheDir);
          const browser = await browserFactory();
          try { await openCanvas(browser, primer.url, address(pages[0].file)); }
          finally { await browser.close(); await primer.close(); }
        }
        const server = await startServer(snapshotDir, fixture.root, cacheDir);
        const browser = await browserFactory();
        try {
          const canvas = await openCanvas(browser, server.url, address(pages[0].file));
          const phases = await framePhases(canvas.target);
          record('S1', cache, { warmup: index < warmups, ms: canvas.firstVisibleMs, wallMs: canvas.wallMs, phases, exceptions: canvas.exceptions.length });
        } catch (error) {
          record('S1', cache, { warmup: index < warmups, error: String(error.message || error), serverStderr: server.stderr() });
        } finally { await browser.close(); await server.close(); }
      }
    }
  }

  // One long-lived server and browser for the browser-warm sequences.
  const server = await startServer(snapshotDir, fixture.root, path.join(cacheRoot, 'session'));
  const browser = await browserFactory();
  try {
    const canvas = await openCanvas(browser, server.url, address(pages[0].file));
    const { target } = canvas;
    const go = hash => evaluate(target,`__labGo(${JSON.stringify(hash)})`);

    // S2: first visits to previews not yet opened in this canvas. The paced
    // cell waits a second before each selection, as someone looks at a page;
    // the back-to-back cell selects as soon as the previous page is visible.
    const walk = pages.slice(1);
    if (want('S2')) {
      let next = 0;
      for (const [cell, pause] of [['browser-warm', 1000], ['back-to-back', 0]]) {
        for (let index = 0; index < warmups + samples && next < walk.length; index++, next++) {
          await sleep(pause);
          try {
            const ms = await go(address(walk[next].file));
            record('S2', cell, { warmup: index < warmups, ms, preview: walk[next].id, phases: await framePhases(target) });
          } catch (error) { record('S2', cell, { warmup: index < warmups, error: String(error.message || error) }); }
        }
      }
    }

    // S3: return to a retained preview, alternating between two.
    if (want('S3')) {
      const a = pages[Math.min(1, pages.length - 1)], b = pages[0];
      await go(address(a.file)); await go(address(b.file));
      for (let index = 0; index < warmups + samples; index++) {
        const next = index % 2 ? b : a;
        try { record('S3', 'retained', { warmup: index < warmups, ms: await go(address(next.file)), preview: next.id }); }
        catch (error) { record('S3', 'retained', { warmup: index < warmups, error: String(error.message || error) }); }
      }
    }

    // S4: Reload the current preview.
    if (want('S4')) {
      for (let index = 0; index < warmups + samples; index++) {
        try {
          const ms = await evaluate(target,`new Promise((resolve, reject) => {
            const start = performance.now(); const timer = setTimeout(() => reject(new Error('Reload produced no frame change')), 60000);
            addEventListener('wb-frame-change', function changed() { removeEventListener('wb-frame-change', changed);
              requestAnimationFrame(() => requestAnimationFrame(() => { clearTimeout(timer); resolve(performance.now() - start); })); });
            document.getElementById('reload').click();
          })`);
          record('S4', 'browser-warm', { warmup: index < warmups, ms });
        } catch (error) { record('S4', 'browser-warm', { warmup: index < warmups, error: String(error.message || error) }); }
      }
    }

    // S5, S6, S9, S10 edit fixture files and wait until the canvas shows them.
    const current = pages[0];
    const interactive = path.join(fixture.root, current.edit.component);
    const shared = fs.existsSync(path.join(fixture.root, 'src/app/Shell.tsx')) ? path.join(fixture.root, 'src/app/Shell.tsx') : path.join(fixture.root, 'src/workbench/environment.tsx');
    async function editAndWait(sequence, file, transform, marker, extra = {}) {
      await settle();
      const change = edit(file, transform);
      try {
        let visibleAt;
        try { visibleAt = await evaluate(target, `__labWaitText(${JSON.stringify(marker)}, 30000)`); }
        catch (error) {
          // What the canvas shows when an edit never appears.
          const seen = await evaluate(target, `(async () => {
            const frame = __labActive(); let win; try { win = frame.contentWindow; } catch (_) {}
            const text = win?.document?.body?.textContent || '';
            const options = win?.__workbenchOptions || null;
            let server = null;
            try { if (options?.revisionUrl) server = (await (await fetch(options.revisionUrl, { cache: 'no-store' })).json()).revision; } catch (_) {}
            return { frames: document.querySelectorAll('iframe').length, active: !!frame, ready: win?.__workbenchReady ?? null, error: (win?.__workbenchError || '').slice(0, 200),
              shown: (/Revision [^\\s<]*/.exec(text) || [null])[0] || text.slice(0, 120), mountedRevision: options?.revision ?? null, serverRevision: server,
              hostUrl: win?.location?.href?.slice(-80) || null };
          })()`).catch(() => null);
          error.message += ' | seen ' + JSON.stringify(seen);
          throw error;
        }
        return { ms: visibleAt - change.written, ...extra };
      } finally {
        // Restoring is itself an edit; let the canvas settle on it.
        change.restore();
        await sleep(500);
        await settle().catch(() => {});
      }
    }
    const markerFor = (sequence, index) => `lab${sequence}x${index}x${crypto.randomBytes(2).toString('hex')}`;
    // Visible text edits: the large fixture's edit marker, or a small
    // fixture component's button label (its title comes from inputs).
    const pageTransform = marker => text => text.includes('Revision ')
      ? text.replace(/Revision [^<]*/, 'Revision ' + marker)
      : text.replace("{open ? 'Hide' : 'Show'}", `{open ? 'Hide' : 'Show ${marker}'}`);
    const sharedTransform = marker => text => text.includes('<nav aria-label="Features">')
      ? text.replace('<nav aria-label="Features">', `<nav aria-label="Features"><span>${marker}</span>`)
      : text.replace('<div className="acme-root">', `<div className="acme-root"><span>${marker}</span>`);
    const settle = () => until(target, `(() => { try { const win = __labActive()?.contentWindow; return win?.__workbenchReady === true; } catch (_) { return false; } })()`, 60000);
    if (want('S5') || want('S6') || want('S9') || want('S10')) await go(address(current.file));

    if (want('S5')) {
      for (let index = 0; index < warmups + samples; index++) {
        const marker = markerFor('S5', index);
        try { record('S5', 'browser-warm', { warmup: index < warmups, ...(await editAndWait('S5', interactive, pageTransform(marker), marker)) }); }
        catch (error) { record('S5', 'browser-warm', { warmup: index < warmups, error: String(error.message || error) }); }
      }
    }
    if (want('S6')) {
      for (let index = 0; index < warmups + samples; index++) {
        const marker = markerFor('S6', index);
        try { record('S6', 'browser-warm', { warmup: index < warmups, ...(await editAndWait('S6', shared, sharedTransform(marker), marker)) }); }
        catch (error) { record('S6', 'browser-warm', { warmup: index < warmups, error: String(error.message || error) }); }
      }
    }

    // S9: state survives a component edit (HMR with Fast Refresh) or not.
    if (want('S9') && fs.existsSync(interactive) && fs.readFileSync(interactive, 'utf8').includes('data-acme="counter"')) {
      for (let index = 0; index < warmups + samples; index++) {
        const marker = markerFor('S9', index);
        try {
          await settle();
          await until(target, `!!__labActive().contentWindow.document.querySelector('[data-acme="counter"]')`, 30000);
          // Separate clicks with time to render between them: a burst of
          // clicks in one task is one React update.
          await evaluate(target, `__labActive().contentWindow.__labSentinel = ${index + 1}`);
          for (let click = 0; click < 3; click++) {
            await evaluate(target, `__labActive().contentWindow.document.querySelector('[data-acme="counter"]').click()`);
            await sleep(60);
          }
          await evaluate(target, `(() => { const win = __labActive().contentWindow;
            const note = win.document.querySelector('[data-acme="note"]');
            const setter = Object.getOwnPropertyDescriptor(win.HTMLInputElement.prototype, 'value').set;
            setter.call(note, 'acme note'); note.dispatchEvent(new win.Event('input', { bubbles: true })); })()`);
          await sleep(100);
          const read = `(() => { const win = __labActive().contentWindow; const doc = win.document;
            return { sameDocument: win.__labSentinel === ${index + 1}, counter: doc.querySelector('[data-acme="counter"]')?.textContent,
              note: doc.querySelector('[data-acme="note"]')?.value }; })()`;
          // The state must be set before the edit for "kept" to mean anything.
          const before = await evaluate(target, read);
          const timing = await editAndWait('S9', interactive, pageTransform(marker), marker);
          const after = await evaluate(target, read);
          record('S9', 'browser-warm', { warmup: index < warmups, ...timing, ...after, before,
            stateSet: before.counter === 'Count 3' && before.note === 'acme note',
            statePreserved: before.counter === 'Count 3' && before.note === 'acme note' && after.counter === before.counter && after.note === before.note,
            inPlace: after.sameDocument });
          // Reset the page state for the next sample: wait for the reloaded
          // frame itself, not the old one, which is still ready.
          await evaluate(target, `new Promise((resolve, reject) => { const timer = setTimeout(() => reject(new Error('Reload produced no frame change')), 60000);
            addEventListener('wb-frame-change', function changed() { removeEventListener('wb-frame-change', changed); clearTimeout(timer); resolve(); });
            document.getElementById('reload').click(); })`);
          await settle();
        } catch (error) { record('S9', 'browser-warm', { warmup: index < warmups, error: String(error.message || error) }); }
      }
    }

    // S10: which edits apply in place and which reload; one sample each.
    if (want('S10') && fixture.manifest.size === 'large') {
      const pageSource = fs.readFileSync(path.join(fixture.root, path.dirname(current.file), 'Page.tsx'), 'utf8');
      const feature = /from '\.\.\/\.\.\/app\/features\/([^']+)'/.exec(pageSource)[1];
      const cases = [
        // A non-component module: the feature's title, which the shell's
        // navigation shows on every page.
        ['module', path.join(fixture.root, 'src/app/features', feature, 'index.ts'), marker => text => text.replace(/export const title = '([^']*)'/, `export const title = '$1 ${marker}'`)],
        ['definition', path.join(fixture.root, current.file), marker => text => text.replace(/title: 'Acme\/Pages\/([^']+)'/, `title: 'Acme/Pages/$1 ${marker}'`)],
        ['environment', path.join(fixture.root, 'src/workbench/environment.tsx'), marker => text => text.replace('<Shell>{tree}</Shell>', `<Shell><span>${marker}</span>{tree}</Shell>`)],
      ];
      for (const [kind, file, transform] of cases) {
        const marker = markerFor('S10', kind);
        await evaluate(target,`__labActive().contentWindow.__labSentinel = 'S10'`);
        try {
          const timing = await editAndWait('S10', file, transform(marker), kind === 'definition' ? 'Count' : marker);
          const inPlace = await evaluate(target,`__labActive().contentWindow.__labSentinel === 'S10'`);
          record('S10', kind, { ms: kind === 'definition' ? null : timing.ms, inPlace, note: kind === 'definition' ? 'definition edits are checked for a visible reload only' : undefined });
        } catch (error) { record('S10', kind, { error: String(error.message || error) }); }
      }
    }

    // S8: broken previews show their own errors; a working one still loads.
    if (want('S8') && broken.length) {
      for (const item of broken) {
        try {
          const ms = await go(address(item.file)).catch(() => null);
          const phases = await framePhases(target);
          const text = await evaluate(target,`(() => { try { return __labActive().contentDocument.body.textContent.slice(0, 300); } catch (_) { return ''; } })()`);
          record('S8', item.broken, { ms, errorShown: !!phases?.error || /error|failed|Missing|broken/i.test(text) });
        } catch (error) { record('S8', item.broken, { error: String(error.message || error) }); }
      }
      try { record('S8', 'recovery', { ms: await go(address(pages[2 % pages.length].file)), phases: await framePhases(target) }); }
      catch (error) { record('S8', 'recovery', { error: String(error.message || error) }); }
    }

    record('exceptions', 'canvas', { count: canvas.exceptions.length, first: canvas.exceptions.slice(0, 3) });
  } finally {
    await browser.close();
    await server.close();
  }
  return results;
}

// Render parity and isolation: every state of up to ten previews at two
// sizes, captured from the active frame after it settles.
async function collectRenders({ size, fixture, snapshotDir, browserFactory }) {
  const pages = fixture.manifest.previews.filter(preview => !preview.broken).slice(0, 10);
  const states = size === 'large' ? ['default', 'empty'] : ['default', 'accent'];
  const server = await startServer(snapshotDir, fixture.root, path.join(runDir, 'cache', 'renders-' + crypto.randomBytes(3).toString('hex')));
  const browser = await browserFactory();
  const renders = [];
  try {
    const canvas = await openCanvas(browser, server.url, address(pages[0].file));
    for (const item of pages) for (const state of states) for (const viewport of ['laptop', 'mobile']) {
      try {
        await evaluate(canvas.target,`__labGo(${JSON.stringify(address(item.file, state, viewport))})`);
        let previous = null, html = null, stable = 0;
        for (let attempt = 0; attempt < 60 && stable < 2; attempt++) {
          html = await evaluate(canvas.target,`(() => { try { return __labActive().contentDocument.getElementById('workbench-preview')?.innerHTML ?? null; } catch (_) { return null; } })()`);
          stable = html !== null && html === previous ? stable + 1 : 0;
          previous = html;
          await sleep(200);
        }
        const phases = await framePhases(canvas.target);
        renders.push({ preview: item.id, state, viewport, html: html && html.replace(/(src|href)="[^"]*?([^/".]+?)(?:[-.][A-Za-z0-9_-]{6,})?\.(svg|png|woff2?|jpg)"/g, '$1="ASSET:$2.$3"'),
          seenPreviews: phases?.seenPreviews ?? null, listeners: phases?.listeners ?? null, error: phases?.error ?? null });
      } catch (error) { renders.push({ preview: item.id, state, viewport, error: String(error.message || error) }); }
    }
  } finally { await browser.close(); await server.close(); }
  return renders;
}

function aggregate(results) {
  const cells = new Map();
  for (const item of results.filter(result => !result.warmup && typeof result.ms === 'number')) {
    const key = [item.fixture, item.sequence, item.cell, item.variant].join('|');
    if (!cells.has(key)) cells.set(key, []);
    cells.get(key).push(item.ms);
  }
  return [...cells].map(([key, values]) => {
    const [fixture, sequence, cell, variant] = key.split('|');
    return { fixture, sequence, cell, variant, samples: values.length, p50: percentile(values, 50), p95: percentile(values, 95), max: Math.max(...values) };
  });
}

function parity(renders) {
  const control = renders['0'] || {};
  const out = {};
  for (const [variant, bySize] of Object.entries(renders)) {
    out[variant] = {};
    for (const [size, items] of Object.entries(bySize)) {
      const reference = new Map((control[size] || []).map(item => [`${item.preview}|${item.state}|${item.viewport}`, item]));
      out[variant][size] = {
        renders: items.length,
        matches: variant === '0' ? null : items.filter(item => item.html && reference.get(`${item.preview}|${item.state}|${item.viewport}`)?.html === item.html).length,
        errors: items.filter(item => item.error).length,
        isolationLeaks: items.filter(item => (item.seenPreviews && item.seenPreviews !== '1') || (item.listeners && item.listeners !== '1')).length,
      };
    }
  }
  return out;
}

function summary(report) {
  const lines = [`# Preview loading run ${report.host.date}`, '', report.smoke ? 'Smoke run: not evidence.' : `Samples per cell: ${settings.samples}, warmups: ${settings.warmups}.`, '',
    '| Fixture | Sequence | Cell | Variant | n | p50 ms | p95 ms | max ms |', '| --- | --- | --- | --- | ---: | ---: | ---: | ---: |'];
  for (const cell of [...report.aggregates].sort((a, b) => (a.fixture + a.sequence + a.cell + a.variant).localeCompare(b.fixture + b.sequence + b.cell + b.variant))) {
    lines.push(`| ${cell.fixture} | ${cell.sequence} | ${cell.cell} | ${cell.variant} | ${cell.samples} | ${Math.round(cell.p50)} | ${Math.round(cell.p95)} | ${Math.round(cell.max)} |`);
  }
  lines.push('', '## HMR (S9, S10)', '');
  for (const item of report.results.filter(result => (result.sequence === 'S9' && !result.warmup) || result.sequence === 'S10')) {
    lines.push(`- variant ${item.variant} ${item.fixture} ${item.sequence} ${item.cell}: ${item.error ? 'ERROR ' + item.error : `in place ${item.inPlace ? 'yes' : 'no'}${'statePreserved' in item ? ', state kept ' + (item.statePreserved ? 'yes' : 'no') : ''}${item.ms != null ? ', ' + Math.round(item.ms) + ' ms' : ''}`}`);
  }
  lines.push('', '## Errors and isolation', '');
  for (const item of report.results.filter(result => result.error || result.sequence === 'S8' || result.sequence === 'exceptions')) {
    lines.push(`- variant ${item.variant} ${item.fixture} ${item.sequence} ${item.cell}: ${item.error ? 'ERROR ' + item.error : JSON.stringify({ ms: item.ms, errorShown: item.errorShown, count: item.count })}`);
  }
  lines.push('', '## Render parity', '', '```json', JSON.stringify(report.parity, null, 2), '```');
  return lines.join('\n') + '\n';
}

// ---------------------------------------------------------------------- main

let prepared;
try {
  acquireLabLock('preview-loading');
  log('Run directory: ' + runDir);
  var startSnapshot = hostSnapshot();
  prepared = prepare();
  const results = [];
  const renders = {};
  const browserFactory = async () => { const browser = new Browser({}); await browser.launch(); return browser; };
  for (const variant of settings.variants) {
    renders[variant] = {};
    for (const size of settings.fixtures) {
      log(`Variant ${variant}, ${size} fixture: sequences`);
      results.push(...await runSequences({ variant, size, fixture: prepared.fixtures[size], snapshotDir: prepared.snapshots[variant].dir, browserFactory, samples: settings.samples, warmups: settings.warmups }));
      if (flag('no-renders')) continue;
      log(`Variant ${variant}, ${size} fixture: renders`);
      renders[variant][size] = await collectRenders({ size, fixture: prepared.fixtures[size], snapshotDir: prepared.snapshots[variant].dir, browserFactory });
    }
  }
  const report = {
    plan: 'labs/preview-loading/README.md', smoke, settings,
    host: { atStart: startSnapshot, atEnd: hostSnapshot(), date: new Date().toISOString(), platform: process.platform, arch: process.arch, cpus: os.cpus().length,
      memoryGb: Math.round(os.totalmem() / 2 ** 30), loadAverage: os.loadavg(), node: process.version, sourceRevision: settings.sourceRevision,
      overlays: Object.fromEntries(Object.entries(prepared.snapshots).map(([variant, value]) => [variant, value.overlay])) },
    fixtures: Object.fromEntries(Object.entries(prepared.fixtures).map(([size, value]) => [size, { files: value.manifest.files, sourceBytes: value.manifest.sourceBytes, previews: value.manifest.previews.length }])),
    results, aggregates: aggregate(results), parity: parity(renders),
  };
  // Reports are committed evidence: local directories become placeholders.
  const redact = text => text.split(runDir).join('<run>').split(workbench).join('<workbench>').split(os.homedir()).join('<home>');
  fs.writeFileSync(path.join(runDir, 'report.json'), redact(JSON.stringify(report, null, 2)));
  fs.writeFileSync(path.join(runDir, 'renders.json'), JSON.stringify(renders));
  fs.writeFileSync(path.join(runDir, 'summary.md'), summary(report));
  if (resultsDir) {
    fs.mkdirSync(resultsDir, { recursive: true });
    for (const name of ['report.json', 'summary.md']) fs.copyFileSync(path.join(runDir, name), path.join(resultsDir, name));
  }
  console.log('\n' + summary(report));
  log('Report: ' + path.join(runDir, 'report.json'));
} finally {
  if (!flag('keep')) {
    for (const name of ['snapshots', 'fixtures', 'cache']) {
      try { fs.rmSync(path.join(runDir, name), { recursive: true, force: true }); }
      catch (error) { console.error('Cleanup failed for ' + name + ': ' + error.message); }
    }
  }
}
