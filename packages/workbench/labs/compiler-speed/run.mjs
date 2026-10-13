// Compiler speed lab runner. See README.md for the plan this implements.
//
//   node labs/compiler-speed/run.mjs [--smoke] [--samples 10] [--warmups 2]
//     [--fixtures small,large] [--variants shipping,esbuild,rolldown,bun,rspack,utoo]
//     [--shapes B1,B2,B3,B4,B5,B6] [--order-seed 20261011] [--source-revision <commit>]
//     [--results <new directory>] [--keep]
//
// Everything it generates, installs and builds lives in a new run directory
// under the system temporary folder. --results writes report.json and
// summary.md to a directory that must not exist yet.
import { spawn, spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { generate } from '../fixtures/acme-app.mjs';
import { install } from '../fixtures/install.mjs';
import { compare, renderAll } from './parity.mjs';
import { acquireLabLock, hostSnapshot, waitForQuiet } from '../fixtures/host.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const workbench = path.resolve(here, '../..');
const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf('--' + name); return index === -1 ? fallback : args[index + 1]; };
const flag = name => args.includes('--' + name);
const smoke = flag('smoke');
const settings = {
  samples: smoke ? 1 : Number(option('samples', 10)),
  warmups: smoke ? 0 : Number(option('warmups', 2)),
  fixtures: option('fixtures', 'small,large').split(','),
  variants: option('variants', 'shipping,esbuild,rolldown,bun,rspack,utoo').split(','),
  shapes: option('shapes', 'B1,B2,B3,B4,B5,B6').split(','),
  orderSeed: Number(option('order-seed', 20261011)),
  // Each sample starts only when the one-minute load average is below this.
  quietLoad: Number(option('quiet-load', 2.5)),
  sourceRevision: option('source-revision', null),
};
const resultsDir = option('results', null);
if (resultsDir && fs.existsSync(resultsDir)) { console.error('Refusing to overwrite ' + resultsDir); process.exit(2); }

const SHAPES = {
  shipping: ['B1', 'B3', 'B4'],
  esbuild: ['B1', 'B2', 'B3', 'B4'],
  rolldown: ['B1', 'B2', 'B3', 'B4', 'B6'],
  bun: ['B1', 'B2', 'B3', 'B4'],
  rspack: ['B1', 'B2', 'B3', 'B4', 'B5', 'B6'],
  utoo: ['B1', 'B2', 'B3', 'B4', 'B5'],
};

// Real path: macOS's temporary folder is behind a symlink, and Turbopack
// rejects project paths that resolve elsewhere.
const runParent = path.join(os.tmpdir(), 'canonic-workbench-labs', 'compiler-speed');
fs.mkdirSync(runParent, { recursive: true });
const runDir = path.join(fs.realpathSync(runParent), new Date().toISOString().replace(/[:.]/g, '-'));
fs.mkdirSync(runDir);
const log = message => console.log(`[${new Date().toISOString().slice(11, 19)}] ${message}`);
const sha = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');

function random(seed) {
  let a = seed >>> 0;
  return () => { a = (a + 0x6D2B79F5) >>> 0; let t = a; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
}
const shuffle = (items, rand) => { const copy = [...items]; for (let i = copy.length - 1; i > 0; i--) { const j = Math.floor(rand() * (i + 1)); [copy[i], copy[j]] = [copy[j], copy[i]]; } return copy; };

function prepare() {
  const tools = path.join(runDir, 'tools');
  fs.mkdirSync(tools);
  fs.copyFileSync(path.join(here, 'tools', 'package.json'), path.join(tools, 'package.json'));
  log('Installing compilers');
  install(tools, path.join(here, 'tools', 'package-lock.json'));
  const fixtures = {};
  for (const size of settings.fixtures) {
    const root = path.join(runDir, 'fixtures', size);
    log(`Generating and installing the ${size} fixture`);
    const manifest = generate(root, { size, seed: 1 });
    install(root, path.join(here, '..', 'fixtures', 'locks', size + '.package-lock.json'));
    const capture = spawnSync(process.execPath, [path.join(here, 'capture-entries.cjs'), root, workbench], { stdio: 'inherit' });
    if (capture.status !== 0) throw new Error('Entry capture failed for ' + size);
    fixtures[size] = { root, spec: path.join(root, '.lab', 'spec.json'), manifest: { files: manifest.files, sourceBytes: manifest.sourceBytes, previews: manifest.previews.length } };
  }
  return { tools, fixtures };
}

// Peak resident memory of a process and all its descendants, sampled with ps.
function treeRss(rootPid) {
  const out = spawnSync('ps', ['-A', '-o', 'pid=,ppid=,rss='], { encoding: 'utf8' }).stdout || '';
  const children = new Map();
  const rss = new Map();
  for (const line of out.trim().split('\n')) {
    const [pid, ppid, kb] = line.trim().split(/\s+/).map(Number);
    rss.set(pid, kb);
    if (!children.has(ppid)) children.set(ppid, []);
    children.get(ppid).push(pid);
  }
  let total = 0;
  const stack = [rootPid];
  while (stack.length) { const pid = stack.pop(); total += rss.get(pid) || 0; stack.push(...(children.get(pid) || [])); }
  return total / 1024;
}

const bunBinary = tools => path.join(tools, 'node_modules', '@oven', `bun-${process.platform}-${process.arch === 'arm64' ? 'aarch64' : 'x64'}`, 'bin', 'bun');

// Probed capabilities, where the lab can check them by running the tool.
function capabilities(tools) {
  const probe = spawnSync(bunBinary(tools), [path.join(here, 'bun-capabilities.mjs'), path.join(runDir, 'capabilities', 'bun')], { encoding: 'utf8' });
  let bun;
  try { bun = JSON.parse(probe.stdout.trim().split('\n').pop()); }
  catch (_) { bun = { ok: false, error: (probe.stderr || probe.stdout || '').slice(-1000) }; }
  return { bun };
}

function sample({ tools, fixture, variant, shape, outdir, marker, publicPath = '' }) {
  const bun = bunBinary(tools);
  const [command, script] = variant === 'bun' ? [bun, path.join(here, 'bun-builder.mjs')] : [process.execPath, path.join(here, 'sample.mjs')];
  return new Promise(resolve => {
    const child = spawn(command, [script, variant, shape, fixture.spec, outdir, tools, '', marker, publicPath], { stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '', stderr = '', peak = 0;
    const timer = setInterval(() => { peak = Math.max(peak, treeRss(child.pid)); }, 100);
    child.stdout.on('data', chunk => { stdout += chunk; });
    child.stderr.on('data', chunk => { stderr += chunk; });
    child.on('close', code => {
      clearInterval(timer);
      let result;
      try { result = JSON.parse(stdout.trim().split('\n').pop()); }
      catch (_) { result = { variant, shape, ok: false, error: `exit ${code}: ${stderr.slice(-2000) || stdout.slice(-2000)}` }; }
      // A sample shorter than the polling interval has no tree reading; its
      // own peak is the best available figure.
      result.treeRssMb = Math.round(Math.max(peak, result.maxRssMb || 0));
      resolve(result);
    });
  });
}

function cleanup(fixture, outdir, result) {
  fs.rmSync(outdir, { recursive: true, force: true });
  if (result?.outputDir) fs.rmSync(result.outputDir, { recursive: true, force: true });
}

const percentile = (values, p) => { const sorted = [...values].sort((a, b) => a - b); return sorted[Math.max(0, Math.ceil(p / 100 * sorted.length) - 1)]; };

async function measure(prepared) {
  const rand = random(settings.orderSeed);
  const samples = [];
  const rounds = settings.warmups + settings.samples;
  let counter = 0;
  for (let round = 0; round < rounds; round++) {
    const warmup = round < settings.warmups;
    for (const size of settings.fixtures) {
      const fixture = prepared.fixtures[size];
      for (const shape of settings.shapes) {
        for (const variant of shuffle(settings.variants.filter(name => SHAPES[name]?.includes(shape)), rand)) {
          const id = `${size}-${variant}-${shape}-${round}`;
          const base = path.join(runDir, 'samples', id);
          const marker = 'lab' + (++counter) + 'x' + crypto.randomBytes(3).toString('hex');
          if (shape === 'B5') {
            const prime = await sample({ tools: prepared.tools, fixture, variant, shape: 'B5P', outdir: path.join(base, 'prime'), marker });
            cleanup(fixture, path.join(base, 'prime'), prime);
            if (!prime.ok) { samples.push({ ...prime, fixture: size, round, warmup, shape: 'B5' }); continue; }
          }
          const outdir = path.join(base, 'measure');
          const quiet = await waitForQuiet(settings.quietLoad, 10 * 60 * 1000, log);
          const before = hostSnapshot();
          const result = await sample({ tools: prepared.tools, fixture, variant, shape, outdir, marker });
          result.host = { loadAverage: before.loadAverage, busiest: before.busiest, quiet: quiet.quiet, waitedMs: quiet.waitedMs };
          Object.assign(result, { fixture: size, round, warmup });
          samples.push(result);
          cleanup(fixture, outdir, result);
          fs.rmSync(base, { recursive: true, force: true });
          log(`${warmup ? 'warmup ' : ''}${id}: ${result.ok ? Math.round(result.ms) + ' ms, ' + result.treeRssMb + ' MB' : 'FAILED ' + String(result.error).split('\n')[0]}`);
        }
      }
    }
  }
  return samples;
}

function aggregate(samples) {
  const cells = new Map();
  for (const item of samples.filter(sample => !sample.warmup)) {
    const key = `${item.fixture}|${item.variant}|${item.shape}`;
    if (!cells.has(key)) cells.set(key, []);
    cells.get(key).push(item);
  }
  return [...cells].map(([key, items]) => {
    const [fixture, variant, shape] = key.split('|');
    const ok = items.filter(item => item.ok);
    const ms = ok.map(item => item.ms);
    const rss = ok.map(item => item.treeRssMb);
    return {
      fixture, variant, shape, samples: items.length, failures: items.length - ok.length,
      p50: ms.length ? percentile(ms, 50) : null, p95: ms.length ? percentile(ms, 95) : null, max: ms.length ? Math.max(...ms) : null,
      rssP50: rss.length ? percentile(rss, 50) : null,
      // Rebuild shapes leave the first build's files beside the new ones, so
      // sizes are reported for cold builds only.
      jsBytes: ['B1', 'B2'].includes(shape) ? ok[0]?.memoryJsBytes ?? ok[0]?.jsBytes ?? null : null,
      jsFiles: ['B1', 'B2'].includes(shape) ? ok[0]?.jsFiles ?? null : null,
      editVisible: ok.some(item => 'editVisible' in item) ? ok.every(item => item.editVisible) : null,
      incremental: ok[0]?.incremental ?? null,
      firstError: items.find(item => !item.ok)?.error?.split('\n').slice(0, 3).join(' ') ?? null,
    };
  });
}

async function parity(prepared) {
  const report = {};
  for (const size of settings.fixtures) {
    const fixture = prepared.fixtures[size];
    const spec = JSON.parse(fs.readFileSync(fixture.spec, 'utf8'));
    const dir = path.join(runDir, 'parity', size);
    log(`Building parity outputs for the ${size} fixture`);
    const shippingDir = path.join(dir, 'shipping');
    const shipped = spawnSync(process.execPath, [path.join(here, 'shipping-outputs.cjs'), fixture.spec, shippingDir], { stdio: 'inherit' });
    if (shipped.status !== 0) throw new Error('Shipping outputs failed for ' + size);
    const outputs = { shipping: { dir: shippingDir, entry: slug => `${slug}/preview.js`, css: slug => fs.existsSync(path.join(shippingDir, slug, 'preview.css')) ? `${slug}/preview.css` : null } };
    const builds = {};
    for (const variant of settings.variants.filter(name => name !== 'shipping')) {
      const outdir = path.join(dir, variant);
      const result = await sample({ tools: prepared.tools, fixture, variant, shape: 'B2', outdir, marker: 'parity', publicPath: `/out/${variant}/` });
      builds[variant] = { ok: result.ok, error: result.error ?? null };
      if (!result.ok) continue;
      const root = result.outputDir || outdir;
      // Utoo's standalone output is classic scripts with its own chunk runtime.
      outputs[variant] = { dir: root, classic: variant === 'utoo', entry: slug => `${slug}.js`, css: slug => fs.existsSync(path.join(root, slug + '.css')) ? `${slug}.css` : null };
    }
    const sanity = chunkSanity(outputs, spec);
    const states = size === 'large' ? ['default', 'empty'] : ['default', 'accent'];
    log(`Rendering ${spec.entries.length} previews × ${states.length} states × ${Object.keys(outputs).length} variants`);
    const rendered = await renderAll({ workbench, outputs, cases: spec.entries.map(entry => ({ slug: entry.slug, states })) });
    const control = rendered.filter(item => item.variant === 'shipping');
    report[size] = {
      builds, chunkSanity: sanity,
      controlFailures: control.filter(item => !item.ready || item.error || item.timedOut).map(({ html, ...rest }) => rest),
      comparisons: compare(rendered),
    };
  }
  return report;
}

// Locale data must stay out of entry files in split builds: it is only
// reachable through dynamic imports.
function chunkSanity(outputs, spec) {
  const result = {};
  for (const [variant, output] of Object.entries(outputs)) {
    if (variant === 'shipping') continue;
    const entryFiles = new Set(spec.entries.map(entry => path.join(output.dir, output.entry(entry.slug))));
    const withLocale = [];
    const walk = directory => { for (const item of fs.readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, item.name);
      if (item.isDirectory()) walk(file);
      else if (/\.m?js$/.test(item.name) && fs.readFileSync(file, 'utf8').includes('Kanada')) withLocale.push(file);
    } };
    walk(output.dir);
    result[variant] = { localeChunks: withLocale.length, localeInEntry: withLocale.some(file => entryFiles.has(file)) };
  }
  return result;
}

function host(tools) {
  const version = name => { try { return JSON.parse(fs.readFileSync(path.join(tools, 'node_modules', name, 'package.json'), 'utf8')).version; } catch (_) { return null; } };
  const measured = ['preview/compiler.cjs', 'preview/engine.cjs', 'preview/build-cache.cjs', 'preview/browser.js', 'preview/requests.js', 'preview/react.js'].map(file => path.join(workbench, file));
  const lab = fs.readdirSync(here).filter(name => /\.(c|m)?js$/.test(name)).map(name => path.join(here, name))
    .concat(['acme-app.mjs', 'install.mjs'].map(name => path.join(here, '..', 'fixtures', name)));
  return {
    date: new Date().toISOString(), platform: process.platform, arch: process.arch, osRelease: os.release(),
    cpu: os.cpus()[0].model, cpus: os.cpus().length, memoryGb: Math.round(os.totalmem() / 2 ** 30), loadAverage: os.loadavg(),
    node: process.version, workbenchEsbuild: JSON.parse(fs.readFileSync(path.join(workbench, 'node_modules', 'esbuild', 'package.json'), 'utf8')).version,
    tools: Object.fromEntries(['esbuild', 'rolldown', '@rspack/core', 'bun', '@utoo/pack'].map(name => [name, version(name)])),
    sourceRevision: settings.sourceRevision,
    sourceHashes: Object.fromEntries([...measured, ...lab].map(file => [path.relative(workbench, file), sha(file)])),
  };
}

function summary(report) {
  const loads = report.samples.map(item => item.host?.loadAverage?.[0]).filter(value => typeof value === 'number');
  const waited = report.samples.filter(item => item.host && !item.host.quiet).length;
  const lines = [`# Compiler speed run ${report.host.date}`, '', `${report.smoke ? 'Smoke run (one sample per cell, no warmups): not evidence.' : `Samples per cell: ${settings.samples}, warmups: ${settings.warmups}.`}`, '',
    loads.length ? `One-minute load average at sample start: ${Math.min(...loads).toFixed(2)}–${Math.max(...loads).toFixed(2)} (threshold ${settings.quietLoad}; samples started above it after waiting 10 minutes: ${waited}).` : '', '',
    '| Fixture | Shape | Variant | p50 ms | p95 ms | max ms | Tree RSS p50 MB | JS MB | Edit visible | Failures |', '| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | ---: |'];
  const fmt = value => value == null ? '—' : Math.round(value);
  for (const cell of [...report.aggregates].sort((a, b) => (a.fixture + a.shape + a.variant).localeCompare(b.fixture + b.shape + b.variant))) {
    lines.push(`| ${cell.fixture} | ${cell.shape} | ${cell.variant} | ${fmt(cell.p50)} | ${fmt(cell.p95)} | ${fmt(cell.max)} | ${fmt(cell.rssP50)} | ${cell.jsBytes == null ? '—' : (cell.jsBytes / 1e6).toFixed(2)} | ${cell.editVisible == null ? '—' : cell.editVisible ? 'yes' : 'NO'} | ${cell.failures} |`);
  }
  lines.push('', '## Render parity', '');
  for (const [size, value] of Object.entries(report.parity || {})) {
    lines.push(`### ${size}`, '');
    if (value.controlFailures.length) lines.push(`Control renders that failed or did not settle: ${value.controlFailures.length}`, '');
    const byVariant = new Map();
    for (const item of value.comparisons) { const entry = byVariant.get(item.variant) || { ok: 0, failed: 0 }; item.ok ? entry.ok++ : entry.failed++; byVariant.set(item.variant, entry); }
    for (const [variant, build] of Object.entries(value.builds)) {
      const counts = byVariant.get(variant);
      lines.push(`- ${variant}: ${build.ok ? `${counts?.ok ?? 0} match, ${counts?.failed ?? 0} differ; locale chunks ${value.chunkSanity[variant]?.localeChunks}, locale in an entry: ${value.chunkSanity[variant]?.localeInEntry ? 'YES' : 'no'}` : 'build failed: ' + String(build.error).split('\n')[0]}`);
    }
    lines.push('');
  }
  return lines.join('\n') + '\n';
}

let prepared;
try {
  acquireLabLock('compiler-speed');
  log('Run directory: ' + runDir);
  var startSnapshot = hostSnapshot();
  prepared = prepare();
  const samples = await measure(prepared);
  const parityReport = await parity(prepared);
  const report = { plan: 'labs/compiler-speed/README.md', smoke, settings, host: { ...host(prepared.tools), atStart: startSnapshot, atEnd: hostSnapshot() }, fixtures: prepared.fixtures,
    capabilities: settings.variants.includes('bun') ? capabilities(prepared.tools) : {}, samples, aggregates: aggregate(samples), parity: parityReport };
  // Reports are committed evidence: local directories become placeholders.
  const redact = text => text.split(runDir).join('<run>').split(workbench).join('<workbench>').split(os.homedir()).join('<home>');
  fs.writeFileSync(path.join(runDir, 'report.json'), redact(JSON.stringify(report, null, 2)));
  fs.writeFileSync(path.join(runDir, 'summary.md'), summary(report));
  if (resultsDir) {
    fs.mkdirSync(resultsDir, { recursive: true });
    fs.copyFileSync(path.join(runDir, 'report.json'), path.join(resultsDir, 'report.json'));
    fs.copyFileSync(path.join(runDir, 'summary.md'), path.join(resultsDir, 'summary.md'));
  }
  console.log('\n' + summary(report));
  log('Report: ' + path.join(runDir, 'report.json'));
} finally {
  // Generated projects, installs and builds are large; keep them only on request.
  if (!flag('keep')) {
    for (const name of ['samples', 'parity', 'fixtures', 'tools', 'capabilities']) {
      try { fs.rmSync(path.join(runDir, name), { recursive: true, force: true }); }
      catch (error) { console.error('Cleanup failed for ' + name + ': ' + error.message); }
    }
  }
}
