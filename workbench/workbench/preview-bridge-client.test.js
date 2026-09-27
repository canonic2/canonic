var assert = require('node:assert/strict');
var test = require('node:test');
var fs = require('node:fs');
var path = require('node:path');
var vm = require('node:vm');

function client() {
  var listeners = {};
  var sent = [];
  var child = { postMessage: function (message, origin) { sent.push([message, origin]); } };
  var frame = { src: 'http://localhost:6006/iframe.html?id=button--default', contentWindow: child, dataset: {} };
  var root = {
    location: { href: 'http://127.0.0.1:3579/_workbench/', origin: 'http://127.0.0.1:3579' },
    addEventListener: function (name, fn) { listeners[name] = fn; },
    removeEventListener: function (name) { delete listeners[name]; },
  };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'preview-bridge-client.js'), 'utf8'), {
    window: root, URL: URL, setTimeout: setTimeout, clearTimeout: clearTimeout,
  });
  return { root: root, frame: frame, child: child, listeners: listeners, sent: sent };
}

test('accepts snapshots only from the active preview and its exact origin', function () {
  var c = client(); var changes = 0; var scrolls = [];
  var bridge = c.root.wbPreviewBridge.create(c.frame, function () { changes++; }, function (value) { scrolls.push(value); });
  assert.equal(c.sent[0][0].type, 'canonic:preview:connect');
  assert.equal(c.sent[0][1], 'http://localhost:6006');

  var snapshot = { revision: 'preview-1' };
  c.listeners.message({ source: {}, origin: 'http://localhost:6006', data: {
    type: 'canonic:preview:snapshot', version: 1, snapshot: snapshot, scroll: { x: 2, y: 90 },
  } });
  c.listeners.message({ source: c.child, origin: 'http://evil.test', data: {
    type: 'canonic:preview:snapshot', version: 1, snapshot: snapshot, scroll: { x: 2, y: 90 },
  } });
  assert.equal(bridge.read(), undefined);

  c.listeners.message({ source: c.child, origin: 'http://localhost:6006', data: {
    type: 'canonic:preview:snapshot', version: 1, snapshot: snapshot, scroll: { x: 2, y: 90 },
  } });
  assert.equal(bridge.read(), snapshot);
  assert.equal(c.frame.dataset.canonicPreviewBridge, 'ready');
  assert.equal(JSON.stringify(scrolls), JSON.stringify([{ x: 2, y: 90 }]));
  assert.equal(changes, 1);
});

test('reconnects after a ready signal and disconnects cleanly', function () {
  var c = client();
  var bridge = c.root.wbPreviewBridge.create(c.frame);
  c.listeners.message({ source: c.child, origin: 'http://localhost:6006', data: {
    type: 'canonic:preview:ready', version: 1,
  } });
  assert.deepEqual(c.sent.map(function (entry) { return entry[0].type; }), [
    'canonic:preview:connect', 'canonic:preview:connect',
  ]);
  bridge.stop();
  assert.equal(c.sent.at(-1)[0].type, 'canonic:preview:disconnect');
  assert.equal(c.frame.dataset.canonicPreviewBridge, undefined);
  assert.equal(c.listeners.message, undefined);
});

test('capture flush leaves an unbridged preview to URL capture', async function () {
  var c = client();
  var bridge = c.root.wbPreviewBridge.create(c.frame);
  await bridge.flush();
  assert.equal(bridge.read(), undefined);
  assert.deepEqual(c.sent.map(function (entry) { return entry[0].type; }), [
    'canonic:preview:connect',
  ]);
});

test('capture flush waits for the matching fresh snapshot after the bridge connects', async function () {
  var c = client();
  var bridge = c.root.wbPreviewBridge.create(c.frame);
  c.listeners.message({ source: c.child, origin: 'http://localhost:6006', data: {
    type: 'canonic:preview:snapshot', version: 1,
    snapshot: { revision: 'initial-1' }, scroll: { x: 0, y: 0 },
  } });
  var flushed = bridge.flush();
  var request = c.sent.at(-1)[0];
  assert.equal(request.type, 'canonic:preview:flush');
  c.listeners.message({ source: c.child, origin: 'http://localhost:6006', data: {
    type: 'canonic:preview:snapshot', version: 1, requestId: request.requestId,
    snapshot: { revision: 'fresh-2' }, scroll: { x: 0, y: 400 },
  } });
  await flushed;
  assert.equal(bridge.read().revision, 'fresh-2');
});
