var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function loadSidebar() {
  var listeners = {};
  var posted = [];
  var diagnostics = { hidden: true, textContent: '' };
  var elements = {
    problem: { hidden: true, textContent: '' },
    diagnostics: diagnostics,
    search: {},
    rail: {},
    nav: {},
  };
  var built = null;
  var config = {
    name: 'Acme',
    sections: [],
    implementations: { storybook: { catalog: true } },
  };
  var window = {
    addEventListener: function (type, listener) { listeners[type] = listener; },
    removeEventListener: function () {},
    wbIcon: function () {},
    wbConfig: { load: function (ok) { ok(config); } },
    wbManifest: {
      mergeSections: function (base, imported) { return base.concat(imported); },
    },
    wbNav: {
      index: function () { return {}; },
      create: function (options) { built = options.groups; return {}; },
    },
  };
  var document = {
    querySelector: function () { return { hidden: false }; },
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
    receive: function (data) { listeners.message({ data: data }); },
    built: function () { return built; },
  };
}

test('the sidebar shows catalog diagnostics while still building imported screens', function () {
  var sidebar = loadSidebar();
  assert.equal(sidebar.posted[0].type, 'canonic-catalog-request');

  var sections = [{ group: 'Components', items: [{ label: 'Button', src: 'button' }] }];
  sidebar.receive({
    type: 'canonic-catalog',
    sections: sections,
    problems: ['Storybook is not running.', 'No matching Simulator was found.'],
  });

  assert.equal(sidebar.diagnostics.hidden, false);
  assert.equal(
    sidebar.diagnostics.textContent,
    'Storybook is not running.\nNo matching Simulator was found.'
  );
  assert.deepEqual(sidebar.built(), sections);
});

test('the sidebar hides catalog diagnostics when every catalog loads', function () {
  var sidebar = loadSidebar();
  sidebar.receive({ type: 'canonic-catalog', sections: [], problems: [] });

  assert.equal(sidebar.diagnostics.hidden, true);
  assert.equal(sidebar.diagnostics.textContent, '');
});
