// Writes the shipping compiler's output for every captured entry, for render
// parity. Usage: node shipping-outputs.cjs <spec.json> <outdir>
const fs = require('node:fs');
const path = require('node:path');

const [specFile, outdir] = process.argv.slice(2).map(value => path.resolve(value));
const spec = JSON.parse(fs.readFileSync(specFile, 'utf8'));
process.env.TMPDIR = path.join(outdir, 'tmp');
fs.mkdirSync(process.env.TMPDIR, { recursive: true });
const { Compiler } = require(path.join(spec.workbench, 'preview', 'compiler.cjs'));

(async () => {
  const compiler = new Compiler(spec.fixture, {});
  for (const entry of spec.entries) {
    // Development builds, as the live canvas loads them and as the entries
    // the other variants build were captured.
    const result = await compiler.compile(entry.definition);
    for (const [name, body] of result.outputs) {
      const file = path.join(outdir, entry.slug, name);
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, body);
    }
  }
})().catch(error => { console.error(error); process.exit(1); });
