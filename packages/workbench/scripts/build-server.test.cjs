const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const vm = require('node:vm');
const { Worker } = require('node:worker_threads');
const { once } = require('node:events');

function fixture(t, overrides = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-server-publish-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const source = path.join(root, 'source');
  const module = { exports: {} };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'build-server.cjs'), 'utf8'), {
    require: name => name === 'node:fs' ? { ...fs, ...overrides } : require(name),
    module, __dirname: path.join(root, 'scripts'),
    process: { env: { WORKBENCH_SERVER_BINARY: source }, platform: process.platform, arch: process.arch },
  });
  return { root, source, build: module.exports.build };
}

test('publishing a server leaves an existing executable reader on its complete original file', t => {
  const { root, source, build } = fixture(t);
  fs.writeFileSync(source, 'original executable bytes');
  const destination = build();
  const original = fs.openSync(destination, 'r');
  t.after(() => fs.closeSync(original));
  const inode = fs.fstatSync(original).ino;
  fs.writeFileSync(source, 'replacement executable bytes with a different length');
  assert.equal(build(), destination);
  assert.equal(fs.readFileSync(original, 'utf8'), 'original executable bytes');
  assert.notEqual(fs.statSync(destination).ino, inode);
  assert.equal(fs.readFileSync(destination, 'utf8'), fs.readFileSync(source, 'utf8'));
  if (process.platform !== 'win32') assert.equal(fs.statSync(destination).mode & 0o111, 0o111);
  assert.deepEqual(fs.readdirSync(path.dirname(destination)), [path.basename(destination)]);
  assert.ok(destination.startsWith(root + path.sep));
});

test('an interrupted copy preserves the published executable and removes its partial staging file', t => {
  let interrupt = false;
  const { source, build } = fixture(t, { copyFileSync(from, to) {
    if (!interrupt) return fs.copyFileSync(from, to);
    fs.writeFileSync(to, 'partial executable');
    throw new Error('Acme copy interrupted');
  } });
  fs.writeFileSync(source, 'working executable');
  const destination = build();
  const inode = fs.statSync(destination).ino;
  interrupt = true;
  assert.throws(() => build(), /Acme copy interrupted/);
  assert.equal(fs.statSync(destination).ino, inode);
  assert.equal(fs.readFileSync(destination, 'utf8'), 'working executable');
  assert.deepEqual(fs.readdirSync(path.dirname(destination)), [path.basename(destination)]);
});

test('a running native server can read its catalog after a build publishes its executable', async t => {
  const server = require('../server/node/server');
  const { root, source, build } = fixture(t);
  fs.copyFileSync(server.binaryPath(), source);
  const destination = build();
  const project = path.join(root, 'project');
  fs.mkdirSync(project);
  fs.writeFileSync(path.join(project, 'workbench.yaml'), 'name: Acme\ncollections: []\n');
  const logs = [];
  const running = await server.start({ root: project, binary: destination, onLog: record => logs.push(record) });
  t.after(() => running.close());
  const release = new SharedArrayBuffer(4);
  const publishing = new Worker(path.join(__dirname, 'fixtures/publish-server.cjs'), { workerData: { builder: path.join(__dirname, 'build-server.cjs'), root, source, release } });
  const exited = once(publishing, 'exit');
  try {
    const [opened] = await once(publishing, 'message');
    assert.equal(opened, 'copy-opened');
    const response = await fetch(new URL('config', running.url), { signal: AbortSignal.timeout(10000) });
    assert.equal(response.ok, true, JSON.stringify(logs));
    assert.equal((await response.json()).ok, true);
  } finally {
    Atomics.store(new Int32Array(release), 0, 1);
    Atomics.notify(new Int32Array(release), 0);
    const [code] = await exited;
    assert.equal(code, 0);
  }
  assert.equal(logs.filter(record => record.event === 'server.started').length, 1, JSON.stringify(logs));
  assert.equal(logs.some(record => record.event === 'server.exited'), false, JSON.stringify(logs));
});
