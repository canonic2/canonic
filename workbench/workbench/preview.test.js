var assert = require('node:assert/strict');
var test = require('node:test');
var vm = require('node:vm');
var fs = require('node:fs');
var path = require('node:path');

// Exercise the shell's actual loading handlers with controlled load/paint
// ordering. No network or rendering timing determines which iframe wins.
function preview() {
  function node() {
    var attrs = {};
    return { classList: { add: function () {}, remove: function () {} },
      getAttribute: function (name) { return attrs[name]; },
      setAttribute: function (name, value) { attrs[name] = value; },
      removeAttribute: function (name) { delete attrs[name]; },
      remove: function () { this.removed = true; },
      addEventListener: function () {},
      appendChild: function () {},
      set src(value) { attrs.src = value; },
    };
  }
  var paints = []; var events = [];
  var state = { frame: node(), frameBuffer: node(), pendingFrame: null, artboardContent: node(), artboard: { hidden: true },
    reportRenderedStory: function () {}, revealExample: function () {}, view: null,
    document: { createElement: node },
    window: { requestAnimationFrame: function (fn) { paints.push(fn); }, dispatchEvent: function (event) {
      events.push({ frame: event.detail.frame, hidden: state.artboard.hidden });
    }, wbPreviewSessions: { create: function (options) {
      return require('./preview-sessions').create(Object.assign({}, options, { setTimeout: function () {}, clearTimeout: function () {} }));
    } } }, CustomEvent: function (_name, options) { this.detail = options.detail; },
  };
  var source = fs.readFileSync(path.join(__dirname, 'workbench.js'), 'utf8');
  state.URL = URL;
  state.location = { href: 'http://127.0.0.1/_workbench/', origin: 'http://127.0.0.1' };
  var handlers = source.slice(source.indexOf('  function warmFrame('), source.indexOf("  frame.addEventListener('load', frameLoaded)"));
  vm.runInNewContext(handlers, state);
  return { state: state, events: events, paint: function () { paints.splice(0).forEach(function (fn) { fn(); }); } };
}

function widthChoice(current) {
  var calls = [];
  var sizeButtons = [
    { dataset: { width: '1512' }, disabled: false },
    { dataset: { width: '393' }, disabled: false },
  ];
  var state = {
    current: current,
    sizeButtons: sizeButtons,
    setWidth: function (mode) { calls.push(['width', mode]); },
    syncHash: function () { calls.push(['sync']); },
  };
  var source = fs.readFileSync(path.join(__dirname, 'workbench.js'), 'utf8');
  var handler = source.slice(source.indexOf('  function chooseWidth('), source.indexOf('  /* ----------------------------------------------------- sidebar resize */'));
  vm.runInNewContext(handler, state);
  return { choose: state.chooseWidth, calls: calls, buttons: sizeButtons };
}

function widthAvailability(mode, viewports) {
  var buttons = ['fit', '1512', '393', 'resizable'].map(function (width) {
    return { dataset: { width: width }, disabled: false, title: width };
  });
  var selected = [];
  var state = {
    widths: buttons.map(function (button) { return button.dataset.width; }),
    sizeButtons: buttons,
    canvas: { dataset: { width: mode } },
    setWidth: function (width) { selected.push(width); state.canvas.dataset.width = width; },
    window: { wbManifest: { viewportWidths: function () {
      var map = { fit: 'fit', desktop: '1512', mobile: '393', responsive: 'resizable' };
      return viewports.map(function (viewport) { return map[viewport]; });
    } } },
  };
  var source = fs.readFileSync(path.join(__dirname, 'workbench.js'), 'utf8');
  var handler = source.slice(source.indexOf('  function updateWidthAvailability('), source.indexOf('  /* A size switcher change'));
  vm.runInNewContext(handler, state);
  state.updateWidthAvailability({ viewports: viewports });
  return { buttons: buttons, selected: selected };
}

function storyNavigation() {
  var loaded = [];
  var reported = [];
  var sent = [];
  var events = [];
  var timers = [];
  var lens = { key: 'storybook', kind: 'storybook', url: 'http://localhost:6006' };
  var item = { implementationOnly: 'storybook', implementations: { storybook: { title: 'Components/Button' } } };
  var frame = { contentWindow: { postMessage: function (data, origin) { sent.push([JSON.parse(data), origin]); } } };
  var frameBuffer = { contentWindow: {} };
  var state = {
    current: 'button', currentState: null, showSeq: 1, frameReady: true,
    pendingFrame: null, frame: frame, frameBuffer: frameBuffer,
    view: { lens: lens }, pageList: null, openLink: {},
    sessionKey: function () { return 'storybook'; },
    sessions: require('./preview-sessions').create({ dispose: function () {}, setTimeout: function () {}, clearTimeout: function () {} }),
    cancelPendingFrame: function () {}, frameLoaded: function () {}, blank: {},
    window: { wbLenses: {
      pick: function (list, picked) { return list.find(function (story) { return story.state === picked; }); },
      storyUrl: function (_lens, id) { return lens.url + '/iframe.html?id=' + id + '&viewMode=story'; },
      storyOpenUrl: function (_lens, id) { return lens.url + '/?path=/story/' + id; },
      upstream: function (_lens, address) { return address; },
      selectStory: function (target, implementation, id) {
        assert.equal(target, frame);
        sent.push([{ event: { type: 'setCurrentStory', args: [{ storyId: id }] } }, implementation.url]);
        return true;
      },
      storybookEvent: function (message, target, implementation) {
        if (message.source !== target.contentWindow || message.origin !== implementation.url) return null;
        return message.data;
      },
    }, wbSimulator: { hide: function () {} }, dispatchEvent: function (event) { events.push(event); } },
    CustomEvent: function (type, options) { this.type = type; this.detail = options.detail; },
    stories: function () { return Promise.resolve([
      { id: 'components-button--default', state: 'default', name: 'Default' },
      { id: 'components-button--secondary', state: 'secondary', name: 'Secondary' },
    ]); },
    setTimeout: function (fn) { var timer = { fn: fn, cleared: false }; timers.push(timer); return timer; },
    clearTimeout: function (timer) { timer.cleared = true; },
    showFrame: function (url) { loaded.push(url); },
    loadPreview: function (url) { loaded.push(url); },
    tellHost: function (_src, story) { reported.push(story); },
    codeFor: function () { return []; }, drawStories: function () {},
    setTitle: function () {}, syncHash: function () {}, say: function () {},
  };
  var source = fs.readFileSync(path.join(__dirname, 'workbench.js'), 'utf8');
  vm.runInNewContext(source.slice(source.indexOf('  var pendingStorySwitch = null;'), source.indexOf('  /* Where the code is,')), state);
  var handler = source.slice(source.indexOf('  function loadStory('), source.indexOf('  /* ------------------------------------------------------------ source */'));
  vm.runInNewContext(handler, state);
  return {
    load: function (story, previous) { state.showSeq += 1; state.loadStory(item, lens, story, state.showSeq, previous ? { lens: lens } : null); },
    acknowledge: function (type, id) { state.storySwitchEvent({ source: frame.contentWindow, origin: lens.url,
      data: { type: type, id: id } }); },
    acknowledgePending: function (type, id) { state.storySwitchEvent({ source: frameBuffer.contentWindow,
      origin: lens.url, data: { type: type, id: id } }); },
    timeout: function () { timers.filter(function (timer) { return !timer.cleared; }).at(-1).fn(); },
    loaded: loaded, sent: sent, events: events, reported: reported, state: state,
  };
}

test('a spare iframe that wins the initial load reveals the preview before capture preparation', function () {
  var p = preview(); var state = p.state;
  state.loadPreview('/initial');
  state.loadPreview('/resized');
  var winner = state.frameBuffer;
  state.frameLoaded({ currentTarget: winner });
  p.paint();
  assert.equal(state.frame, winner);
  assert.equal(state.artboard.hidden, false);
  assert.equal(p.events[0].frame, winner);
  assert.equal(p.events[0].hidden, false);
});

test('a superseded pending frame cannot replace the active preview during paint', function () {
  var p = preview(); var state = p.state;
  state.loadPreview('/initial'); state.loadPreview('/old');
  var initial = state.frame;
  state.frameLoaded({ currentTarget: state.frameBuffer });
  state.pendingFrame = null;
  p.paint();
  assert.equal(state.frame, initial); assert.equal(p.events.length, 0);
});

test('managed previews retain the outgoing mounted session and resume it without mounting again', async function () {
  var p = preview(); var state = p.state;
  var loaded = []; var resets = 0;
  state.frameReady = true;
  state.frame.setAttribute('src', '/_workbench/preview-host.html');
  state.frame.wbWarmHost = true;
  var resumed = [];
  var oldUrl = 'http://127.0.0.1/first.workbench.ts';
  state.frame.contentWindow = { wbPreviewHost: { reset: function () { resets++; return Promise.resolve(); },
    resume: function (url) { resumed.push(url); return Promise.resolve(); } } };
  state.frame.wbSessionKey = state.sessionKey(oldUrl);
  state.sessions.add(state.frame.wbSessionKey, state.frame);
  state.sessions.activate(state.frame);
  state.frameBuffer.setAttribute('src', '/_workbench/preview-host.html');
  state.frameBuffer.wbWarmHost = true;
  state.frameBuffer.contentWindow = { wbPreviewHost: { load: function (url) { loaded.push(url); return Promise.resolve(); } } };
  var previous = state.frame;
  state.loadPreview('http://127.0.0.1/button.workbench.ts');
  await new Promise(setImmediate);
  p.paint();
  assert.deepEqual(loaded, ['http://127.0.0.1/button.workbench.ts']);
  assert.equal(state.frameBuffer, previous);
  assert.equal(state.frameBuffer.getAttribute('src'), '/_workbench/preview-host.html');
  assert.equal(resets, 0);
  state.loadPreview(oldUrl);
  await new Promise(setImmediate);
  p.paint();
  assert.equal(state.frame, previous);
  assert.deepEqual(resumed, [oldUrl]);
  assert.equal(resets, 0);
});

test('changing width resizes the current iframe without routing or reloading it', function () {
  var choice = widthChoice('preview/components-button.html');
  choice.choose('1512');
  assert.deepEqual(choice.calls, [['width', '1512'], ['sync']]);
});

test('URL sessions retain their iframe across other pages, while Reload replaces the document', async function () {
  var p = preview(); var state = p.state;
  state.loadPreview('/page-a'); state.frameLoaded({ currentTarget: state.frame }); p.paint();
  var a = state.frame; a.contentWindow = { edited: 'kept' };
  state.loadPreview('/page-b'); state.frameLoaded({ currentTarget: state.pendingFrame }); p.paint();
  assert.equal(a.getAttribute('src'), '/page-a');
  state.loadPreview('/page-a'); await new Promise(setImmediate); p.paint();
  assert.equal(state.frame, a); assert.equal(state.frame.contentWindow.edited, 'kept');
  state.loadPreview('/page-a', true);
  assert.notEqual(state.pendingFrame, a);
  assert.equal(a.removed, undefined, 'the old document stays visible while Reload prepares its replacement');
  state.frameLoaded({ currentTarget: state.pendingFrame }); p.paint();
  assert.equal(a.removed, true);
});

test('changing width before a page is selected does not write an address', function () {
  var choice = widthChoice(null);
  choice.choose('393');
  assert.deepEqual(choice.calls, [['width', '393']]);
});

test('an unsupported width cannot resize the current page', function () {
  var choice = widthChoice('preview/components-button.html');
  choice.buttons[0].disabled = true;
  choice.choose('1512');
  assert.deepEqual(choice.calls, []);
});

test('a page disables unsupported sizes and falls back to its first supported one', function () {
  var result = widthAvailability('fit', ['desktop', 'responsive']);
  assert.deepEqual(result.buttons.map(function (button) { return button.disabled; }), [true, false, true, false]);
  assert.deepEqual(result.selected, ['1512']);
});

test('switches stories in the loaded Storybook preview without navigating', async function () {
  var stories = storyNavigation();
  stories.load('default', false);
  await Promise.resolve();
  assert.deepEqual(stories.reported, [], 'catalog lookup does not acknowledge an unrendered story');
  stories.acknowledge('storyRendered', 'components-button--default');
  stories.load('secondary', true);
  await Promise.resolve();
  assert.deepEqual(stories.loaded, [
    'http://localhost:6006/iframe.html?id=components-button--default&viewMode=story',
  ]);
  assert.equal(stories.sent[0][0].event.args[0].storyId, 'components-button--secondary');
  stories.acknowledge('currentStoryWasSet', 'components-button--secondary');
  assert.deepEqual(stories.reported, ['default'], 'accepting a switch does not settle the new story');
  stories.acknowledge('storyRendered', 'components-button--secondary');
  assert.equal(stories.events[0].type, 'wb-frame-change');
  assert.equal(stories.loaded.length, 1);
  assert.deepEqual(stories.reported, ['default', 'secondary']);
});

test('navigates only if Storybook does not acknowledge the switch', async function () {
  var stories = storyNavigation();
  stories.load('secondary', true);
  await Promise.resolve();
  stories.acknowledge('storyRendered', 'components-button--default');
  stories.timeout();
  assert.deepEqual(stories.loaded, [
    'http://localhost:6006/iframe.html?id=components-button--secondary&viewMode=story',
  ]);
});

test('reports a fallback story only after its spare frame becomes visible', async function () {
  var stories = storyNavigation();
  stories.load('secondary', true);
  await Promise.resolve();
  stories.timeout();
  stories.state.pendingFrame = stories.state.frameBuffer;
  stories.acknowledgePending('storyRendered', 'components-button--secondary');
  assert.deepEqual(stories.reported, []);
  stories.state.frame = stories.state.frameBuffer;
  stories.state.pendingFrame = null;
  stories.state.reportRenderedStory();
  assert.deepEqual(stories.reported, ['secondary']);
});

function lensSwitcher(item) {
  var source = fs.readFileSync(path.join(__dirname, 'workbench.js'), 'utf8');
  var start = source.indexOf('  function lensesOf(');
  var end = source.indexOf('  /* Through a lens the page is served by somebody else');
  var box = { hidden: false, innerHTML: '', children: [], appendChild: function (el) { this.children.push(el); } };
  var context = {
    config: {
      implementations: {
        storybook: { key: 'storybook', label: 'Storybook' },
        dev: { key: 'dev', label: 'Dev' },
      },
    },
    lensesBox: box,
    document: {
      createElement: function () {
        return { dataset: {}, setAttribute: function () {}, addEventListener: function () {} };
      },
    },
  };
  vm.runInNewContext(source.slice(start, end) + '\ndrawLenses(item, null);', Object.assign(context, { item: item }));
  return { hidden: box.hidden, labels: box.children.map(function (b) { return b.textContent; }) };
}

test('opening on no page still tells the host it is ready', function () {
  /* The editor holds sidebar picks until the first wb-here. A throw on the
     empty route meant it never came, and no page could be picked. */
  var source = fs.readFileSync(path.join(__dirname, 'workbench.js'), 'utf8');
  var start = source.indexOf('  function show(');
  var end = source.indexOf('  /* ----------------------------------------------------------- actions */');
  var told = [];
  function noop() {}
  var el = function () { return { hidden: false, textContent: '', removeAttribute: noop }; };
  var context = {
    index: {}, showSeq: 0, view: null, current: 'old.html', currentState: null, renderedStory: null,
    pageList: { setCurrent: noop },
    cancelStorySwitch: noop, effectiveLens: function () { return null; }, stateOf: noop,
    cancelPendingFrame: noop, parkPreview: noop, setCanvasMode: noop,
    updateWidthAvailability: noop, drawLenses: noop, drawStates: noop, drawStateMenu: noop,
    drawSources: noop, setActionsAvailability: noop,
    tellHost: function (src, state, lens) { told.push([src, state, lens]); },
    window: { wbSimulator: { hide: noop } },
    crumb: el(), crumbPage: el(), artboard: el(), frame: el(), frameBuffer: el(),
    blank: el(), openLink: el(), artboardName: el(), document: {}, config: { name: 'Acme' },
    BLANK_TEXT: '',
  };
  vm.runInNewContext(source.slice(start, end) + '\nshow(null, null);', context);
  assert.deepEqual(told, [[null, null, null]]);
  assert.equal(context.blank.hidden, false);
});

test('the lens switcher only appears when there are two lenses to switch between', function () {
  assert.deepEqual(lensSwitcher({ src: 'a.html' }), { hidden: true, labels: [] });
  assert.deepEqual(
    lensSwitcher({ implementationOnly: 'storybook', implementations: { storybook: { title: 'Button' } } }),
    { hidden: true, labels: [] }
  );
  assert.deepEqual(
    lensSwitcher({ src: 'a.html', implementations: { dev: '/a' } }),
    { hidden: false, labels: ['Design', 'Dev'] }
  );
  assert.deepEqual(
    lensSwitcher({ implementationOnly: 'storybook', implementations: { storybook: { title: 'Button' }, dev: '/b' } }),
    { hidden: false, labels: ['Storybook', 'Dev'] }
  );
});
