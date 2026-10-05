var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function loadSidebar(overrides, spaces) {
  var shell = { hidden: false, dimmed: false, classList: { toggle: function (name, on) { shell.dimmed = on; } } };
  var listeners = {};
  var posted = [];
  var diagnostics = { hidden: true, textContent: '' };
  var elements = {
    problem: { hidden: true, textContent: '' },
    diagnostics: diagnostics,
    search: {},
    loading: { hidden: false },
    loadingText: { textContent: '' },
    rail: {},
    pageList: {},
    spaces: { hidden: true, addEventListener: function (name, callback) { listeners[name] = callback; } },
    spaceButton: {},
    spaceMenu: {},
  };
  var built = null;
  var config = Object.assign({
    name: 'Acme',
    collections: [],
    implementations: { storybook: { catalog: true } },
  }, overrides);
  var window = {
    addEventListener: function (type, listener) { listeners[type] = listener; },
    removeEventListener: function () {},
    wbIcon: function () {},
    wbConfig: { load: function (ok) { ok(config); } },
    wbManifest: {
      mergeCollections: function (base, imported) { return base.concat(imported); },
    },
    wbPageList: {
      index: function () { return {}; },
      create: function (options) { built = options.collections; return {}; },
    },
  };
  var document = {
    querySelector: function (selector) {
      if (selector === '.sb') return shell;
      if (selector === 'meta[name="canonic-spaces"]') return spaces ? { content: spaces } : null;
      return { hidden: false };
    },
    querySelectorAll: function () { return []; },
    getElementById: function (id) { return elements[id]; },
  };

  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'sidebar.js'), 'utf8'), {
    window: window,
    document: document,
    console: { error: function () {} },
    acquireVsCodeApi: function () {
      return { postMessage: function (message) { posted.push(message); } };
    },
  });

  return {
    posted: posted,
    diagnostics: diagnostics,
    loading: elements.loading,
    loadingText: elements.loadingText,
    search: elements.search,
    receive: function (data) { listeners.message({ data: data }); },
    built: function () { return built; },
    switcher: function () { return elements.spaces.hidden ? null : elements.spaces; },
    intent: function (name, detail) { listeners[name]({ detail: detail || {} }); },
    shell: shell,
    spaces: elements.spaces,
  };
}

test('the sidebar shows the space switcher from the spaces the extension lists', function () {
  var listed = { current: 'a1', spaces: [{ id: 'a1', name: 'Acme', removable: false }, { id: 'b2', name: 'Example', removable: true }] };
  var sidebar = loadSidebar({ implementations: {}, previews: false }, JSON.stringify(listed));
  var switcher = sidebar.switcher();

  assert.equal(sidebar.spaces.hidden, false);
  assert.equal(switcher.currentId, 'a1');
  assert.equal(switcher.spaces.map(function (p) { return p.name; }).join(), 'Acme,Example');
  assert.equal(switcher.allowAdd, true);
  assert.equal(switcher.allowRemove, true);

  sidebar.intent('wb-space-pick', { id: 'b2' });
  sidebar.intent('wb-space-add');
  sidebar.intent('wb-space-remove', { id: 'b2' });
  assert.deepEqual(JSON.parse(JSON.stringify(sidebar.posted.slice(-3))), [
    { type: 'canonic-space', id: 'b2' },
    { type: 'canonic-add-space' },
    { type: 'canonic-remove-space', id: 'b2' },
  ]);

  sidebar.intent('wb-space-toggle', { open: true });
  assert.equal(sidebar.shell.dimmed, true);
  sidebar.intent('wb-space-toggle', { open: false });
  assert.equal(sidebar.shell.dimmed, false);
});

test('the sidebar has no switcher without a space list', function () {
  var sidebar = loadSidebar({ implementations: {}, previews: false });
  assert.equal(sidebar.switcher(), null);
  assert.equal(sidebar.spaces.hidden, true);
});

test('the sidebar shows catalog diagnostics while still building imported pages', function () {
  var sidebar = loadSidebar();
  assert.equal(sidebar.posted[0].type, 'canonic-catalog-request');
  assert.equal(sidebar.loading.hidden, false);
  assert.equal(sidebar.loadingText.textContent, 'Waiting for storybook…');
  assert.equal(sidebar.search.disabled, true);

  var collections = [{ name: 'Components', items: [{ label: 'Button', src: 'button' }] }];
  sidebar.receive({
    type: 'canonic-catalog',
    collections: collections,
    problems: ['Storybook is not running.', 'No matching Simulator was found.'],
  });

  assert.equal(sidebar.diagnostics.hidden, false);
  assert.equal(
    sidebar.diagnostics.textContent,
    'Storybook is not running.\nNo matching Simulator was found.'
  );
  assert.deepEqual(sidebar.built(), collections);
  assert.equal(sidebar.loading.hidden, true);
  assert.equal(sidebar.search.disabled, false);
});

test('the sidebar hides catalog diagnostics when every catalog loads', function () {
  var sidebar = loadSidebar();
  sidebar.receive({ type: 'canonic-catalog', collections: [], problems: [] });

  assert.equal(sidebar.diagnostics.hidden, true);
  assert.equal(sidebar.diagnostics.textContent, '');
});

test('the sidebar requests discovered previews without an implementation catalog', function () {
  var sidebar = loadSidebar({ implementations: {} });
  assert.equal(sidebar.posted[0].type, 'canonic-catalog-request');

  var collections = [{ name: 'Website', items: [{ label: 'Overview', src: 'previews/overview.workbench.ts' }] }];
  sidebar.receive({ type: 'canonic-catalog', collections: collections, problems: [] });
  assert.deepEqual(sidebar.built(), collections);
});

test('the sidebar builds manual pages immediately when preview discovery is disabled', function () {
  var collections = [{ name: 'Pages', items: [{ label: 'Overview', src: 'overview.html' }] }];
  var sidebar = loadSidebar({ implementations: {}, previews: false, collections: collections });
  assert.equal(sidebar.posted[0].type, 'canonic-ready');
  assert.deepEqual(sidebar.built(), collections);
});
