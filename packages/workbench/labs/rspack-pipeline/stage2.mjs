// Rspack pipeline lab runner, stage 2 (performance). See README.md, "Stage 2
// method", for the cells this runs.
//
//   node labs/rspack-pipeline/stage2.mjs --set <label> [--results <new directory>]
//     [--groups start,nav,edit,parity] [--variants 0,1,2,3,D2,D3] [--fixtures ...]
//     [--samples 10] [--warmups 3] [--smoke] [--keep]
//
// Each variant runs from a snapshot of this package (see lib.mjs). Fixtures,
// tools, caches and browser profiles live in a new run directory under the
// system temporary folder. Product code and the fixtures' sources are never
// changed, apart from the timed edits, which are undone.
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { install } from '../fixtures/install.mjs';
import { acquireLabLock, hostSnapshot } from '../fixtures/host.mjs';
import { LOCKFILE } from '../fixtures/acme-app.mjs';
import { address, directorySize, evaluate, here, launchBrowser, measuredFiles, openCanvas, percentile, select, sleep, snapshot, startServer, traffic, treeDigest, until, workbench } from './lib.mjs';

const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf('--' + name); return index === -1 ? fallback : args[index + 1]; };
const flag = name => args.includes('--' + name);
const smoke = flag('smoke');
const settings = {
  set: option('set', smoke ? 'smoke' : null),
  groups: option('groups', 'start,nav,edit,parity').split(','),
  variants: option('variants', '0,1,2,3,D2,D3').split(','),
  fixtures: option('fixtures', 'small,large-system,large-web,huge,formats').split(','),
  samples: Number(option('samples', smoke ? 2 : 10)),
  warmups: Number(option('warmups', smoke ? 1 : 3)),
};
if (!settings.set) { console.error('Name the run set with --set <label>.'); process.exit(2); }
const resultsDir = option('results', null);
if (resultsDir && fs.existsSync(resultsDir)) { console.error('Refusing to overwrite ' + resultsDir); process.exit(2); }

const runParent = path.join(os.tmpdir(), 'canonic-workbench-labs', 'rspack-pipeline');
fs.mkdirSync(runParent, { recursive: true });
const runDir = path.join(fs.realpathSync(runParent), 'stage2-' + new Date().toISOString().replace(/[:.]/g, '-'));
fs.mkdirSync(runDir);
const log = message => console.log(`[${new Date().toISOString().slice(11, 19)}] ${message}`);

// Variants: which snapshot, environment and canvas flags each uses.
const VARIANTS = {
  0: { snapshot: '0' },
  1: { snapshot: '1' },
  2: { snapshot: '2', pipeline: true },
  3: { snapshot: '2', pipeline: true, query: 'lab-warm-document=1' },
  D2: { snapshot: '2', pipeline: true, env: { WORKBENCH_LAB_NO_PREWARM: '1' } },
  D3: { snapshot: '2', pipeline: true, env: { WORKBENCH_LAB_READY_ON_MOUNT: '1' } },
};

// The cells, by group: which fixtures and variants each runs (README, Stage 2 method).
const MATRIX = {
  start: { fixtures: ['small', 'large-system', 'huge'], variants: ['0', '2', 'D2'] },
  nav: { fixtures: ['small', 'large-system', 'large-web', 'huge', 'formats'], variants: ['0', '1', '2', '3', 'D3'], huge: ['0', '2', '3'], formats: ['0', '1', '2', '3'] },
  edit: { fixtures: ['small', 'large-system', 'formats'], variants: ['0', '2', '3'] },
  parity: { fixtures: ['large-system', 'formats'], variants: ['0', '1', '2', '3'] },
};

const results = [];
const checks = [];
const record = (entry) => { const host = hostSnapshot(); results.push({ ...entry, load: host.loadAverage[0] }); };
const check = (entry) => { checks.push(entry); if (!entry.ok) log(`  CHECK FAILED ${entry.variant} ${entry.fixture} ${entry.check}: ${JSON.stringify(entry).slice(0, 300)}`); };

// Seeded shuffle, so the variant order differs per group and per run set.
function shuffled(items, seedText) {
  const out = [...items];
  let seed = crypto.createHash('sha256').update(seedText).digest().readUInt32LE(0);
  const rand = () => { seed = (seed + 0x6D2B79F5) >>> 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
  for (let index = out.length - 1; index > 0; index--) { const swap = Math.floor(rand() * (index + 1)); [out[index], out[swap]] = [out[swap], out[index]]; }
  return out;
}

// ---------------------------------------------------------------- fixtures

const fixtures = {};
async function fixture(size) {
  if (fixtures[size]) return fixtures[size];
  const root = path.join(runDir, 'fixtures', size);
  log(`Generating and installing the ${size} fixture`);
  const module = size === 'formats' ? await import('../fixtures/formats.mjs') : await import('../fixtures/acme-app.mjs');
  const manifest = module.generate(root, { size, seed: 1 });
  install(root, path.join(here, '..', 'fixtures', 'locks', (size === 'formats' ? 'formats' : LOCKFILE[size]) + '.package-lock.json'));
  fixtures[size] = { size, root, manifest, digest: treeDigest(root) };
  return fixtures[size];
}

let toolsDir = null;
function tools() {
  if (toolsDir) return toolsDir;
  toolsDir = path.join(runDir, 'tools');
  fs.mkdirSync(toolsDir);
  fs.copyFileSync(path.join(here, 'tools', 'package.json'), path.join(toolsDir, 'package.json'));
  log('Installing lab tools');
  install(toolsDir, path.join(here, 'tools', 'package-lock.json'));
  return toolsDir;
}

function serverFor(variant, fx, cache) {
  const spec = VARIANTS[variant];
  return startServer({ snapshotDir: snapshot(runDir, spec.snapshot), root: fx.root, cache, env: { WORKBENCH_LAB_TOOLS: tools(), ...(spec.env || {}) }, log });
}

const query = variant => (VARIANTS[variant].query ? '?' + VARIANTS[variant].query : '');

// What shows that a page is on screen: its own element, or for the formats
// fixture its sentinel class, or a docs page's marker.
function pageCheck(fx, page) {
  if (page.selector) return `!!doc.querySelector(${JSON.stringify(page.selector)})`;
  if (page.docs) return `!!win.__workbenchDocs && !!doc.querySelector('[data-acme="docs-marker"]')`;
  if (page.owner === 'landing') return "doc.body.classList.contains('acme-landing') && !!doc.querySelector('[data-acme-probe=\"html\"]')";
  if (page.owner) return `doc.body.classList.contains(${JSON.stringify('acme-owner-' + page.owner)})`;
  throw new Error('No visible check for ' + page.id);
}
const pageHash = page => page.address || address(page.file);
const working = fx => fx.manifest.previews.filter(page => !page.broken);

async function waitWarm(server, variant) {
  // The control builds on request; the pipeline reports when pre-warming ends.
  await fetch(new URL('/_workbench/previews/index', server.url)).then(answer => answer.json()).catch(() => null);
  if (!VARIANTS[variant].pipeline || variant === 'D2') return;
  const deadline = Date.now() + 600000;
  while (Date.now() < deadline) {
    try { const status = await (await fetch(new URL('/_workbench/previews/rspack/status', server.url))).json(); if (status.prewarmed) return status.prewarmed; } catch (_) {}
    await sleep(100);
  }
  throw new Error('Pre-warming did not finish');
}

// Opens the canvas on `page` and resolves when it is visible, timed from navigation.
async function firstVisible(browser, server, variant, fx, page, options = {}) {
  const canvas = await openCanvas(browser, server.url, { query: query(variant), hash: pageHash(page), latencyMs: options.latencyMs || 0 });
  const shown = await select(canvas.target, null, pageCheck(fx, page), { timeout: options.timeout || 300000, action: '/* navigating */', fromNavigation: true });
  return { canvas, shown };
}

// ---------------------------------------------------------------- start (A1–A5)

async function startGroup(fx, variant) {
  const first = working(fx)[0];
  const total = settings.warmups + settings.samples;
  for (let index = 0; index < total; index++) {
    const warmup = index < settings.warmups;
    const cache = path.join(runDir, 'start', variant + '-' + fx.size + '-' + index);
    const profile = path.join(runDir, 'profiles', 'start-' + variant + '-' + fx.size + '-' + index);
    for (const sequence of ['A1', 'A2', 'A4']) {
      const server = await serverFor(variant, fx, cache);
      const browser = await launchBrowser(profile);
      try {
        let warm = null;
        if (sequence === 'A4') warm = await waitWarm(server, variant);
        const { canvas, shown } = await firstVisible(browser, server, variant, fx, first);
        const fromNavigation = shown.ms;
        const entry = { group: 'start', fixture: fx.size, variant, sample: index, warmup, error: shown.error || null, peakRss: server.peakRss() };
        if (sequence === 'A4') record({ ...entry, sequence: 'A4', cell: 'after pre-warm', ms: fromNavigation, prewarm: warm });
        else {
          const sinceSpawn = shown.ms == null ? null : canvas.navigated - server.spawned + fromNavigation;
          record({ ...entry, sequence, cell: sequence === 'A1' ? 'cold, from server start' : 'hot, from server start', ms: sinceSpawn, serverStartMs: server.urlAt - server.spawned });
          // A3: the same open, timed from the server reporting its URL.
          record({ ...entry, sequence: sequence === 'A1' ? 'A3-cold' : 'A3', cell: 'from URL', ms: shown.ms == null ? null : canvas.navigated - server.urlAt + fromNavigation });
        }
        if (shown.error) log(`  ${variant} ${fx.size} ${sequence} #${index}: ${shown.error}`);
        if (index === 0 && sequence === 'A1') check({ check: 'console-start', variant, fixture: fx.size, ok: true, entries: canvas.consoleEntries.slice(0, 10) });
      } finally {
        await browser.close().catch(() => {});
        await server.close();
      }
      if (sequence === 'A1') record({ group: 'start', fixture: fx.size, variant, sample: index, warmup, sequence: 'cache', cell: 'compile cache bytes after cold start', value: ['canonic-workbench-previews', 'workbench-rspack-cache'].reduce((sum, name) => sum + directorySize(path.join(cache, name)), 0),
        other: Object.fromEntries(fs.readdirSync(cache).map(name => [name, directorySize(path.join(cache, name))])) });
    }
    log(`  start ${variant} ${fx.size} #${index} done`);
    fs.rmSync(profile, { recursive: true, force: true });
    fs.rmSync(cache, { recursive: true, force: true });
  }
}

// ---------------------------------------------------------------- navigation (N1–N8)

const STATE_CHECKS = {
  small: { alternate: 'accent', on: "!!doc.querySelector('.acme-tone-accent')", off: "!!doc.querySelector('.acme-tone-neutral')" },
  large: { alternate: 'empty', on: "!!doc.querySelector('[data-acme=\"empty\"]')", off: "!doc.querySelector('[data-acme=\"empty\"]') && !doc.body.innerText.includes('Loading')" },
};

async function readSentinels(target, page) {
  try {
    return await until(target, `(() => { const html = __labWin().document.documentElement; return html.dataset.acmeProbed === ${JSON.stringify(page.id)} ? { leaks: html.dataset.acmeLeaks || '', seen: html.dataset.acmeSeenPreviews || null, listeners: html.dataset.acmeListeners || null } : null; })()`, 3000);
  } catch (error) { return { error: 'no probe result' }; }
}

async function navGroup(fx, variant) {
  const pages = working(fx);
  const total = settings.warmups + settings.samples;
  const cache = path.join(runDir, 'nav', variant + '-' + fx.size);
  const server = await serverFor(variant, fx, cache);
  const base = { group: 'nav', fixture: fx.size, variant };
  const sessions = [];
  try {
    await waitWarm(server, variant);
    const first = pages[0];
    if (fx.size === 'formats') { await formatsNav(fx, variant, server, base, sessions); return; }
    const walk = pages.slice(1).filter(page => !page.mono);
    // N1: a second page right after the first, each sample in a new browser.
    for (let index = 0; index < total; index++) {
      const browser = await launchBrowser();
      try {
        const { canvas, shown } = await firstVisible(browser, server, variant, fx, first);
        if (shown.error) throw new Error(shown.error);
        const page = walk[index % walk.length];
        const mark = canvas.network.length;
        const r = await select(canvas.target, pageHash(page), pageCheck(fx, page));
        record({ ...base, sequence: 'N1', cell: 'second page', sample: index, warmup: index < settings.warmups, ms: r.ms, error: r.error || null, traffic: traffic(canvas, mark), page: page.id });
      } catch (error) { record({ ...base, sequence: 'N1', cell: 'second page', sample: index, warmup: index < settings.warmups, ms: null, error: String(error.message || error) }); }
      finally { await browser.close().catch(() => {}); }
    }
    // The main session: N2–N6 and F1 in one canvas.
    const browser = await launchBrowser();
    sessions.push(browser);
    const { canvas, shown } = await firstVisible(browser, server, variant, fx, first);
    if (shown.error) throw new Error('first page: ' + shown.error);
    await sleep(2000);
    const n2 = walk.slice(0, total), n3 = walk.slice(total, total * 2);
    for (const [sequence, list, pause] of [['N2', n2, 1000], ['N3', n3, 0]]) {
      for (const [index, page] of list.entries()) {
        if (pause) await sleep(pause);
        const mark = canvas.network.length;
        const r = await select(canvas.target, pageHash(page), pageCheck(fx, page));
        const entry = { ...base, sequence, cell: pause ? 'paced' : 'back to back', sample: index, warmup: index < settings.warmups, ms: r.ms, error: r.error || null, traffic: traffic(canvas, mark), atVisible: r.at || null, page: page.id };
        if (sequence === 'N2' && fx.manifest.sentinels) entry.sentinels = await readSentinels(canvas.target, page);
        record(entry);
        if (r.at && variant !== 'D3') check({ check: 'readiness', variant, fixture: fx.size, page: page.id, ok: r.at.fonts === 'loaded' && r.at.imagesIncomplete === 0, at: r.at });
        if (entry.sentinels) check({ check: 'isolation', variant, fixture: fx.size, page: page.id, ok: !entry.sentinels.error && !entry.sentinels.leaks && entry.sentinels.seen === '1' && entry.sentinels.listeners === '1', sentinels: entry.sentinels });
      }
    }
    const heap = await canvas.target.send('Runtime.getHeapUsage').catch(() => null);
    record({ ...base, sequence: 'heap', cell: 'after N2 and N3', value: heap ? heap.usedSize : null });
    // N4: return to retained pages, alternating between the last two.
    const [a, b] = n3.slice(-2).length === 2 ? n3.slice(-2) : [pages[0], walk[0]];
    for (let index = 0; index < total; index++) {
      const page = index % 2 ? b : a;
      const r = await select(canvas.target, pageHash(page), pageCheck(fx, page));
      record({ ...base, sequence: 'N4', cell: 'retained', sample: index, warmup: index < settings.warmups, ms: r.ms, error: r.error || null, page: page.id });
    }
    // N5: state and size on the current page.
    const current = b;
    const states = STATE_CHECKS[fx.size === 'small' ? 'small' : 'large'];
    await select(canvas.target, address(current.file), pageCheck(fx, current));
    for (let index = 0; index < total; index++) {
      const on = index % 2 === 0;
      const r = await select(canvas.target, address(current.file, on ? states.alternate : 'default'), `${pageCheck(fx, current)} && ${on ? states.on : states.off}`);
      record({ ...base, sequence: 'N5', cell: 'state', sample: index, warmup: index < settings.warmups, ms: r.ms, error: r.error || null });
    }
    await select(canvas.target, address(current.file), `${pageCheck(fx, current)} && ${states.off}`);
    for (let index = 0; index < total; index++) {
      const mobile = index % 2 === 0;
      const r = await select(canvas.target, address(current.file, 'default', mobile ? 'mobile' : 'laptop'), `${pageCheck(fx, current)} && ${mobile ? 'win.innerWidth < 700' : 'win.innerWidth > 900'}`);
      record({ ...base, sequence: 'N5', cell: 'size', sample: index, warmup: index < settings.warmups, ms: r.ms, error: r.error || null });
    }
    await select(canvas.target, address(current.file), `${pageCheck(fx, current)} && win.innerWidth > 900`);
    // N6: Reload.
    for (let index = 0; index < total; index++) {
      const token = 'reload' + index;
      const r = await select(canvas.target, null, `(win.__labToken !== ${JSON.stringify(token)} || win.workbench !== win.__labContext) && ${pageCheck(fx, current)}`,
        { action: `{ const w = __labWin(); w.__labToken = ${JSON.stringify(token)}; w.__labContext = w.workbench; document.getElementById('reload').click(); }` });
      record({ ...base, sequence: 'N6', cell: 'reload', sample: index, warmup: index < settings.warmups, ms: r.ms, error: r.error || null });
    }
    // F1: broken previews show their errors, then a working page loads.
    for (const broken of fx.manifest.previews.filter(page => page.broken)) {
      const r = await select(canvas.target, address(broken.file), 'false', { timeout: 30000, allowError: true });
      check({ check: 'error-shown', variant, fixture: fx.size, page: broken.id, ok: !!r.error, error: r.error });
    }
    const recovery = await select(canvas.target, pageHash(a), pageCheck(fx, a));
    check({ check: 'error-recovery', variant, fixture: fx.size, ok: !recovery.error && recovery.ms != null });
    await idleChecks(canvas, base);
    consoleCheck(canvas, base);
    if (fx.size === 'large-web') await fontCells(fx, variant, server, base);
  } finally {
    for (const browser of sessions) await browser.close().catch(() => {});
    record({ ...base, sequence: 'worker', cell: 'peak RSS', value: server.peakRss() });
    await server.close();
    fs.rmSync(cache, { recursive: true, force: true });
  }
}

// No revision requests while nothing changes, with every page still open.
async function idleChecks(canvas, base) {
  const mark = canvas.network.length;
  await sleep(5000);
  const polls = canvas.network.slice(mark).filter(item => /\/revision\?/.test(item.url)).length;
  check({ check: 'no-polling', variant: base.variant, fixture: base.fixture, ok: polls === 0, polls });
}

function consoleCheck(canvas, base) {
  check({ check: 'console', variant: base.variant, fixture: base.fixture, ok: true, entries: canvas.consoleEntries.slice(0, 40), count: canvas.consoleEntries.length, failures: canvas.failures.slice(0, 40) });
}

// N8 and the 150 ms latency cells on the web-font fixture.
async function fontCells(fx, variant, server, base) {
  const pages = working(fx);
  const total = settings.warmups + settings.samples;
  const mono = pages.filter(page => page.mono);
  const plain = pages.filter(page => !page.mono);
  for (const latencyMs of [0, 150]) {
    for (let index = 0; index < total; index++) {
      const browser = await launchBrowser();
      try {
        const { canvas, shown } = await firstVisible(browser, server, variant, fx, plain[0], { latencyMs });
        if (shown.error) throw new Error(shown.error);
        await sleep(1000);
        const page = mono[index % mono.length];
        const mark = canvas.network.length;
        const r = await select(canvas.target, pageHash(page), `${pageCheck(fx, page)} && doc.fonts.check("16px 'JetBrains Mono'")`);
        record({ ...base, sequence: 'N8', cell: latencyMs ? 'new font, +150 ms' : 'new font', sample: index, warmup: index < settings.warmups, ms: r.ms, error: r.error || null, traffic: traffic(canvas, mark), page: page.id });
      } catch (error) { record({ ...base, sequence: 'N8', cell: latencyMs ? 'new font, +150 ms' : 'new font', sample: index, warmup: index < settings.warmups, ms: null, error: String(error.message || error) }); }
      finally { await browser.close().catch(() => {}); }
    }
  }
  // N2 again with fonts and images 150 ms slower.
  const browser = await launchBrowser();
  try {
    const { canvas, shown } = await firstVisible(browser, server, variant, fx, plain[0], { latencyMs: 150 });
    if (shown.error) throw new Error(shown.error);
    await sleep(2000);
    for (const [index, page] of plain.slice(1, 1 + total).entries()) {
      await sleep(1000);
      const mark = canvas.network.length;
      const r = await select(canvas.target, pageHash(page), pageCheck(fx, page));
      record({ ...base, sequence: 'N2', cell: 'paced, +150 ms', sample: index, warmup: index < settings.warmups, ms: r.ms, error: r.error || null, traffic: traffic(canvas, mark), page: page.id });
    }
  } catch (error) { record({ ...base, sequence: 'N2', cell: 'paced, +150 ms', ms: null, error: String(error.message || error) }); }
  finally { await browser.close().catch(() => {}); }
}

// N7 on the formats fixture: switching between adapters, plus its leak sentinels.
async function formatsNav(fx, variant, server, base, sessions) {
  const byId = id => fx.manifest.previews.find(page => page.id === id);
  const docs = fx.manifest.docs[0];
  const cycle = [byId('pages/page-a'), byId('vue/counter'), byId('astro/card'), byId('html/landing'), byId('native/counter'), { id: docs.id, docs: true, address: docs.address }];
  const browser = await launchBrowser();
  sessions.push(browser);
  const { canvas, shown } = await firstVisible(browser, server, variant, fx, cycle[0]);
  if (shown.error) throw new Error(shown.error);
  await sleep(1000);
  const total = settings.warmups + settings.samples;
  for (let index = 0; index < total * 2; index++) {
    const page = cycle[(index + 1) % cycle.length];
    await sleep(500);
    const r = await select(canvas.target, pageHash(page), pageCheck(fx, page));
    record({ ...base, sequence: 'N7', cell: 'between adapters', sample: index, warmup: index < settings.warmups, ms: r.ms, error: r.error || null, page: page.id });
    if (!page.docs) {
      await sleep(250);
      const leaks = await evaluate(canvas.target, `(() => __labWin().document.documentElement.dataset.acmeLeaks || '')()`).catch(() => 'unreadable');
      check({ check: 'isolation', variant, fixture: fx.size, page: page.id, ok: !leaks, leaks });
    }
  }
  await idleChecks(canvas, base);
  consoleCheck(canvas, base);
}

// ---------------------------------------------------------------- edits (E1–E11)

// Edits on the large fixture's first page, each with a unique visible marker.
function largeEdits(fx) {
  const page = working(fx)[0];
  const dir = path.dirname(page.file);
  const slug = path.basename(dir);
  const Name = slug.split('-').map(part => part[0].toUpperCase() + part.slice(1)).join('');
  return [
    { id: 'E2', name: 'React component', file: `${dir}/Interactive.tsx`, apply: (text, m) => text.replace(/Revision [^<]*/, 'Revision ' + m), keepState: true },
    { id: 'E3', name: 'module exporting more than components', file: `${dir}/sections/Section0.tsx`, apply: (text, m) => text.replace(`title = '${Name}Section0'`, `title = '${Name}Section0 ${m}'`) },
    { id: 'E8', name: 'preview definition', file: page.file, apply: (text, m) => text.replace(/"label":"[^"]*"/, `"label":"${m}"`) },
    { id: 'E8', name: 'environment', file: 'src/workbench/environment.tsx', apply: (text, m) => text.replace('<Shell>{tree}</Shell>', `<Shell><span>${m}</span>{tree}</Shell>`) },
    { id: 'E9', name: 'module shared by every page', file: 'src/app/Shell.tsx', apply: (text, m) => text.replace('<nav aria-label="Features">', `<nav aria-label="Features"><span>${m}</span>`) },
  ].map(edit => ({ ...edit, page }));
}

function smallEdits(fx) {
  const page = working(fx)[0];
  const component = page.file.replace('.workbench.tsx', '.tsx');
  return [{ id: 'E2', name: 'React component', file: component, apply: (text, m) => text.replace("{open ? 'Hide' : 'Show'}", `{open ? 'Hide' : 'Show ${m}'}`), page }];
}

const STATE_READ = `(() => { const doc = __labWin().document; const counter = doc.querySelector('[data-acme="counter"]'); const note = doc.querySelector('[data-acme="note"]'); return { counter: counter ? counter.textContent : null, note: note ? note.value : null }; })()`;
const STATE_SET = `(() => { const win = __labWin(); const doc = win.document; const counter = doc.querySelector('[data-acme="counter"]'); if (counter) { counter.click(); }
  const note = doc.querySelector('[data-acme="note"]'); if (note) { Object.getOwnPropertyDescriptor(win.HTMLInputElement.prototype, 'value').set.call(note, 'kept'); note.dispatchEvent(new win.Event('input', { bubbles: true })); } return true; })()`;

// Writes the edit and returns the milliseconds until the canvas shows `check`.
async function timedEdit(target, file, next, check, timeout = 30000, { error = false } = {}) {
  const waiting = evaluate(target, `new Promise(resolve => {
    const start = Date.now();
    const tick = () => { let ok = false; try { const win = __labWin(); const doc = win.document; ok = ${error ? '' : 'win.__workbenchReady !== false && '}(${check}); } catch (_) {}
      if (ok) { requestAnimationFrame(() => requestAnimationFrame(() => resolve(Date.now()))); return; }
      if (Date.now() - start > ${timeout + 2000}) { resolve(null); return; }
      setTimeout(tick, 4); };
    tick();
  })`, timeout + 10000);
  await sleep(30);
  const written = Date.now();
  fs.writeFileSync(file, next);
  const visibleAt = await waiting;
  return visibleAt == null ? null : visibleAt - written;
}

async function editGroup(fx, variant) {
  const cache = path.join(runDir, 'edit', variant + '-' + fx.size);
  const server = await serverFor(variant, fx, cache);
  const base = { group: 'edit', fixture: fx.size, variant };
  const browser = await launchBrowser();
  const total = settings.warmups + settings.samples;
  try {
    await waitWarm(server, variant);
    if (fx.size === 'formats') { await formatsEdits(fx, variant, server, browser, base); return; }
    const edits = fx.size === 'small' ? smallEdits(fx) : largeEdits(fx);
    const { canvas, shown } = await firstVisible(browser, server, variant, fx, edits[0].page);
    if (shown.error) throw new Error(shown.error);
    await sleep(1500);
    for (const edit of edits) {
      const file = path.join(fx.root, edit.file);
      const original = fs.readFileSync(file, 'utf8');
      for (let index = 0; index < total; index++) {
        const marker = 'lab' + edit.id + 'x' + index + 'x' + crypto.randomBytes(2).toString('hex');
        const next = edit.apply(original, marker);
        if (next === original) throw new Error('Edit did not apply: ' + edit.name);
        let before = null;
        if (edit.keepState) { await evaluate(canvas.target, STATE_SET); await sleep(150); before = await evaluate(canvas.target, STATE_READ); }
        await evaluate(canvas.target, `__labWin().__labToken = 'edit'`);
        let ms = null, error = null;
        try { ms = await timedEdit(canvas.target, file, next, `doc.body.innerText.includes(${JSON.stringify(marker)})`); if (ms == null) error = 'edit not visible within 30 s'; }
        catch (thrown) { error = String(thrown.message || thrown); }
        const after = edit.keepState ? await evaluate(canvas.target, STATE_READ).catch(() => null) : null;
        const sameDocument = await evaluate(canvas.target, `__labWin().__labToken === 'edit'`).catch(() => false);
        record({ ...base, sequence: edit.id, cell: edit.name, sample: index, warmup: index < settings.warmups, ms, error, sameDocument, stateKept: edit.keepState ? !!(before && after && before.counter === after.counter && before.note === after.note) : null });
        fs.writeFileSync(file, original);
        try { await until(canvas.target, `(() => { const win = __labWin(); return win.__workbenchReady === true && !win.document.body.innerText.includes(${JSON.stringify(marker)}); })()`, 30000); }
        catch (thrown) { check({ check: 'edit-undo', variant, fixture: fx.size, ok: false, edit: edit.name, error: String(thrown.message || thrown).slice(0, 200) }); }
        await sleep(300);
      }
    }
    // Freshness: the page now matches a fresh load of the restored sources.
    await freshness(browser, server, variant, fx, canvas, edits[0].page, base);
    consoleCheck(canvas, base);
  } finally {
    await browser.close().catch(() => {});
    await server.close();
    fs.rmSync(cache, { recursive: true, force: true });
  }
}

const SIGNATURE = `(() => { const root = __labWin().document.getElementById('workbench-preview') || __labWin().document.body;
  const tags = []; const walk = node => { for (const child of node.children) { if (/^(STYLE|LINK|SCRIPT|META|TITLE|BASE)$/.test(child.tagName)) continue; tags.push(child.tagName); walk(child); } }; walk(root);
  return { text: root.innerText.replace(/\\s+/g, ' ').trim(), tags: tags.join(','), images: root.querySelectorAll('img').length }; })()`;

async function freshness(browser, server, variant, fx, canvas, page, base) {
  const live = await evaluate(canvas.target, SIGNATURE);
  const second = await openCanvas(browser, server.url, { query: query(variant), hash: pageHash(page) });
  // The fresh copy must finish its mocked data request before it is compared.
  const loaded = fx.size === 'small' || fx.size === 'formats' ? 'true' : STATE_CHECKS.large.off;
  const fresh = await select(second.target, null, `${pageCheck(fx, page)} && ${loaded}`, { action: '/* navigating */', timeout: 60000 });
  await sleep(300);
  const other = fresh.error ? null : await evaluate(second.target, SIGNATURE);
  await second.target.close().catch(() => {});
  // Live components keep state that a fresh load lacks: compare structure and drop counters.
  const strip = value => value && { ...value, text: value.text.replace(/Count \d+/g, 'Count n').replace(/\d{4}-\d{2}-\d{2}/g, 'date') };
  const a = strip(live), b = strip(other);
  const ok = !!b && JSON.stringify(a) === JSON.stringify(b);
  // Where the two first differ, so a failure can be told apart from a runner defect.
  let at = 0; if (a && b) while (at < a.text.length && a.text[at] === b.text[at]) at++;
  check({ check: 'freshness', variant, fixture: fx.size, ok, ...(ok ? {} : { textDiffersAt: at, live: a?.text.slice(Math.max(0, at - 80), at + 200), fresh: b?.text.slice(Math.max(0, at - 80), at + 200), tagsEqual: !!(a && b && a.tags === b.tags), images: [a?.images, b?.images] }) });
}

const observe = (o, value) => o.style ? `win.getComputedStyle(doc.querySelector(${JSON.stringify(o.selector)}))[${JSON.stringify(o.style)}] === ${JSON.stringify(value)}`
  : o.attribute ? `doc.querySelector(${JSON.stringify(o.selector)}).getAttribute(${JSON.stringify(o.attribute)}) === ${JSON.stringify(value)}`
  // Exact text when one value contains the other, or the shorter one matches both.
  : (o.text && o.before && (o.text.includes(o.before) || o.before.includes(o.text)))
    ? `(doc.querySelector(${JSON.stringify(o.selector)}).textContent || '').trim() === ${JSON.stringify(value)}`
    : `(doc.querySelector(${JSON.stringify(o.selector)}).textContent || '').includes(${JSON.stringify(value)})`;

const FORMAT_EDIT_IDS = { cssGlobal: 'E1', cssModule: 'E1', vueScoped: 'E1', reactComponent: 'E2', nonComponentModule: 'E3', vueTemplate: 'E4', vueScript: 'E4', htmlMarkup: 'E5', htmlStylesheet: 'E5', astroComponent: 'E6', docsMarkdown: 'E7', docsExample: 'E7', definition: 'E8', environment: 'E8', config: 'E8', shared: 'E9' };

// Each manifest edit is applied and undone repeatedly; both directions are
// timed samples, since each shows a different value.
async function formatsEdits(fx, variant, server, browser, base) {
  const docs = fx.manifest.docs[0];
  const total = settings.warmups + settings.samples;
  const pageOf = edit => edit.docs ? { id: docs.id, docs: true, address: edit.address || docs.address } : fx.manifest.previews.find(page => page.id === edit.preview);
  const start = pageOf(fx.manifest.edits.cssGlobal);
  const { canvas, shown } = await firstVisible(browser, server, variant, fx, start);
  if (shown.error) throw new Error(shown.error);
  await sleep(1000);
  for (const [name, edit] of Object.entries(fx.manifest.edits)) {
    if (edit.break || !FORMAT_EDIT_IDS[name]) continue;
    const page = pageOf(edit);
    const r = await select(canvas.target, pageHash(page), `${pageCheck(fx, page)} && ${observe(edit.observe, edit.observe.before)}`);
    if (r.error) { record({ ...base, sequence: FORMAT_EDIT_IDS[name], cell: name, ms: null, error: 'page: ' + r.error }); continue; }
    await sleep(500);
    const file = path.join(fx.root, edit.file);
    const original = fs.readFileSync(file, 'utf8');
    const changed = original.replace(edit.find, edit.replace);
    const after = edit.observe.text ?? edit.observe.expect;
    for (let index = 0; index < total; index++) {
      const forward = index % 2 === 0;
      let ms = null, error = null;
      try { ms = await timedEdit(canvas.target, file, forward ? changed : original, observe(edit.observe, forward ? after : edit.observe.before)); if (ms == null) error = 'edit not visible within 30 s'; }
      catch (thrown) { error = String(thrown.message || thrown); }
      record({ ...base, sequence: FORMAT_EDIT_IDS[name], cell: name, sample: index, warmup: index < settings.warmups, ms, error, direction: forward ? 'apply' : 'undo' });
      await sleep(name === 'config' ? 1500 : 400);
    }
    fs.writeFileSync(file, original);
    await sleep(name === 'config' ? 2000 : 500);
  }
  // E10 and E11 once: a syntax error and its fix, and two quick saves.
  const syntax = fx.manifest.edits.syntaxError;
  const page = pageOf(syntax);
  await select(canvas.target, pageHash(page), pageCheck(fx, page));
  const file = path.join(fx.root, syntax.file);
  const original = fs.readFileSync(file, 'utf8');
  try {
    const broken = original.replace(syntax.break.find, syntax.break.replace);
    const shownMs = await timedEdit(canvas.target, file, broken, '!!win.__workbenchError', 30000, { error: true }).catch(() => null);
    const recovered = await timedEdit(canvas.target, file, broken.replace(syntax.fix.find, syntax.fix.replace), `!win.__workbenchError && ${observe(syntax.observe, syntax.observe.text)}`, 30000).catch(() => null);
    check({ check: 'syntax-error', variant, fixture: fx.size, ok: shownMs != null && recovered != null, errorShownMs: shownMs, recoveredMs: recovered });
  } finally { fs.writeFileSync(file, original); }
  await sleep(1000);
  const interactive = path.join(fx.root, 'src/pages/page-a/Interactive.tsx');
  const text = fs.readFileSync(interactive, 'utf8');
  try {
    fs.writeFileSync(interactive, text.replace('Revision 0', 'Revision 1'));
    await sleep(150);
    fs.writeFileSync(interactive, text.replace('Revision 0', 'Revision 2'));
    let ok = false;
    try { await until(canvas.target, `(() => { const doc = __labWin().document; return (doc.querySelector('[data-acme="marker"]') || {}).textContent === 'Revision 2'; })()`, 30000); await sleep(2000); ok = await evaluate(canvas.target, `(__labWin().document.querySelector('[data-acme="marker"]') || {}).textContent === 'Revision 2'`); } catch (_) {}
    check({ check: 'double-save', variant, fixture: fx.size, ok });
  } finally { fs.writeFileSync(interactive, text); }
  await sleep(1000);
  await freshness(browser, server, variant, fx, canvas, start, base);
  consoleCheck(canvas, base);
}

// ---------------------------------------------------------------- render parity

async function parityGroup(fx, variant, renders) {
  const cache = path.join(runDir, 'parity', variant + '-' + fx.size);
  const server = await serverFor(variant, fx, cache);
  const browser = await launchBrowser();
  const out = [];
  try {
    await waitWarm(server, variant);
    let pages;
    if (fx.size === 'formats') {
      const docs = fx.manifest.docs[0];
      pages = [...working(fx).map(page => ({ page, state: 'default' })), { page: { id: docs.id, docs: true, address: docs.address }, state: null }];
    } else pages = working(fx).slice(0, 10).flatMap(page => ['default', 'empty'].map(state => ({ page, state })));
    const startPage = pages[0].page;
    const { canvas } = await firstVisible(browser, server, variant, fx, startPage);
    for (const { page, state } of pages) {
      for (const size of ['laptop', 'mobile']) {
        const hash = page.docs ? page.address : address(page.file, state, size);
        const extra = state === 'empty' ? STATE_CHECKS.large.on : state === 'default' && fx.size !== 'formats' ? STATE_CHECKS.large.off : 'true';
        const sizeCheck = page.docs ? 'true' : size === 'mobile' ? 'win.innerWidth < 700' : 'win.innerWidth > 900';
        const r = await select(canvas.target, hash, `${pageCheck(fx, page)} && ${extra} && ${sizeCheck}`);
        await sleep(400);
        const signature = r.error ? null : await evaluate(canvas.target, SIGNATURE).catch(() => null);
        out.push({ key: `${page.id}|${state}|${size}`, error: r.error || null, signature });
        if (page.docs) break;
      }
    }
  } finally {
    await browser.close().catch(() => {});
    await server.close();
    fs.rmSync(cache, { recursive: true, force: true });
  }
  renders[variant] = renders[variant] || {};
  renders[variant][fx.size] = out;
}

function parityChecks(renders) {
  for (const [variant, bySize] of Object.entries(renders)) {
    if (variant === '0') continue;
    for (const [size, items] of Object.entries(bySize)) {
      const control = new Map((renders['0']?.[size] || []).map(item => [item.key, item]));
      const strip = value => value && JSON.stringify({ ...value, text: value.text.replace(/\d{4}-\d{2}-\d{2}/g, 'date') });
      const mismatches = items.filter(item => !item.signature || strip(item.signature) !== strip(control.get(item.key)?.signature)).map(item => ({ key: item.key, error: item.error, variant: item.signature?.text.slice(0, 120), control: control.get(item.key)?.signature?.text.slice(0, 120) }));
      check({ check: 'render-parity', variant, fixture: size, ok: control.size > 0 && mismatches.length === 0, renders: items.length, mismatches: mismatches.slice(0, 10) });
    }
  }
}

// ---------------------------------------------------------------- report

function aggregate() {
  const cells = new Map();
  for (const item of results) {
    if (item.warmup || typeof item.ms !== 'number') continue;
    const key = [item.fixture, item.sequence, item.cell, item.variant].join('|');
    if (!cells.has(key)) cells.set(key, []);
    cells.get(key).push(item.ms);
  }
  return [...cells].map(([key, values]) => {
    const [fixture, sequence, cell, variant] = key.split('|');
    const errors = results.filter(item => !item.warmup && [item.fixture, item.sequence, item.cell, item.variant].join('|') === key && item.error).length;
    return { fixture, sequence, cell, variant, samples: values.length, errors, p50: percentile(values, 50), p95: percentile(values, 95), max: Math.max(...values) };
  });
}

function summary(report) {
  const lines = [`# Rspack pipeline stage 2, run set ${settings.set}`, '', smoke ? 'Smoke run: not evidence.' : `Samples per cell: ${settings.samples}, warmups: ${settings.warmups}.`, '',
    '| Fixture | Sequence | Cell | Variant | n | errors | p50 ms | p95 ms | max ms |', '| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: |'];
  const order = ['A1', 'A2', 'A3', 'A3-cold', 'A4', 'N1', 'N2', 'N3', 'N4', 'N5', 'N6', 'N7', 'N8', 'E1', 'E2', 'E3', 'E4', 'E5', 'E6', 'E7', 'E8', 'E9'];
  const sorted = [...report.aggregates].sort((a, b) => a.fixture.localeCompare(b.fixture) || order.indexOf(a.sequence) - order.indexOf(b.sequence) || a.cell.localeCompare(b.cell) || a.variant.localeCompare(b.variant));
  for (const cell of sorted) lines.push(`| ${cell.fixture} | ${cell.sequence} | ${cell.cell} | ${cell.variant} | ${cell.samples} | ${cell.errors} | ${Math.round(cell.p50)} | ${Math.round(cell.p95)} | ${Math.round(cell.max)} |`);
  lines.push('', '## Values', '');
  for (const item of results.filter(entry => entry.value !== undefined && !entry.warmup)) lines.push(`- ${item.fixture} ${item.variant} ${item.sequence} ${item.cell}${item.sample !== undefined ? ' #' + item.sample : ''}: ${item.value == null ? 'n/a' : (item.value / 1e6).toFixed(1) + ' MB'}`);
  lines.push('', '## Checks', '');
  const grouped = new Map();
  for (const item of checks) { const key = `${item.variant} ${item.fixture} ${item.check}`; const entry = grouped.get(key) || { pass: 0, fail: 0 }; entry[item.ok ? 'pass' : 'fail']++; grouped.set(key, entry); }
  for (const [key, value] of [...grouped].sort()) lines.push(`- ${key}: ${value.pass} pass, ${value.fail} fail`);
  return lines.join('\n') + '\n';
}

const release = acquireLabLock('rspack-pipeline stage 2 ' + settings.set);
const report = { plan: 'labs/rspack-pipeline/README.md', stage: 2, settings, smoke, host: { atStart: hostSnapshot(), platform: process.platform, arch: process.arch, osRelease: os.release(), cpu: os.cpus()[0].model, cpus: os.cpus().length, memoryGb: Math.round(os.totalmem() / 2 ** 30), node: process.version }, measured: measuredFiles(), started: new Date().toISOString() };
const renders = {};
let failure = null;
try {
  log('Run directory: ' + runDir);
  for (const group of ['start', 'nav', 'edit', 'parity']) {
    if (!settings.groups.includes(group)) continue;
    const spec = MATRIX[group];
    for (const size of spec.fixtures) {
      if (!settings.fixtures.includes(size)) continue;
      const fx = await fixture(size);
      const variants = shuffled((spec[size] || spec.variants).filter(variant => settings.variants.includes(variant) || (group === 'parity' && variant === '0')), settings.set + group + size);
      for (const variant of variants) {
        log(`${group}: ${size}, variant ${variant}`);
        try {
          if (group === 'start') await startGroup(fx, variant);
          if (group === 'nav') await navGroup(fx, variant);
          if (group === 'edit') await editGroup(fx, variant);
          if (group === 'parity') await parityGroup(fx, variant, renders);
        } catch (error) {
          log(`  ${group} ${size} ${variant} failed: ${error.stack || error}`);
          record({ group, fixture: size, variant, sequence: group, cell: 'group failed', ms: null, error: String(error.message || error).slice(0, 500) });
        }
      }
    }
  }
  parityChecks(renders);
} catch (error) { failure = String(error.stack || error); log('Run failed: ' + failure); }
finally {
  for (const fx of Object.values(fixtures)) check({ check: 'no-project-writes', variant: 'all', fixture: fx.size, ok: treeDigest(fx.root) === fx.digest });
  report.fixtures = Object.fromEntries(Object.values(fixtures).map(fx => [fx.size, { previews: fx.manifest.previews.length, files: fx.manifest.files, sourceBytes: fx.manifest.sourceBytes, fonts: fx.manifest.fonts }]));
  report.finished = new Date().toISOString();
  report.host.atEnd = hostSnapshot();
  report.failure = failure;
  report.results = results;
  report.checks = checks;
  report.aggregates = aggregate();
  const redact = text => text.split(runDir).join('<run>').split(workbench).join('<workbench>').split(os.homedir()).join('~');
  fs.writeFileSync(path.join(runDir, 'report.json'), redact(JSON.stringify(report, null, 2)));
  fs.writeFileSync(path.join(runDir, 'summary.md'), summary(report));
  if (resultsDir) { fs.mkdirSync(resultsDir, { recursive: true }); for (const name of ['report.json', 'summary.md']) fs.copyFileSync(path.join(runDir, name), path.join(resultsDir, name)); }
  console.log('\n' + summary(report));
  log('Report: ' + path.join(runDir, 'report.json'));
  if (!flag('keep')) for (const name of ['snapshots', 'tools', 'fixtures', 'start', 'nav', 'edit', 'parity', 'profiles']) {
    try { fs.rmSync(path.join(runDir, name), { recursive: true, force: true }); } catch (error) { console.error('Cleanup failed for ' + name + ': ' + error.message); }
  }
  release();
}
