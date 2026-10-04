/* Package the pinned native compiler for the VSIX target, including when a
   developer builds another platform from this host. No runtime downloads. */
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const tar = require('tar');
const ROOT = path.resolve(__dirname, '..');
const targets = require('./bundle-runtime.cjs').targets;

/* pnpm-lock.yaml records every platform's esbuild package with its integrity,
   even those this host didn't install; the tarball sits at the registry's
   conventional address for that name and version. */
function lockedPackage(lockfile, name, version) {
  const key = "\n  '" + name + '@' + version + "':\n";
  const at = lockfile.indexOf(key);
  if (at < 0) return null;
  const integrity = /^\s+resolution: \{integrity: ([^,}\s]+)/m.exec(lockfile.slice(at + key.length, lockfile.indexOf('\n\n', at + key.length)));
  if (!integrity) return null;
  const base = name.slice(name.indexOf('/') + 1);
  return { version, integrity: integrity[1], resolved: 'https://registry.npmjs.org/' + name + '/-/' + base + '-' + version + '.tgz' };
}

function specification(target, lockfile) {
  if (!targets.includes(target)) throw new Error('Unsupported compiler target: ' + target);
  const version = require('../package.json').dependencies.esbuild;
  const entry = lockedPackage(lockfile || fs.readFileSync(path.join(ROOT, 'pnpm-lock.yaml'), 'utf8'), '@esbuild/' + target, version);
  if (!entry) throw new Error('Missing locked esbuild binary for ' + target);
  return { target, version, entry, member: target.startsWith('win32-') ? 'esbuild.exe' : 'bin/esbuild' };
}
function verify(body, integrity) {
  const [algorithm, expected] = integrity.split('-');
  if (crypto.createHash(algorithm).update(body).digest('base64') !== expected) throw new Error('Compiler archive integrity check failed');
}
async function bundle(target) {
  const spec = specification(target);
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-compiler-build-'));
  try {
    let source;
    try {
      const manifest = require.resolve('@esbuild/' + target + '/package.json', { paths: [ROOT] });
      if (JSON.parse(fs.readFileSync(manifest, 'utf8')).version === spec.version) source = path.join(path.dirname(manifest), spec.member);
    } catch (error) { if (error.code !== 'MODULE_NOT_FOUND') throw error; }
    if (!source || !fs.existsSync(source)) {
      const response = await fetch(spec.entry.resolved);
      if (!response.ok) throw new Error('Compiler download failed: HTTP ' + response.status);
      const body = Buffer.from(await response.arrayBuffer());
      verify(body, spec.entry.integrity);
      const archive = path.join(work, 'compiler.tgz');
      fs.writeFileSync(archive, body);
      await tar.x({ file: archive, cwd: work, strict: true, filter: name => name === 'package/' + spec.member });
      source = path.join(work, 'package', spec.member);
    }
    const destination = path.join(ROOT, 'preview', 'bin');
    fs.rmSync(destination, { recursive: true, force: true });
    fs.mkdirSync(destination, { recursive: true });
    const binary = path.join(destination, target.startsWith('win32-') ? 'esbuild.exe' : 'esbuild');
    fs.copyFileSync(source, binary);
    fs.chmodSync(binary, 0o755);
    fs.writeFileSync(path.join(destination, 'compiler.json'), JSON.stringify({ target, version: spec.version }) + '\n');
    console.log('Bundled native preview compiler for ' + target);
  } finally { fs.rmSync(work, { recursive: true, force: true }); }
}
module.exports = { bundle, specification, verify };
