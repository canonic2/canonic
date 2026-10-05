var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function plain(value) {
  return JSON.parse(JSON.stringify(value));
}

function loadPanel(vscode) {
  var file = path.join(__dirname, 'panel.js');
  var module = { exports: {} };
  vm.runInNewContext(fs.readFileSync(file, 'utf8'), {
    module: module,
    exports: module.exports,
    require: function (name) {
      if (name === 'vscode') return vscode;
      throw new Error('unexpected require: ' + name);
    },
  }, { filename: file });
  return module.exports;
}

function fixture() {
  var received = null;
  var disposed = null;
  var messages = [];
  var body = '';
  var webview = {
    postMessage: function (message) {
      messages.push(message);
      return Promise.resolve(true);
    },
    onDidReceiveMessage: function (fn) {
      received = fn;
    },
  };
  Object.defineProperty(webview, 'html', {
    get: function () { return body; },
    set: function (value) { body = value; },
  });

  var view = {
    viewColumn: 1,
    webview: webview,
    reveal: function () {},
    onDidDispose: function (fn) { disposed = fn; },
  };
  var vscode = {
    env: {
      asExternalUri: function (uri) {
        return Promise.resolve({ toString: function () { return uri; } });
      },
    },
    Uri: {
      parse: function (value) { return value; },
      joinPath: function () { return 'icon.svg'; },
    },
    ViewColumn: { Active: 1 },
    window: {
      createWebviewPanel: function () { return view; },
    },
  };

  return {
    context: { extensionUri: 'extension', subscriptions: [] },
    dispose: function () { if (disposed) disposed(); },
    messages: messages,
    panel: loadPanel(vscode),
    receive: function (message) { received(message); },
    view: view,
  };
}

test('opens a loading tab immediately and reuses it when the server becomes ready', async function () {
  var f = fixture();
  var finish;
  var ready = new Promise(function (resolve) { finish = resolve; });
  var opening = f.panel.show(f.context, ready, 'pages/login.html');
  assert.match(f.view.webview.html, /Opening Workbench/);
  assert.doesNotMatch(f.view.webview.html, /<iframe/);
  var latest = f.panel.show(f.context, ready, 'pages/new.html');
  finish('http://127.0.0.1:3579/_workbench/');
  assert.equal(await opening, f.view);
  assert.equal(await latest, f.view);
  assert.match(f.view.webview.html, /#pages\/new.html/);
  f.dispose();
});

test('closing the loading tab prevents delayed startup from reopening it', async function () {
  var f = fixture();
  var finish;
  var opening = f.panel.show(f.context, new Promise(function (resolve) { finish = resolve; }));
  f.dispose();
  finish('http://127.0.0.1:3579/_workbench/');
  assert.equal(await opening, null);
  assert.match(f.view.webview.html, /Opening Workbench/);
});

test('failed startup replaces the loading state with recovery instructions', async function () {
  var f = fixture();
  assert.equal(await f.panel.show(f.context, Promise.resolve(null)), null);
  assert.match(f.view.webview.html, /Workbench couldn’t start/);
  assert.match(f.view.webview.html, /Workbench log/);
  f.dispose();
});

test('keeps sidebar navigation ready when wb-here precedes the iframe load event', async function () {
  var f = fixture();
  var url = 'http://127.0.0.1:3579/_workbench/';

  await f.panel.show(f.context, url, 'pages/login.html');

  var match = /<script nonce="[^"]+">([\s\S]*?)<\/script>/.exec(f.view.webview.html);
  assert.ok(match, 'generated panel contains its bridge script');

  var windowMessage = null;
  var frameLoad = null;
  var forwarded = [];
  var editorMessages = [];
  var dispatched = [];
  var frame = {
    contentWindow: {
      postMessage: function (message) { forwarded.push(message); },
    },
    addEventListener: function (type, fn) {
      if (type === 'load') frameLoad = fn;
    },
    src: '',
  };

  vm.runInNewContext(match[1], {
    acquireVsCodeApi: function () {
      return { postMessage: function (message) { editorMessages.push(message); } };
    },
    document: { getElementById: function () { return frame; }, addEventListener: function () {} },
    window: {
      addEventListener: function (type, fn) {
        if (type === 'message') windowMessage = fn;
      },
      dispatchEvent: function (event) { dispatched.push(event); },
    },
    KeyboardEvent: function (type, init) {
      this.type = type;
      Object.assign(this, init);
    },
  });

  assert.equal(frame.src, url + '#pages/login.html');
  assert.equal(typeof windowMessage, 'function');
  assert.equal(frameLoad, null, 'load must not clear a readiness signal that arrived first');

  /* A fast child can post from its own scripts before its load event reaches
     the parent wrapper. This was the order that permanently stalled picks. */
  windowMessage({
    source: frame.contentWindow,
    data: { type: 'wb-here', src: 'pages/login.html', state: null, pending: true },
  });
  windowMessage({ data: { type: 'canonic-go', hash: 'pages/login.html:welcome-dark' } });

  assert.deepEqual(plain(editorMessages), [], 'a pending story does not acknowledge the selection');
  assert.deepEqual(plain(forwarded), [
    { type: 'wb-go', hash: 'pages/login.html:welcome-dark' },
  ]);
  assert.deepEqual(dispatched, []);

  windowMessage({ source: frame.contentWindow,
    data: { type: 'wb-here', src: 'pages/login.html', state: null } });
  assert.deepEqual(plain(editorMessages), [
    { type: 'wb-here', src: 'pages/login.html', state: null },
  ]);

  /* A stale sidebar can name a page the already-open workbench has not read
     yet. Its null acknowledgement rejects the pick; it must not turn into an
     immediate reject/retry loop that starves the whole editor webview. */
  windowMessage({
    source: frame.contentWindow,
    data: { type: 'wb-here', src: null, state: null },
  });
  assert.deepEqual(plain(forwarded), [
    { type: 'wb-go', hash: 'pages/login.html:welcome-dark' },
  ]);
  assert.deepEqual(plain(editorMessages), [
    { type: 'wb-here', src: 'pages/login.html', state: null },
    { type: 'wb-here', src: null, state: null },
  ]);

  windowMessage({ data: { type: 'canonic-go', hash: 'preview/components-code-input.html' } });
  assert.deepEqual(plain(forwarded), [
    { type: 'wb-go', hash: 'pages/login.html:welcome-dark' },
    { type: 'wb-go', hash: 'preview/components-code-input.html' },
  ], 'the next selection is forwarded immediately after a rejection');

  f.dispose();
});

test('relays a nested workbench shortcut through the native webview dispatcher', async function () {
  var f = fixture();
  var url = 'http://127.0.0.1:3579/_workbench/';
  await f.panel.show(f.context, url, 'pages/login.html');

  var match = /<script nonce="[^"]+">([\s\S]*?)<\/script>/.exec(f.view.webview.html);
  var windowMessage = null;
  var dispatched = [];
  var frame = {
    contentWindow: { postMessage: function () {} },
    addEventListener: function () {},
    src: '',
  };

  function KeyboardEvent(type, init) {
    this.type = type;
    Object.assign(this, init);
  }

  vm.runInNewContext(match[1], {
    acquireVsCodeApi: function () { return { postMessage: function () {} }; },
    document: { getElementById: function () { return frame; }, addEventListener: function () {} },
    KeyboardEvent: KeyboardEvent,
    window: {
      addEventListener: function (type, fn) {
        if (type === 'message') windowMessage = fn;
      },
      dispatchEvent: function (event) { dispatched.push(event); },
    },
  });

  windowMessage({
    source: frame.contentWindow,
    data: {
      type: 'wb-keyboard',
      eventType: 'keydown',
      event: {
        key: 'P', code: 'KeyP', keyCode: 80,
        metaKey: true, shiftKey: true,
      },
    },
  });

  assert.equal(dispatched.length, 1);
  assert.equal(dispatched[0].type, 'keydown');
  assert.equal(dispatched[0].key, 'P');
  assert.equal(dispatched[0].code, 'KeyP');
  assert.equal(dispatched[0].keyCode, 80);
  assert.equal(dispatched[0].which, 80);
  assert.equal(dispatched[0].metaKey, true);
  assert.equal(dispatched[0].shiftKey, true);

  windowMessage({
    source: {},
    data: { type: 'wb-keyboard', eventType: 'keydown', event: { key: 'P' } },
  });
  assert.equal(dispatched.length, 1, 'messages from outside the workbench frame are ignored');

  f.dispose();
});

test('drops select-all at the webview and clears any selection that lands on it', async function () {
  var f = fixture();
  await f.panel.show(f.context, 'http://127.0.0.1:3579/_workbench/', 'pages/login.html');
  var html = f.view.webview.html;
  var style = /<style>([\s\S]*?)<\/style>/.exec(html)[1];
  assert.match(style, /html, body \{[^}]*user-select: none/);
  assert.match(style, /iframe \{[^}]*user-select: none/);

  var script = /<script nonce="[^"]+">([\s\S]*?)<\/script>/.exec(html)[1];
  var windowListeners = {};
  var documentListeners = {};
  var cleared = 0;
  var selection = { rangeCount: 1, isCollapsed: false, removeAllRanges: function () { cleared += 1; } };
  vm.runInNewContext(script, {
    acquireVsCodeApi: function () { return { postMessage: function () {} }; },
    document: {
      getElementById: function () { return { contentWindow: {} }; },
      addEventListener: function (type, fn) { documentListeners[type] = fn; },
      getSelection: function () { return selection; },
    },
    window: {
      addEventListener: function (type, fn, capture) { windowListeners[type] = { fn: fn, capture: capture }; },
    },
  });

  function key(overrides) {
    return Object.assign({
      key: 'a', metaKey: true, ctrlKey: false, shiftKey: false, altKey: false,
      prevented: false, stopped: false,
      preventDefault: function () { this.prevented = true; },
      stopImmediatePropagation: function () { this.stopped = true; },
    }, overrides);
  }
  assert.equal(windowListeners.keydown.capture, true);
  var selectAll = key();
  windowListeners.keydown.fn(selectAll);
  assert.equal(selectAll.prevented, true);
  assert.equal(selectAll.stopped, true, 'VS Code never sees a select-all aimed at the wrapper');
  var chord = key({ shiftKey: true });
  windowListeners.keydown.fn(chord);
  assert.equal(chord.prevented, false, 'a relayed ⇧⌘A still reaches VS Code');
  var typed = key({ metaKey: false });
  windowListeners.keydown.fn(typed);
  assert.equal(typed.prevented, false);

  documentListeners.selectionchange();
  assert.equal(cleared, 1);
  selection.isCollapsed = true;
  documentListeners.selectionchange();
  assert.equal(cleared, 1, 'a collapsed selection is left alone');
  f.dispose();
});

test('shows the preview menu at the translated position and copies its selection once', async function () {
  var f = fixture();
  await f.panel.show(f.context, 'http://127.0.0.1:3579/_workbench/', 'pages/login.html');
  var script = /<script nonce="[^"]+">([\s\S]*?)<\/script>/.exec(f.view.webview.html)[1];
  var message;
  var listeners = {};
  var menus = [];
  var forwarded = [];
  var frame = {
    contentWindow: { postMessage: function (packet) { forwarded.push(packet); } },
    getBoundingClientRect: function () { return { left: 10, top: 20 }; },
    dispatchEvent: function (event) { menus.push(event); },
  };
  vm.runInNewContext(script, {
    acquireVsCodeApi: function () { return { postMessage: function () {} }; },
    document: { getElementById: function () { return frame; }, addEventListener: function (type, fn) { listeners[type] = fn; } },
    window: { addEventListener: function (type, fn) { message = fn; } },
    MouseEvent: function (type, init) { this.type = type; Object.assign(this, init); },
  });
  var context = { type: 'wb-context-menu', x: 30, y: 40, selection: 'Preview selection', editable: true };
  message({ source: {}, data: context });
  assert.equal(menus.length, 0);
  message({ source: frame.contentWindow, data: context });
  assert.equal(menus[0].type, 'contextmenu');
  assert.equal(menus[0].clientX, 40);
  assert.equal(menus[0].clientY, 60);
  assert.equal(menus[0].bubbles, true);
  var clipboard = [];
  var prevented = 0;
  var e = { clipboardData: {
    setData: function (type, value) { clipboard.push([type, value]); },
    getData: function () { return 'Pasted text'; },
  }, preventDefault: function () { prevented += 1; } };
  listeners.copy(e);
  listeners.copy(e);
  assert.deepEqual(clipboard, [['text/plain', 'Preview selection']]);
  assert.equal(prevented, 1);
  message({ source: frame.contentWindow, data: context });
  listeners.paste(e);
  assert.deepEqual(plain(forwarded), [{ type: 'wb-edit', command: 'paste', text: 'Pasted text' }]);
  message({ source: frame.contentWindow, data: context });
  message({ source: frame.contentWindow, data: { type: 'wb-context-reset' } });
  listeners.copy(e);
  assert.equal(clipboard.length, 1);
  for (var command of ['copy', 'cut']) {
    message({ source: frame.contentWindow, data: Object.assign({}, context, { selection: '' }) });
    listeners[command](e);
  }
  assert.equal(clipboard.length, 1, 'empty or protected selections leave the clipboard unchanged');
  assert.equal(forwarded.length, 1, 'cut without selected text sends no edit command');
  message({ source: frame.contentWindow, data: context });
  listeners.cut(e);
  assert.deepEqual(plain(forwarded[1]), { type: 'wb-edit', command: 'cut' });
  assert.deepEqual(clipboard[1], ['text/plain', 'Preview selection']);
  f.dispose();
});

test('retries an unacknowledged pick and stops after wb-here', async function () {
  var f = fixture();
  var url = 'http://127.0.0.1:3579/_workbench/';
  var target = 'pages/login.html:welcome-quiet';

  await f.panel.show(f.context, url, 'pages/login.html');
  await f.panel.show(f.context, url, target);
  await f.panel.show(f.context, url, target);

  assert.deepEqual(plain(f.messages), [
    { type: 'canonic-go', hash: target },
    { type: 'canonic-go', hash: target },
  ]);

  f.receive({ type: 'wb-here', src: 'pages/login.html', state: 'welcome-quiet' });
  await f.panel.show(f.context, url, target);
  assert.equal(f.messages.length, 2, 'acknowledged target is the only repeat suppressed');

  f.dispose();
});

test('refreshes an open workbench and queues new picks until its config is ready', async function () {
  var f = fixture();
  var url = 'http://127.0.0.1:3579/_workbench/';
  await f.panel.show(f.context, url, 'pages/login.html');

  var match = /<script nonce="[^"]+">([\s\S]*?)<\/script>/.exec(f.view.webview.html);
  var windowMessage = null;
  var forwarded = [];
  var frame = {
    contentWindow: {
      postMessage: function (message) { forwarded.push(message); },
    },
    src: '',
  };

  vm.runInNewContext(match[1], {
    acquireVsCodeApi: function () { return { postMessage: function () {} }; },
    document: { getElementById: function () { return frame; }, addEventListener: function () {} },
    KeyboardEvent: function () {},
    window: {
      addEventListener: function (type, fn) {
        if (type === 'message') windowMessage = fn;
      },
      dispatchEvent: function () {},
    },
  });

  windowMessage({
    source: frame.contentWindow,
    data: { type: 'wb-here', src: 'pages/login.html', state: null },
  });
  await f.panel.refresh();
  windowMessage({ data: f.messages.pop() });

  assert.deepEqual(plain(forwarded), [{ type: 'wb-refresh' }]);

  windowMessage({ data: { type: 'canonic-go', hash: 'preview/components-new.html:hairline' } });
  assert.equal(forwarded.length, 1, 'the fresh pick waits for the refreshed config');

  windowMessage({
    source: frame.contentWindow,
    data: { type: 'wb-here', src: 'pages/login.html', state: null },
  });
  assert.deepEqual(plain(forwarded), [
    { type: 'wb-refresh' },
    { type: 'wb-go', hash: 'preview/components-new.html:hairline' },
  ]);

  f.dispose();
});

test('retries refresh after an invalid manifest without releasing a queued pick', async function () {
  var f = fixture();
  await f.panel.show(f.context, 'http://127.0.0.1:3579/_workbench/', 'pages/login.html');

  var match = /<script nonce="[^"]+">([\s\S]*?)<\/script>/.exec(f.view.webview.html);
  var windowMessage = null;
  var forwarded = [];
  var frame = {
    contentWindow: { postMessage: function (message) { forwarded.push(message); } },
    src: '',
  };
  vm.runInNewContext(match[1], {
    acquireVsCodeApi: function () { return { postMessage: function () {} }; },
    document: { getElementById: function () { return frame; }, addEventListener: function () {} },
    KeyboardEvent: function () {},
    window: {
      addEventListener: function (type, fn) {
        if (type === 'message') windowMessage = fn;
      },
      dispatchEvent: function () {},
    },
  });

  windowMessage({
    source: frame.contentWindow,
    data: { type: 'wb-here', src: 'pages/login.html', state: null },
  });
  windowMessage({ data: { type: 'canonic-refresh' } });
  windowMessage({ data: { type: 'canonic-go', hash: 'preview/new.html' } });
  windowMessage({ source: frame.contentWindow, data: { type: 'wb-refresh-failed' } });
  assert.deepEqual(plain(forwarded), [{ type: 'wb-refresh' }]);

  windowMessage({ data: { type: 'canonic-refresh' } });
  assert.deepEqual(plain(forwarded), [{ type: 'wb-refresh' }, { type: 'wb-refresh' }]);
  windowMessage({
    source: frame.contentWindow,
    data: { type: 'wb-here', src: 'pages/login.html', state: null },
  });
  assert.deepEqual(plain(forwarded), [
    { type: 'wb-refresh' },
    { type: 'wb-refresh' },
    { type: 'wb-go', hash: 'preview/new.html' },
  ]);

  f.dispose();
});

test('refreshing a rendered docs page releases the next sidebar pick without reloading its preview', async function () {
  var f = fixture();
  await f.panel.show(f.context, 'http://127.0.0.1:3579/_workbench/', 'docs/card.md');
  var bridge = /<script nonce="[^"]+">([\s\S]*?)<\/script>/.exec(f.view.webview.html)[1];
  var receive;
  var child;
  var picks = [];
  var hostMessages = [];
  var previewReloads = 0;
  var finishRefresh;
  var frame = { contentWindow: { postMessage: function (message) {
    if (message.type === 'wb-refresh') finishRefresh = function () { child.show('docs/card.md', null); };
    if (message.type === 'wb-go') picks.push(message.hash);
  } } };
  vm.runInNewContext(bridge, {
    acquireVsCodeApi: function () { return { postMessage: function (message) { hostMessages.push(message); } }; },
    document: { getElementById: function () { return frame; }, addEventListener: function () {} },
    window: { addEventListener: function (type, fn) { if (type === 'message') receive = fn; } },
  });
  var lens = { key: 'html', label: 'HTML', kind: 'docs' };
  child = vm.createContext({
    lensRules: require('./src/modules/docs/canvas/lenses.ts'),
    index: { 'docs/card.md': { src: 'docs/card.md', markdown: 'docs/card.md' } },
    view: { src: 'docs/card.md', state: null, lens: lens },
    frameReady: true,
    stateOf: function () { return null; },
    effectiveLens: function () { return lens; },
    scrollToExample: function () {}, syncHash: function () {},
    setCanvasMode: function () { previewReloads += 1; },
    tellHost: function (src, state, key, pending) {
      receive({ source: frame.contentWindow, data: { type: 'wb-here', src: src, state: state, lens: key, pending: !!pending } });
    },
  });
  var source = fs.readFileSync(path.join(__dirname, 'workbench/workbench.js'), 'utf8');
  vm.runInContext(source.slice(source.indexOf('  function show('), source.indexOf('  /* ----------------------------------------------------------- actions */')), child);
  receive({ source: frame.contentWindow, data: { type: 'wb-here', src: 'docs/card.md', state: null } });
  receive({ data: { type: 'canonic-refresh' } });
  receive({ data: { type: 'canonic-go', hash: 'pages/sidebar.html' } });
  assert.deepEqual(picks, [], 'the new pick waits while the config is loading');
  finishRefresh();
  assert.deepEqual(picks, ['pages/sidebar.html']);
  assert.equal(previewReloads, 0, 'the rendered docs preview is retained');
  assert.equal(hostMessages.length, 2, 'the refresh acknowledges the retained selection');
  f.dispose();
});

test('switching spaces loads the other server in the open tab and drops a slower earlier load', async function () {
  var f = fixture();
  var finishFirst;
  var first = f.panel.show(f.context, new Promise(function (resolve) { finishFirst = resolve; }), 'pages/login.html');
  var switched = f.panel.retarget(Promise.resolve('http://127.0.0.1:3580/_workbench/'));
  assert.equal(await switched, f.view);
  assert.match(f.view.webview.html, /127\.0\.0\.1:3580\/_workbench\//);
  /* The old space's page doesn't follow the tab to the new space. */
  assert.doesNotMatch(f.view.webview.html, /pages\/login\.html/);

  finishFirst('http://127.0.0.1:3579/_workbench/');
  assert.equal(await first, null);
  assert.match(f.view.webview.html, /127\.0\.0\.1:3580/);
  assert.doesNotMatch(f.view.webview.html, /127\.0\.0\.1:3579/);
  f.dispose();
});

test('switching spaces with the tab closed leaves it closed', async function () {
  var f = fixture();
  assert.equal(f.panel.isOpen(), false);
  assert.equal(await f.panel.retarget(Promise.resolve('http://127.0.0.1:3580/_workbench/')), null);
  assert.equal(f.view.webview.html, '');
});

test('a space picked in the canvas reaches the extension', async function () {
  var f = fixture();
  var picked = [];
  f.panel.onSpace(function (id) { picked.push(id); });
  await f.panel.show(f.context, Promise.resolve('http://127.0.0.1:3579/_workbench/'));
  f.receive({ type: 'wb-space', id: 'b2c3d4e5f6' });
  assert.deepEqual(picked, ['b2c3d4e5f6']);
  f.dispose();
});
