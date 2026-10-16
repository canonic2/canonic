// Instant first page lab: what the stage 1 runner and its groups share.
// The compiler hosts (the Rust host and node-host.mjs) over their line
// protocol, entry sources from the shipping pipeline's previewEntry (loaded
// read-only), and fixtures.
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';
import { targetDir } from './cargo.mjs';
import { install } from '../fixtures/install.mjs';
import { LOCKFILE } from '../fixtures/acme-app.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const workbench = path.resolve(here, '..', '..');
const require = createRequire(import.meta.url);
const { Compiler, validate, docsRequest, mockFetches } = require(path.join(workbench, 'server/node/preview', 'compiler.cjs'));
const { Pipeline } = await import(path.join(workbench, 'server/node/src', 'modules', 'preview-pipeline', 'index.ts'));
const { hotClientSource } = await import(path.join(workbench, 'server/node/src', 'modules', 'preview-pipeline', 'client-sources.ts'));
const log = message => console.log(`[${new Date().toISOString().slice(11, 19)}] ${message}`);

// ---------------------------------------------------------------- host process

export const HOST = path.join(targetDir, 'release', 'instant-first-page-host');

/** A compiler host, one JSON request and answer per line: the Rust host, or
 * the same protocol over @rspack/core (node-host.mjs). */
export class Host {
  constructor(kind = 'rust') {
    this.kind = kind;
    this.child = kind === 'rust' ? spawn(HOST, ['serve'], { stdio: ['pipe', 'pipe', 'inherit'] })
      : spawn(process.execPath, [path.join(here, 'node-host.mjs')], { stdio: ['pipe', 'pipe', 'inherit'], cwd: workbench });
    this.answers = [];
    this.waiting = [];
    readline.createInterface({ input: this.child.stdout }).on('line', line => {
      let value; try { value = JSON.parse(line); } catch (_) { value = { ok: false, error: 'unreadable answer: ' + line.slice(0, 200) }; }
      const next = this.waiting.shift();
      if (next) next(value); else this.answers.push(value);
    });
    this.exited = new Promise(resolve => this.child.on('exit', code => { for (const next of this.waiting.splice(0)) next({ ok: false, error: 'host exited ' + code }); resolve(code); }));
  }
  request(value) {
    this.child.stdin.write(JSON.stringify(value) + '\n');
    return new Promise(resolve => { const ready = this.answers.shift(); if (ready) resolve(ready); else this.waiting.push(resolve); });
  }
  async close() {
    if (this.child.exitCode !== null) return;
    await Promise.race([this.request({ op: 'close' }), new Promise(resolve => setTimeout(resolve, 5000))]);
    if (this.child.exitCode === null) this.child.kill();
    await this.exited;
  }
}

// ---------------------------------------------------------------- entries

const refreshDir = path.dirname(require.resolve('@rspack/plugin-react-refresh/package.json', { paths: [workbench] }));
export const refresh = {
  entry: path.join(refreshDir, 'client', 'reactRefreshEntry.js'),
  runtime: path.join(refreshDir, 'client', 'reactRefresh.js'),
  utils: path.join(refreshDir, 'client', 'refreshUtils.js'),
  runtime_dir: path.dirname(require.resolve('react-refresh/runtime', { paths: [workbench] })),
};

/** The shipping pipeline, used only to generate entry sources. */
export function entryMaker(root) {
  const compiler = new Compiler(root, {});
  const pipeline = new Pipeline(compiler, { validate, docsRequest, mockFetches, createAstro: () => { throw new Error('Astro is outside the lab subset'); } },
    { previewDir: path.join(workbench, 'ui/preview') });
  return {
    compiler,
    async entries(files) {
      const out = [];
      for (const file of files) { const entry = await pipeline.previewEntry(file); out.push({ slug: entry.slug, file: entry.file, source: entry.source }); }
      return out;
    },
    hotClient(generation) {
      return { slug: 'hot-client', file: path.join(root, '.workbench-rspack', 'hot-client-web.js'), source: hotClientSource('web', generation) };
    },
  };
}

export function job(root, cacheDir, maker, entries, generation = 1) {
  return { op: 'start', root, preview_dir: path.join(workbench, 'ui/preview'), cache_dir: cacheDir, generation, hot_client: maker.hotClient(generation), entries, refresh, stable_chunks: true };
}

// ---------------------------------------------------------------- fixtures

export async function fixture(runDir, size) {
  const root = path.join(runDir, 'fixtures', size);
  if (fs.existsSync(path.join(root, 'workbench.yaml'))) return { size, root, manifest: JSON.parse(fs.readFileSync(path.join(runDir, 'fixtures', size + '.json'), 'utf8')) };
  log(`Generating and installing the ${size} fixture`);
  const module = size === 'formats' ? await import('../fixtures/formats.mjs') : await import('../fixtures/acme-app.mjs');
  const manifest = module.generate(root, { size, seed: 1 });
  install(root, path.join(here, '..', 'fixtures', 'locks', (size === 'formats' ? 'formats' : LOCKFILE[size]) + '.package-lock.json'));
  fs.writeFileSync(path.join(runDir, 'fixtures', size + '.json'), JSON.stringify(manifest));
  return { size, root, manifest };
}
