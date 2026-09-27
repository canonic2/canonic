var test = require('node:test');
var assert = require('node:assert/strict');
var path = require('node:path');
var plan = require('./scripts/package-all.cjs').plan;
var targets = require('./scripts/bundle-capture.cjs').targets;

test('builds every target with the host last, so its runtime is left in place', function () {
  var p = plan([], 'darwin-arm64');
  assert.deepEqual(p.targets.slice().sort(), targets.slice().sort());
  assert.equal(p.targets[p.targets.length - 1], 'darwin-arm64');
  assert.equal(p.out, path.resolve(__dirname, 'dist'));
});

test('builds only the named targets, into the requested directory', function () {
  var p = plan(['linux-x64', '--out', 'builds', 'win32-arm64'], 'linux-x64');
  assert.deepEqual(p.targets, ['win32-arm64', 'linux-x64']);
  assert.equal(p.out, path.resolve('builds'));
});

test('rejects a target the capture runtime cannot be bundled for', function () {
  assert.throws(function () { plan(['alpine-x64'], 'darwin-x64'); }, /Unsupported target: alpine-x64/);
});
