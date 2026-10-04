const test = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const { specification, verify } = require('./bundle-compiler.cjs');
const targets = require('./bundle-runtime.cjs').targets;

test('each supported VSIX target has a matching locked native compiler', () => {
  for (const target of targets) {
    const spec = specification(target);
    assert.equal(spec.entry.version, require('../package.json').dependencies.esbuild);
    assert.ok(spec.entry.resolved.includes('@esbuild/' + target + '/'));
    assert.equal(spec.member, target.startsWith('win32-') ? 'esbuild.exe' : 'bin/esbuild');
  }
  assert.throws(() => specification('unknown-x64'), /Unsupported/);
});

test('compiler downloads reject a damaged archive before extraction', () => {
  const body = Buffer.from('Acme compiler archive');
  const integrity = 'sha512-' + crypto.createHash('sha512').update(body).digest('base64');
  assert.doesNotThrow(() => verify(body, integrity));
  assert.throws(() => verify(Buffer.from('damaged'), integrity), /integrity/);
});
