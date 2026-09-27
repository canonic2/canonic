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
