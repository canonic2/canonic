var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function load() {
  var window = {};
  var file = path.join(__dirname, 'lenses.js');
  vm.runInNewContext(fs.readFileSync(file, 'utf8'), { window: window, URL: URL }, { filename: file });
  return window.wbLenses;
}

var dev = { key: 'dev', kind: 'url', base: 'http://localhost:3710' };
var storybook = { key: 'storybook', kind: 'storybook', url: 'http://localhost:6006' };

test('joins a url implementation’s base with the screen’s path there', function () {
  var lenses = load();
  assert.equal(lenses.url(dev, { path: '/' }, null), 'http://localhost:3710/');
  assert.equal(lenses.url(dev, { path: '/' }, 'error'), 'http://localhost:3710/');
  assert.equal(lenses.url(dev, { path: '/', states: { error: '/?error=1' } }, 'error'), 'http://localhost:3710/?error=1');
  assert.equal(lenses.url(dev, { path: '/', states: { error: '/?error=1' } }, 'other'), 'http://localhost:3710/');
});

test('addresses one story alone, and inside Storybook', function () {
  var lenses = load();
  assert.equal(
    lenses.storyUrl(storybook, 'components-button--icon-only'),
    'http://localhost:6006/iframe.html?id=components-button--icon-only&viewMode=story'
  );
  assert.equal(
    lenses.storyOpenUrl(storybook, 'components-button--icon-only'),
    'http://localhost:6006/?path=/story/components-button--icon-only'
  );
});

test('picks the story the address means, else the first', function () {
  var lenses = load();
  var stories = [{ id: 'a--default', state: 'default' }, { id: 'a--icon-only', state: 'icon-only' }];
  assert.equal(lenses.pick(stories, 'icon-only'), stories[1]);
  assert.equal(lenses.pick(stories, 'error'), stories[0]);
  assert.equal(lenses.pick(stories, null), stories[0]);
  assert.equal(lenses.pick([], null), null);
});

test('sends a Storybook switch and accepts only matching preview acknowledgements', function () {
  var lenses = load();
  var sent = [];
  var frame = { contentWindow: { postMessage: function (data, origin) { sent.push([data, origin]); } } };
  assert.equal(lenses.selectStory(frame, storybook, 'components-button--secondary'), true);
  assert.equal(sent[0][1], 'http://localhost:6006');
  assert.equal(JSON.parse(sent[0][0]).event.args[0].storyId, 'components-button--secondary');

  var rendered = { source: frame.contentWindow, origin: 'http://localhost:6006',
    data: JSON.stringify({ key: 'storybook-channel', event: {
      type: 'storyRendered', args: ['components-button--secondary'],
    } }) };
  assert.equal(lenses.storybookEvent(rendered, frame, storybook).id, 'components-button--secondary');
  assert.equal(lenses.storybookEvent(Object.assign({}, rendered, { origin: 'https://example.com' }), frame, storybook), null);
  assert.equal(lenses.storybookEvent(Object.assign({}, rendered, { source: {} }), frame, storybook), null);
});
