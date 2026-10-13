// Source edits for rebuild shapes, and output summaries. Shared by the Node
// and Bun sample workers.
import fs from 'node:fs';
import path from 'node:path';

// B3 edits the first preview's own component (a large page's Interactive.tsx,
// or a small fixture's component). B4 edits a module every preview imports:
// the large fixture's app shell, or the small fixture's environment. Returns a
// function that restores the file.
export function applyEdit(spec, shape, entry, marker, beforeWrite) {
  const shell = path.join(spec.fixture, 'src/app/Shell.tsx');
  const large = fs.existsSync(shell);
  const file = shape === 'B4'
    ? (large ? shell : path.join(spec.fixture, 'src/workbench/environment.tsx'))
    : path.join(spec.fixture, entry.edit.component);
  const original = fs.readFileSync(file, 'utf8');
  const edited = shape === 'B4'
    ? (large ? original.replace('aria-label="Features"', `aria-label="Features ${marker}"`) : original.replace('className="acme-root"', `className="acme-root ${marker}"`))
    : (large ? original.replace(/Revision [^<]*/, `Revision ${marker}`) : original.replace(/title = '([^']*)'/, `title = '$1 ${marker}'`));
  if (edited === original) throw new Error('Edit did not apply to ' + file);
  beforeWrite();
  fs.writeFileSync(file, edited);
  return () => fs.writeFileSync(file, original);
}

export function outputSummary(outdir, marker) {
  let files = 0, bytes = 0, jsFiles = 0, jsBytes = 0, markerFound = false;
  const walk = directory => {
    for (const item of fs.readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, item.name);
      if (item.isDirectory()) { if (item.name !== 'tmp' && !item.name.startsWith('persistent-cache')) walk(file); continue; }
      const size = fs.statSync(file).size;
      files++; bytes += size;
      if (/\.m?js$/.test(item.name)) {
        jsFiles++; jsBytes += size;
        if (marker && !markerFound && fs.readFileSync(file, 'utf8').includes(marker)) markerFound = true;
      }
    }
  };
  walk(outdir);
  return marker ? { markerFound } : { files, bytes, jsFiles, jsBytes };
}
