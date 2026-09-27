var assert = require('node:assert/strict');
var test = require('node:test');
var fs = require('node:fs');
var path = require('node:path');
var vm = require('node:vm');

function captureUI() {
  var calls = [];
  var blob = { type: 'image/jpeg' };
  var state = {
    nativeAvailable: true,
    liveMirror: null,
    simulatorView: function () { return false; },
    captureSync: { paused: 0, pause: function () { this.paused++; }, resume: function () { this.paused--; } },
    offOrigin: function () { return false; },
    captureRequest: function () { return { url: '/page', format: 'jpeg', mirror: { revision: 'live-1' } }; },
    shotName: function () { return 'page.jpg'; }, say: function () {},
    traceCapture: function () {},
    marks: [], anchors: function () { return [{ x: 10, y: 20 }]; },
    image: function (response) { return response.blob(); },
    json: function (response) { return response.json(); },
    fetch: async function (url, options) {
      calls.push({ url: url, options: options });
      return { ok: true, json: async function () { return { ok: true, file: '.canonic/.handoffs/page.jpg', targets: ['h1'] }; },
        blob: async function () { return blob; } };
    },
  };
  var source = fs.readFileSync(path.join(__dirname, 'markup.js'), 'utf8');
  var transportStart = source.indexOf('  function mirrorFetch(');
  vm.runInNewContext(source.slice(transportStart, source.indexOf('  function json(', transportStart)), state);
  var start = source.indexOf('  function captured(');
  vm.runInNewContext(source.slice(start, source.indexOf('  function renderShot()', start)), state);
  start = source.indexOf('  function takeShot(');
  vm.runInNewContext(source.slice(start, source.indexOf('\n  /* Native capture', start)), state);
  state.location = { protocol: 'http:' };
  state.anchors = function () { return [{ x: 10, y: 20 }]; };
  return { state: state, calls: calls, blob: blob };
}

function captureRequestFor(view) {
  var state = {
    frame: {
      src: 'http://localhost:6006/iframe.html?id=button--primary&viewMode=story',
      getAttribute: function () { return this.src; },
      contentWindow: { location: {} },
    },
    frameWrap: { getBoundingClientRect: function () { return { width: 800, height: 600 }; } },
    viewNow: function () { return view; },
    scrollOf: function () { return { x: 0, y: 0 }; },
    captureMarkup: function () { return ''; },
    previewRevision: 'story-2',
    liveMirror: null,
  };
  var source = fs.readFileSync(path.join(__dirname, 'markup.js'), 'utf8');
  var start = source.indexOf('  function captureRequest(');
  vm.runInNewContext(source.slice(start, source.indexOf('  function mirrorFetch(', start)), state);
  return state.captureRequest(false, false);
}

test('browser diagnostics post bounded metadata to the central log route', async function () {
  var calls = [];
  var state = {
    fetch: function (url, options) {
      calls.push({ url: url, body: JSON.parse(options.body), keepalive: options.keepalive });
      return Promise.resolve({ ok: true });
    },
  };
  var source = fs.readFileSync(path.join(__dirname, 'markup.js'), 'utf8');
  var start = source.indexOf('  function diagnostic(');
  vm.runInNewContext(source.slice(start, source.indexOf('\n  function copyToastPath(', start)), state);
  state.diagnostic('error', 'handoff.browser.failed', { phase: 'send' });
  await Promise.resolve();
  assert.deepEqual(calls, [{
    url: '/_workbench/log',
    body: { level: 'error', event: 'handoff.browser.failed', details: { phase: 'send' } },
    keepalive: true,
  }]);
});

test('capture diagnostics include the sampled viewport and bridge state', function () {
  var entry;
  var state = { diagnostic: function (level, event, details) { entry = { level: level, event: event, details: details }; } };
  var source = fs.readFileSync(path.join(__dirname, 'markup.js'), 'utf8');
  var start = source.indexOf('  function traceCapture(');
  vm.runInNewContext(source.slice(start, source.indexOf('  function mirrorFetch(', start)), state);
  state.traceCapture('/_workbench/capture/page', {
    mirror: undefined, scroll: { x: 4, y: 280 }, width: 393, height: 852,
  });
  assert.equal(JSON.stringify(entry), JSON.stringify({
    level: 'info', event: 'capture.browser.requested', details: {
      route: '/_workbench/capture/page', mirrored: false,
      requestedX: 4, requestedY: 280, width: 393, height: 852,
    },
  }));
});

test('a persisted native capture finishes without reading back its saved image', async function () {
  var ui = captureUI();
  var result = await ui.state.nativeShot();
  assert.equal(result.file, '.canonic/.handoffs/page.jpg');
  assert.equal(result.blob, undefined);
  assert.equal(ui.calls.length, 1);
  assert.equal(ui.calls[0].url, '/_workbench/capture');
});

test('a missing patch base retries the exact requested snapshot and acknowledges only success', async function () {
  var ui = captureUI(); var calls = []; var acknowledgements = [];
  var snapshot = { revision: 'requested', base: 'missing' };
  var request = { mirror: snapshot, mirrorSource: {
    full: function (value) { assert.equal(value, snapshot); return { revision: value.revision, base: null }; },
    acknowledge: function (value) { acknowledgements.push(value); },
  } };
  ui.state.fetch = async function (_, options) {
    calls.push(JSON.parse(options.body));
    if (calls.length === 1) throw new Error('MIRROR_RESYNC: missing base revision');
    return { json: async function () { return { ok: true }; } };
  };
  await ui.state.mirrorFetch('/capture', request);
  assert.deepEqual(calls.map(function (call) { return call.mirror; }), [snapshot, { revision: 'requested', base: null }]);
  assert.deepEqual(acknowledgements, [snapshot]);
  ui.state.fetch = async function () { throw new Error('Renderer unavailable'); };
  await assert.rejects(ui.state.mirrorFetch('/capture', request), /unavailable/);
  assert.equal(acknowledgements.length, 1);
});

test('hosted capture flushes live state and never falls back to DOM rendering on failure', async function () {
  var ui = captureUI(); var flushed = false; var fallback = false;
  ui.state.captureRequest = function (_, flush) { flushed = flush; return { mirror: { revision: 'edited' } }; };
  ui.state.fetch = async function () { throw new Error('Renderer restarting'); };
  ui.state.legacyShot = function () { fallback = true; };
  await assert.rejects(ui.state.takeShot(), /Renderer restarting/);
  assert.equal(flushed, true); assert.equal(fallback, false);
  assert.equal(ui.state.captureSync.paused, 0);
  ui.state.captureRequest = function () { return {}; };
  await assert.rejects(ui.state.takeShot(), /still loading/);
  assert.equal(ui.state.captureSync.paused, 0);
});

test('capture pauses speculative work through saving, then resumes synchronization', async function () {
  var ui = captureUI(); var finish;
  ui.state.nativeShot = function () { return new Promise(function (resolve) { finish = resolve; }); };
  var shot = ui.state.takeShot();
  await Promise.resolve();
  assert.equal(ui.state.captureSync.paused, 1);
  finish({ file: '.canonic/.handoffs/ready.jpg' });
  assert.equal((await shot).file, '.canonic/.handoffs/ready.jpg');
  assert.equal(ui.state.captureSync.paused, 0);
});

test('handoff capture does not read its saved image back into the canvas', async function () {
  var ui = captureUI();
  var result = await ui.state.nativeShot(true);
  assert.equal(result.blob, undefined);
  assert.deepEqual(ui.calls.map(function (call) { return call.url; }), ['/_workbench/capture']);
});

test('implementation capture inspects marked elements only for handoff', async function () {
  var ui = captureUI();
  await ui.state.pageShot();
  assert.deepEqual(JSON.parse(ui.calls[0].options.body).anchors, []);
  assert.equal(ui.calls.length, 1);
  await ui.state.pageShot(true);
  assert.deepEqual(JSON.parse(ui.calls[1].options.body).anchors, [{ x: 10, y: 20 }]);
  assert.equal(ui.calls.length, 2);
});

test('handoff saves, snapshots its prompt, clears markup, then copies', async function () {
  var order = [];
  var state = {
    frameShell: { hidden: false },
    handoffButton: { disabled: false, setAttribute: function () {}, removeAttribute: function () {} },
    takeShot: async function () { order.push('save'); return { file: '.canonic/.handoffs/page.jpg', targets: ['h1'] }; },
    payload: function (file, targets) { order.push('prompt'); return { file: file, marks: targets }; },
    clearAll: function () { order.push('clear'); },
    fetch: async function (url, options) {
      order.push('copy');
      assert.equal(url, '/_workbench/handoff');
      assert.deepEqual(JSON.parse(options.body), { file: '.canonic/.handoffs/page.jpg', marks: ['h1'] });
      return { json: async function () { return { ok: true }; } };
    },
    say: function () {}, diagnostic: function () {}, console: console,
  };
  var source = fs.readFileSync(path.join(__dirname, 'markup.js'), 'utf8');
  var start = source.indexOf('  var handoffBusy = false;');
  var end = source.indexOf("\n  fetch('/_workbench/handoff')", start);
  vm.runInNewContext(source.slice(start, end), state);
  await state.handoff();
  assert.deepEqual(order, ['save', 'prompt', 'clear', 'copy']);
  assert.equal(state.handoffButton.disabled, false);
});

test('an in-place Storybook switch captures the selected story rather than the iframe’s initial URL', function () {
  var url = 'http://localhost:6006/iframe.html?id=button--secondary&viewMode=story';
  var request = captureRequestFor({ lens: { kind: 'storybook' }, url: url });
  assert.equal(request.url, url);
});

test('the camera returns native bytes without saving a local preview', async function () {
  var ui = captureUI();
  var result = await ui.state.downloadShot();
  assert.equal(result, ui.blob);
  assert.deepEqual(ui.calls.map(function (call) { return call.url; }), ['/_workbench/capture/image']);
  assert.equal(ui.state.captureSync.paused, 0);
});

test('the camera returns external preview bytes without saving or inspecting marks', async function () {
  var ui = captureUI();
  ui.state.offOrigin = function () { return true; };
  ui.state.captureRequest = function () { return { url: 'http://localhost:6006/iframe.html', format: 'jpeg' }; };
  var result = await ui.state.downloadShot();
  assert.equal(result, ui.blob);
  assert.equal(ui.calls[0].url, '/_workbench/capture/page/image');
  assert.deepEqual(JSON.parse(ui.calls[0].options.body).anchors, []);
});

test('an external capture flushes the bridge before reading its snapshot', async function () {
  var ui = captureUI(); var order = [];
  ui.state.offOrigin = function () { return true; };
  ui.state.liveMirror = { flush: async function () { order.push('flush'); } };
  ui.state.captureRequest = function () {
    order.push('read');
    return { url: 'http://localhost:6006/iframe.html', format: 'jpeg', mirror: { revision: 'fresh' } };
  };
  await ui.state.downloadShot();
  assert.deepEqual(order, ['flush', 'read']);
});

test('Simulator screenshots use the visible canvas renderer', async function () {
  var ui = captureUI();
  ui.state.simulatorView = function () { return true; };
  ui.state.renderShot = async function () { return ui.blob; };
  ui.state.legacyShot = async function () { return { blob: ui.blob, file: '.canonic/.handoffs/simulator.jpg' }; };
  assert.equal(await ui.state.downloadShot(), ui.blob);
  assert.deepEqual(await ui.state.takeShot(true), { blob: ui.blob, file: '.canonic/.handoffs/simulator.jpg' });
  assert.equal(ui.calls.length, 0);
});
