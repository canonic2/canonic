var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function element(extra) {
  return Object.assign({
    hidden: true,
    disabled: false,
    textContent: '',
    classList: { add: function () {}, remove: function () {} },
    addEventListener: function () {},
    setAttribute: function (name, value) { this[name] = value; },
  }, extra || {});
}

/* simulator.js in a stub page, with fetch answering a JPEG stream whose
   frames the test hands over through `deliver`. */
function surface(answers) {
  var page = { requests: [], sockets: [], draws: [], deliver: null };
  var canvas = element({
    width: 0,
    height: 0,
    getContext: function () {
      return { drawImage: function () { page.draws.push(Array.from(arguments)); } };
    },
  });
  page.nodes = {
    simulatorCanvas: canvas,
    simulatorPrompt: element(),
    shareSimulator: element(),
    simulatorStatus: element(),
  };
  page.window = { parent: {} };

  function fetch(url, options) {
    if (!options.body) {
      page.requests.push([url, null]);
      return Promise.resolve({
        ok: true,
        status: 200,
        body: { getReader: function () {
          return {
            read: function () { return new Promise(function (resolve) { page.deliver = resolve; }); },
            cancel: function () { return Promise.resolve(); },
          };
        } },
      });
    }
    page.requests.push([url, JSON.parse(options.body)]);
    var answer = url.endsWith('/stream') ? answers.stream : answers.other;
    return Promise.resolve({ ok: true, json: function () { return Promise.resolve(answer); } });
  }

  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'simulator.js'), 'utf8'), {
    Blob: function Blob(parts) { this.parts = parts; },
    console: console,
    createImageBitmap: function () {
      return Promise.resolve({ width: 640, height: 480, close: function () {} });
    },
    document: { getElementById: function (id) { return page.nodes[id]; } },
    fetch: fetch,
    location: { protocol: 'http:', host: '127.0.0.1:3579' },
    performance: { now: function () { return 0; } },
    setTimeout: setTimeout,
    clearTimeout: clearTimeout,
    Uint8Array: Uint8Array,
    DataView: DataView,
    WebSocket: function WebSocket(url) { page.sockets.push(url); },
    window: page.window,
  }, { filename: 'simulator.js' });
  return page;
}

function jpegRecord() {
  var packet = new Uint8Array(12);
  packet.set([0xff, 0xd8, 0xff], 9);
  var record = new Uint8Array(4 + packet.length);
  new DataView(record.buffer).setUint32(0, packet.length, false);
  record.set(packet, 4);
  return record;
}

test('a window lens streams the page’s declared window without starting WDA', async function () {
  var page = surface({ stream: { ok: true, codec: 'jpeg', stream: '/_workbench/window/stream?token=w' } });
  page.window.wbSimulator.show({ kind: 'window', implementation: 'emulator', src: 'pages/sign-in.html', label: 'Sign in' });
  await new Promise(function (resolve) { setImmediate(resolve); });

  assert.deepEqual(page.requests, [
    ['/_workbench/window/stream', { implementation: 'emulator', src: 'pages/sign-in.html', codec: 'jpeg' }],
    ['/_workbench/window/stream?token=w', null],
  ]);
  assert.equal(page.nodes.simulatorCanvas['aria-label'], 'Live window: Sign in');

  page.deliver({ done: false, value: jpegRecord() });
  await new Promise(function (resolve) { setImmediate(resolve); });
  assert.equal(page.draws.length, 1);
  assert.equal(page.nodes.simulatorPrompt.hidden, true);
  assert.equal(page.nodes.simulatorStatus.textContent, 'Live');
});

test('embedded workbench starts WDA and uses JPEG without WebCodecs', async function () {
  var requests = [];
  var sockets = [];
  var draws = [];
  var deliver = null;
  var canvas = element({
    width: 0,
    height: 0,
    getContext: function () {
      return { drawImage: function () { draws.push(Array.from(arguments)); } };
    },
  });
  var nodes = {
    simulatorCanvas: canvas,
    simulatorPrompt: element(),
    shareSimulator: element(),
    simulatorStatus: element(),
  };
  var window = { parent: {} };

  function fetch(url, options) {
    if (!options.body) {
      requests.push([url, null]);
      return Promise.resolve({
        ok: true,
        status: 200,
        body: { getReader: function () {
          return {
            read: function () { return new Promise(function (resolve) { deliver = resolve; }); },
            cancel: function () { return Promise.resolve(); },
          };
        } },
      });
    }
    var body = JSON.parse(options.body);
    requests.push([url, body]);
    var answer = url.endsWith('/stream')
      ? { ok: true, codec: 'jpeg', stream: '/_workbench/simulator/stream?token=x' }
      : { ok: true, screen: { width: 393, height: 852 } };
    return Promise.resolve({ ok: true, json: function () { return Promise.resolve(answer); } });
  }

  function WebSocket(url) {
    this.url = url;
    sockets.push(this);
  }

  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'simulator.js'), 'utf8'), {
    Blob: function Blob(parts) { this.parts = parts; },
    console: console,
    createImageBitmap: function () {
      return Promise.resolve({ width: 480, height: 1092, close: function () {} });
    },
    document: { getElementById: function (id) { return nodes[id]; } },
    fetch: fetch,
    location: { protocol: 'http:', host: '127.0.0.1:3579' },
    performance: { now: function () { return 0; } },
    setTimeout: setTimeout,
    clearTimeout: clearTimeout,
    Uint8Array: Uint8Array,
    DataView: DataView,
    WebSocket: WebSocket,
    window: window,
  }, { filename: 'simulator.js' });

  window.wbSimulator.show({ kind: 'ios-simulator', implementation: 'ios', udid: 'SIM-1', label: 'iPhone 17' });
  await new Promise(function (resolve) { setImmediate(resolve); });

  assert.deepEqual(requests, [
    ['/_workbench/simulator', { implementation: 'ios', udid: 'SIM-1' }],
    ['/_workbench/simulator/stream', { implementation: 'ios', udid: 'SIM-1', codec: 'jpeg' }],
    ['/_workbench/simulator/stream?token=x', null],
  ]);
  assert.equal(sockets.length, 0);

  var packet = new Uint8Array(12);
  packet.set([0xff, 0xd8, 0xff], 9);
  var record = new Uint8Array(4 + packet.length);
  new DataView(record.buffer).setUint32(0, packet.length, false);
  record.set(packet, 4);
  deliver({ done: false, value: record });
  await new Promise(function (resolve) { setImmediate(resolve); });

  assert.equal(canvas.width, 480);
  assert.equal(canvas.height, 1092);
  assert.equal(draws.length, 1);
  assert.equal(nodes.simulatorPrompt.hidden, true);
});
