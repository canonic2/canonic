const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');

function harness(initialize = true) {
  const timers = new Map();
  const paints = [];
  const changes = [];
  const failures = [];
  let nextTimer = 0;
  const frames = [];
  function frame() {
    const element = new EventTarget();
    const attrs = new Map();
    const classes = new Set();
    Object.assign(element, {
      style: {},
      contentWindow: new EventTarget(),
      classList: { add: name => classes.add(name), remove: name => classes.delete(name) },
      getAttribute: name => attrs.get(name),
      setAttribute: (name, value) => attrs.set(name, value),
      removeAttribute: name => attrs.delete(name),
      remove() { element.removed = true; },
    });
    element.contentWindow.postMessage = () => {};
    Object.defineProperty(element, 'src', { set: value => attrs.set('src', value) });
    frames.push(element);
    return element;
  }
  const window = {
    wbPreviewSessions: require('./preview-sessions.js'),
    requestAnimationFrame: callback => paints.push(callback),
    setTimeout: callback => { timers.set(++nextTimer, callback); return nextTimer; },
    clearTimeout: id => timers.delete(id),
    dispatchEvent: event => changes.push(event.detail.frame),
  };
  const context = {
    window, document: { createElement: frame }, URL,
    location: { href: 'http://example.com/_workbench/', origin: 'http://example.com' },
    CustomEvent: class { constructor(type, options) { this.type = type; this.detail = options.detail; } },
    console,
    setTimeout: window.setTimeout, clearTimeout: window.clearTimeout,
  };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'lenses.js'), 'utf8'), context);
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'content-frames.js'), 'utf8'), context);
  const initial = frame();
  const runtime = window.wbContentFrames.create({
    frame: initial, buffer: frame(),
    viewport: { classList: { add() {}, remove() {} }, appendChild() {} },
    surface: { hidden: false }, changed() {}, reveal() {},
    failed: error => failures.push(error.message),
  });
  function show(url, lens, force) { runtime.show({ url, lens }, force); }
  function paint() { paints.splice(0).forEach(callback => callback()); }
  function poll() {
    const entry = timers.entries().next().value;
    assert.ok(entry, 'a fallback poll is pending');
    timers.delete(entry[0]);
    entry[1]();
  }
  if (initialize) {
  show('/first.md');
  initial.contentWindow.__workbenchDocs = {};
  initial.contentWindow.__workbenchReady = false;
  initial.dispatchEvent(new Event('load'));
  }
  return { runtime, initial, show, paint, poll, timers, paints, changes, failures, frames };
}

test('docs readiness promotes the loaded document without waiting for a poll', () => {
  const h = harness();
  h.paint();
  assert.equal(h.runtime.ready(), false, 'examples and fonts still block readiness');
  assert.equal(h.timers.size, 1);
  h.initial.contentWindow.__workbenchReady = true;
  h.initial.contentWindow.dispatchEvent(new Event('workbench:ready'));
  assert.equal(h.timers.size, 0, 'the obsolete poll is cancelled');
  h.paint();
  assert.equal(h.runtime.ready(), true);
  assert.deepEqual(h.changes, [h.initial]);
  h.initial.contentWindow.dispatchEvent(new Event('workbench:ready'));
  h.paint();
  assert.equal(h.changes.length, 1, 'the readiness listener was removed');
  h.runtime.close();
});

test('a readiness event cannot bypass the document readiness flag', () => {
  const h = harness();
  h.initial.contentWindow.dispatchEvent(new Event('workbench:ready'));
  h.paint();
  assert.equal(h.runtime.ready(), false);
  assert.equal(h.timers.size, 1);
  h.runtime.close();
  assert.equal(h.timers.size, 0);
});

test('renderers that only set the readiness flag keep the polling fallback', () => {
  const h = harness();
  h.initial.contentWindow.__workbenchReady = true;
  h.poll();
  h.paint();
  assert.equal(h.runtime.ready(), true);
  assert.equal(h.timers.size, 0);
  h.runtime.close();
});

test('superseding an unfinished document removes its readiness listener and timer', () => {
  const h = harness();
  const oldWindow = h.initial.contentWindow;
  h.show('/replacement.md');
  assert.equal(h.timers.size, 0);
  oldWindow.__workbenchReady = true;
  oldWindow.dispatchEvent(new Event('workbench:ready'));
  h.paint();
  assert.equal(h.runtime.ready(), false);
  assert.deepEqual(h.changes, []);
  h.runtime.close();
});

test('closing an unfinished renderer releases its readiness listener and timer', () => {
  const h = harness();
  h.runtime.close();
  assert.equal(h.timers.size, 0);
  h.initial.contentWindow.__workbenchReady = true;
  h.initial.contentWindow.dispatchEvent(new Event('workbench:ready'));
  assert.equal(h.paints.length, 0);
  assert.deepEqual(h.failures, []);
});

test('Storybook loading and acknowledgements cannot replace a matching render event', () => {
  const h = harness(false);
  const lens = { key: 'storybook', kind: 'storybook', url: 'http://storybook.example.com' };
  function show(id) {
    h.runtime.show({ lens, story: { id }, url: lens.url + '/iframe.html?id=' + id + '&viewMode=story' });
  }
  function event(type, id, overrides = {}) {
    h.runtime.storyEvent({ source: h.initial.contentWindow, origin: lens.url,
      data: JSON.stringify({ key: 'storybook-channel', event: { type, args: [type === 'currentStoryWasSet' ? { storyId: id } : id] } }), ...overrides });
  }
  try {
    show('page--default');
    h.initial.dispatchEvent(new Event('load'));
    h.paint();
    assert.equal(h.runtime.ready(), false, 'document loading alone is not story readiness');
    event('currentStoryWasSet', 'page--default');
    assert.equal(h.runtime.ready(), false);
    event('storyRendered', 'page--default');
    assert.equal(h.runtime.ready(), true);
    show('page--secondary');
    event('currentStoryWasSet', 'page--secondary');
    h.paint();
    assert.equal(h.runtime.ready(), false, 'a warm channel acknowledgement is not a render');
    event('storyRendered', 'page--default');
    event('storyRendered', 'page--secondary', { origin: 'http://other.example.com' });
    event('storyRendered', 'page--secondary', { source: {} });
    assert.equal(h.runtime.ready(), false, 'stale IDs and unauthenticated renders cannot settle the switch');
    event('storyRendered', 'page--secondary');
    assert.equal(h.runtime.ready(), true);
    assert.equal(h.runtime.frame(), h.initial, 'switching retains the loaded Storybook iframe');
  } finally { h.runtime.close(); }
});

test('a warm iteration cannot inherit readiness from the previous mount or a superseded load', async () => {
  const h = harness(false);
  const loads = [];
  const content = h.initial.contentWindow;
  content.wbPreviewHost = {
    load: () => new Promise(resolve => loads.push(resolve)),
    reset: async () => {},
  };
  content.__workbenchOptions = {};
  content.__workbenchReady = true;
  try {
    h.show('/component.workbench.ts?iteration=first');
    h.initial.dispatchEvent(new Event('load'));
    loads.shift()();
    await new Promise(resolve => setImmediate(resolve));
    h.paint();
    assert.equal(h.runtime.ready(), true);
    h.show('/component.workbench.ts?iteration=second');
    assert.equal(h.runtime.ready(), false, 'a requested URL is not evidence of a completed warm mount');
    const second = loads.shift();
    h.show('/component.workbench.ts?iteration=latest');
    second();
    await new Promise(resolve => setImmediate(resolve));
    h.paint();
    assert.equal(h.runtime.ready(), false, 'a superseded load cannot complete the latest mount');
    loads.shift()();
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.runtime.ready(), false, 'promotion still waits for the owning paint');
    h.paint();
    assert.equal(h.runtime.ready(), true);
    assert.equal(h.runtime.frame(), h.initial);
  } finally { h.runtime.close(); }
});

async function docs(h, name, lens = 'web', force = false) {
  const url = '/' + name + '.md?lens=' + lens;
  h.show(url, { kind: 'docs', key: lens }, force);
  const target = h.frames.findLast(frame => !frame.removed && frame.wbRequestedUrl === url);
  assert.ok(target);
  if (!target.contentWindow.wbDocsHost) {
    const content = target.contentWindow;
    content.loads = [];
    content.loadSizes = [];
    content.resumes = [];
    content.resets = 0;
    content.wbDocsHost = {
      load: async address => {
        content.loads.push(address);
        content.loadSizes.push({ width: target.style.width, height: target.style.height });
        content.__workbenchDocs = {};
        content.__workbenchReady = true;
      },
      resume: async address => { content.resumes.push(address); },
      reset: async () => { content.resets++; },
    };
    target.dispatchEvent(new Event('load'));
  }
  await new Promise(resolve => setImmediate(resolve));
  h.paint();
  assert.equal(h.runtime.ready(), true);
  return target;
}

test('Docs retains four documents, resumes exact pages, and reuses the oldest idle same-lens host', async () => {
  const h = harness(false);
  try {
    const one = await docs(h, 'one');
    const two = await docs(h, 'two');
    const three = await docs(h, 'three');
    const four = await docs(h, 'four');
    assert.equal(new Set([one, two, three, four]).size, 4);
    assert.match(one.getAttribute('src'), /app\/docs\/docs-host.html\?lens=web$/);
    assert.equal(await docs(h, 'two'), two);
    assert.equal(two.contentWindow.loads.length, 1);
    assert.equal(two.contentWindow.resumes.length, 1);
    one.style.width = '300px'; one.style.height = '400px';
    const five = await docs(h, 'five');
    assert.equal(five, one);
    assert.equal(one.contentWindow.loads.length, 2);
    assert.deepEqual(one.contentWindow.loadSizes.at(-1), { width: '', height: '' }, 'mount uses the current viewport instead of frozen idle dimensions');
    assert.equal(one.contentWindow.resets, 0, 'reuse keeps the document runtime');
    assert.equal(h.frames.filter(frame => frame.wbWarmKind === 'docs:web' && !frame.removed).length, 4);
    assert.deepEqual(h.failures, []);
  } finally { h.runtime.close(); }
  await new Promise(resolve => setImmediate(resolve));
  assert.ok(h.frames.filter(frame => frame.wbWarmKind).every(frame => frame.contentWindow.resets === 1));
});

test('Docs never reuses an active or different-lens document, and Reload disposes the retained host', async () => {
  const h = harness(false);
  try {
    const first = await docs(h, 'one', 'web');
    await docs(h, 'two', 'web');
    await docs(h, 'three', 'web');
    const active = await docs(h, 'four', 'web');
    const native = await docs(h, 'five', 'native');
    assert.notEqual(native, active);
    assert.equal(native.wbWarmKind, 'docs:native');
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(first.removed, true, 'the shared retention limit still evicts oldest idle');
    const fresh = await docs(h, 'five', 'native', true);
    assert.notEqual(fresh, native);
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(native.contentWindow.resets, 1);
    assert.equal(native.removed, true);
  } finally { h.runtime.close(); }
});
