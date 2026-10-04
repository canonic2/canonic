var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function action(target, selected, filtering, disclosureOnly) {
  var source = fs.readFileSync(path.join(__dirname, 'page-list.js'), 'utf8');
  var start = source.indexOf('  function stateHeadAction(');
  var end = source.indexOf('\n\n  /* src -> item', start);
  var state = {};
  vm.runInNewContext(source.slice(start, end), state);
  return state.stateHeadAction(target, selected, filtering, disclosureOnly);
}

function empty(filter) {
  var source = fs.readFileSync(path.join(__dirname, 'page-list.js'), 'utf8');
  var start = source.indexOf('  function emptyText(');
  var end = source.indexOf('\n\n  /* src -> item', start);
  var state = {};
  vm.runInNewContext(source.slice(start, end), state);
  return state.emptyText(filter);
}

function pick(item, state, index) {
  var source = fs.readFileSync(path.join(__dirname, 'page-list.js'), 'utf8');
  var start = source.indexOf('  function statePick(');
  var end = source.indexOf('\n\n  function emptyText', start);
  var context = {};
  vm.runInNewContext(source.slice(start, end), context);
  return context.statePick(item, state, index);
}

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
