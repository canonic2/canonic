var assert = require('node:assert/strict');
var test = require('node:test');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var crypto = require('node:crypto');
var tar = require('tar');
var runtime = require('./electron-runtime');

async function fixture(t) {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-runtime-test-'));
  t.after(function () { fs.rmSync(root, { recursive: true, force: true }); });
  var source = path.join(root, 'source'); var bundle = path.join(root, 'bundle');
  var storage = path.join(root, 'storage');
  var binary = runtime.executable(source);
  fs.mkdirSync(path.dirname(binary), { recursive: true }); fs.mkdirSync(bundle);
  fs.writeFileSync(binary, 'fake native executable', { mode: 0o755 });
  if (process.platform !== 'win32') fs.symlinkSync(path.relative(source, binary), path.join(source, 'framework-link'));
  var archive = path.join(bundle, 'runtime.tar.gz');
  await tar.c({ file: archive, gzip: true, cwd: source }, ['.']);
  var manifest = { target: process.platform + '-' + process.arch, sha256: crypto.createHash('sha256').update(fs.readFileSync(archive)).digest('hex') };
  fs.writeFileSync(path.join(bundle, 'runtime.json'), JSON.stringify(manifest));
  return { source: source, bundle: bundle, storage: storage, archive: archive, manifest: manifest };
}

test('extracts a complete runtime once, preserving executable bits and framework links', async function (t) {
  var f = await fixture(t);
  var binary = await runtime.prepare(f.storage, f.bundle);
  assert.equal(fs.readFileSync(binary, 'utf8'), 'fake native executable');
  if (process.platform !== 'win32') {
    assert.ok(fs.statSync(binary).mode & 0o100);
    assert.ok(fs.lstatSync(path.join(f.storage, f.manifest.sha256, 'framework-link')).isSymbolicLink());
  }
  fs.rmSync(f.archive);
  assert.equal(await runtime.prepare(f.storage, f.bundle), binary, 'Cache reuse needs no archive read');
});

test('concurrent extension hosts converge on one complete cache', async function (t) {
  var f = await fixture(t);
  var files = await Promise.all([runtime.prepare(f.storage, f.bundle), runtime.prepare(f.storage, f.bundle)]);
  assert.equal(files[0], files[1]);
  assert.deepEqual(fs.readdirSync(f.storage), [f.manifest.sha256]);
});

test('removes previous runtimes and abandoned extractions once the current one is in place', async function (t) {
  var f = await fixture(t);
  var old = path.join(f.storage, 'a'.repeat(64)); var stale = path.join(f.storage, '.unpack-stale'); var fresh = path.join(f.storage, '.unpack-fresh');
  [old, stale, fresh].forEach(function (dir) { fs.mkdirSync(dir, { recursive: true }); fs.writeFileSync(path.join(dir, 'file'), 'x'); });
  var yesterday = new Date(Date.now() - 24 * 60 * 60 * 1000);
  fs.utimesSync(stale, yesterday, yesterday);
  await runtime.prepare(f.storage, f.bundle);
  assert.deepEqual(fs.readdirSync(f.storage).sort(), ['.unpack-fresh', f.manifest.sha256]);
});

test('rejects a damaged archive without publishing a partial runtime', async function (t) {
  var f = await fixture(t); fs.appendFileSync(f.archive, 'damage');
  await assert.rejects(runtime.prepare(f.storage, f.bundle), /damaged/);
  assert.deepEqual(fs.readdirSync(f.storage), []);
});

test('rejects a runtime for another platform before extraction', async function (t) {
  var f = await fixture(t); f.manifest.target = 'wrong-platform';
  fs.writeFileSync(path.join(f.bundle, 'runtime.json'), JSON.stringify(f.manifest));
  await assert.rejects(runtime.prepare(f.storage, f.bundle), /does not match/);
  assert.equal(fs.existsSync(f.storage), false);
});
