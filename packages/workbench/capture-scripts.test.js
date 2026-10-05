var assert = require('node:assert/strict');
var test = require('node:test');
var vm = require('node:vm');

var scripts = require('./capture-scripts');

test('capture overlays force the requested scroll instead of starting a smooth scroll', async function () {
  var behavior = 'smooth';
  var scrolls = [];
  var style = {
    getPropertyValue: function () { return behavior; },
    getPropertyPriority: function () { return ''; },
    setProperty: function (_, value) { behavior = value; },
    removeProperty: function () { behavior = ''; },
  };
  var root = { style: style, appendChild: function () {} };
  var document = {
    documentElement: root,
    fonts: { ready: Promise.resolve() },
    getElementById: function () { return null; },
    createElement: function () { return { style: {}, remove: function () {}, innerHTML: '', id: '', className: '' }; },
  };
  var window = { scrollTo: function (x, y) { scrolls.push([x, y, behavior]); } };
  await vm.runInNewContext(scripts.overlayScript({ scroll: { x: 12, y: 345 } }), {
    document: document,
    window: window,
    Promise: Promise,
    requestAnimationFrame: function (fn) { fn(); },
  });
  assert.deepEqual(scrolls, [[12, 345, 'auto']]);
  assert.equal(behavior, 'smooth');
});

test('the Storybook switch script asks the loaded preview for the story', function () {
  var source = scripts.storybookSwitchScript('http://localhost:6006/iframe.html?id=button--primary');
  assert.match(source, /setCurrentStory/);
  assert.match(source, /button--primary/);
  assert.equal(scripts.storybookSwitchScript('http://localhost:6006/'), 'false');
});

test('the external-page settling gate recognizes Storybook render state', function () {
  var source = scripts.settleScript();
  assert.match(source, /sb-show-main/);
  assert.match(source, /sb-loader/);
  assert.match(source, /Storybook did not finish rendering before capture/);
  assert.match(source, /Promise\.race\(\[images/);
  assert.match(source, /MutationObserver/);
});

test('the warm-page settling gate skips the mutation quiet period', function () {
  var source = scripts.settleScript({ quiet: false, imageTimeout: 250 });
  assert.match(source, /sleep\(250\)/);
  assert.doesNotMatch(source, /MutationObserver/);
});
