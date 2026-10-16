// Stage 1's groups: capabilities (C2–C8), the page index (I1–I3) and builds
// (B1–B4). Run by stage1.mjs, which owns the run directory, lock and report.
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { HOST, Host, entryMaker, fixture, job } from './lib.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const HOSTS = ['rust', 'node'];

function seeded(seedText) {
  let seed = crypto.createHash('sha256').update(seedText).digest().readUInt32LE(0);
  return () => { seed = (seed + 0x6D2B79F5) >>> 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
}
const shuffled = (items, seedText) => { const out = [...items]; const rand = seeded(seedText); for (let i = out.length - 1; i > 0; i--) { const j = Math.floor(rand() * (i + 1)); [out[i], out[j]] = [out[j], out[i]]; } return out; };
const working = fx => fx.manifest.previews.filter(page => !page.broken);
// The build's own outcome, kept apart from a check's `ok`.
const brief = answer => ({ built: !!answer.ok, ms: answer.ms ?? null, errors: (answer.errors || []).map(error => String(error).slice(0, 300)), error: answer.error || null });
const moved = (before, after) => Object.entries(before.chunk_of || {}).filter(([id, chunk]) => after.chunk_of && after.chunk_of[id] !== undefined && after.chunk_of[id] !== chunk).map(([id, chunk]) => id.split('!').pop() + ': ' + chunk + ' -> ' + after.chunk_of[id]);

export async function run(context) {
  const { runDir, settings, report, log } = context;
  const fixtures = {};
  for (const size of settings.fixtures) fixtures[size] = await fixture(runDir, size);
  const record = entry => { const load = context.hostSnapshot().loadAverage[0]; report.results.push({ ...entry, load }); };
  const check = entry => { report.checks.push(entry); if (!entry.ok) log(`  CHECK FAILED ${entry.check} ${entry.host || ''} ${entry.fixture}: ${JSON.stringify(entry).slice(0, 300)}`); };
  const quiet = async entry => { const waited = await context.waitForQuiet(settings.quiet, 600000, log); entry.quietWaitMs = waited.waitedMs; if (!waited.quiet) entry.loud = true; return entry; };
  check({ check: 'C1', fixture: 'all', ok: fs.existsSync(HOST), detail: 'crates 0.102.7 build with Rspack\'s nightly-2026-04-16 and its v2.2.7 Cargo.lock; stable 1.99.0 does not (regress `pattern`)' });
  if (settings.groups.includes('capabilities')) for (const size of ['large-system', 'formats']) if (fixtures[size]) await capabilities(fixtures[size], context, { record, check });
  if (settings.groups.includes('index')) for (const fx of Object.values(fixtures)) if (fx.size !== 'formats') await indexGroup(fx, context, { record, check, quiet });
  if (settings.groups.includes('builds')) for (const fx of Object.values(fixtures)) if (fx.size !== 'formats') await buildsGroup(fx, context, { record, check, quiet });
}

// ---------------------------------------------------------------- capabilities

async function capabilities(fx, { runDir, log }, { check }) {
  const maker = entryMaker(fx.root);
  for (const kind of HOSTS) {
    log(`capabilities ${fx.size} ${kind}`);
    const cache = path.join(runDir, 'caches', 'capabilities-' + kind + '-' + fx.size);
    const base = { host: kind, fixture: fx.size };
    if (fx.size === 'formats') {
      // Which formats each host builds, one page per host process.
      for (const page of fx.manifest.previews.filter(item => !item.broken)) {
        const host = new Host(kind);
        try {
          const entries = await maker.entries([page.file]).catch(error => { throw new Error('entry: ' + error.message); });
          const built = await host.request(job(fx.root, cache + '-' + page.id.replace(/\//g, '-'), maker, entries));
          const ownFiles = built.entrypoints && built.entrypoints[entries[0].slug] || [];
          check({ check: 'C2-format', ...base, page: page.id, adapter: page.adapter, ok: !!built.ok && !(built.errors || []).length && ownFiles.length > 0, ...brief(built) });
        } catch (error) { check({ check: 'C2-format', ...base, page: page.id, adapter: page.adapter, ok: false, error: String(error.message || error).slice(0, 300) }); }
        finally { await host.close(); }
      }
      continue;
    }
    const pages = working(fx);
    const [a, b] = await maker.entries([pages[0].file, pages[1].file]);
    let host = new Host(kind);
    try {
      const first = await host.request(job(fx.root, cache, maker, [a]));
      const ownFirst = (first.entrypoints[a.slug] || []).find(name => name.startsWith(a.slug));
      const ownSource = ownFirst ? (await host.request({ op: 'asset', name: ownFirst })).source : null;
      check({ check: 'C2', ...base, ok: !!first.ok && !(first.errors || []).length && (first.entrypoints[a.slug] || []).some(name => name.endsWith('.js')), ...brief(first) });
      const added = await host.request({ op: 'add', entries: [b] });
      const before = first.entrypoints[a.slug] || [], after = added.entrypoints[a.slug] || [];
      // A's own chunk content is compared, not its file name: a content hash
      // in the name can change with the chunk graph while the content does not.
      const ownBefore = before.find(name => name.startsWith(a.slug)), ownAfter = after.find(name => name.startsWith(a.slug));
      const sameContent = ownBefore && ownAfter && (await host.request({ op: 'asset', name: ownAfter })).source === ownSource;
      check({ check: 'C3', ...base, ok: !!added.ok && !!(added.entrypoints[b.slug] || []).length && !(added.errors || []).length, ...brief(added), renamed: before.filter(name => !after.includes(name)), sameContent: sameContent ?? null });
      check({ check: 'C6', ...base, ok: moved(first, added).length === 0, moved: moved(first, added).slice(0, 10), sharedModules: Object.keys(added.chunk_of || {}).length });
      const file = path.join(fx.root, pages[0].edit.component);
      const original = fs.readFileSync(file, 'utf8');
      try {
        fs.writeFileSync(file, original.replace(/Revision [^<]*/, 'Revision lab-c4'));
        const edited = await host.request({ op: 'rebuild', changed: [file], deleted: [] });
        const update = (edited.assets || []).find(name => name.endsWith('.hot-update.js') && !name.includes('runtime'));
        const source = update ? (await host.request({ op: 'asset', name: update })).source || '' : '';
        check({ check: 'C4', ...base, ok: !!edited.ok && source.includes('lab-c4'), ...brief(edited), hotFiles: (edited.assets || []).filter(name => name.startsWith('hot/')).map(name => name.replace(/\.[0-9a-f]{16}\./, '.<hash>.')) });
      } finally { fs.writeFileSync(file, original); await host.request({ op: 'rebuild', changed: [file], deleted: [] }); }
      const broken = fx.manifest.previews.find(item => item.broken === 'compile');
      const [bad] = await maker.entries([broken.file]);
      const withBad = await host.request({ op: 'add', entries: [bad] });
      check({ check: 'C7', ...base, ok: !!withBad.ok && (withBad.errors || []).some(error => error.includes('does-not-exist')) && (withBad.entrypoints[a.slug] || []).length > 0 && (withBad.entrypoints[b.slug] || []).length > 0,
        ...brief(withBad), note: 'errors are reported for the build; attributing them to an entry is the host\'s job, measured in stage 2' });
      if (kind === 'rust') {
        const watching = await host.request({ op: 'watch', files: [path.join(fx.root, 'workbench.config.ts')] });
        await host.request({ op: 'events' });
        const target = path.join(fx.root, pages[0].edit.component);
        const config = path.join(fx.root, 'workbench.config.ts');
        const saved = [fs.readFileSync(target, 'utf8'), fs.readFileSync(config, 'utf8')];
        try {
          fs.writeFileSync(target, saved[0] + '\n// lab c8\n');
          fs.writeFileSync(config, saved[1] + '\n// lab c8\n');
          let seen = { changed: [] };
          for (let wait = 0; wait < 50 && !(seen.changed.includes(target) && seen.changed.includes(config)); wait++) {
            await new Promise(resolve => setTimeout(resolve, 100));
            const next = await host.request({ op: 'events' });
            seen = { changed: [...seen.changed, ...(next.changed || [])] };
          }
          check({ check: 'C8', ...base, ok: !!watching.ok && seen.changed.includes(target) && seen.changed.includes(config), watchedFiles: watching.files, sawSource: seen.changed.includes(target), sawConfig: seen.changed.includes(config) });
        } finally { fs.writeFileSync(target, saved[0]); fs.writeFileSync(config, saved[1]); }
      }
      await host.close();
      host = new Host(kind);
      const warm = await host.request(job(fx.root, cache, maker, [a, b]));
      check({ check: 'C5', ...base, ok: !!warm.ok && !(warm.errors || []).length, ...brief(warm) });
    } catch (error) { check({ check: 'capabilities', ...base, ok: false, error: String(error.stack || error).slice(0, 500) }); }
    finally { await host.close(); }
  }
}

// ---------------------------------------------------------------- index

function rustIndex(root, args) {
  const started = performance.now();
  const result = spawnSync(HOST, ['index', root, ...args], { encoding: 'utf8', cwd: path.resolve(here, '..', '..'), maxBuffer: 1 << 26 });
  const wall = performance.now() - started;
  try { return { ...JSON.parse(result.stdout), wall }; } catch (_) { return { error: (result.stderr || '').slice(-500), wall }; }
}
function shippingIndex(root, out) {
  const started = performance.now();
  const result = spawnSync(process.execPath, [path.join(here, 'index-node.mjs'), root, '--out', out], { encoding: 'utf8', cwd: path.resolve(here, '..', '..'), maxBuffer: 1 << 26 });
  const wall = performance.now() - started;
  try { return { ...JSON.parse(result.stdout), wall }; } catch (_) { return { error: (result.stderr || '').slice(-500), wall }; }
}
const previewsOf = file => JSON.stringify(JSON.parse(fs.readFileSync(file, 'utf8')).previews);

async function indexGroup(fx, { runDir, settings, log }, { record, check, quiet }) {
  log(`index ${fx.size}`);
  const dir = path.join(runDir, 'index', fx.size);
  fs.mkdirSync(dir, { recursive: true });
  const expected = path.join(dir, 'shipping.json');
  const saved = path.join(dir, 'saved.bin');
  const out = path.join(dir, 'rust.json');
  const total = settings.warmups + settings.samples;
  const base = { group: 'index', fixture: fx.size };
  const same = (cell, sample) => { const ok = fs.existsSync(out) && previewsOf(out) === previewsOf(expected); check({ check: 'index-matches', fixture: fx.size, cell, sample, ok }); };
  for (let index = 0; index < total; index++) {
    const warmup = index < settings.warmups;
    for (const side of shuffled(['rust', 'shipping'], 'I1' + fx.size + index)) {
      const entry = await quiet({ ...base, cell: 'I1', host: side, sample: index, warmup });
      if (side === 'shipping') { const r = shippingIndex(fx.root, expected); record({ ...entry, ms: r.ms ?? null, wall: r.wall, error: r.error || null }); }
      else { fs.rmSync(saved, { force: true }); fs.rmSync(saved.replace(/\.bin$/, '.answer.json'), { force: true }); const r = rustIndex(fx.root, ['--saved', saved, '--out', out]); record({ ...entry, ms: r.process_ms ?? null, wall: r.wall, parts: r, error: r.error || null }); }
    }
    same('I1', index);
  }
  for (const mode of ['dirs', 'walk']) for (let index = 0; index < total; index++) {
    const entry = await quiet({ ...base, cell: 'I2', host: 'rust', mode, sample: index, warmup: index < settings.warmups });
    const r = rustIndex(fx.root, ['--saved', saved, '--check', mode, '--out', out]);
    record({ ...entry, ms: r.process_ms ?? null, wall: r.wall, parts: r, error: r.error || null });
    same('I2-' + mode, index);
  }
  // I3: 1% of the literal definitions retitled since the save.
  const literal = working(fx).filter(page => !page.computed);
  const count = Math.max(1, Math.round(working(fx).length / 100));
  for (let index = 0; index < total; index++) {
    const chosen = shuffled(literal, 'I3' + fx.size + index).slice(0, count);
    const originals = chosen.map(page => [path.join(fx.root, page.file), fs.readFileSync(path.join(fx.root, page.file), 'utf8')]);
    const entry = await quiet({ ...base, cell: 'I3', host: 'rust', mode: 'dirs', retitled: count, sample: index, warmup: index < settings.warmups });
    try {
      for (const [file, text] of originals) fs.writeFileSync(file, text.replace(/title: '([^']*)'/, "title: '$1 Retitled'"));
      const r = rustIndex(fx.root, ['--saved', saved, '--check', 'dirs', '--out', out]);
      record({ ...entry, ms: r.process_ms ?? null, wall: r.wall, parts: r, error: r.error || null });
      const listed = JSON.parse(fs.readFileSync(out, 'utf8')).previews.filter(item => / Retitled$/.test(item.title)).length;
      check({ check: 'index-retitled', fixture: fx.size, sample: index, ok: listed === count, listed, count });
    } finally {
      for (const [file, text] of originals) fs.writeFileSync(file, text);
      rustIndex(fx.root, ['--saved', saved, '--check', 'dirs', '--out', out]);
    }
    same('I3-restored', index);
  }
}

// ---------------------------------------------------------------- builds

async function buildsGroup(fx, { runDir, settings, log }, { record, check, quiet }) {
  const maker = entryMaker(fx.root);
  const pages = working(fx);
  const order = shuffled(pages, 'builds' + fx.size);
  const total = settings.warmups + settings.samples;
  const base = { group: 'builds', fixture: fx.size };
  log(`builds ${fx.size}: B1/B2`);
  for (let index = 0; index < total; index++) {
    const warmup = index < settings.warmups;
    const [entry] = await maker.entries([order[index % order.length].file]);
    for (const kind of shuffled(HOSTS, 'B1' + fx.size + index)) {
      const cache = path.join(runDir, 'caches', 'b-' + kind + '-' + fx.size + '-' + index);
      for (const cell of ['B1', 'B2']) {
        const sample = await quiet({ ...base, cell, host: kind, sample: index, warmup, page: entry.slug });
        const spawned = performance.now();
        const host = new Host(kind);
        try {
          const built = await host.request(job(fx.root, cache, maker, [entry]));
          record({ ...sample, ms: built.ms ?? null, wall: performance.now() - spawned, created: built.created_ms ?? null, error: built.ok ? null : built.error, errors: (built.errors || []).length });
        } finally { await host.close(); }
      }
      fs.rmSync(cache, { recursive: true, force: true });
    }
  }
  for (const held of [1, 20, 100].filter(n => n + total + 1 <= pages.length)) {
    log(`builds ${fx.size}: B3/B4 holding ${held}`);
    for (const kind of shuffled(HOSTS, 'B3' + fx.size + held)) {
      const cache = path.join(runDir, 'caches', 'b3-' + kind + '-' + fx.size + '-' + held);
      const host = new Host(kind);
      try {
        const start = await host.request(job(fx.root, cache, maker, await maker.entries(order.slice(0, held).map(page => page.file))));
        if (!start.ok) throw new Error('start: ' + start.error);
        let previous = start;
        for (let index = 0; index < total; index++) {
          const [entry] = await maker.entries([order[held + index].file]);
          const sample = await quiet({ ...base, cell: 'B3', host: kind, held: held + index, sample: index, warmup: index < settings.warmups });
          const added = await host.request({ op: 'add', entries: [entry] });
          record({ ...sample, ms: added.ms ?? null, error: added.ok ? null : added.error, errors: (added.errors || []).length });
          check({ check: 'B3-no-moved-modules', host: kind, fixture: fx.size, held: held + index, ok: moved(previous, added).length === 0, moved: moved(previous, added).slice(0, 5) });
          previous = added;
        }
        // B4: a component of the first page, then a module every page shares.
        const edits = [
          { cell: 'B4-component', file: path.join(fx.root, order[0].edit.component), apply: (text, mark) => text.replace(/Revision [^<]*/, 'Revision ' + mark) },
          ...(fs.existsSync(path.join(fx.root, 'src/app/Shell.tsx')) ? [{ cell: 'B4-shared', file: path.join(fx.root, 'src/app/Shell.tsx'), apply: (text, mark) => text.replace('<nav aria-label="Features">', `<nav aria-label="Features"><span>${mark}</span>`) }] : []),
        ];
        for (const edit of edits) for (let index = 0; index < total; index++) {
          const original = fs.readFileSync(edit.file, 'utf8');
          const sample = await quiet({ ...base, cell: edit.cell, host: kind, held: held + total, sample: index, warmup: index < settings.warmups });
          try {
            fs.writeFileSync(edit.file, edit.apply(original, 'lab' + index));
            const edited = await host.request({ op: 'rebuild', changed: [edit.file], deleted: [] });
            record({ ...sample, ms: edited.ms ?? null, error: edited.ok ? null : edited.error, errors: (edited.errors || []).length, hot: (edited.assets || []).filter(name => name.endsWith('.hot-update.js')).length });
          } finally { fs.writeFileSync(edit.file, original); await host.request({ op: 'rebuild', changed: [edit.file], deleted: [] }); }
        }
      } catch (error) { record({ ...base, cell: 'B3', host: kind, held, ms: null, error: String(error.message || error).slice(0, 300) }); }
      finally { await host.close(); fs.rmSync(cache, { recursive: true, force: true }); }
    }
  }
}
