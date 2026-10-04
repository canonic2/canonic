var assert = require('node:assert/strict');
var test = require('node:test');
var EventEmitter = require('node:events');
var PassThrough = require('node:stream').PassThrough;
var fs = require('node:fs');
var capture = require('./electron-capture');
var bundledRuntime = require('./electron-runtime');

function fake(options) {
  options = options || {};
  var children = [];
  var commands = [];
  function spawn(executable, args, settings) {
    var child = new EventEmitter();
    child.stdin = new PassThrough(); child.stdout = new PassThrough(); child.stderr = new PassThrough();
    child.exitCode = null;
    child.kill = function () {
      if (child.exitCode !== null) return;
      child.exitCode = 0;
      child.emit('exit', 0);
    };
    child.profile = settings.env.CANONIC_CAPTURE_PROFILE;
    children.push(child);
    assert.equal(settings.env.ELECTRON_RUN_AS_NODE, undefined);
    assert.equal(settings.env.NODE_OPTIONS, undefined);
    assert.equal(settings.windowsHide, true);
    var buffer = '';
    child.stdin.on('data', function (data) {
      buffer += data;
      var at;
      while ((at = buffer.indexOf('\n')) >= 0) {
        var request = JSON.parse(buffer.slice(0, at)); buffer = buffer.slice(at + 1);
        if (request.method === 'close') { child.kill(); continue; }
        commands.push(request);
        var answer = function (result) { child.stdout.write(JSON.stringify({ id: request.id, result: result || { data: Buffer.from('png').toString('base64') } }) + '\n'); };
        if (options.command) options.command(request, answer, child);
        else answer();
      }
    });
    setImmediate(function () {
      if (options.noReady) return;
      child.stdout.write('{"ready":true}\n');
    });
    return child;
  }
  return { spawn: spawn, children: children, commands: commands };
}

async function until(predicate) {
  var deadline = Date.now() + 2000;
  while (!predicate()) {
    if (Date.now() > deadline) throw new Error('Condition did not settle');
    await new Promise(function (resolve) { setTimeout(resolve, 5); });
  }
}

test('reuses a warm process, captures the requested view, and cleans up its profile', async function (t) {
  var runtime = fake(); var helper = capture.create({ spawn: runtime.spawn });
  t.after(function () { return helper.close(); });
  await helper.warm('http://127.0.0.1:3579');
  await helper.prepare('http://127.0.0.1:3579', { url: '/one', width: 393 });
  assert.equal((await helper.capture('http://127.0.0.1:3579', { url: '/two', width: 960, annotations: 'new annotation' })).toString(), 'png');
  assert.equal(runtime.children.length, 1);
  assert.deepEqual(runtime.commands.map(function (x) { return x.method; }), ['warm', 'prepare', 'capture']);
  assert.equal(runtime.commands[2].payload.annotations, 'new annotation');
  var profile = runtime.children[0].profile;
  await helper.close();
  assert.equal(fs.existsSync(profile), false);
  await assert.rejects(helper.warm('http://127.0.0.1:3579'), /closed/);
});

test('drops superseded speculative prepares and never substitutes one for a capture', async function (t) {
  var runtime = fake(); var helper = capture.create({ spawn: runtime.spawn });
  t.after(function () { return helper.close(); });
  await Promise.all([helper.prepare('http://localhost', { url: '/old' }), helper.prepare('http://localhost', { url: '/latest' })]);
  assert.deepEqual(runtime.commands.map(function (x) { return x.payload.url; }), ['/latest']);
  await Promise.all([helper.capture('http://localhost', { url: '/shot' }), helper.prepare('http://localhost', { url: '/next' })]);
  assert.deepEqual(runtime.commands.slice(1).map(function (x) { return [x.method, x.payload.url]; }), [['capture', '/shot'], ['prepare', '/next']]);
});

test('a crashed helper automatically restores the last view without another caller', async function (t) {
  var runtime = fake(); var helper = capture.create({ spawn: runtime.spawn, retryDelay: 10 });
  t.after(function () { return helper.close(); });
  await helper.preparePage({ url: 'https://example.com/current', revision: '2' });
  runtime.children[0].kill();
  await until(function () { return runtime.commands.length === 2; });
  assert.equal(runtime.children.length, 2);
  assert.deepEqual(runtime.commands[1].payload, runtime.commands[0].payload);
  await helper.close();
  assert.ok(runtime.children.every(function (child) { return !fs.existsSync(child.profile); }));
});

test('times out a blocked renderer, rejects the shot, and does not replay the view that hung', async function (t) {
  var runtime = fake({ command: function (request, answer) { if (request.payload.url !== '/slow') answer(); } });
  var helper = capture.create({ spawn: runtime.spawn, timeout: 30, retryDelay: 10 });
  t.after(function () { return helper.close(); });
  await assert.rejects(helper.capture('http://localhost', { url: '/slow' }), /timed out/);
  await until(function () { return runtime.children[0].exitCode !== null; });
  await new Promise(function (resolve) { setTimeout(resolve, 60); });
  assert.deepEqual(runtime.commands.map(function (x) { return x.payload.url; }), ['/slow']);
  assert.equal(helper.child, null);
  await helper.prepare('http://localhost', { url: '/next' });
  assert.equal(runtime.children.length, 2);
  assert.deepEqual(runtime.commands.map(function (x) { return x.payload.url; }), ['/slow', '/next']);
});

test('closing during startup terminates the process and prevents recovery', async function () {
  var runtime = fake({ noReady: true });
  var helper = capture.create({ spawn: runtime.spawn, timeout: 100, retryDelay: 10 });
  var starting = helper.warm('http://localhost');
  await until(function () { return runtime.children.length; });
  var rejected = assert.rejects(starting, /exited/);
  await helper.close(); await rejected;
  assert.equal(helper.retry, null);
  assert.equal(fs.existsSync(runtime.children[0].profile), false);
});

test('a missing executable does not poison subsequent launch attempts or leave profiles', async function (t) {
  var profiles = [];
  var helper = capture.create({ spawn: function (_file, _args, options) {
    profiles.push(options.env.CANONIC_CAPTURE_PROFILE); throw new Error('Missing executable');
  } });
  t.after(function () { return helper.close(); });
  await assert.rejects(helper.warm('http://localhost'), /Missing executable/);
  await assert.rejects(helper.warm('http://localhost'), /Missing executable/);
  assert.equal(profiles.length, 2);
  assert.ok(profiles.every(function (profile) { return !fs.existsSync(profile); }));
});

test('a host without the bundled runtime gets the reason from every request', async function (t) {
  t.mock.method(bundledRuntime, 'unavailable', function () { return 'it needs macOS 13 or later'; });
  var service = capture.createService({ storage: '/capture/storage' });
  t.after(function () { return service.close(); });
  await assert.rejects(service.warm('http://localhost'), /bundled capture runtime, and it needs macOS 13 or later/);
  await assert.rejects(service.capturePage({ url: 'https://example.com/' }), /it needs macOS 13 or later/);
});

test('a startup failure is reported rather than retried in another engine', async function (t) {
  t.mock.method(bundledRuntime, 'unavailable', function () { return null; });
  t.mock.method(bundledRuntime, 'prepare', async function () { throw new Error('Damaged archive'); });
  var service = capture.createService({ storage: '/capture/storage', retryDelay: 60000 });
  t.after(function () { return service.close(); });
  await assert.rejects(service.capturePage({ url: 'https://example.com/' }), /Damaged archive/);
});

test('retries one capture when a running Electron helper exits unexpectedly', async function (t) {
  var attempts = 0;
  var runtime = fake({ command: function (request, answer, child) {
    if (request.method === 'capturePage' && attempts++ === 0) child.kill();
    else answer();
  } });
  var helper = capture.createService({ spawn: runtime.spawn, retryDelay: 1000 });
  t.after(function () { return helper.close(); });
  var result = await helper.capturePage({ url: 'https://example.com/story', width: 393, height: 852 });
  assert.equal(result.png.toString(), 'png');
  assert.equal(runtime.children.length, 2);
  assert.deepEqual(runtime.commands.map(function (request) { return request.method; }), ['capturePage', 'capturePage']);
});

test('JPEG requests reach the helper and return format-neutral implementation images', async function (t) {
  var runtime = fake();
  var helper = capture.create({ spawn: runtime.spawn });
  t.after(function () { return helper.close(); });
  var payload = { url: 'http://localhost/page', format: 'jpeg' };
  await helper.prepare('http://localhost', payload);
  assert.equal((await helper.capture('http://localhost', payload)).toString(), 'png');
  var result = await helper.capturePage(payload);
  assert.equal(result.image.toString(), 'png');
  assert.equal(result.png, undefined);
  assert.ok(runtime.commands.every(function (request) { return request.payload.format === 'jpeg'; }));
  assert.equal(runtime.commands.find(function (request) { return request.method === 'capturePage'; }).settle, undefined);
});

test('an export pool reuses the warm service and closes only its extra workers', async function () {
  var runtime = fake();
  var service = capture.createService({ spawn: runtime.spawn });
  await service.warm('http://localhost:3579/');
  assert.equal(runtime.children.length, 1);
  var pool = service.createPool(4);
  assert.equal(pool.workers.length, 4);
  await Promise.all(pool.workers.map(function (worker, index) {
    return worker.captureExportPage({
      url: 'http://localhost:6006/iframe.html?id=story--' + index,
      width: 393, height: 852, reuse: 'storybook',
    });
  }));
  assert.equal(runtime.children.length, 4);
  assert.ok(runtime.commands.filter(function (command) { return command.method === 'captureExportPage'; }).every(function (command) {
    return command.payload.reuse === 'storybook' && /Storybook did not finish rendering/.test(command.settle);
  }));
  await pool.close();
  assert.equal(runtime.children.filter(function (child) { return child.exitCode === null; }).length, 1);
  await service.close();
  assert.ok(runtime.children.every(function (child) { return child.exitCode !== null; }));
});
