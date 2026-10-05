/* The installed packages the extension needs at runtime: its dependencies and
   optional dependencies, and theirs, resolved the way Node resolves them in a
   flat node_modules. vsce would otherwise ask `npm list`, which on a pnpm tree
   also reports dev-only packages it can't place as production ones. Optional
   and peer dependencies that aren't installed are skipped; a missing required
   dependency is an error, since the extension would fail without it. */
const fs = require('node:fs');
const path = require('node:path');

function readManifest(dir) {
  return JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'));
}

/* node_modules/<name> in `from` or the nearest folder above it, up to `root`. */
function locate(root, from, name) {
  for (let dir = from; ; dir = path.dirname(dir)) {
    const candidate = path.join(dir, 'node_modules', name);
    if (fs.existsSync(path.join(candidate, 'package.json'))) return candidate;
    if (dir === root || path.dirname(dir) === dir) return null;
  }
}

function productionDependencies(root) {
  root = path.resolve(root);
  const found = new Set();
  const queue = [root];
  while (queue.length) {
    const dir = queue.shift();
    const manifest = readManifest(dir);
    const required = Object.keys(manifest.dependencies || {});
    const optional = Object.keys(manifest.optionalDependencies || {}).concat(Object.keys(manifest.peerDependencies || {}));
    for (const name of required.concat(optional)) {
      const location = locate(root, dir, name);
      if (!location) {
        if (required.includes(name) && !optional.includes(name)) {
          throw new Error('Missing production dependency ' + name + ' of ' + (manifest.name || path.relative(root, dir)));
        }
        continue;
      }
      if (found.has(location)) continue;
      found.add(location);
      queue.push(location);
    }
  }
  return Array.from(found).sort();
}

module.exports = { productionDependencies };
