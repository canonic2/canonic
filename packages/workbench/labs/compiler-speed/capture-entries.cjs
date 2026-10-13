// Captures the entry modules Workbench's shipping compiler generates for each
// preview, so every variant builds the same entries. Writes them, plus copies
// of the Workbench runtime modules they import, into <fixture>/.lab/.
// Usage: node capture-entries.cjs <fixture> <workbench-package-dir>
const fs = require('node:fs');
const path = require('node:path');

const [fixture, workbench] = process.argv.slice(2).map(value => path.resolve(value));
const previewDir = path.join(workbench, 'preview');
const engine = require.resolve(path.join(previewDir, 'engine.cjs'));
const esbuild = require(engine);
const captured = new Map();
let current;
require.cache[engine].exports = { ...esbuild, build: async options => {
  if (options.stdin && options.entryNames === 'preview') captured.set(current, options);
  return esbuild.build(options);
} };
const { Compiler } = require(path.join(previewDir, 'compiler.cjs'));

(async () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(fixture, 'acme-manifest.json'), 'utf8'));
  const compiler = new Compiler(fixture, {});
  compiler.diskCache = { read: () => null, write: () => {} };
  const lab = path.join(fixture, '.lab');
  const runtime = path.join(lab, 'runtime');
  fs.mkdirSync(path.join(lab, 'entries'), { recursive: true });
  fs.mkdirSync(runtime, { recursive: true });
  for (const name of ['react.js', 'api.js']) fs.copyFileSync(path.join(previewDir, name), path.join(runtime, name));
  const entries = [];
  for (const preview of manifest.previews.filter(item => !item.broken)) {
    current = preview.id;
    await compiler.compile(preview.file);
    const options = captured.get(preview.id);
    if (!options) throw new Error('No entry captured for ' + preview.id);
    // Point Workbench's runtime imports at the copies inside the fixture, so
    // packages they import resolve from the project for every compiler.
    const source = options.stdin.contents.split(previewDir + path.sep).join(runtime + path.sep);
    const slug = preview.id.replace(/\//g, '--');
    const file = path.join(lab, 'entries', slug + '.js');
    fs.writeFileSync(file, source);
    entries.push({ id: preview.id, slug, file, definition: path.join(fixture, preview.file), edit: preview.edit });
  }
  const sample = captured.values().next().value;
  const spec = {
    fixture, workbench, runtime, entries,
    alias: { '@canonic2/workbench': path.join(runtime, 'api.js') },
    external: sample.external,
    define: sample.define,
    target: sample.target, jsx: sample.jsx,
    resolveExtensions: sample.resolveExtensions,
    fileLoaders: Object.keys(sample.loader).filter(extension => sample.loader[extension] === 'file'),
  };
  fs.writeFileSync(path.join(lab, 'spec.json'), JSON.stringify(spec, null, 2) + '\n');
  console.log(`Captured ${entries.length} entries into ${path.join(lab, 'entries')}`);
})().catch(error => { console.error(error); process.exit(1); });
