var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function action(target, selected, filtering, disclosureOnly) {
  var source = fs.readFileSync(path.join(__dirname, "./page-list.js"), 'utf8');
  var start = source.indexOf('  function stateHeadAction(');
  var end = source.indexOf('\n\n  /* src -> item', start);
  var state = {};
  vm.runInNewContext(source.slice(start, end), state);
  return state.stateHeadAction(target, selected, filtering, disclosureOnly);
}

function empty(filter) {
  var source = fs.readFileSync(path.join(__dirname, "./page-list.js"), 'utf8');
  var start = source.indexOf('  function emptyText(');
  var end = source.indexOf('\n\n  /* src -> item', start);
  var state = {};
  vm.runInNewContext(source.slice(start, end), state);
  return state.emptyText(filter);
}

function pick(item, state, index) {
  var source = fs.readFileSync(path.join(__dirname, "./page-list.js"), 'utf8');
  var start = source.indexOf('  function statePick(');
  var end = source.indexOf('\n\n  function emptyText', start);
  var context = {};
  vm.runInNewContext(source.slice(start, end), context);
  return context.statePick(item, state, index);
}

function picking() {
  var source = fs.readFileSync(path.join(__dirname, "./page-list.js"), 'utf8');
  var start = source.indexOf('  function statePick(');
  var end = source.indexOf('\n\n  function emptyText', start);
  var context = {};
  vm.runInNewContext(source.slice(start, end), context);
  return context;
}

test('a collection or group opens on its first page as authored', function () {
  var first = picking().firstPick([
    { src: 'pages/home.html', states: [{ id: 'default' }, { id: 'dark' }] },
    { src: 'pages/pricing.html' },
  ]);
  assert.deepEqual(JSON.parse(JSON.stringify(first)), { src: 'pages/home.html', state: null });
});

test('an imported title with no page of its own opens on its first named state', function () {
  var first = picking().firstPick([
    { src: 'Components/Button', implementationOnly: 'storybook', states: [{ id: 'primary' }, { id: 'ghost' }] },
  ]);
  assert.deepEqual(JSON.parse(JSON.stringify(first)), { src: 'Components/Button', state: 'primary' });
});

test('a collection or group without pages opens nothing', function () {
  assert.equal(picking().firstPick([]), null);
});

test('a space opens the first page in collection and group order, skipping empty collections', function () {
  var window = {};
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, './page-list.js'), 'utf8'), { window: window });
  var first = window.wbPageList.firstPage([
    { name: 'Empty', items: [{ group: 'Empty group', items: [] }] },
    { name: 'Components', items: [{ group: 'Buttons', items: [
      { src: 'Components/Button', implementationOnly: 'storybook', states: [{ id: 'primary' }] },
    ] }, { src: 'card.html' }] },
    { name: 'Pages', items: [{ src: 'home.html' }] },
  ]);
  assert.deepEqual(JSON.parse(JSON.stringify(first)), { src: 'Components/Button', state: 'primary' });
  assert.equal(window.wbPageList.firstPage([]), null);
});

test('a group header opens its first page unless it folds', function () {
  var groupHeadAction = picking().groupHeadAction;
  var label = { closest: function () { return null; } };
  var caret = { closest: function (selector) { return selector === '.wb-fold' ? {} : null; } };

  assert.equal(groupHeadAction(label, false), 'pick');
  assert.equal(groupHeadAction(label, true), 'toggle');
  assert.equal(groupHeadAction(caret, false), 'toggle');
});

test('the disclosure caret folds a state branch without selecting its page', function () {
  var caret = {};
  var pathInsideCaret = {
    closest: function (selector) {
      assert.equal(selector, '.wb-fold');
      return caret;
    },
  };

  assert.equal(action(pathInsideCaret, false, false), 'toggle');
});

test('the rest of a state header retains its select and selected-row fold behavior', function () {
  var label = { closest: function () { return null; } };

  assert.equal(action(label, false, false), 'pick');
  assert.equal(action(label, true, false), 'toggle');
  assert.equal(action(label, true, true), 'pick');
});

test('an imported implementation title is disclosure-only', function () {
  var label = { closest: function () { return null; } };

  assert.equal(action(label, false, false, true), 'toggle');
  assert.equal(action(label, true, false, true), 'toggle');
  assert.equal(action(label, false, true, true), 'toggle');
});

test('every imported story row keeps its real state id, including the first', function () {
  var item = { implementationOnly: 'storybook' };
  assert.equal(pick(item, { id: 'homepages-preview' }, 0), 'homepages-preview');
  assert.equal(pick(item, { id: 'default' }, 1), 'default');
  assert.equal(pick({}, { id: 'default' }, 0), null);
});

test('an empty list is distinct from a filter with no matches', function () {
  assert.equal(empty(''), 'No pages available.');
  assert.equal(empty('   '), 'No pages available.');
  assert.equal(empty('button'), 'Nothing matches “button”.');
});
