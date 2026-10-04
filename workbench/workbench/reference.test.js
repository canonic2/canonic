var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function load(options) {
  options = options || {};
  var copied = [];
  var messages = [];
  var click;
  var input;
  var removed = false;
  var focused = false;
  var document = {
    activeElement: { focus: function () { focused = true; } },
    body: { appendChild: function (node) { input = node; } },
    createElement: function () {
      return { style: {}, setAttribute: function () {}, select: function () {}, remove: function () { removed = true; } };
    },
    execCommand: function (command) {
      assert.equal(command, 'copy');
      if (options.fallbackFails) return false;
      copied.push(input.value);
      return true;
    },
    getElementById: function (id) {
      assert.equal(id, 'copyReference');
      return { addEventListener: function (event, fn) { assert.equal(event, 'click'); click = fn; } };
    },
  };
  var window = {
    wbView: function () { return options.view; },
    dispatchEvent: function (event) { messages.push(event.detail.message); },
  };
  var navigator = options.noClipboard ? {} : { clipboard: {
    writeText: function (value) {
      if (options.clipboardDenied) return Promise.reject(new Error('Denied'));
      copied.push(value);
      return Promise.resolve();
    },
  } };
  var file = path.join(__dirname, 'reference.js');
  vm.runInNewContext(fs.readFileSync(file, 'utf8'), {
    window: window, document: document, navigator: navigator,
    CustomEvent: function (type, init) { this.type = type; this.detail = init.detail; },
  }, { filename: file });
  return {
    text: window.wbReference.text,
    copied: copied, messages: messages,
    cleaned: function () { return removed && focused; },
    click: async function () { click(); await new Promise(function (resolve) { setImmediate(resolve); }); },
  };
}

function design() {
  return {
    src: 'pages/sign-in.html', item: { label: 'Sign in' },
    state: null, lens: null, story: null, url: 'http://localhost:3579/pages/sign-in.html',
  };
}

test('copies a design reference with an explicit default, without capture or handoff details', function () {
  var view = design();
  view.code = ['/example/source.js'];
  assert.equal(load().text(view), [
    '- Page: Sign in — `pages/sign-in.html`',
    '- State: Default — `default`',
    '- Lens: Design',
  ].join('\n'));
});

test('uses the declared default and selected state names and ids', function () {
  var view = design();
  view.item.states = [{ id: 'empty', label: 'Empty form' }, { id: 'error', label: 'Wrong password' }];
  var format = load().text;
  assert.match(format(view), /State: Empty form — `empty`/);
  view.state = 'error';
  assert.match(format(view), /State: Wrong password — `error`/);
});

test('names the resolved story and lens instead of a design state', function () {
  var view = design();
  view.lens = { key: 'storybook', label: 'Storybook', kind: 'storybook' };
  view.story = { name: 'Icon Only', id: 'button--icon-only' };
  view.url = 'http://localhost:6006/iframe.html?id=button--icon-only&viewMode=story';
  var result = load().text(view);
  assert.match(result, /Story: Icon Only — `button--icon-only`/);
  assert.match(result, /Lens: Storybook — `storybook` — `http:\/\/localhost:6006\/iframe.html\?id=button--icon-only&viewMode=story`/);
  assert.doesNotMatch(result, /State:/);
});

test('reports mapped URL states and does not carry design states into an unmapped lens', function () {
  var view = design();
  view.lens = { key: 'dev', label: 'Development', kind: 'url' };
  view.state = 'error';
  view.item.states = [{ id: 'empty', label: 'Empty form' }, { id: 'error', label: 'Wrong password' }];
  view.item.implementations = { dev: { states: { empty: '/', error: '/?error=1' } } };
  view.url = 'http://localhost:3000/?error=1';
  var format = load().text;
  assert.match(format(view), /State: Wrong password — `error`/);
  assert.match(format(view), /Lens: Development — `dev` — `http:\/\/localhost:3000\/\?error=1`/);
  view.item.implementations.dev = { path: '/' };
  view.url = 'http://localhost:3000/';
  assert.match(format(view), /State: Default — `default`/);
});

test('does not copy a missing or unresolved selection', async function () {
  var f = load();
  await f.click();
  assert.deepEqual(f.copied, []);
  assert.deepEqual(f.messages, ['Pick a page or a component first.']);
  var view = design();
  view.url = null;
  assert.equal(f.text(view), null);
  view.url = 'http://localhost:6006/';
  view.lens = { kind: 'storybook' };
  assert.equal(f.text(view), null);
});

test('reads the selection when clicked and copies only its reference', async function () {
  var options = { view: design() };
  var f = load(options);
  options.view = design();
  options.view.item.label = 'Updated page';
  await f.click();
  assert.deepEqual(f.copied, [f.text(options.view)]);
  assert.match(f.copied[0], /Page: Updated page/);
  assert.deepEqual(f.messages, ['Copied reference']);
});

test('falls back when the Clipboard API is unavailable or denied and restores focus', async function () {
  for (var options of [{ noClipboard: true }, { clipboardDenied: true }]) {
    options.view = design();
    var f = load(options);
    await f.click();
    assert.deepEqual(f.copied, [f.text(options.view)]);
    assert.deepEqual(f.messages, ['Copied reference']);
    assert.equal(f.cleaned(), true);
  }
});

test('reports clipboard failure and cleans up the temporary selection', async function () {
  var f = load({ view: design(), clipboardDenied: true, fallbackFails: true });
  await f.click();
  assert.deepEqual(f.copied, []);
  assert.deepEqual(f.messages, ['Couldn’t copy the reference.']);
  assert.equal(f.cleaned(), true);
});
