var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function loadSidebar(overrides, projects) {
  var switcher = null;
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
    nav: {},
    projects: { hidden: true },
    projectButton: {},
    projectMenu: {},
  };
  var built = null;
  var config = Object.assign({
    name: 'Acme',
    sections: [],
    implementations: { storybook: { catalog: true } },
  }, overrides);
  var window = {
    addEventListener: function (type, listener) { listeners[type] = listener; },
    removeEventListener: function () {},
    wbIcon: function () {},
    wbProjects: {
      create: function (options) {
        switcher = { options: options, set: function (list, current) { switcher.list = list; switcher.current = current; } };
        return switcher;
      },
    },
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
    querySelector: function (selector) {
      if (selector === '.sb') return shell;
      if (selector === 'meta[name="canonic-projects"]') return projects ? { content: projects } : null;
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
    switcher: function () { return switcher; },
    shell: shell,
    projects: elements.projects,
  };
}

test('the sidebar shows the project switcher from the projects the extension lists', function () {
  var listed = { current: 'a1', projects: [{ id: 'a1', name: 'Acme', removable: false }, { id: 'b2', name: 'Example', removable: true }] };
  var sidebar = loadSidebar({ implementations: {}, previews: false }, JSON.stringify(listed));
  var switcher = sidebar.switcher();

  assert.equal(sidebar.projects.hidden, false);
  assert.equal(switcher.current, 'a1');
  assert.equal(switcher.list.map(function (p) { return p.name; }).join(), 'Acme,Example');

  switcher.options.onPick('b2');
  switcher.options.onAdd();
  switcher.options.onRemove('b2');
  assert.deepEqual(JSON.parse(JSON.stringify(sidebar.posted.slice(-3))), [
    { type: 'canonic-project', id: 'b2' },
    { type: 'canonic-add-project' },
    { type: 'canonic-remove-project', id: 'b2' },
  ]);

  switcher.options.onToggle(true);
  assert.equal(sidebar.shell.dimmed, true);
  switcher.options.onToggle(false);
  assert.equal(sidebar.shell.dimmed, false);
});

test('the sidebar has no switcher without a project list', function () {
  var sidebar = loadSidebar({ implementations: {}, previews: false });
  assert.equal(sidebar.switcher(), null);
  assert.equal(sidebar.projects.hidden, true);
});

test('the sidebar shows catalog diagnostics while still building imported screens', function () {
  var sidebar = loadSidebar();
  assert.equal(sidebar.posted[0].type, 'canonic-catalog-request');
  assert.equal(sidebar.loading.hidden, false);
  assert.equal(sidebar.loadingText.textContent, 'Waiting for storybook…');
  assert.equal(sidebar.search.disabled, true);

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
  assert.equal(sidebar.loading.hidden, true);
  assert.equal(sidebar.search.disabled, false);
});

test('the sidebar hides catalog diagnostics when every catalog loads', function () {
  var sidebar = loadSidebar();
  sidebar.receive({ type: 'canonic-catalog', sections: [], problems: [] });

  assert.equal(sidebar.diagnostics.hidden, true);
  assert.equal(sidebar.diagnostics.textContent, '');
});

test('the sidebar requests discovered previews without an implementation catalog', function () {
  var sidebar = loadSidebar({ implementations: {} });
  assert.equal(sidebar.posted[0].type, 'canonic-catalog-request');

  var sections = [{ group: 'Website', items: [{ label: 'Overview', src: 'previews/overview.workbench.ts' }] }];
  sidebar.receive({ type: 'canonic-catalog', sections: sections, problems: [] });
  assert.deepEqual(sidebar.built(), sections);
});

test('the sidebar builds manual screens immediately when preview discovery is disabled', function () {
  var sections = [{ group: 'Pages', items: [{ label: 'Overview', src: 'overview.html' }] }];
  var sidebar = loadSidebar({ implementations: {}, previews: false, sections: sections });
  assert.equal(sidebar.posted[0].type, 'canonic-ready');
  assert.deepEqual(sidebar.built(), sections);
});
