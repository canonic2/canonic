var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function messages() {
  var source = fs.readFileSync(path.join(__dirname, 'screens.js'), 'utf8');
  var start = source.indexOf('function catalogMessage(');
  var end = source.indexOf('\n\n/* Builds the view', start);
  var state = {};
  vm.runInNewContext(source.slice(start, end), state);
  return state;
}

test('the screen catalog message carries server diagnostics into the sidebar', function () {
  var helpers = messages();
  var message = helpers.catalogMessage({
    catalogSections: [{ group: 'Components', items: [] }],
    problems: ['Storybook is not running.'],
  });

  assert.equal(message.type, 'canonic-catalog');
  assert.equal(message.sections[0].group, 'Components');
  assert.equal(message.problems[0], 'Storybook is not running.');
});

test('an unexpected catalog failure is sent as a diagnostic', function () {
  var message = messages().catalogFailure(new Error('server stopped'));

  assert.equal(message.type, 'canonic-catalog');
  assert.equal(message.sections.length, 0);
  assert.equal(message.problems[0], 'Couldn’t load the screen catalogs — server stopped');
});

/* The view module against a stand-in editor: just enough of vscode to
   register the provider and resolve a view. */
function resolveView(visible) {
  var provider = null;
  var visibility = null;
  var shown = 0;
  var vscode = {
    Uri: {
      file: function (p) { return { fsPath: p }; },
      joinPath: function (base) {
        return { fsPath: path.join.apply(path, [base.fsPath].concat([].slice.call(arguments, 1))) };
      },
    },
    RelativePattern: function () {},
    window: {
      registerWebviewViewProvider: function (id, registered) { provider = registered; return {}; },
    },
    workspace: {
      createFileSystemWatcher: function () {
        return { onDidChange: function () {}, onDidCreate: function () {}, onDidDelete: function () {} };
      },
    },
    commands: { registerCommand: function () { return {}; } },
  };
  var file = path.join(__dirname, 'screens.js');
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

  module.exports.register(
    { extensionUri: { fsPath: __dirname }, subscriptions: [] },
    '/project',
    function () {},
    function () { return function () {}; },
    null,
    null,
    function () { shown += 1; }
  );

  var view = {
    visible: visible,
    webview: {
      cspSource: 'vscode-resource:',
      asWebviewUri: function (uri) { return { toString: function () { return String(uri.fsPath); } }; },
      onDidReceiveMessage: function () {},
      postMessage: function () {},
    },
    onDidChangeVisibility: function (fn) { visibility = fn; return { dispose: function () {} }; },
    onDidDispose: function () {},
  };
  provider.resolveWebviewView(view);

  return {
    shown: function () { return shown; },
    show: function (next) { view.visible = next; visibility(); },
  };
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
