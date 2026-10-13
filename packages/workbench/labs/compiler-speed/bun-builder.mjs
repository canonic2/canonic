// Bun's sample worker, run by the Bun binary from the run's tools directory.
// Same arguments and output as sample.mjs, with variant "bun".
import fs from 'node:fs';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { applyEdit, outputSummary } from './edits.mjs';

const [, shape, specFile, outdir, , entrySlug, marker = 'lab-' + process.pid] = process.argv.slice(2);
const spec = JSON.parse(fs.readFileSync(specFile, 'utf8'));
const entries = shape === 'B1' ? spec.entries.filter(entry => !entrySlug || entry.slug === entrySlug).slice(0, 1) : spec.entries;
const result = { variant: 'bun', shape, entries: entries.length, incremental: 'none: Bun.build has no incremental API; full rebuild in the same process' };
fs.mkdirSync(outdir, { recursive: true });

const alias = {
  name: 'lab-alias',
  setup(build) {
    for (const [name, target] of Object.entries(spec.alias)) {
      build.onResolve({ filter: new RegExp('^' + name.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&') + '$') }, () => ({ path: target }));
    }
  },
};
const options = {
  entrypoints: entries.map(entry => entry.file), outdir, root: path.dirname(entries[0].file),
  target: 'browser', format: 'esm', splitting: shape !== 'B1', minify: false, sourcemap: 'none',
  define: spec.define, external: spec.external, plugins: [alias],
  loader: Object.fromEntries(spec.fileLoaders.map(extension => [extension, 'file'])),
  naming: { entry: '[name].[ext]', chunk: 'chunks/[name]-[hash].[ext]', asset: 'assets/[name]-[hash].[ext]' },
  throw: true,
};

try {
  if (shape === 'B1' || shape === 'B2') {
    const start = performance.now();
    await Bun.build(options);
    result.ms = performance.now() - start;
  } else if (shape === 'B3' || shape === 'B4') {
    await Bun.build(options);
    let editStart;
    const restore = applyEdit(spec, shape, spec.entries[0], marker, () => { editStart = performance.now(); });
    try {
      await Bun.build(options);
      result.ms = performance.now() - editStart;
    } finally { restore(); }
    result.editVisible = outputSummary(outdir, marker).markerFound;
  } else {
    throw new Error('Bun has no persistent cache; shape ' + shape + ' does not apply.');
  }
  Object.assign(result, outputSummary(outdir));
  result.ok = true;
} catch (error) {
  result.ok = false;
  result.error = String(error?.stack || error).slice(0, 4000);
}
result.maxRssMb = Math.round(process.resourceUsage().maxRSS / 1024);
process.stdout.write(JSON.stringify(result) + '\n');
process.exit(0);
