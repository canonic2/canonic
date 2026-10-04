var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function messages() {
  var source = fs.readFileSync(path.join(__dirname, 'sidebar-view.js'), 'utf8');
  var start = source.indexOf('function catalogMessage(');
  var end = source.indexOf('\n\n/* Builds the view', start);
  var state = {};
  vm.runInNewContext(source.slice(start, end), state);
  return state;
}

test('the page catalog message carries server diagnostics into the sidebar', function () {
  var helpers = messages();
  var message = helpers.catalogMessage({
    catalogCollections: [{ name: 'Components', items: [] }],
    problems: ['Storybook is not running.'],
  });

  assert.equal(message.type, 'canonic-catalog');
  assert.equal(message.collections[0].name, 'Components');
  assert.equal(message.problems[0], 'Storybook is not running.');
});

test('an unexpected catalog failure is sent as a diagnostic', function () {
  var message = messages().catalogFailure(new Error('server stopped'));

  assert.equal(message.type, 'canonic-catalog');
  assert.equal(message.collections.length, 0);
  assert.equal(message.problems[0], 'Couldn’t load the page catalogs — server stopped');
});

/* The view module against a stand-in editor: just enough of vscode to
   register the provider and resolve a view. */
function resolveView(visible, overrides) {
  var provider = null;
  var visibility = null;
  var receive = null;
  var shown = 0;
  var watchers = [];
  var picks = [];
  var space = { root: '/acme', list: { current: 'a1', spaces: [{ id: 'a1', name: 'Acme' }] } };
  var vscode = {
    Uri: {
      file: function (p) { return { fsPath: p }; },
      joinPath: function (base) {
        return { fsPath: path.join.apply(path, [base.fsPath].concat([].slice.call(arguments, 1))) };
      },
    },
    RelativePattern: function (base, pattern) { this.base = base.fsPath; this.pattern = pattern; },
    window: {
      registerWebviewViewProvider: function (id, registered) { provider = registered; return {}; },
    },
    workspace: {
      createFileSystemWatcher: function (pattern) {
        var watcher = {
          base: pattern.base, disposed: false,
          onDidChange: function () {}, onDidCreate: function () {}, onDidDelete: function () {},
          dispose: function () { watcher.disposed = true; },
        };
        watchers.push(watcher);
        return watcher;
      },
    },
    commands: { registerCommand: function () { return {}; } },
  };
  var file = path.join(__dirname, 'sidebar-view.js');
  var module = { exports: {} };
  var fakeRequire = function (name) {
    if (name === 'vscode') return vscode;
    if (name === 'path') return path;
    if (name === 'fs') return fs;
    throw new Error('unexpected require: ' + name);
  };
  fakeRequire.resolve = function () { return '/lucide/dist/umd/lucide.min.js'; };
  vm.runInNewContext(fs.readFileSync(file, 'utf8'), {
    module: module, exports: module.exports, require: fakeRequire, console: console,
  }, { filename: file });

  var sidebar = module.exports.register(
    { extensionUri: { fsPath: __dirname }, subscriptions: [] },
    Object.assign({
      space: function () { return { root: space.root, dir: space.dir || space.root, key: space.key || null }; },
      spaces: function () { return space.list; },
      open: function () {},
      follow: function () { return function () {}; },
      refresh: null,
      catalog: null,
      shown: function () { shown += 1; },
      pickSpace: function (id) { picks.push(['pick', id]); },
      addSpace: function () { picks.push(['add']); },
      removeSpace: function (id) { picks.push(['remove', id]); },
    }, overrides || {})
  );

  var view = {
    visible: visible,
    webview: {
      html: '',
      options: null,
      cspSource: 'vscode-resource:',
      asWebviewUri: function (uri) { return { toString: function () { return String(uri.fsPath); } }; },
      onDidReceiveMessage: function (fn) { receive = fn; },
      postMessage: function () {},
    },
    onDidChangeVisibility: function (fn) { visibility = fn; return { dispose: function () {} }; },
    onDidDispose: function () {},
  };
  provider.resolveWebviewView(view);

  return {
    view: view,
    watchers: watchers,
    picks: picks,
    space: space,
    sidebar: sidebar,
    receive: function (message) { receive(message); },
    shown: function () { return shown; },
    show: function (next) { view.visible = next; visibility(); },
  };
}

function meta(html, name) {
  var match = new RegExp('<meta name="' + name + '" content="([^"]*)"').exec(html);
  return match && match[1].replace(/&lt;/g, '<').replace(/&quot;/g, '"').replace(/&amp;/g, '&');
}

test('selecting the Workbench view opens the canvas', function () {
  var sidebar = resolveView(true);
  assert.equal(sidebar.shown(), 1);
});

test('the canvas opens each time the view comes into sight, not when it leaves', function () {
  var sidebar = resolveView(false);
  assert.equal(sidebar.shown(), 0);
  sidebar.show(true);
  assert.equal(sidebar.shown(), 1);
  sidebar.show(false);
  assert.equal(sidebar.shown(), 1);
  sidebar.show(true);
  assert.equal(sidebar.shown(), 2);
});

test('the page carries the current space root and the spaces to switch between', function () {
  var sidebar = resolveView(true);
  var html = sidebar.view.webview.html;
  assert.equal(meta(html, 'canonic-root'), '/acme/');
  assert.deepEqual(JSON.parse(meta(html, 'canonic-spaces')), sidebar.space.list);
  assert.ok(sidebar.view.webview.options.localResourceRoots.some(function (uri) { return uri.fsPath === '/acme'; }));
});

test('switching spaces moves the page, its resources and the watcher to the new root', function () {
  var sidebar = resolveView(true);
  assert.equal(sidebar.watchers.length, 1);
  assert.equal(sidebar.watchers[0].base, '/acme');

  sidebar.space.root = '/example-ui';
  sidebar.space.list = { current: 'b2', spaces: [{ id: 'a1', name: 'Acme' }, { id: 'b2', name: 'Example <UI> & "Co"' }] };
  sidebar.sidebar.retarget();

  var html = sidebar.view.webview.html;
  assert.equal(meta(html, 'canonic-root'), '/example-ui/');
  assert.equal(JSON.parse(meta(html, 'canonic-spaces')).spaces[1].name, 'Example <UI> & "Co"');
  assert.equal(sidebar.watchers[0].disposed, true);
  assert.equal(sidebar.watchers[1].base, '/example-ui');
  var roots = sidebar.view.webview.options.localResourceRoots.map(function (uri) { return uri.fsPath; });
  assert.ok(roots.indexOf('/example-ui') > -1);
  assert.equal(roots.indexOf('/acme'), -1);
});

test('a relisted space keeps its watcher and rebuilds the page', function () {
  var sidebar = resolveView(true);
  var before = sidebar.view.webview.html;
  sidebar.sidebar.retarget();
  assert.equal(sidebar.watchers.length, 1);
  assert.notEqual(sidebar.view.webview.html, before);
});

test('the switcher’s choices reach the extension', function () {
  var sidebar = resolveView(true);
  sidebar.receive({ type: 'canonic-space', id: 'b2' });
  sidebar.receive({ type: 'canonic-add-space' });
  sidebar.receive({ type: 'canonic-remove-space', id: 'b2' });
  assert.deepEqual(sidebar.picks, [['pick', 'b2'], ['add'], ['remove', 'b2']]);
});

test('with no space left the view says so instead of loading a page', function () {
  var sidebar = resolveView(true, { space: function () { return null; } });
  assert.match(sidebar.view.webview.html, /no space to show/);
  assert.equal(sidebar.watchers.length, 0);
});

test('a space of a file that lists several tells the page where the file is and which space it is', function () {
  var sidebar = resolveView(true);
  sidebar.space.root = '/acme/packages/ui';
  sidebar.space.dir = '/acme';
  sidebar.space.key = 'ui';
  sidebar.sidebar.retarget();

  var html = sidebar.view.webview.html;
  assert.equal(meta(html, 'canonic-root'), '/acme/packages/ui/');
  assert.equal(meta(html, 'canonic-config'), '/acme/');
  assert.equal(meta(html, 'canonic-space'), 'ui');
  /* The file didn't move, so neither did the watcher. */
  assert.equal(sidebar.watchers.length, 1);
  assert.equal(sidebar.watchers[0].base, '/acme');
  var roots = sidebar.view.webview.options.localResourceRoots.map(function (uri) { return uri.fsPath; });
  assert.ok(roots.indexOf('/acme/packages/ui') > -1 && roots.indexOf('/acme') > -1);
});

test('a picked row reaches the canvas with its state', function () {
  var opened = [];
  var sidebar = resolveView(true, { open: function (target) { opened.push(JSON.parse(JSON.stringify(target))); } });
  sidebar.receive({ type: 'canonic-pick', hash: 'pages/sign-in.html:error' });
  sidebar.receive({ type: 'canonic-pick', hash: 'docs/card.md' });
  assert.deepEqual(opened, [
    { src: 'pages/sign-in.html', state: 'error' },
    { src: 'docs/card.md', state: null },
  ]);
});
