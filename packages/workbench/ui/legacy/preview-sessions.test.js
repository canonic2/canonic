var test = require('node:test');
var assert = require('node:assert/strict');
var create = require('./preview-sessions').create;
function fixture() {
  var now = 0; var next = 0; var timers = new Map(); var disposed = [];
  var pool = create({ ttl: 100, limit: 3, now: function () { return now; },
    setTimeout: function (fn, delay) { timers.set(++next, { fn: fn, due: now + delay }); return next; },
    clearTimeout: function (id) { timers.delete(id); }, dispose: function (frame) { disposed.push(frame); } });
  return { pool: pool, disposed: disposed,
    visit: function (key) { var frame = { key: key }; pool.add(key, frame); pool.activate(frame); return frame; },
    advance: function (ms) { now += ms; Array.from(timers).forEach(function (entry) { if (entry[1].due <= now) { timers.delete(entry[0]); entry[1].fn(); } }); },
    timers: timers };
}
test('retains three inactive sessions and evicts the least recently used without evicting active or loading frames', function () {
  var f = fixture(); var a = f.visit('a'); var b = f.visit('b'); f.visit('c'); f.visit('d');
  f.pool.activate(a);
  var loading = {}; f.pool.add('loading', loading);
  assert.equal(f.disposed.length, 0);
  f.pool.activate(loading);
  assert.deepEqual(f.disposed, [b]);
  assert.equal(f.pool.get('a').frame, a);
  assert.equal(f.pool.get('loading').frame, loading);
});
test('idle expiry runs without another navigation and never expires the active session', function () {
  var f = fixture(); var a = f.visit('a'); var b = f.visit('b');
  f.advance(99); assert.equal(f.pool.get('a').frame, a);
  f.advance(1); assert.deepEqual(f.disposed, [a]);
  assert.equal(f.pool.get('b').frame, b);
  f.pool.park(); f.advance(100); assert.deepEqual(f.disposed, [a, b]);
});
test('restoring a session protects it through loading and restarts its idle period when parked again', function () {
  var f = fixture(); var a = f.visit('a'); f.visit('b');
  f.advance(90); f.pool.protect(a, true); f.advance(20);
  assert.equal(f.pool.get('a').frame, a);
  f.pool.activate(a); f.visit('c'); f.advance(99);
  assert.equal(f.pool.get('a').frame, a);
  f.advance(1); assert.equal(f.pool.get('a'), null);
});
test('reload can forget an active session without tearing down its visible document; close releases all retained sessions and timers', function () {
  var f = fixture(); var a = f.visit('a'); f.pool.forget(a);
  assert.equal(f.disposed.length, 0);
  var fresh = f.visit('a'); f.visit('b'); f.pool.close();
  assert.ok(f.disposed.includes(fresh)); assert.equal(f.disposed.length, 2); assert.equal(f.timers.size, 0);
});

test('idle candidates are ordered by departure, exclude protected frames, and expire before reuse', function () {
  var f = fixture(); var a = f.visit('a'); var b = f.visit('b'); var c = f.visit('c');
  var loading = {}; f.pool.add('loading', loading);
  assert.deepEqual(f.pool.idleFrames(), [a, b]);
  f.pool.protect(a, true);
  assert.deepEqual(f.pool.idleFrames(), [b]);
  f.pool.activate(a);
  assert.deepEqual(f.pool.idleFrames(), [b, c]);
  f.pool.forget(b);
  f.pool.add('replacement', b);
  assert.equal(f.pool.get('b'), null);
  assert.deepEqual(f.pool.idleFrames(), [c]);
  f.advance(100);
  assert.deepEqual(f.pool.idleFrames(), []);
  assert.equal(f.pool.get('replacement').frame, b, 'loading replacement is protected');
  f.pool.close();
});
