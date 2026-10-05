var assert = require('node:assert/strict');
var test = require('node:test');
var sync = require('./capture-sync');
async function until(predicate) {
  var deadline = Date.now() + 2000;
  while (!predicate()) {
    if (Date.now() > deadline) throw new Error('Preparation did not settle');
    await new Promise(function (resolve) { setTimeout(resolve, 5); });
  }
}

test('prepares only the latest view after rapid changes during a pending prepare', async function (t) {
  var view = 'first'; var calls = []; var release;
  var service = sync.create({ read: function () { return view; }, prepare: function (request) {
    calls.push(request);
    if (calls.length === 1) return new Promise(function (resolve) { release = resolve; });
  } });
  t.after(service.stop);
  service.schedule(0); await until(function () { return release; });
  view = 'second'; service.schedule(0);
  view = 'third'; service.schedule(0);
  assert.deepEqual(calls, ['first']);
  release(); await until(function () { return calls.length === 2; });
  assert.deepEqual(calls, ['first', 'third']);
});

test('a heartbeat recovers from a failed prepare without a preview change', async function (t) {
  var calls = 0; var failures = 0; var ready = false;
  var service = sync.create({ heartbeat: 100, read: function () { return 'same view'; },
    prepare: function () { if (++calls === 1) throw new Error('Renderer restarting'); },
    failed: function () { failures++; }, ready: function () { ready = true; },
  });
  t.after(service.stop);
  service.schedule(0);
  await until(function () { return ready; });
  assert.equal(failures, 1); assert.equal(calls, 2);
});

test('a heartbeat does not cut a failure backoff short, but a change does', async function (t) {
  var calls = 0;
  var service = sync.create({ heartbeat: 20, read: function () { return 'view'; },
    prepare: function () { calls++; throw new Error('Renderer restarting'); },
  });
  t.after(service.stop);
  service.schedule(0);
  await until(function () { return calls === 1; });
  await new Promise(function (resolve) { setTimeout(resolve, 150); });
  assert.equal(calls, 1);
  service.schedule(0);
  await until(function () { return calls === 2; });
});

test('stopping during preparation prevents queued changes and heartbeat work', async function () {
  var release; var calls = 0;
  var service = sync.create({ read: function () { return 'view'; }, prepare: function () {
    calls++; return new Promise(function (resolve) { release = resolve; });
  } });
  service.schedule(0); await until(function () { return release; });
  service.schedule(0); service.stop(); release();
  await new Promise(function (resolve) { setTimeout(resolve, 20); });
  assert.equal(calls, 1);
});

test('continuous changes cannot starve background preparation', async function (t) {
  var calls = 0;
  var service = sync.create({ read: function () { return 'animated view'; }, prepare: function () { calls++; } });
  var updates = setInterval(service.schedule, 10);
  t.after(function () { clearInterval(updates); service.stop(); });
  await until(function () { return calls >= 2; });
});

test('snapshot failures are reported and recover on change; an absent preview is not ready', async function (t) {
  var value = null; var errors = []; var ready = 0;
  var service = sync.create({ read: function () {
    if (value === 'broken') throw new Error('Snapshot unavailable');
    return value;
  }, prepare: function () {}, failed: function (error) { errors.push(error.message); }, ready: function () { ready++; } });
  t.after(service.stop);
  service.schedule(0); await new Promise(function (resolve) { setTimeout(resolve, 20); });
  assert.equal(ready, 0);
  value = 'broken'; service.schedule(0); await until(function () { return errors.length; });
  assert.deepEqual(errors, ['Snapshot unavailable']);
  value = 'valid'; service.schedule(0); await until(function () { return ready; });
  assert.equal(ready, 1);
});

test('captures pause speculative reads and resume with the latest state after all captures finish', async function (t) {
  var calls = []; var value = 'before'; var finish;
  var service = sync.create({ heartbeat: 10, read: function () { calls.push(value); return value; },
    prepare: function () { if (calls.length === 1) return new Promise(function (resolve) { finish = resolve; }); },
  });
  t.after(service.stop);
  service.schedule(0); await until(function () { return finish; });
  service.pause(); service.pause(); value = 'latest'; service.schedule(0); finish();
  await new Promise(function (resolve) { setTimeout(resolve, 100); });
  assert.deepEqual(calls, ['before']);
  service.resume(); await new Promise(function (resolve) { setTimeout(resolve, 100); });
  assert.deepEqual(calls, ['before']);
  service.resume(); await until(function () { return calls.length > 1; });
  assert.equal(calls[1], 'latest');
});
