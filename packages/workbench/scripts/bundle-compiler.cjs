/* Package the pinned native compilers for the VSIX target, including when a
   developer builds another platform from this host: esbuild's binary and
   Rspack's native binding. No runtime downloads. */
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const tar = require('tar');
const ROOT = path.resolve(__dirname, '..');
const targets = require('./bundle-runtime.cjs').targets;

/* Rspack names its Linux and Windows bindings by C library and toolchain. */
const RSPACK_TARGETS = {
  'darwin-arm64': 'darwin-arm64', 'darwin-x64': 'darwin-x64',
  'linux-x64': 'linux-x64-gnu', 'linux-arm64': 'linux-arm64-gnu',
  'win32-x64': 'win32-x64-msvc', 'win32-arm64': 'win32-arm64-msvc',
};

/* pnpm-lock.yaml records every platform's compiler package with its
   integrity, even those this host didn't install; the tarball sits at the
   registry's conventional address for that name and version. A Windows
   checkout can have CRLF line endings, so they are normalized first. */
function lockedPackage(lockfile, name, version) {
  lockfile = lockfile.replace(/\r\n/g, '\n');
  const key = "\n  '" + name + '@' + version + "':\n";
  const at = lockfile.indexOf(key);
  if (at < 0) return null;
  const integrity = /^\s+resolution: \{integrity: ([^,}\s]+)/m.exec(lockfile.slice(at + key.length, lockfile.indexOf('\n\n', at + key.length)));
  if (!integrity) return null;
  const base = name.slice(name.indexOf('/') + 1);
  return { version, integrity: integrity[1], resolved: 'https://registry.npmjs.org/' + name + '/-/' + base + '-' + version + '.tgz' };
}

function readLockfile(lockfile) {
  return lockfile || fs.readFileSync(path.join(ROOT, 'pnpm-lock.yaml'), 'utf8');
}

function specification(target, lockfile) {
  if (!targets.includes(target)) throw new Error('Unsupported compiler target: ' + target);
  const version = require('../package.json').dependencies.esbuild;
  const entry = lockedPackage(readLockfile(lockfile), '@esbuild/' + target, version);
  if (!entry) throw new Error('Missing locked esbuild binary for ' + target);
  return { target, version, entry, name: '@esbuild/' + target, member: target.startsWith('win32-') ? 'esbuild.exe' : 'bin/esbuild', file: target.startsWith('win32-') ? 'esbuild.exe' : 'esbuild', executable: true };
}

function rspackSpecification(target, lockfile) {
  if (!targets.includes(target)) throw new Error('Unsupported compiler target: ' + target);
  const version = require('../package.json').dependencies['@rspack/core'];
  const platform = RSPACK_TARGETS[target];
  const name = '@rspack/binding-' + platform;
  const entry = lockedPackage(readLockfile(lockfile), name, version);
  if (!entry) throw new Error('Missing locked Rspack binding for ' + target);
  return { target, version, entry, name, member: 'rspack.' + platform + '.node', file: 'rspack.node', executable: false };
}

function verify(body, integrity) {
  const [algorithm, expected] = integrity.split('-');
  if (crypto.createHash(algorithm).update(body).digest('base64') !== expected) throw new Error('Compiler archive integrity check failed');
}

/* The installed package when it is the locked version, otherwise the locked
   tarball, verified before extraction. */
async function fetchMember(spec, work) {
  try {
    const manifest = require.resolve(spec.name + '/package.json', { paths: [ROOT] });
    const source = path.join(path.dirname(manifest), spec.member);
    if (JSON.parse(fs.readFileSync(manifest, 'utf8')).version === spec.version && fs.existsSync(source)) return source;
  } catch (error) { if (error.code !== 'MODULE_NOT_FOUND') throw error; }
  const response = await fetch(spec.entry.resolved);
  if (!response.ok) throw new Error('Compiler download failed: HTTP ' + response.status);
  const body = Buffer.from(await response.arrayBuffer());
  verify(body, spec.entry.integrity);
  const archive = path.join(work, path.basename(spec.file) + '.tgz');
  fs.writeFileSync(archive, body);
  await tar.x({ file: archive, cwd: work, strict: true, filter: name => name === 'package/' + spec.member });
  return path.join(work, 'package', spec.member);
}

async function bundle(target) {
  const specs = [specification(target), rspackSpecification(target)];
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-compiler-build-'));
  try {
    const destination = path.join(ROOT, 'preview', 'bin');
    fs.rmSync(destination, { recursive: true, force: true });
    fs.mkdirSync(destination, { recursive: true });
    for (const spec of specs) {
      const source = await fetchMember(spec, work);
      const file = path.join(destination, spec.file);
      fs.copyFileSync(source, file);
      if (spec.executable) fs.chmodSync(file, 0o755);
      fs.rmSync(path.join(work, 'package'), { recursive: true, force: true });
    }
    fs.writeFileSync(path.join(destination, 'compiler.json'), JSON.stringify({ target, version: specs[0].version, rspack: specs[1].version }) + '\n');
    console.log('Bundled native preview compilers for ' + target);
  } finally { fs.rmSync(work, { recursive: true, force: true }); }
}
module.exports = { bundle, specification, rspackSpecification, verify, RSPACK_TARGETS };
