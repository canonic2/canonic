var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function action(target, selected, filtering) {
  var source = fs.readFileSync(path.join(__dirname, "./page-list.js"), 'utf8');
  var start = source.indexOf('  function scenarioHeadAction(');
  var end = source.indexOf('\n\n  /* src -> item', start);
  var state = {};
  vm.runInNewContext(source.slice(start, end), state);
  return state.scenarioHeadAction(target, selected, filtering);
}

function empty(filter) {
  var source = fs.readFileSync(path.join(__dirname, "./page-list.js"), 'utf8');
  var start = source.indexOf('  function emptyText(');
  var end = source.indexOf('\n\n  /* src -> item', start);
  var state = {};
  vm.runInNewContext(source.slice(start, end), state);
  return state.emptyText(filter);
}

function picking() {
  var source = fs.readFileSync(path.join(__dirname, "./page-list.js"), 'utf8');
  var start = source.indexOf('  function scenarioPick(');
  var end = source.indexOf('\n\n  function emptyText', start);
  var context = {};
  vm.runInNewContext(source.slice(start, end), context);
  return context;
}

test('a collection or group opens on its first page as authored', function () {
  var first = picking().firstPick([
    { src: 'pages/home.html', scenarios: [{ id: 'default' }, { id: 'dark' }] },
    { src: 'pages/pricing.html' },
  ]);
  assert.deepEqual(JSON.parse(JSON.stringify(first)), { src: 'pages/home.html', scenario: null });
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
      { src: 'button.html', scenarios: [{ id: 'primary' }, { id: 'ghost' }] },
    ] }, { src: 'card.html' }] },
    { name: 'Pages', items: [{ src: 'home.html' }] },
  ]);
  assert.deepEqual(JSON.parse(JSON.stringify(first)), { src: 'button.html', scenario: null });
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

test('the disclosure caret folds a scenario branch without selecting its page', function () {
  var caret = {};
  var pathInsideCaret = {
    closest: function (selector) {
      assert.equal(selector, '.wb-fold');
      return caret;
    },
  };

  assert.equal(action(pathInsideCaret, false, false), 'toggle');
});

test('the rest of a scenario header retains its select and selected-row fold behavior', function () {
  var label = { closest: function () { return null; } };

  assert.equal(action(label, false, false), 'pick');
  assert.equal(action(label, true, false), 'toggle');
  assert.equal(action(label, true, true), 'pick');
});

test('a scenario row picks none for the page as authored and its id otherwise', function () {
  var scenarioPick = picking().scenarioPick;
  assert.equal(scenarioPick({ id: 'default' }, 0), null);
  assert.equal(scenarioPick({ id: 'error' }, 1), 'error');
});

test('with no scenario reported, the page’s first scenario row is current whatever its id', function () {
  var rowIsCurrent = picking().rowIsCurrent;
  var first = { src: 'pages/sign-in.html', scenario: 'signed-out', first: 'true' };
  var error = { src: 'pages/sign-in.html', scenario: 'error' };
  assert.equal(rowIsCurrent(first, 'pages/sign-in.html', null), true);
  assert.equal(rowIsCurrent(error, 'pages/sign-in.html', null), false);
  assert.equal(rowIsCurrent(first, 'pages/sign-in.html', 'error'), false);
  assert.equal(rowIsCurrent(error, 'pages/sign-in.html', 'error'), true);
  assert.equal(rowIsCurrent(first, 'pages/home.html', null), false);
  // A page row without scenarios is current by its src alone.
  assert.equal(rowIsCurrent({ src: 'pages/home.html' }, 'pages/home.html', null), true);
});

test('an empty list is distinct from a filter with no matches', function () {
  assert.equal(empty(''), 'No pages available.');
  assert.equal(empty('   '), 'No pages available.');
  assert.equal(empty('button'), 'Nothing matches “button”.');
});
