var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function startup(hash, collections, imported, server) {
  var window = {};
  var location = { hash: hash || '' };
  var pending;
  var routed = [];
  var elements = {};
  var context = {
    window: window,
    location: location,
    history: { replaceState: function (_, title, address) { location.hash = address; } },
    loadResolved: function (then) { pending = then; },
    resolved: Object.assign({ catalogCollections: imported || [] }, server || {}),
    blank: { classList: { remove: function () {} } },
    loadSpaceSizes: function () {},
    sizeSwitcher: {},
    showSizes: function () {},
    current: null,
    index: {},
    view: null,
    document: { getElementById: function (id) { return elements[id] || (elements[id] = {}); } },
    spaceName: {},
    loadSpaces: function () {},
    restore: function () {},
    pageList: { update: function () {} },
    parseHash: function () { return window.wbAddress.parse(location.hash); },
    route: function () { routed.push(window.wbAddress.parse(location.hash)); },
  };
  vm.createContext(context);
  for (var file of ['address.js', 'page-list.js']) {
    vm.runInContext(fs.readFileSync(path.join(__dirname, file), 'utf8'), context);
  }
  window.wbManifest = { mergeCollections: function (base, extra) { return base.concat(extra); } };
  var source = fs.readFileSync(path.join(__dirname, 'workbench.js'), 'utf8');
  var start = source.indexOf('  function applyConfig(');
  var end = source.indexOf('\n  /* Size and docs bootstraps', start);
  vm.runInContext(source.slice(start, end), context);
  return {
    apply: function (first, problems) { context.applyConfig({ name: 'Acme', collections: collections, problems: problems || [] }, first); },
    finish: function () { pending(); },
    elements: elements,
    context: context,
    location: location,
    routed: routed,
    window: window,
  };
}

test('a Workbench lens takes the preview address the server resolved for its page', function () {
  var page = { src: 'components/button.html', implementations: { implementation: { preview: 'components/button' }, staging: { path: '/button' } } };
  var resolvedRef = {
    preview: 'components/button',
    path: '/components/button.workbench.ts',
    scenarios: { default: '/components/button.workbench.ts?scenario=default', ghost: '/components/button.workbench.ts?scenario=ghost' },
  };
  var app = startup('#components/button.html', [{ name: 'Components', items: [{ group: 'Inputs', items: [page] }] }], [], {
    implementations: { implementation: { key: 'implementation', kind: 'workbench', base: '' }, staging: { key: 'staging', kind: 'url', base: 'https://staging.example.com' } },
    collections: [{ name: 'Components', items: [{ group: 'Inputs', items: [{ src: 'components/button.html', implementations: { implementation: resolvedRef, staging: { path: '/button' } } }] }] }],
  });
  app.apply(true);
  app.finish();
  assert.deepEqual(page.implementations.implementation, resolvedRef);
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'lenses.js'), 'utf8'), { window: app.window, URL: URL });
  var lens = { key: 'implementation', kind: 'workbench', base: '' };
  assert.equal(app.window.wbLenses.url(lens, page.implementations.implementation, null), '/components/button.workbench.ts');
  assert.equal(app.window.wbLenses.url(lens, page.implementations.implementation, 'ghost'), '/components/button.workbench.ts?scenario=ghost');
});

test('the browser sidebar shows the server’s problems, or the reader’s own without a server', function () {
  var pages = [{ name: 'Pages', items: [{ src: 'home.html' }] }];
  var app = startup('#home.html', pages, [], { problems: ['Pages › Sign in: every page needs a label and a src.'] });
  app.apply(true, ['a browser-only problem']);
  app.finish();
  assert.equal(app.elements.problems.hidden, false);
  assert.equal(app.elements.problems.textContent, 'Pages › Sign in: every page needs a label and a src.');

  var offline = startup('#home.html', pages);
  offline.context.resolved = null;
  offline.apply(true, ['Pages › Unlinked: every page needs a label and a src.']);
  offline.finish();
  assert.equal(offline.elements.problems.textContent, 'Pages › Unlinked: every page needs a label and a src.');

  var clean = startup('#home.html', pages, [], { problems: [] });
  clean.apply(true);
  clean.finish();
  assert.equal(clean.elements.problems.hidden, true);
});

test('opening another space without a destination routes to its first resolved page', function () {
  var app = startup('', [{ name: 'Pages', items: [{ group: 'Start', items: [{ src: 'home.html' }] }] }]);
  app.apply(true);
  app.finish();
  assert.equal(app.routed.length, 1);
  assert.equal(app.routed[0].src, 'home.html');
});

test('an imported-only space opens its first catalog page after catalog resolution', function () {
  var app = startup('', [], [{ name: 'Simulators', items: [
    { label: 'iPhone 17', src: '__ios-simulator/ios/SIM-1.html', implementationOnly: 'ios' },
  ] }]);
  app.apply(true);
  assert.equal(app.routed.length, 0);
  app.finish();
  assert.equal(app.routed[0].src, '__ios-simulator/ios/SIM-1.html');
  assert.equal(app.routed[0].scenario, null);
});

test('explicit destinations, including invalid links, take precedence over the first page', function () {
  for (var hash of ['#card.html:dark@mobile~design', '#missing.html']) {
    var app = startup(hash, [{ name: 'Pages', items: [{ src: 'home.html' }] }]);
    app.apply(true);
    app.finish();
    assert.equal(app.location.hash, hash);
  }
});

test('a pick arriving while configuration loads takes precedence over the first page', function () {
  var app = startup('', [{ name: 'Pages', items: [{ src: 'home.html' }] }]);
  app.apply(true);
  app.location.hash = '#card.html';
  app.finish();
  assert.equal(app.routed[0].src, 'card.html');
});

test('an empty space stays empty and config refresh does not select a first page', function () {
  var empty = startup('', []);
  empty.apply(true);
  empty.finish();
  assert.equal(empty.location.hash, '');
  var refresh = startup('', [{ name: 'Pages', items: [{ src: 'home.html' }] }]);
  refresh.apply(false);
  refresh.finish();
  assert.equal(refresh.location.hash, '');
});
