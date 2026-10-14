// First page lab runner. See README.md for the plan.
//
//   node labs/first-page/run.mjs --set <label> [--results <new directory>]
//     [--variants 0,1,2,3,4,D2] [--fixtures small,large-system,huge,giant,formats]
//     [--samples 10] [--warmups 3] [--quiet 8] [--smoke] [--keep]
//
// Every variant runs from a snapshot of this package with overlay/ copied over
// it; WORKBENCH_LAB_START picks the variant. Fixtures, caches and browser
// profiles live in a new run directory under the system temporary folder.
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { install } from '../fixtures/install.mjs';
import { acquireLabLock, hostSnapshot, waitForQuiet } from '../fixtures/host.mjs';
import { LOCKFILE } from '../fixtures/acme-app.mjs';
import { address, evaluate, launchBrowser, measuredFiles, openCanvas, percentile, select, sleep, snapshot, startServer, treeDigest, until, workbench } from '../rspack-pipeline/lib.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf('--' + name); return index === -1 ? fallback : args[index + 1]; };
const flag = name => args.includes('--' + name);
const smoke = flag('smoke');
const settings = {
  set: option('set', smoke ? 'smoke' : null),
  variants: option('variants', '0,1,2,3,4,D2').split(','),
  fixtures: option('fixtures', 'small,large-system,huge,giant,formats').split(','),
  samples: Number(option('samples', smoke ? 1 : 10)),
  warmups: Number(option('warmups', smoke ? 0 : 3)),
  quiet: Number(option('quiet', 8)),
};
if (!settings.set) { console.error('Name the run set with --set <label>.'); process.exit(2); }
const resultsDir = option('results', null);
if (resultsDir && fs.existsSync(resultsDir)) { console.error('Refusing to overwrite ' + resultsDir); process.exit(2); }

const runParent = path.join(os.tmpdir(), 'canonic-workbench-labs', 'first-page');
fs.mkdirSync(runParent, { recursive: true });
const runDir = path.join(fs.realpathSync(runParent), settings.set + '-' + new Date().toISOString().replace(/[:.]/g, '-'));
fs.mkdirSync(runDir);
const log = message => console.log(`[${new Date().toISOString().slice(11, 19)}] ${message}`);

const VARIANTS = {
  0: { start: 'control' },
  1: { start: 'requested' },
  2: { start: 'lazy' },
  3: { start: 'first-compiler' },
  4: { start: 'cached-index' },
  D2: { start: 'requested', chunks: 'default' },
};

const results = [];
const checks = [];
const record = entry => { const host = hostSnapshot(); results.push({ ...entry, load: host.loadAverage[0] }); };
const check = entry => { checks.push(entry); if (!entry.ok) log(`  CHECK FAILED ${entry.variant} ${entry.fixture} ${entry.check}: ${JSON.stringify(entry).slice(0, 300)}`); };

function seeded(seedText) {
  let seed = crypto.createHash('sha256').update(seedText).digest().readUInt32LE(0);
  return () => { seed = (seed + 0x6D2B79F5) >>> 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
}
function shuffled(items, seedText) {
  const out = [...items]; const rand = seeded(seedText);
  for (let index = out.length - 1; index > 0; index--) { const swap = Math.floor(rand() * (index + 1)); [out[index], out[swap]] = [out[swap], out[index]]; }
  return out;
}

// ---------------------------------------------------------------- fixtures

const fixtures = {};
let toolsDir = null;
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

function serverFor(variant, fx, cache) {
  const spec = VARIANTS[variant];
  return startServer({ snapshotDir: snapshot(runDir, 'first-page', path.join(here, 'overlay')), root: fx.root, cache, log,
    env: { WORKBENCH_LAB_START: spec.start, ...(spec.chunks ? { WORKBENCH_LAB_CHUNKS: spec.chunks } : {}) } });
}

const working = fx => fx.manifest.previews.filter(page => !page.broken);
function pageCheck(page) {
  if (page.selector) return `!!doc.querySelector(${JSON.stringify(page.selector)})`;
  if (page.owner === 'landing') return "doc.body.classList.contains('acme-landing')";
  if (page.owner) return `doc.body.classList.contains(${JSON.stringify('acme-owner-' + page.owner)})`;
  return 'true';
}

async function lab(server) {
  try { return await (await fetch(new URL('/_workbench/previews/rspack/lab', server.url))).json(); } catch (_) { return null; }
}
async function fullBuild(server, timeoutMs = 900000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const state = await lab(server);
    if (state && state.prewarmed) return state;
    await sleep(200);
  }
  return null;
}

// Where the time to the first page went, from the worker's timeline.
function phases(state, server) {
  if (!state) return null;
  const at = event => (state.timeline.find(item => item.event === event) || {}).at;
  const created = at('pipeline.created');
  const since = value => value == null || created == null ? null : value - created;
  const firstBuild = state.timeline.find(item => item.event === 'build' && item.entries > 0);
  return {
    workerToListed: since(at('index.listed')), workerIndexMs: (state.timeline.find(item => item.event === 'worker.index.answered') || {}).ms ?? null,
    workerToFirstBuild: firstBuild ? since(firstBuild.at) : null, firstBuildEntries: firstBuild ? firstBuild.entries : null, firstBuildMs: firstBuild ? firstBuild.ms : null,
    workerToServed: since(at('module.served')), workerToFull: since(at('full.built')), serverToWorker: created == null ? null : created - server.spawned,
    builds: state.timeline.filter(item => item.event === 'build').map(item => ({ at: since(item.at), entries: item.entries, ms: item.ms, single: item.single })),
  };
}

async function waitForHost(entry) {
  const waited = await waitForQuiet(settings.quiet, 600000, log);
  entry.quietWaitMs = waited.waitedMs;
  if (!waited.quiet) entry.loud = true;
}

// ---------------------------------------------------------------- starts (S1–S4)

async function startGroup(fx, variant) {
  const pages = working(fx);
  const order = shuffled(pages, 'first-page' + fx.size);
  const total = settings.warmups + settings.samples;
  const base = { group: 'start', fixture: fx.size, variant };
  // One warm cache per variant and fixture, built once and kept warm by each
  // S2 sample's full build.
  const warmCache = path.join(runDir, 'start', variant + '-' + fx.size + '-warm');
  { const warming = await serverFor(variant, fx, warmCache); await fullBuild(warming); await warming.close(); }
  for (let index = 0; index < total; index++) {
    const warmup = index < settings.warmups;
    const page = order[index % order.length];
    const second = order[(index + 1) % order.length];
    const coldCache = path.join(runDir, 'start', variant + '-' + fx.size + '-cold-' + index);
    const profile = path.join(runDir, 'profiles', variant + '-' + fx.size + '-' + index);
    for (const sequence of ['S1', 'S2', 'S3']) {
      const entry = { ...base, sequence, sample: index, warmup, page: page.id };
      await waitForHost(entry);
      const server = await serverFor(variant, fx, sequence === 'S1' ? coldCache : warmCache);
      const browser = await launchBrowser(profile);
      try {
        if (sequence === 'S3') await sleep(2000);
        const canvas = await openCanvas(browser, server.url, { hash: address(page.file) });
        const shown = await select(canvas.target, null, pageCheck(page), { timeout: 600000, action: '/* navigating */', fromNavigation: true });
        const fromUrl = shown.ms == null ? null : canvas.navigated - server.urlAt + shown.ms;
        record({ ...entry, cell: 'from URL', ms: fromUrl, error: shown.error || null, fromServerStart: shown.ms == null ? null : canvas.navigated - server.spawned + shown.ms, fromNavigation: shown.ms });
        if (sequence === 'S2') {
          // S4: a second page at once, then the full build's time and the timeline.
          const next = await select(canvas.target, address(second.file), pageCheck(second), { timeout: 600000 });
          record({ ...base, sequence: 'S4', sample: index, warmup, cell: 'second page', ms: next.ms, error: next.error || null });
          const state = await fullBuild(server);
          record({ ...base, sequence: 'full', sample: index, warmup, cell: 'full build from worker start', ms: phases(state, server)?.workerToFull ?? null, phases: phases(state, server) });
        } else record({ ...base, sequence: sequence + '-phases', sample: index, warmup, cell: 'timeline', phases: phases(await lab(server), server) });
      } catch (error) { record({ ...entry, cell: 'from URL', ms: null, error: String(error.message || error).slice(0, 300) }); }
      finally { await browser.close().catch(() => {}); await server.close(); }
    }
    log(`  start ${variant} ${fx.size} #${index}`);
    fs.rmSync(profile, { recursive: true, force: true });
    fs.rmSync(coldCache, { recursive: true, force: true });
  }
  fs.rmSync(warmCache, { recursive: true, force: true });
}

// ---------------------------------------------------------------- correctness and S5–S7

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

async function sessionGroup(fx, variant) {
  const base = { group: 'session', fixture: fx.size, variant };
  const cache = path.join(runDir, 'session', variant + '-' + fx.size);
  // A warm cache first, as after any earlier start.
  { const warm = await serverFor(variant, fx, cache); await fullBuild(warm); await warm.close(); }
  await waitForHost(base);
  const server = await serverFor(variant, fx, cache);
  const browser = await launchBrowser();
  try {
    const index = await (await fetch(new URL('/_workbench/previews/index', server.url))).json();
    record({ ...base, sequence: 'list', cell: 'page list', value: index.previews.length, list: crypto.createHash('sha256').update(index.previews.map(item => item.id).sort().join('\n')).digest('hex').slice(0, 12) });
    if (fx.size === 'formats') { await formatsWalk(fx, variant, server, browser, base); return; }
    const pages = working(fx);
    const first = pages[0];
    const canvas = await openCanvas(browser, server.url, { hash: address(first.file) });
    const shown = await select(canvas.target, null, pageCheck(first), { timeout: 600000, action: '/* navigating */', fromNavigation: true });
    if (shown.error) throw new Error('first page: ' + shown.error);
    // Let the rest of the project join the build, then edit (finding 4's scenario).
    const state = await fullBuild(server);
    check({ check: 'full-build', variant, fixture: fx.size, ok: !!state });
    await sleep(1000);
    const later = pages[Math.min(7, pages.length - 1)];
    const edits = [
      { name: 'React component, first page', page: first, file: `${path.dirname(first.file)}/Interactive.tsx`, apply: (text, m) => text.replace(/Revision [^<]*/, 'Revision ' + m) },
      { name: 'shared module, first page', page: first, file: 'src/app/Shell.tsx', apply: (text, m) => text.replace('<nav aria-label="Features">', `<nav aria-label="Features"><span>${m}</span>`) },
      { name: 'React component, page opened later', page: later, file: `${path.dirname(later.file)}/Interactive.tsx`, apply: (text, m) => text.replace(/Revision [^<]*/, 'Revision ' + m) },
      { name: 'shared module, page opened later', page: later, file: 'src/app/Shell.tsx', apply: (text, m) => text.replace('<nav aria-label="Features">', `<nav aria-label="Features"><span>${m}</span>`) },
    ];
    for (const edit of edits) {
      const r = await select(canvas.target, address(edit.page.file), pageCheck(edit.page), { timeout: 120000 });
      if (r.error) { check({ check: 'hot-update', variant, fixture: fx.size, ok: false, edit: edit.name, error: r.error }); continue; }
      await sleep(500);
      await evaluate(canvas.target, `__labWin().__labToken = 'edit'`);
      const target = path.join(fx.root, edit.file);
      const original = fs.readFileSync(target, 'utf8');
      const marker = 'lab' + crypto.randomBytes(3).toString('hex');
      const ms = await timedEdit(canvas.target, target, edit.apply(original, marker), `doc.body.innerText.includes(${JSON.stringify(marker)})`);
      const sameDocument = await evaluate(canvas.target, `__labWin().__labToken === 'edit'`).catch(() => false);
      record({ ...base, sequence: 'S6', cell: edit.name, ms, sameDocument });
      check({ check: 'hot-update', variant, fixture: fx.size, edit: edit.name, ok: ms != null && sameDocument, ms, sameDocument });
      fs.writeFileSync(target, original);
      await until(canvas.target, `(() => { const w = __labWin(); return w.__workbenchReady === true && !w.document.body.innerText.includes(${JSON.stringify(marker)}); })()`, 30000).catch(() => {});
      await sleep(300);
    }
    // S5: five more pages, paced, with the isolation sentinels.
    for (const [index, page] of pages.slice(10, 15).entries()) {
      await sleep(1000);
      const r = await select(canvas.target, address(page.file), pageCheck(page), { timeout: 120000 });
      record({ ...base, sequence: 'S5', cell: 'paced', sample: index, ms: r.ms, error: r.error || null });
    }
    // A page that fails to compile, opened after start, shows its error.
    const broken = fx.manifest.previews.find(page => page.broken === 'compile');
    if (broken) {
      const r = await select(canvas.target, address(broken.file), 'false', { timeout: 60000, allowError: true });
      check({ check: 'error-shown', variant, fixture: fx.size, ok: !!r.error, ms: r.ms });
    }
    // S7: a config edit restarts the compilers; the page comes back.
    const config = path.join(fx.root, 'workbench.config.ts');
    const configText = fs.readFileSync(config, 'utf8');
    const back = pages[1];
    await select(canvas.target, address(back.file), pageCheck(back), { timeout: 120000 });
    await evaluate(canvas.target, "__labWin().__labToken = 'config'");
    fs.writeFileSync(config, configText + '\n// lab ' + Date.now() + '\n');
    const started = Date.now();
    let returned = false;
    try { await until(canvas.target, `(() => { const w = __labWin(); return !!w && w.__labToken !== 'config' && w.__workbenchReady === true && !!w.document.querySelector(${JSON.stringify(back.selector)}); })()`, 600000); returned = true; } catch (_) {}
    record({ ...base, sequence: 'S7', cell: 'config edit', ms: returned ? Date.now() - started : null });
    check({ check: 'config-edit', variant, fixture: fx.size, ok: returned });
    fs.writeFileSync(config, configText);
    const final = await lab(server);
    check({ check: 'no-moved-modules', variant, fixture: fx.size, ok: !!final && final.moved.length === 0, modules: final && final.modules, moved: final && final.moved.slice(0, 5) });
    check({ check: 'console', variant, fixture: fx.size, ok: true, entries: canvas.consoleEntries.slice(0, 30) });
  } catch (error) {
    check({ check: 'session', variant, fixture: fx.size, ok: false, error: String(error.stack || error).slice(0, 400) });
  } finally {
    await browser.close().catch(() => {});
    record({ ...base, sequence: 'worker', cell: 'peak RSS', value: server.peakRss() });
    await server.close();
    fs.rmSync(cache, { recursive: true, force: true });
  }
}

async function formatsWalk(fx, variant, server, browser, base) {
  const pages = working(fx);
  const canvas = await openCanvas(browser, server.url, { hash: address(pages[0].file) });
  for (const page of pages) {
    const r = await select(canvas.target, address(page.file), pageCheck(page), { timeout: 120000 });
    check({ check: 'render', variant, fixture: fx.size, page: page.id, ok: !r.error && r.ms != null, error: r.error });
  }
  const docs = fx.manifest.docs[0];
  const r = await select(canvas.target, docs.address, `!!win.__workbenchDocs && !!doc.querySelector('[data-acme="docs-marker"]')`, { timeout: 120000 });
  check({ check: 'render', variant, fixture: fx.size, page: docs.id, ok: !r.error && r.ms != null, error: r.error });
  const final = await lab(server);
  check({ check: 'no-moved-modules', variant, fixture: fx.size, ok: !!final && final.moved.length === 0, moved: final && final.moved.slice(0, 5) });
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
    return { fixture, sequence, cell, variant, samples: values.length, p50: percentile(values, 50), p95: percentile(values, 95), max: Math.max(...values) };
  });
}

function summary(report) {
  const lines = [`# First page lab, run set ${settings.set}`, '', smoke ? 'Smoke run: not evidence.' : `Samples per cell: ${settings.samples}, warmups: ${settings.warmups}.`, '',
    '| Fixture | Sequence | Cell | Variant | n | p50 ms | p95 ms | max ms |', '| --- | --- | --- | --- | ---: | ---: | ---: | ---: |'];
  for (const cell of [...report.aggregates].sort((a, b) => a.fixture.localeCompare(b.fixture) || a.sequence.localeCompare(b.sequence) || a.variant.localeCompare(b.variant))) {
    lines.push(`| ${cell.fixture} | ${cell.sequence} | ${cell.cell} | ${cell.variant} | ${cell.samples} | ${Math.round(cell.p50)} | ${Math.round(cell.p95)} | ${Math.round(cell.max)} |`);
  }
  lines.push('', '## Checks', '');
  const grouped = new Map();
  for (const item of checks) { const key = `${item.variant} ${item.fixture} ${item.check}`; const entry = grouped.get(key) || { pass: 0, fail: 0 }; entry[item.ok ? 'pass' : 'fail']++; grouped.set(key, entry); }
  for (const [key, value] of [...grouped].sort()) lines.push(`- ${key}: ${value.pass} pass, ${value.fail} fail`);
  const loud = results.filter(item => item.loud).length;
  lines.push('', `Samples started above the load threshold after waiting: ${loud}.`);
  return lines.join('\n') + '\n';
}

const release = acquireLabLock('first-page ' + settings.set);
const report = { plan: 'labs/first-page/README.md', settings, smoke, host: { atStart: hostSnapshot(), platform: process.platform, arch: process.arch, cpus: os.cpus().length, memoryGb: Math.round(os.totalmem() / 2 ** 30), node: process.version }, measured: measuredFiles(), started: new Date().toISOString() };
let failure = null;
try {
  log('Run directory: ' + runDir);
  for (const size of settings.fixtures) {
    const fx = await fixture(size);
    for (const variant of shuffled(settings.variants, settings.set + size)) {
      log(`${size}, variant ${variant}`);
      try {
        if (size !== 'formats') await startGroup(fx, variant);
        if (['large-system', 'formats', 'giant'].includes(size)) await sessionGroup(fx, variant);
      } catch (error) {
        log(`  ${size} ${variant} failed: ${error.stack || error}`);
        record({ fixture: size, variant, sequence: 'group', cell: 'failed', ms: null, error: String(error.message || error).slice(0, 500) });
      }
    }
  }
} catch (error) { failure = String(error.stack || error); log('Run failed: ' + failure); }
finally {
  for (const fx of Object.values(fixtures)) check({ check: 'no-project-writes', variant: 'all', fixture: fx.size, ok: treeDigest(fx.root) === fx.digest });
  Object.assign(report, { finished: new Date().toISOString(), failure, results, checks, aggregates: aggregate() });
  report.host.atEnd = hostSnapshot();
  const redact = text => text.split(runDir).join('<run>').split(workbench).join('<workbench>').split(os.homedir()).join('~');
  fs.writeFileSync(path.join(runDir, 'report.json'), redact(JSON.stringify(report, null, 2)));
  fs.writeFileSync(path.join(runDir, 'summary.md'), summary(report));
  if (resultsDir) { fs.mkdirSync(resultsDir, { recursive: true }); for (const name of ['report.json', 'summary.md']) fs.copyFileSync(path.join(runDir, name), path.join(resultsDir, name)); }
  console.log('\n' + summary(report));
  log('Report: ' + path.join(runDir, 'report.json'));
  if (!flag('keep')) for (const name of ['snapshots', 'fixtures', 'start', 'session', 'profiles']) {
    try { fs.rmSync(path.join(runDir, name), { recursive: true, force: true }); } catch (error) { console.error('Cleanup failed for ' + name + ': ' + error.message); }
  }
  release();
}
