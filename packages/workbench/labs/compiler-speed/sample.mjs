// Runs one timed sample in a fresh process and prints one JSON line.
// node sample.mjs <variant> <shape> <spec.json> <outdir> <tools> [entry-slug] [marker]
//   variant: shipping | esbuild | rolldown | rspack | utoo   (bun: bun-builder.mjs)
//   shape:   B1 (one entry, no splitting) | B2 (all entries, split)
//            B3 (rebuild after a page edit) | B4 (rebuild after a shared edit)
//            B5 (split build with a warm persistent cache after a page edit)
//            B5P (untimed: split build that primes the persistent cache)
//            B6 (development build with HMR: time to the hot update for an edit)
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';
import { applyEdit, outputSummary } from './edits.mjs';

const [variant, shape, specFile, outdir, tools, entrySlug, marker = 'lab-' + process.pid, publicPath] = process.argv.slice(2);
const spec = JSON.parse(fs.readFileSync(specFile, 'utf8'));
const here = path.dirname(fileURLToPath(import.meta.url));
const pick = () => entrySlug ? spec.entries.filter(entry => entry.slug === entrySlug) : spec.entries.slice(0, 1);
const entries = shape === 'B1' || variant === 'shipping' ? pick() : spec.entries;
const split = shape !== 'B1';
const result = { variant, shape, entries: entries.length };
let summaryDir = outdir;
fs.mkdirSync(outdir, { recursive: true });

async function shipping() {
  // Workbench's shipping compiler, with its disk cache under the run directory.
  process.env.TMPDIR = path.join(outdir, 'tmp');
  fs.mkdirSync(process.env.TMPDIR, { recursive: true });
  const start = performance.now();
  const { Compiler } = createRequire(import.meta.url)(path.join(spec.workbench, 'preview', 'compiler.cjs'));
  const compiler = new Compiler(spec.fixture, {});
  const definition = entries[0].definition;
  let built = await compiler.compile(definition);
  if (shape === 'B1') {
    result.ms = performance.now() - start;
    result.memoryJsBytes = [...built.outputs].filter(([name]) => /\.js$/.test(name)).reduce((sum, [, body]) => sum + body.length, 0);
    return built;
  }
  const restore = applyEdit(spec, shape, entries[0], marker, () => {});
  try {
    const editStart = performance.now();
    built = await compiler.compile(definition);
    result.ms = performance.now() - editStart;
  } finally { restore(); }
  result.editVisible = built.outputs.get('preview.js').toString().includes(marker);
  return built;
}

async function hot() {
  const { hmrBuilders } = await import('./builders.mjs');
  const builder = await hmrBuilders[variant]({ spec, entries, outdir, tools });
  try {
    await builder.start();
    let editStart;
    const pending = builder.update();
    const restore = applyEdit(spec, 'B3', spec.entries[0], marker, () => { editStart = performance.now(); });
    try {
      const update = await Promise.race([pending, new Promise((_, reject) => setTimeout(() => reject(new Error('No hot update within 30 s')), 30000))]);
      result.ms = performance.now() - editStart;
      result.hotUpdates = update.updates;
      result.editVisible = update.text.includes(marker);
    } finally { restore(); }
  } finally { await builder.close(); }
}

async function bundler() {
  if (shape === 'B6') return hot();
  const start = performance.now();
  const { builders } = await import('./builders.mjs');
  // B5P primes the persistent cache that the following B5 sample reads.
  const persistentCache = shape === 'B5' || shape === 'B5P' ? path.join(path.dirname(outdir), 'persistent-cache-' + variant) : undefined;
  const builder = await builders[variant]({ spec, entries, split, outdir, tools, persistentCache, publicPath });
  result.incremental = builder.incremental;
  if (builder.outputDir) { result.outputDir = builder.outputDir; summaryDir = builder.outputDir; }
  const close = builder.close;
  let closed;
  builder.close = () => (closed ||= close());
  try {
    if (shape === 'B1' || shape === 'B2' || shape === 'B5P') {
      await builder.build();
      result.ms = performance.now() - start;
    } else if (shape === 'B5') {
      // The caller ran a B2 with this cache first; edit, then build cold.
      const restore = applyEdit(spec, 'B3', spec.entries[0], marker, () => {});
      try {
        const editStart = performance.now();
        await builder.build();
        result.ms = performance.now() - editStart;
      } finally { restore(); }
      result.editVisible = outputSummary(summaryDir, marker).markerFound;
    } else {
      await builder.prepareRebuild();
      let editStart;
      const restore = applyEdit(spec, shape, spec.entries[0], marker, () => { editStart = performance.now(); });
      try {
        await builder.rebuild();
        result.ms = performance.now() - editStart;
      } finally {
        await builder.close();
        restore();
      }
      result.editVisible = outputSummary(summaryDir, marker).markerFound;
    }
  } finally { await builder.close(); }
}

try {
  if (variant === 'shipping') await shipping(); else await bundler();
  Object.assign(result, outputSummary(summaryDir));
  result.ok = true;
} catch (error) {
  result.ok = false;
  result.error = String(error?.stack || error).slice(0, 4000);
}
result.maxRssMb = Math.round(process.resourceUsage().maxRSS / 1024);
process.stdout.write(JSON.stringify(result) + '\n');
// Native compiler threads can keep the process alive after close.
process.exit(0);
