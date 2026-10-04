const test = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const { specification, verify } = require('./bundle-compiler.cjs');
const targets = require('./bundle-runtime.cjs').targets;

test('each supported VSIX target has a matching locked native compiler', () => {
  const version = require('../package.json').dependencies.esbuild;
  for (const target of targets) {
    const spec = specification(target);
    assert.equal(spec.entry.version, version);
    assert.equal(spec.entry.resolved, 'https://registry.npmjs.org/@esbuild/' + target + '/-/' + target + '-' + version + '.tgz');
    assert.match(spec.entry.integrity, /^sha512-[A-Za-z0-9+/]+=*$/);
    assert.equal(spec.member, target.startsWith('win32-') ? 'esbuild.exe' : 'bin/esbuild');
  }
  assert.throws(() => specification('unknown-x64'), /Unsupported/);
});

test('a target missing from the lockfile, or locked at another version, is refused', () => {
  const version = require('../package.json').dependencies.esbuild;
  const lockfile = "packages:\n\n  '@esbuild/linux-x64@0.0.1':\n    resolution: {integrity: sha512-AAAA}\n\n";
  assert.throws(() => specification('linux-x64', lockfile), /Missing locked esbuild binary/);
  const locked = "packages:\n\n  '@esbuild/linux-x64@" + version + "':\n    resolution: {integrity: sha512-AAAA}\n    cpu: [x64]\n\n";
  assert.equal(specification('linux-x64', locked).entry.integrity, 'sha512-AAAA');
  assert.throws(() => specification('darwin-arm64', locked), /Missing locked esbuild binary/);
});

test('a lockfile checked out with CRLF line endings, as on Windows, still names each target', () => {
  const lockfile = require('node:fs').readFileSync(require('node:path').join(__dirname, '..', 'pnpm-lock.yaml'), 'utf8').replace(/\r?\n/g, '\r\n');
  for (const target of targets) assert.match(specification(target, lockfile).entry.integrity, /^sha512-/);
});

test('compiler downloads reject a damaged archive before extraction', () => {
  const body = Buffer.from('Acme compiler archive');
  const integrity = 'sha512-' + crypto.createHash('sha512').update(body).digest('base64');
  assert.doesNotThrow(() => verify(body, integrity));
  assert.throws(() => verify(Buffer.from('damaged'), integrity), /integrity/);
});
