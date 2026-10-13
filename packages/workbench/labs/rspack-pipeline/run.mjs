// Rspack pipeline lab runner, stage 1 (feasibility). See README.md.
//
//   node labs/rspack-pipeline/run.mjs [--variants 0,2,3] [--fixture formats|small]
//     [--project <path> --project-config <file>] [--results <new directory>] [--keep] [--debug]
//
// Each variant runs from a snapshot of this package; variants 2 and 3 copy
// overlay/ over it. Product code is never changed. Generated projects, tools,
// caches and browser profiles live in a new run directory under the system
// temporary folder. A selected project is read-only; its results are written
// to the run directory only, never to results/.
import { spawn, spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { install } from '../fixtures/install.mjs';
import { acquireLabLock, hostSnapshot } from '../fixtures/host.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const workbench = path.resolve(here, '../..');
const { Browser } = createRequire(import.meta.url)(path.join(workbench, 'scripts', 'chrome.cjs'));
const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf('--' + name); return index === -1 ? fallback : args[index + 1]; };
const flag = name => args.includes('--' + name);
const settings = {
  variants: option('variants', '0,2,3').split(','),
  fixture: option('fixture', 'formats'),
  project: option('project', null),
  sourceRevision: option('source-revision', null),
};
const resultsDir = option('results', null);
if (resultsDir && settings.project) { console.error('A selected project\'s results stay in the run directory; drop --results.'); process.exit(2); }
if (resultsDir && fs.existsSync(resultsDir)) { console.error('Refusing to overwrite ' + resultsDir); process.exit(2); }

const runParent = path.join(os.tmpdir(), 'canonic-workbench-labs', 'rspack-pipeline');
fs.mkdirSync(runParent, { recursive: true });
const runDir = path.join(fs.realpathSync(runParent), new Date().toISOString().replace(/[:.]/g, '-'));
fs.mkdirSync(runDir);
const log = message => console.log(`[${new Date().toISOString().slice(11, 19)}] ${message}`);
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

// ---------------------------------------------------------------- preparation

const SNAPSHOT_SKIP = new Set(['node_modules', 'dist', 'labs', '.DS_Store']);
const snapshots = new Map();
function snapshot(variant) {
  if (snapshots.has(variant)) return snapshots.get(variant);
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
  const applied = [];
  if (variant !== '0') {
    const walk = (directory, relative = '') => {
      for (const item of fs.readdirSync(directory, { withFileTypes: true })) {
        const rel = path.join(relative, item.name);
        if (item.isDirectory()) walk(path.join(directory, item.name), rel);
        else { fs.mkdirSync(path.dirname(path.join(target, rel)), { recursive: true }); fs.copyFileSync(path.join(directory, item.name), path.join(target, rel)); applied.push(rel); }
      }
    };
    walk(path.join(here, 'overlay'));
  }
  snapshots.set(variant, { dir: target, overlay: applied });
  return snapshots.get(variant);
}

async function prepareFixture() {
  if (settings.project) {
    const root = path.resolve(settings.project);
    return { root, manifest: { size: 'selected', previews: [], edits: {} }, selected: true };
  }
  const root = path.join(runDir, 'fixture');
  log(`Generating and installing the ${settings.fixture} fixture`);
  const module = settings.fixture === 'formats' ? await import('../fixtures/formats.mjs') : await import('../fixtures/acme-app.mjs');
  const manifest = module.generate(root, { size: settings.fixture, seed: 1 });
  install(root, path.join(here, '..', 'fixtures', 'locks', settings.fixture + '.package-lock.json'));
  return { root, manifest, selected: false };
}

function installTools() {
  const tools = path.join(runDir, 'tools');
  fs.mkdirSync(tools);
  fs.copyFileSync(path.join(here, 'tools', 'package.json'), path.join(tools, 'package.json'));
  log('Installing lab tools');
  install(tools, path.join(here, 'tools', 'package-lock.json'));
  return tools;
}

// Size, modification time and path of every file under root, hashed, so a
// run can show it wrote nothing into the project.
function treeDigest(root) {
  const hash = crypto.createHash('sha256');
  const walk = directory => {
    for (const item of fs.readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const file = path.join(directory, item.name);
      if (item.isSymbolicLink()) { hash.update('L' + path.relative(root, file)); continue; }
      if (item.isDirectory()) { hash.update('D' + path.relative(root, file)); walk(file); continue; }
      const stat = fs.statSync(file);
      // Project files by content (the runner restores its own edits); installed
      // packages by size and time, which is enough to see a cache appear.
      hash.update('F' + path.relative(root, file) + ':' + (/node_modules/.test(file) ? stat.size + ':' + stat.mtimeMs : crypto.createHash('sha1').update(fs.readFileSync(file)).digest('hex')));
    }
  };
  walk(root);
  return hash.digest('hex');
}

function treeList(root) {
  const out = [];
  const walk = directory => { for (const item of fs.readdirSync(directory, { withFileTypes: true })) { const file = path.join(directory, item.name); out.push(path.relative(root, file)); if (item.isDirectory() && !item.isSymbolicLink()) walk(file); } };
  walk(root);
  return out;
}

// ----------------------------------------------------------- server and page

async function startServer(snapshotDir, root, cache, tools) {
  fs.mkdirSync(cache, { recursive: true });
  const child = spawn(process.execPath, [path.join(here, '..', 'preview-loading', 'serve.cjs'), snapshotDir, root], {
    stdio: ['pipe', 'pipe', 'pipe'], env: { ...process.env, TMPDIR: cache, WORKBENCH_LAB_TOOLS: tools, ...(flag('no-persistent-cache') ? { WORKBENCH_LAB_NO_PERSISTENT_CACHE: '1' } : {}), ...(flag('trace') ? { WORKBENCH_LAB_TRACE: '1' } : {}), ...(flag('lazy') ? { WORKBENCH_LAB_LAZY: '1' } : {}), ...(option('project-config') ? { WORKBENCH_LAB_RSPACK_CONFIG: path.resolve(option('project-config')) } : {}) },
  });
  const logs = [];
  let buffer = '';
  const ready = new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Server did not start')), 90000);
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
  return {
    url, logs, stderr: () => stderr,
    close: () => new Promise(resolve => {
      if (flag('debug')) {
        for (const item of logs.filter(entry => entry.log.level !== 'info' || /worker|pipeline/.test(entry.log.event))) console.log('[server]', JSON.stringify(item.log).slice(0, 1500));
        if (stderr) console.log('[server stderr]', stderr.slice(-3000));
      }
      const timer = setTimeout(() => { child.kill('SIGKILL'); resolve(); }, 8000);
      child.once('exit', () => { clearTimeout(timer); resolve(); });
      child.kill('SIGTERM');
    }),
  };
}

const CANVAS_SCRIPT = `if (window.top === window) {
  window.__lab = { changes: 0 };
  addEventListener('wb-frame-change', () => { __lab.changes++; __lab.last = performance.now(); });
  window.__labGo = hash => new Promise((resolve, reject) => {
    const start = performance.now();
    // Some changes (another lens of the same docs page, the page already shown)
    // update the current frame without swapping it; the caller then waits for
    // readiness itself.
    const timer = setTimeout(() => { removeEventListener('wb-frame-change', changed); resolve(-1); }, 4000);
    function changed() {
      removeEventListener('wb-frame-change', changed);
      requestAnimationFrame(() => requestAnimationFrame(() => { clearTimeout(timer); resolve(performance.now() - start); }));
    }
    addEventListener('wb-frame-change', changed);
    if (location.hash === '#' + hash) { clearTimeout(timer); removeEventListener('wb-frame-change', changed); resolve(0); return; }
    location.hash = hash;
  });
  window.__labActive = () => document.querySelector('iframe.is-active');
  if (/lab-trace/.test(location.search)) window.__wbTrace = true;
  window.__labWin = () => { try { return __labActive().contentWindow; } catch (_) { return null; } };
}`;

async function evaluate(target, expression, timeout = 90000) {
  const result = await target.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }, { timeout });
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text || 'The page threw');
  return result.result?.value;
}

async function until(target, expression, timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs;
  let last;
  while (Date.now() < deadline) {
    try { last = await evaluate(target, expression); if (last) return last; } catch (error) { last = String(error.message || error); }
    await sleep(25);
  }
  throw new Error('Timed out waiting for ' + expression.slice(0, 160) + (last ? ' (last: ' + String(last).slice(0, 160) + ')' : ''));
}

async function openCanvas(browser, url, query = '') {
  const target = await browser.createTarget();
  const consoleEntries = [];
  const requests = [];
  target.on('Runtime.exceptionThrown', event => consoleEntries.push({ level: 'exception', text: (event.exceptionDetails?.exception?.description || event.exceptionDetails?.text || '').slice(0, 400) }));
  target.on('Runtime.consoleAPICalled', event => {
    if (flag('trace') && /\[workbench\]/.test(String(event.args[0] && event.args[0].value))) log('    trace: ' + event.args.map(arg => arg.value ?? arg.description ?? '').join(' ').slice(0, 200));
    if (['error', 'warning', 'assert'].includes(event.type) || (event.type === 'info' && /\[workbench\]/.test(String(event.args[0] && event.args[0].value)))) consoleEntries.push({ level: event.type, text: event.args.map(arg => arg.value ?? arg.description ?? '').join(' ').slice(0, 400) });
  });
  const failures = [];
  const timed = [];
  target.on('Network.requestWillBeSent', event => { requests.push(event.request.url); timed.push({ url: event.request.url, at: Date.now() }); });
  target.on('Network.responseReceived', event => { if (event.response.status >= 400) failures.push(event.response.status + ' ' + event.response.url.replace(/^https?:\/\/[^/]+/, '').slice(0, 200)); });
  await target.send('Runtime.enable');
  await target.send('Network.enable');
  await target.send('Emulation.setDeviceMetricsOverride', { width: 1600, height: 1000, deviceScaleFactor: 1, mobile: false });
  await target.send('Page.addScriptToEvaluateOnNewDocument', { source: CANVAS_SCRIPT });
  await target.navigate(url + 'index.html' + query);
  await until(target, 'document.readyState === "complete" && !!window.__lab', 60000);
  return { target, consoleEntries, requests, failures, timed };
}

const address = (file, state = 'default', size = 'laptop') => `${file}:${state}@${size}`;

// What the active frame shows: readiness, error, text and the fixture's sentinels.
const FRAME_STATE = `(() => {
  const win = __labWin(); if (!win || !win.document || !win.document.body) return null;
  const root = win.document.documentElement;
  return { ready: win.__workbenchReady === true, error: win.__workbenchError || null, docs: !!win.__workbenchDocs,
    text: (win.document.body.innerText || '').slice(0, 4000), leaks: root.dataset.acmeLeaks ?? null,
    seen: root.dataset.acmeSeenPreviews ?? null, listeners: root.dataset.acmeListeners ?? null, href: win.location.href };
})()`;

// ---------------------------------------------------------------- checks

const fileText = file => fs.readFileSync(file, 'utf8');

async function walkPreviews(canvas, previews, record) {
  for (const preview of previews) {
    const started = Date.now();
    let outcome;
    try {
      await evaluate(canvas.target, `__labGo(${JSON.stringify(preview.address || address(preview.file))})`, 45000);
      outcome = await until(canvas.target, `(() => { const s = ${FRAME_STATE}; return s && (s.ready || s.error) ? s : null; })()`, 45000);
    } catch (error) {
      let state = null; try { state = await evaluate(canvas.target, FRAME_STATE); } catch (_) {}
      outcome = { ready: false, error: String(error.message || error), ...(state ? { shown: state.text.slice(0, 300), frameError: state.error } : {}) };
    }
    const expected = preview.expect || {};
    const textOk = !expected.text || expected.text.every(part => (outcome.text || '').includes(part));
    const ok = preview.broken ? !!outcome.error : (!!outcome.ready && !outcome.error && textOk);
    record({ check: 'render', preview: preview.id, adapter: preview.adapter, ok, ms: Date.now() - started,
      error: outcome.error ? String(outcome.error).slice(0, 500) : null, leaks: outcome.leaks || null,
      missingText: textOk ? null : expected.text.filter(part => !(outcome.text || '').includes(part)), shown: outcome.ready ? undefined : (outcome.shown || (outcome.text || '').slice(0, 200)) });
  }
}

// What each stage edit must do, from the performance spec: style and React
// component edits apply in place with state kept; edits that cannot apply in
// place may reload; docs pages update without reloading.
const EXPECT = {
  cssGlobal: { how: ['hot update'], keepState: true },
  cssModule: { how: ['hot update'], keepState: true },
  vueScoped: { how: ['hot update'], keepState: true },
  reactComponent: { how: ['hot update'], keepState: true },
  nonComponentModule: { how: ['hot update', 'in-place re-render'] },
  vueTemplate: { how: ['hot update'], keepState: true },
  vueScript: { how: ['hot update', 'in-place re-render'] },
  htmlMarkup: { how: ['hot update', 'in-place re-render'] },
  htmlStylesheet: { how: ['hot update'] },
  astroComponent: { how: ['hot update', 'in-place re-render'] },
  docsMarkdown: { how: ['in place'] },
  docsExample: { how: ['in place'] },
  definition: { how: null },
  environment: { how: null },
  config: { how: null },
  shared: { how: ['hot update', 'in-place re-render'], hidden: { preview: 'pages/page-b', selector: '[data-acme="title"]', text: 'Acme / Page B' } },
};

const observeExpression = (observe, value) => `(() => { const w = __labWin(); const el = w && w.document && w.document.querySelector(${JSON.stringify(observe.selector)});
  if (!el) return false;
  ${observe.style ? `return w.getComputedStyle(el)[${JSON.stringify(observe.style)}] === ${JSON.stringify(value)};`
    : observe.attribute ? `return el.getAttribute(${JSON.stringify(observe.attribute)}) === ${JSON.stringify(value)};`
    : `return (el.textContent || '').includes(${JSON.stringify(value)});`} })()`;

async function go(canvas, target) {
  await evaluate(canvas.target, `__labGo(${JSON.stringify(target)})`, 45000);
  return until(canvas.target, `(() => { const s = ${FRAME_STATE}; return s && (s.ready || s.error) ? s : null; })()`, 45000);
}

// Interacts with the active page (counter and note), applies an edit, waits
// for its observable result, and reports whether the document reloaded, the
// preview re-rendered, and the page state survived. The edit is then undone
// and the page waits until it shows the original again.
async function editCheck(canvas, root, edit, stateSelectors, record) {
  const file = path.join(root, edit.file);
  const original = fileText(file);
  const expect = EXPECT[edit.name] || { how: null };
  const prepare = `(() => {
    const frame = __labActive(); const win = frame.contentWindow; const doc = win.document;
    win.__labToken = Math.random().toString(36).slice(2); win.__labContext = win.workbench || null;
    frame.dataset.labId = frame.dataset.labId || Math.random().toString(36).slice(2);
    const sel = ${JSON.stringify(stateSelectors || null)};
    if (sel) {
      const button = doc.querySelector(sel.increment); const input = doc.querySelector(sel.note);
      if (button) { button.click(); button.click(); button.click(); }
      if (input) { const setter = Object.getOwnPropertyDescriptor(win.HTMLInputElement.prototype, 'value').set; setter.call(input, 'kept text'); input.dispatchEvent(new win.Event('input', { bubbles: true })); }
    }
    return { token: win.__labToken, frame: frame.dataset.labId };
  })()`;
  const readState = `(() => { const doc = __labWin().document; const sel = ${JSON.stringify(stateSelectors || null)}; if (!sel) return null;
    const b = doc.querySelector(sel.count); const i = doc.querySelector(sel.note);
    return { count: b ? (b.textContent.match(/\\d+/) || [null])[0] : null, note: i ? i.value : null }; })()`;
  const before = await evaluate(canvas.target, prepare);
  await sleep(200);
  const stateBefore = await evaluate(canvas.target, readState);
  const started = Date.now();
  try {
    if (edit.break) {
      fs.writeFileSync(file, original.replace(edit.break.find, edit.break.replace));
      await until(canvas.target, `(() => { const w = __labWin(); return !!(w && w.__workbenchError); })()`, 30000);
      const shownMs = Date.now() - started;
      const broken = fileText(file);
      fs.writeFileSync(file, broken.replace(edit.fix.find, edit.fix.replace));
      await until(canvas.target, `(() => { const w = __labWin(); return !!(w && !w.__workbenchError && w.__workbenchReady); })() && ${observeExpression(edit.observe, edit.observe.text)}`, 30000);
      record({ check: 'edit', edit: edit.name, ok: true, errorShownMs: shownMs, ms: Date.now() - started });
      return;
    }
    if (!original.includes(edit.find)) throw new Error('find text not in ' + edit.file);
    fs.writeFileSync(file, original.replace(edit.find, edit.replace));
    const value = edit.observe.text ?? edit.observe.expect;
    await until(canvas.target, observeExpression(edit.observe, value), 30000);
    const ms = Date.now() - started;
    await sleep(250);
    const after = await evaluate(canvas.target, `(() => { const frame = __labActive(); const win = frame.contentWindow;
      return { sameFrame: frame.dataset.labId === ${JSON.stringify(before.frame)}, sameDocument: win.__labToken === ${JSON.stringify(before.token)},
        sameRender: !!win.__labContext && win.workbench === win.__labContext, docs: !!win.__workbenchDocs, error: win.__workbenchError || null }; })()`);
    const stateAfter = await evaluate(canvas.target, readState);
    const how = !after.sameFrame || !after.sameDocument ? 'document reload' : after.docs ? 'in place' : after.sameRender ? 'hot update' : 'in-place re-render';
    const stateKept = stateBefore ? stateAfter && stateAfter.count === stateBefore.count && stateAfter.note === stateBefore.note : null;
    let hidden = null;
    if (expect.hidden) {
      const hiddenPreview = edit.previews.find(item => item.id === expect.hidden.preview);
      const shownStarted = Date.now();
      await go(canvas, address(hiddenPreview.file));
      await until(canvas.target, observeExpression({ selector: expect.hidden.selector }, expect.hidden.text), 30000);
      hidden = { ok: true, ms: Date.now() - shownStarted };
    }
    const ok = (!expect.how || expect.how.includes(how)) && (!expect.keepState || stateKept === true);
    record({ check: 'edit', edit: edit.name, ok, ms, how, stateKept, expected: expect.how, keepState: !!expect.keepState, hidden,
      frameError: after.error ? String(after.error).slice(0, 200) : null });
  } catch (error) {
    let shown = null; try { shown = await evaluate(canvas.target, FRAME_STATE); } catch (_) {}
    let actual = null; try { actual = await evaluate(canvas.target, `(() => { const w = __labWin(); const el = w.document.querySelector(${JSON.stringify(edit.observe.selector)}); if (!el) return 'missing'; return ${edit.observe.style ? `w.getComputedStyle(el)[${JSON.stringify(edit.observe.style)}]` : edit.observe.attribute ? `el.getAttribute(${JSON.stringify(edit.observe.attribute)})` : 'el.textContent'} + ' | el: ' + el.outerHTML.slice(0, 200) + ' | styles: ' + Array.from(w.document.styleSheets).map(sheet => { let rules = ''; try { rules = Array.from(sheet.cssRules).map(rule => rule.cssText).join(' ').slice(0, 300); } catch (_) {} return (sheet.href || 'inline').slice(-40) + (sheet.disabled ? '(off)' : '') + ': ' + rules; }).join(' || '); })()`); } catch (_) {}
    record({ check: 'edit', edit: edit.name, ok: false, actual, error: String(error.message || error).slice(0, 400), shown: shown ? { ready: shown.ready, error: shown.error && shown.error.slice(0, 300), text: shown.text.slice(0, 200) } : null });
  } finally {
    fs.writeFileSync(file, original);
    await sleep(400);
  }
}

// Two saves 150 ms apart: the page must end with the second.
async function doubleSave(canvas, root, record) {
  const file = path.join(root, 'src/pages/page-a/Interactive.tsx');
  const original = fileText(file);
  const started = Date.now();
  try {
    fs.writeFileSync(file, original.replace('Revision 0', 'Revision 1'));
    await sleep(150);
    fs.writeFileSync(file, original.replace('Revision 0', 'Revision 2'));
    await until(canvas.target, observeExpression({ selector: '[data-acme="marker"]' }, 'Revision 2'), 30000);
    await sleep(2000);
    const still = await evaluate(canvas.target, observeExpression({ selector: '[data-acme="marker"]' }, 'Revision 2'));
    record({ check: 'edit', edit: 'doubleSave', ok: !!still, ms: Date.now() - started });
  } catch (error) { record({ check: 'edit', edit: 'doubleSave', ok: false, error: String(error.message || error).slice(0, 300) }); }
  finally { fs.writeFileSync(file, original); await sleep(400); }
}

// ------------------------------------------------------------------- run

async function runVariant(variant, fixture, tools) {
  const results = [];
  const record = entry => { results.push({ variant, ...entry }); log(`  ${variant} ${entry.check} ${entry.preview || entry.edit || entry.name || ''}: ${entry.ok ? 'pass' : 'FAIL'}${entry.how ? ' (' + entry.how + (entry.stateKept === false ? ', state lost' : '') + ')' : ''}${entry.ms !== undefined ? ' ' + entry.ms + ' ms' : ''}${entry.error ? ' — ' + String(entry.error).slice(0, 160) : ''}`); };
  const snap = snapshot(variant === '3' ? '2' : variant);
  const cache = path.join(runDir, 'cache-' + variant);
  const digestBefore = treeDigest(fixture.root);
  const listBefore = treeList(fixture.root);
  const server = await startServer(snap.dir, fixture.root, cache, tools);
  const browser = new Browser({});
  await browser.launch();
  try {
    const canvas = await openCanvas(browser, server.url, '?' + [variant === '3' ? 'lab-warm-document=1' : '', flag('trace') ? 'lab-trace=1' : ''].filter(Boolean).join('&'));
    if (fixture.selected) {
      const index = await (await fetch(new URL('/_workbench/previews/index', server.url))).json();
      fixture.manifest.previews = (index.previews || []).map(preview => ({ id: preview.id, adapter: preview.adapter, file: preview.file }));
      record({ check: 'index', ok: Array.isArray(index.previews), previews: fixture.manifest.previews.length, errors: (index.errors || []).length });
    }
    const previews = fixture.manifest.previews;
    const pages = [...previews, ...(fixture.manifest.docs || []).flatMap(page => [{ id: page.id, adapter: 'docs', address: page.address },
      ...Object.keys(page.lenses || {}).slice(1).map(lens => ({ id: page.id + '~' + lens, adapter: 'docs', address: page.address + '~' + lens }))])];
    await walkPreviews(canvas, pages, record);
    // Variant 3 and reused hosts: walk again in a different order and read the sentinels.
    await walkPreviews(canvas, [...pages].reverse().filter(preview => !preview.broken), entry => record({ ...entry, check: 'render-again' }));
    // Plugin checks: each project plugin's visible effect.
    for (const check of fixture.manifest.pluginChecks || []) {
      const preview = previews.find(item => item.id === check.preview);
      try { await go(canvas, address(preview.file)); await until(canvas.target, observeExpression({ selector: check.selector }, check.text), 15000); record({ check: 'plugin', name: check.plugin + ' on ' + check.preview, ok: true }); }
      catch (error) { record({ check: 'plugin', name: check.plugin + ' on ' + check.preview, ok: false, error: String(error.message || error).slice(0, 200) }); }
    }
    const docsPages = fixture.manifest.docs || [];
    // Starting state: every probe reads the value the shipping compiler shows,
    // so a style or text the build drops fails here, before any edit.
    for (const [name, edit] of Object.entries(fixture.manifest.edits || {})) {
      if (!edit.observe || edit.break) continue;
      const page = edit.docs ? docsPages.find(item => item.id === edit.docs) : previews.find(candidate => candidate.id === edit.preview);
      try { await go(canvas, edit.docs ? (edit.address || page.address) : address(page.file)); await until(canvas.target, observeExpression(edit.observe, edit.observe.before), 10000); record({ check: 'start', name, ok: true }); }
      catch (error) { record({ check: 'start', name, ok: false, error: String(error.message || error).slice(0, 160) }); }
    }

    for (const [name, edit] of Object.entries(fixture.manifest.edits || {})) {
      if (settings.only && !settings.only.includes(name)) continue;
      const preview = edit.docs ? docsPages.find(item => item.id === edit.docs) : previews.find(candidate => candidate.id === edit.preview);
      if (!preview) { record({ check: 'edit', edit: name, ok: false, error: 'no page ' + (edit.preview || edit.docs) }); continue; }
      const pageAddress = edit.docs ? (edit.address || preview.address) : address(preview.file);
      try { const shown = await go(canvas, pageAddress); if (!shown.ready) throw new Error(shown.error || 'not ready'); }
      catch (error) { record({ check: 'edit', edit: name, ok: false, error: 'page did not load: ' + String(error.message || error).slice(0, 200) }); continue; }
      await editCheck(canvas, fixture.root, { name, ...edit, previews }, edit.docs ? null : fixture.manifest.state?.[preview.interactive], record);
      // Let the undo land before the next edit starts.
      if (edit.observe && !edit.break) { try { await go(canvas, pageAddress); await until(canvas.target, observeExpression(edit.observe, edit.observe.before), 30000); } catch (error) { let shown = null; try { shown = await evaluate(canvas.target, FRAME_STATE); } catch (_) {} record({ check: 'edit-undo', edit: name, ok: false, error: String(error.message || error).slice(0, 200), shown: shown && { ready: shown.ready, error: shown.error && shown.error.slice(0, 200), text: shown.text.slice(0, 200), href: shown.href } }); } }
    }
    if (!settings.only || settings.only.includes('doubleSave')) {
      const pageA = previews.find(item => item.id === 'pages/page-a');
      if (pageA) { await go(canvas, address(pageA.file)); await doubleSave(canvas, fixture.root, record); }
    }
    // F3: export and portable output still build (they stay on esbuild).
    try {
      const started = Date.now();
      const answer = await fetch(new URL('/_workbench/previews/export', server.url));
      const body = await answer.json();
      record({ check: 'export', ok: answer.ok && Array.isArray(body.files) && body.files.length > 0, ms: Date.now() - started,
        files: body.files ? body.files.length : 0, warnings: (body.warnings || []).slice(0, 10), error: body.error || null });
    } catch (error) { record({ check: 'export', ok: false, error: String(error.message || error) }); }
    // Polling shows as revision requests while nothing changes: watch an idle
    // window with every page still open.
    const idleStart = Date.now();
    await sleep(5000);
    const polls = canvas.timed.filter(item => item.at >= idleStart && /\/revision\?/.test(item.url)).length;
    record({ check: 'no-polling', ok: polls === 0, idleMs: 5000, polls, urls: [...new Set(canvas.requests.filter(url => /\/revision\?/.test(url)).map(url => url.replace(/^https?:\/\/[^/]+/, '').slice(0, 120)))].slice(0, 5) });
    record({ check: 'console', ok: !canvas.consoleEntries.length, entries: canvas.consoleEntries.slice(0, 40), count: canvas.consoleEntries.length });
    // Expected: the canvas's optional files, the fixture's isolation probe (a
    // page fetches another page's mock route and expects no answer), and the
    // control's answers for its broken and syntax-error previews.
    const expectedRequest = item => /workbench\.local\.yaml|favicon\.ico|\/api\/only-/.test(item);
    const unexpected = canvas.failures.filter(item => !expectedRequest(item));
    record({ check: 'failed-requests', ok: variant === '0' || !unexpected.length, unexpected: unexpected.slice(0, 40), expected: canvas.failures.filter(expectedRequest).length });
  } finally {
    await browser.close().catch(() => {});
    await server.close();
  }
  const digestAfter = treeDigest(fixture.root);
  const listAfter = treeList(fixture.root);
  record({ check: 'no-project-writes', ok: digestBefore === digestAfter, added: listAfter.filter(item => !listBefore.includes(item)).slice(0, 20) });
  const worker = server.logs.find(entry => entry.log.event === 'preview.worker.starting');
  record({ check: 'worker-runtime', ok: true, runtime: worker ? worker.log.details.runtime : 'unknown' });
  return { results, overlay: snap.overlay, logs: server.logs.filter(entry => /pipeline/.test(entry.log.event)).map(entry => entry.log).slice(-80) };
}

const release = acquireLabLock('rspack-pipeline stage 1');
const report = { plan: 'labs/rspack-pipeline/README.md', stage: 1, settings: { ...settings, project: settings.project ? 'selected (path withheld)' : null }, host: hostSnapshot(), started: new Date().toISOString(), variants: {} };
try {
  settings.only = option('only', null) ? option('only').split(',') : null;
  const tools = installTools();
  const fixture = await prepareFixture();
  report.fixture = fixture.selected ? { size: 'selected' } : { size: fixture.manifest.size, previews: fixture.manifest.previews.length };
  for (const variant of settings.variants) {
    log('Variant ' + variant);
    report.variants[variant] = await runVariant(variant, fixture, tools);
  }
} finally {
  // Console entries the control also produces are not the variant's.
  const line = entry => entry.text.split('\n')[0].replace(/:\d+(?:\/[^\s]*)?/g, '').replace(/https?:\/\/[^/\s]+/g, '');
  const control = report.variants['0'] && report.variants['0'].results.find(entry => entry.check === 'console');
  const baseline = new Set(control ? control.entries.map(line) : ['[workbench] no handoff available Error: clipboard handoff is unavailable', 'TypeError: Cannot read properties of null (reading \'implementations\')']);
  for (const variant of Object.values(report.variants)) {
    const entry = variant.results.find(item => item.check === 'console');
    // The prototype's own reload notices (level info) are expected behavior.
    // Also expected: the syntax-error edit's build error, which the page shows.
    if (entry) { entry.extra = entry.entries.filter(item => !baseline.has(line(item))); entry.ok = entry.extra.filter(item => item.level !== 'info' && !/Module build failed[\s\S]*Syntax Error/.test(item.text)).length === 0; }
  }
  report.finished = new Date().toISOString();
  const redact = value => JSON.stringify(value, null, 2).split(runDir).join('<run>').split(os.homedir()).join('~');
  fs.writeFileSync(path.join(runDir, 'report.json'), redact(report));
  if (resultsDir) { fs.mkdirSync(resultsDir, { recursive: true }); fs.writeFileSync(path.join(resultsDir, 'report.json'), redact(report)); }
  log('Report: ' + path.join(runDir, 'report.json'));
  if (!flag('keep')) for (const name of ['snapshots', 'tools']) fs.rmSync(path.join(runDir, name), { recursive: true, force: true });
  release();
}
