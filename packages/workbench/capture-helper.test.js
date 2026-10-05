var assert = require('node:assert/strict');
var test = require('node:test');
var fs = require('node:fs');
var path = require('node:path');
var vm = require('node:vm');

test('preparation primes readback once without resizing or encoding a discarded image', async function () {
  var reads = 0;
  var app = { setPath: function () {}, commandLine: { appendSwitch: function () {} },
    whenReady: function () { return { then: function () { return { catch: function () {} }; } }; } };
  var state = {
    require: function (name) { return name === 'electron' ? { app: app } : {}; },
    process: { platform: 'linux', env: { CANONIC_CAPTURE_PROFILE: '/temporary-profile' },
      stdout: { on: function () {} }, stdin: { on: function () {} } },
  };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'capture-helper/main.cjs'), 'utf8'), state);
  state.win = { webContents: { capturePage: async function () {
    reads++;
    // No resizing or encoding API: warm-up must only request pixels.
    return { isEmpty: function () { return false; } };
  } } };
  await state.prime('first');
  await state.prime('first');
  assert.equal(reads, 1);
  await state.prime('changed');
  assert.equal(reads, 2);
});

test('the Electron mirror restores the visible hover position and clears it when the pointer leaves', async function () {
  var app = { setPath: function () {}, whenReady: function () { return { then: function () { return { catch: function () {} }; } }; } };
  var state = { require: function (name) { return name === 'electron' ? { app: app } : {}; },
    process: { platform: 'linux', env: { CANONIC_CAPTURE_PROFILE: '/temporary-profile' },
      stdout: { on: function () {} }, stdin: { on: function () {} } } };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'capture-helper/main.cjs'), 'utf8'), state);
  var moved = [];
  state.win = { webContents: { sendInputEvent: function (event) { moved.push([event.x, event.y]); },
    executeJavaScript: async function () {} } };
  await state.restorePointer({ mirror: { pointer: { x: 45.2, y: 28.8 } } });
  state.width = 300; state.height = 200;
  await state.restorePointer({ mirror: { pointer: null } });
  assert.deepEqual(moved, [[45, 29], [301, 201]]);
});

test('switches stories inside an already loaded Storybook preview', async function () {
  var app = { setPath: function () {}, commandLine: { appendSwitch: function () {} },
    whenReady: function () { return { then: function () { return { catch: function () {} }; } }; } };
  var state = {
    require: function (name) { return name === 'electron' ? { app: app } : {}; },
    URL: URL,
    process: { platform: 'linux', env: { CANONIC_CAPTURE_PROFILE: '/temporary-profile' },
      stdout: { on: function () {} }, stdin: { on: function () {} } },
  };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'capture-helper/main.cjs'), 'utf8'), state);
  var scripts = [];
  state.loaded = 'http://localhost:6006/iframe.html?id=button--default&viewMode=story';
  state.win = { webContents: {
    executeJavaScript: async function (script) { scripts.push(script); },
  } };
  var next = 'http://localhost:6006/iframe.html?id=button--primary&viewMode=story';
  assert.equal(await state.navigatePage({ payload: { url: next, reuse: 'storybook' }, switchStory: 'switch-story()' }), true);
  assert.deepEqual(scripts, ['switch-story()']);
  assert.equal(state.loaded, next);
  assert.equal(await state.navigatePage({ payload: { url: next, reuse: 'storybook' }, switchStory: 'switch-story()' }), false);
  assert.deepEqual(scripts, ['switch-story()']);
});

test('interactive page preparation restores scroll after visible assets settle', async function () {
  var app = { setPath: function () {}, commandLine: { appendSwitch: function () {} },
    whenReady: function () { return { then: function () { return { catch: function () {} }; } }; } };
  var state = {
    require: function (name) { return name === 'electron' ? { app: app } : {}; }, URL: URL,
    process: { platform: 'linux', env: { CANONIC_CAPTURE_PROFILE: '/temporary-profile' },
      stdout: { on: function () {} }, stdin: { on: function () {} } },
  };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'capture-helper/main.cjs'), 'utf8'), state);
  var scrolls = [];
  var style = { getPropertyValue: function () { return ''; }, getPropertyPriority: function () { return ''; },
    setProperty: function () {}, removeProperty: function () {} };
  var page = {
    document: { documentElement: { style: style }, fonts: { ready: Promise.resolve() }, images: [{
      getBoundingClientRect: function () { return { width: 10, height: 10, left: 0, top: 0, right: 10, bottom: 10 }; },
      decode: async function () { page.window.scrollY = 0; },
    }] },
    window: { innerWidth: 960, innerHeight: 720, scrollX: 0, scrollY: 280,
      scrollTo: function (x, y) { this.scrollX = x; this.scrollY = y; scrolls.push([x, y]); } },
    Promise: Promise,
  };
  var url = 'http://localhost:6006/page';
  state.loaded = url;
  state.pageRevision = 'same';
  state.win = { setContentSize: function () {}, webContents: {
    executeJavaScript: function (script) { return vm.runInNewContext(script, page); },
  } };
  var details = await state.preparePage({ payload: {
    url: url, revision: 'same', width: 960, height: 720, scroll: { x: 0, y: 280 },
  } });
  assert.deepEqual(scrolls, [[0, 280], [0, 280]]);
  assert.equal(details.scroll.appliedY, 280);
});
