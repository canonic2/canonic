var assert = require('node:assert/strict');
var EventEmitter = require('node:events');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var stream = require('node:stream');
var test = require('node:test');

var chrome = require('./chrome.cjs');

test('JPEG screenshots keep quality and format when retrying without optimizeForSpeed', async function () {
  var calls = [];
  var target = { send: async function (_method, options) {
    calls.push(Object.assign({}, options));
    if (calls.length === 1) throw new Error('Invalid parameters: optimizeForSpeed');
    return { data: Buffer.from('jpeg').toString('base64') };
  } };
  assert.equal((await chrome.Target.prototype.screenshot.call(target, 'jpeg')).toString(), 'jpeg');
  assert.deepEqual(calls.map(function (call) { return [call.format, call.quality]; }), [['jpeg', 90], ['jpeg', 90]]);
  assert.equal(calls[1].optimizeForSpeed, undefined);
});

test('puts an explicitly configured Chrome ahead of platform defaults', function () {
  var candidates = chrome.chromeCandidates(
    { CHROME_PATH: '/chosen/chrome' },
    'darwin'
  );
  assert.equal(candidates[0], '/chosen/chrome');
  assert.ok(candidates.some(function (file) { return file.indexOf('Google Chrome.app') > -1; }));
});

test('finds an explicit executable and rejects a missing one', function (t) {
  var dir = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-chrome-test-'));
  var executable = path.join(dir, 'chrome');
  fs.writeFileSync(executable, '');
  t.after(function () { fs.rmSync(dir, { recursive: true, force: true }); });

  assert.equal(chrome.findChrome({ chromePath: executable }), executable);
  assert.equal(chrome.findChrome({ chromePath: path.join(dir, 'missing') }), null);
});

function fakeChild() {
  var child = new EventEmitter();
  child.stdio = [null, null, new stream.PassThrough(), new stream.PassThrough(), new stream.PassThrough()];
  return child;
}

test('matches pipe replies to commands', async function () {
  var child = fakeChild();
  var pipe = new chrome.Pipe(child);
  var raw = '';
  child.stdio[3].setEncoding('utf8');
  child.stdio[3].on('data', function (chunk) { raw += chunk; });

  var answer = pipe.send('Page.enable');
  await new Promise(function (resolve) { setImmediate(resolve); });
  var sent = JSON.parse(raw.replace(/\0$/, ''));
  child.stdio[4].write(JSON.stringify({ id: sent.id, result: { enabled: true } }) + '\0');

  assert.deepEqual(await answer, { enabled: true });
});

test('keeps a watcher on every event until told to stop, and tells it when the pipe dies', async function () {
  var child = fakeChild();
  var pipe = new chrome.Pipe(child);
  var seen = [];
  var failed = null;
  var off = pipe.on('Page.screencastFrame', 'cast-session', function (params) {
    seen.push(params.sessionId);
  }, function (error) {
    failed = error;
  });

  function frame(session, n) {
    child.stdio[4].write(JSON.stringify({
      method: 'Page.screencastFrame', sessionId: session, params: { sessionId: n, data: 'x' },
    }) + '\0');
  }
  frame('cast-session', 1);
  frame('other-session', 2);
  frame('cast-session', 3);
  await new Promise(function (resolve) { setImmediate(resolve); });
  assert.deepEqual(seen, [1, 3]);

  off();
  frame('cast-session', 4);
  await new Promise(function (resolve) { setImmediate(resolve); });
  assert.deepEqual(seen, [1, 3]);

  var kept = pipe.on('Page.screencastFrame', 'cast-session', function () {}, function (error) {
    failed = error;
  });
  child.emit('exit', 0, null);
  assert.match(String(failed && failed.message), /Chromium exited/);
  kept();
});

test('a command can be given its own, shorter deadline', async function () {
  var child = fakeChild();
  var pipe = new chrome.Pipe(child);
  var started = Date.now();
  await assert.rejects(pipe.send('Input.dispatchKeyEvent', {}, 'session', { timeout: 30 }), /timed out during Input\.dispatchKeyEvent/);
  assert.ok(Date.now() - started < 1000);
});

/* A browser that answers every command with an empty result, so launching
   and creating targets can be walked through without Chrome. */
function fakeSpawn(record) {
  return function (executable, args) {
    var child = fakeChild();
    child.exitCode = null;
    child.killed = false;
    child.kill = function () {
      child.killed = true;
      child.exitCode = 0;
      child.emit('exit', 0, null);
    };
    child.stderr = new stream.PassThrough();
    record.push({ executable: executable, args: args, child: child });
    child.stdio[3].setEncoding('utf8');
    var buffer = '';
    child.stdio[3].on('data', function (chunk) {
      buffer += chunk;
      var at = buffer.indexOf('\0');
      while (at > -1) {
        var message = JSON.parse(buffer.slice(0, at));
        buffer = buffer.slice(at + 1);
        var result = {};
        if (message.method === 'Target.createTarget') result = { targetId: 'target-1' };
        if (message.method === 'Target.attachToTarget') result = { sessionId: 'session-1' };
        child.stdio[4].write(JSON.stringify({ id: message.id, result: result }) + '\0');
        at = buffer.indexOf('\0');
      }
    });
    return child;
  };
}

test('a browser keeps a profile it was given and removes one it made', async function (t) {
  var dir = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-browser-test-'));
  var executable = path.join(dir, 'chrome');
  fs.writeFileSync(executable, '');
  var profile = path.join(dir, 'profile');
  t.after(function () { fs.rmSync(dir, { recursive: true, force: true }); });

  var spawned = [];
  var kept = new chrome.Browser({ chromePath: executable, profile: profile, spawn: fakeSpawn(spawned) });
  await kept.launch();
  assert.ok(fs.existsSync(profile));
  assert.ok(spawned[0].args.indexOf('--user-data-dir=' + profile) > -1);
  assert.ok(spawned[0].args.indexOf('--headless=new') > -1);
  var target = await kept.createTarget('about:blank');
  assert.equal(target.sessionId, 'session-1');
  await kept.close();
  assert.ok(fs.existsSync(profile), 'a given profile is left alone');

  var headed = new chrome.Browser({ chromePath: executable, headless: false, spawn: fakeSpawn(spawned) });
  await headed.launch();
  var temp = headed.profile;
  assert.ok(fs.existsSync(temp));
  assert.equal(spawned[1].args.indexOf('--headless=new'), -1);
  await headed.close();
  assert.ok(!fs.existsSync(temp), 'a made profile is removed');
});

test('waits for a session event without consuming another session', async function () {
  var child = fakeChild();
  var pipe = new chrome.Pipe(child);
  var loaded = pipe.waitFor('Page.loadEventFired', 'capture-session');

  child.stdio[4].write(JSON.stringify({
    method: 'Page.loadEventFired', sessionId: 'other-session', params: { timestamp: 1 },
  }) + '\0');
  child.stdio[4].write(JSON.stringify({
    method: 'Page.loadEventFired', sessionId: 'capture-session', params: { timestamp: 2 },
  }) + '\0');

  assert.deepEqual(await loaded, { timestamp: 2 });
});
