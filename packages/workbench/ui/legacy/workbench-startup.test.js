var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function startup(hash, collections, imported) {
  var window = {};
  var location = { hash: hash || '' };
  var pending;
  var routed = [];
  var context = {
    window: window,
    location: location,
    history: { replaceState: function (_, title, address) { location.hash = address; } },
    loadResolved: function (then) { pending = then; },
    resolved: { catalogCollections: imported || [] },
    blank: { classList: { remove: function () {} } },
    loadSpaceSizes: function () {},
    sizeSwitcher: {},
    showSizes: function () {},
    current: null,
    index: {},
    view: null,
    document: { getElementById: function () { return {}; } },
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
    apply: function (first) { context.applyConfig({ name: 'Acme', collections: collections }, first); },
    finish: function () { pending(); },
    location: location,
    routed: routed,
  };
}

test('opening another space without a destination routes to its first resolved page', function () {
  var app = startup('', [{ name: 'Pages', items: [{ group: 'Start', items: [{ src: 'home.html' }] }] }]);
  app.apply(true);
  app.finish();
  assert.equal(app.routed.length, 1);
  assert.equal(app.routed[0].src, 'home.html');
});

test('an imported-only space opens its first named scenario after catalog resolution', function () {
  var app = startup('', [], [{ name: 'Stories', items: [
    { src: 'Components/Button', implementationOnly: 'storybook', scenarios: [{ id: 'primary' }] },
  ] }]);
  app.apply(true);
  assert.equal(app.routed.length, 0);
  app.finish();
  assert.equal(app.routed[0].src, 'Components/Button');
  assert.equal(app.routed[0].scenario, 'primary');
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
