var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

/* A small document: elements with attributes, found by attribute selectors. */
function element(id, attributes) {
  var values = Object.assign({}, attributes);
  var node = {
    id: id,
    removed: false,
    getAttribute: function (name) { return Object.prototype.hasOwnProperty.call(values, name) ? values[name] : null; },
    setAttribute: function (name, value) { values[name] = String(value); },
    removeAttribute: function (name) { delete values[name]; },
    hasAttribute: function (name) { return Object.prototype.hasOwnProperty.call(values, name); },
    remove: function () { node.removed = true; },
  };
  return node;
}

function load(search, elements) {
  var root = element('html', {});
  var document = {
    readyState: 'complete',
    documentElement: root,
    addEventListener: function () {},
    querySelectorAll: function (selector) {
      var name = /^\[([^\]]+)\]$/.exec(selector)[1];
      return elements.filter(function (el) { return !el.removed && el.hasAttribute(name); });
    },
  };
  var window = {};
  var file = path.join(__dirname, './scenarios.js');
  vm.runInNewContext(fs.readFileSync(file, 'utf8'), { window: window, document: document, location: { search: search } }, { filename: file });
  return { root: root, window: window };
}

function kept(elements) {
  return elements.filter(function (el) { return !el.removed; }).map(function (el) { return el.id; });
}

test('a page without an iteration has no iteration attribute and drops iteration-only markup', function () {
  var elements = [
    element('hint', { 'data-wb-iteration-only': 'inline-hint' }),
    element('banner', { 'data-wb-iteration-not': 'inline-hint' }),
  ];
  var page = load('?scenario=error', elements);
  assert.equal(page.root.getAttribute('data-wb-scenario'), 'error');
  assert.equal(page.root.hasAttribute('data-wb-iteration'), false);
  assert.deepEqual(kept(elements), ['banner']);
});

test('the selected iteration marks the root and keeps or removes elements by its id', function () {
  var elements = [
    element('hint', { 'data-wb-iteration-only': 'inline-hint compact' }),
    element('other', { 'data-wb-iteration-only': 'compact' }),
    element('banner', { 'data-wb-iteration-not': 'inline-hint' }),
    element('footer', { 'data-wb-iteration-not': 'compact' }),
    element('alert', { 'data-wb-scenario-only': 'error' }),
  ];
  var page = load('?scenario=error&iteration=inline-hint', elements);
  assert.equal(page.root.getAttribute('data-wb-scenario'), 'error');
  assert.equal(page.root.getAttribute('data-wb-iteration'), 'inline-hint');
  assert.deepEqual(kept(elements), ['hint', 'footer', 'alert']);
});

test('an invalid iteration id shows the authored design', function () {
  var elements = [element('hint', { 'data-wb-iteration-only': 'inline-hint' })];
  var page = load('?iteration=Inline%20Hint', elements);
  assert.equal(page.root.hasAttribute('data-wb-iteration'), false);
  assert.deepEqual(kept(elements), []);
});

test('an iteration or scenario id that breaks the shared id rule shows the default and the authored design', function () {
  for (var id of ['2-up', 'a--b', '-hint', 'hint-', 'Hint', 'inline_hint']) {
    var elements = [
      element('hint', { 'data-wb-iteration-only': id }),
      element('alert', { 'data-wb-scenario-only': id }),
    ];
    var page = load('?scenario=' + encodeURIComponent(id) + '&iteration=' + encodeURIComponent(id), elements);
    assert.equal(page.root.getAttribute('data-wb-scenario'), 'default', id);
    assert.equal(page.root.hasAttribute('data-wb-iteration'), false, id);
    assert.deepEqual(kept(elements), [], id);
    page.window.wbPageScenarios.apply(id, id);
    assert.equal(page.root.getAttribute('data-wb-scenario'), 'default', id);
    assert.equal(page.root.hasAttribute('data-wb-iteration'), false, id);
  }
  var valid = load('?scenario=step-2&iteration=v2-compact', []);
  assert.equal(valid.root.getAttribute('data-wb-scenario'), 'step-2');
  assert.equal(valid.root.getAttribute('data-wb-iteration'), 'v2-compact');
});

test('apply takes a render\'s scenario and iteration and clears the iteration when none is given', function () {
  var elements = [];
  var page = load('?scenario=error&iteration=inline-hint', elements);
  elements.push(element('compact', { 'data-wb-iteration-only': 'compact' }), element('empty', { 'data-wb-scenario-only': 'empty' }));
  page.window.wbPageScenarios.apply('empty', 'compact');
  assert.equal(page.root.getAttribute('data-wb-scenario'), 'empty');
  assert.equal(page.root.getAttribute('data-wb-iteration'), 'compact');
  assert.deepEqual(kept(elements), ['compact', 'empty']);
  page.window.wbPageScenarios.apply('empty');
  assert.equal(page.root.hasAttribute('data-wb-iteration'), false);
  assert.deepEqual(kept(elements), ['empty']);
});
