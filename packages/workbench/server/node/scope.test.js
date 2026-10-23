/* Gate is an external CLI boundary. Each fake owns only temporary checkout
   reports; no test can read the user's profile or start its manager. */
var test = require('node:test');
var assert = require('node:assert/strict');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var http = require('node:http');
var server = require('./server');

test('Gate lenses follow checkout endpoints across refresh, readiness and trust', async function (t) {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-scope-lens-'));
  var running = [];
  var upstreams = [];
  t.after(async function () {
    for (var host of running) await host.close();
    for (var upstream of upstreams) await new Promise(function (resolve) { upstream.close(resolve); });
    fs.rmSync(root, { recursive: true, force: true });
  });
  var bin = path.join(root, 'bin');
  fs.mkdirSync(bin);
  fs.writeFileSync(path.join(bin, 'gate'), '#!' + process.execPath + '\n' +
    'const fs = require("node:fs");\n' +
    'fs.appendFileSync("scope-invocations.jsonl", JSON.stringify({cwd:process.cwd(),args:process.argv.slice(2)})+"\\n");\n' +
    'process.stdout.write(fs.readFileSync("scope-urls.json"));\n', { mode: 0o755 });
  for (var name of ['first', 'second']) {
    var upstream = http.createServer(function (_req, res) { res.end('<p>Acme</p>'); });
    await new Promise(function (resolve) { upstream.listen(0, '127.0.0.1', resolve); });
    upstreams.push(upstream);
    var checkout = path.join(root, name);
    fs.mkdirSync(checkout);
    fs.mkdirSync(path.join(checkout, 'demo'));
    fs.writeFileSync(path.join(checkout, 'workbench.yaml'), 'name: Acme\npreviews: false\nspaces:\n  acme:\n    name: Demo\n    root: demo\n    implementations:\n      dev:\n        kind: url\n        base: gate://demo/dev\n    collections:\n      - name: Pages\n        items:\n          - label: Sign in\n            src: sign-in.html\n            implementations:\n              dev: /sign-in.html\n');
    var report = { task_id: name, routes: { demo: { state: 'published', direct_url: 'http://127.0.0.1:' + upstream.address().port + '/' } } };
    fs.writeFileSync(path.join(checkout, 'demo', 'scope-urls.json'), JSON.stringify({ version: name === 'first' ? 1 : 2, ok: true, data: report }));
    var oldPath = process.env.PATH;
    process.env.PATH = bin + path.delimiter + oldPath;
    try {
      running.push(await server.start({ root: path.join(checkout, 'demo'), config: { dir: checkout, key: 'acme' }, capture: { close: function () {} } }));
    } finally { process.env.PATH = oldPath; }
  }
  var first = await config(running[0]);
  var second = await config(running[1]);
  assert.equal(first.implementations.dev.upstream, 'http://127.0.0.1:' + upstreams[0].address().port + '/dev');
  assert.equal(second.implementations.dev.upstream, 'http://127.0.0.1:' + upstreams[1].address().port + '/dev');
  assert.notEqual(first.implementations.dev.base, second.implementations.dev.base);
  var rendered = await fetch(first.implementations.dev.base + '/sign-in.html');
  assert.equal(rendered.status, 200);
  assert.match(await rendered.text(), /Acme/);
  var folder = path.join(root, 'first', 'demo');
  var invocationFile = path.join(folder, 'scope-invocations.jsonl');
  var beforeInput = fs.readFileSync(invocationFile, 'utf8');
  for (var attempt = 0; attempt < 2; attempt++) {
    var input = await fetch('http://127.0.0.1:' + running[0].port + '/_workbench/simulator/input', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ implementation: 'missing', udid: 'unavailable', action: 'tap', x: 1, y: 1 }),
    });
    assert.equal(input.status, 403, 'the simulator route assembles the view before rejecting an unavailable device');
  }
  assert.equal(fs.readFileSync(invocationFile, 'utf8'), beforeInput, 'simulator input must use cached Gate discovery');
  var invocations = fs.readFileSync(path.join(folder, 'scope-invocations.jsonl'), 'utf8').trim().split('\n').map(JSON.parse);
  assert.ok(invocations.every(function (entry) { return entry.cwd === fs.realpathSync(folder) && JSON.stringify(entry.args) === '["urls","--json"]'; }), JSON.stringify(invocations));
  fs.writeFileSync(path.join(folder, 'scope-urls.json'), JSON.stringify({ version: 2, ok: true, data: { routes: { demo: { state: 'suspended', direct_url: first.implementations.dev.upstream } } } }));
  var stopped = await config(running[0]);
  assert.match(stopped.implementations.dev.problem, /not ready.*start it with Gate/);
  assert.deepEqual(stopped.problems, [], 'a stopped service belongs to its lens, not the sidebar');
  assert.equal(stopped.implementations.dev.upstream, undefined);
  fs.writeFileSync(path.join(folder, 'scope-urls.json'), JSON.stringify({ version: 2, ok: true, data: { routes: { demo: { state: 'published', direct_url: 'http://127.0.0.1:' + upstreams[1].address().port + '/' } } } }));
  assert.equal((await config(running[0])).implementations.dev.upstream, second.implementations.dev.upstream);
  fs.writeFileSync(path.join(folder, 'scope-urls.json'), JSON.stringify({ version: 2, ok: true, data: { routes: { demo: { state: 'published', direct_url: 'http://127.0.0.1:80/' } } } }));
  assert.equal((await config(running[0])).implementations.dev.upstream, 'http://127.0.0.1:80/dev');
  for (var invalid of [
    { routes: { demo: { state: 'published', direct_url: 'http://127.0.0.1:49173/' } } },
    { version: 2, ok: true, data: { routes: { demo: { state: 'published', direct_url: 'https://example.com/' } } } },
    { version: 2, ok: true, data: { routes: { demo: { state: 'published', direct_url: 'http://127.0.0.1:0/' } } } },
  ]) {
    fs.writeFileSync(path.join(folder, 'scope-urls.json'), JSON.stringify(invalid));
    var refused = await config(running[0]);
    assert.match(refused.implementations.dev.problem, /Invalid Gate/);
    assert.deepEqual(refused.problems, [], 'external CLI failures stay on the affected lens');
    assert.equal(refused.implementations.dev.upstream, undefined);
  }
  var untrusted = await server.start({ root: folder, config: { dir: path.join(root, 'first'), key: 'acme' }, isTrusted: false, capture: { close: function () {} } });
  running.push(untrusted);
  var before = fs.readFileSync(path.join(folder, 'scope-invocations.jsonl'), 'utf8');
  var blocked = await config(untrusted);
  assert.match(blocked.implementations.dev.problem, /trusted workspace/);
  assert.deepEqual(blocked.problems, []);
  assert.equal(fs.readFileSync(path.join(folder, 'scope-invocations.jsonl'), 'utf8'), before);
  var manifest = path.join(root, 'first', 'workbench.yaml');
  fs.writeFileSync(manifest, fs.readFileSync(manifest, 'utf8').replace('base: gate://demo/dev', 'base: gate://demo/dev\n        start: node serve.mjs'));
  var invalidConfig = await config(running[0]);
  assert.ok(invalidConfig.problems.some(function (problem) { return /Gate owns service startup/.test(problem); }), 'invalid configuration remains in shared problems');
});

async function config(host) {
  var response = await fetch('http://127.0.0.1:' + host.port + '/_workbench/config');
  assert.equal(response.status, 200);
  return response.json();
}
