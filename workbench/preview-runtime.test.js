const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function runtime(state = 'default', painted = true) {
  const handlers = new Map();
  const messages = [];
  const diagnostics = [];
  const canvas = { children: [], removed: false, remove() { this.removed = true; }, replaceChildren(...children) { this.children = children; } };
  const window = {
    addEventListener(name, handler) { handlers.set(name, handler); },
    removeEventListener(name, handler) { if (handlers.get(name) === handler) handlers.delete(name); },
    dispatchEvent() {},
  };
  const parent = { postMessage(message) { messages.push(message); } };
  const document = {
    createElement: () => canvas,
    body: { append() {} },
    documentElement: { dataset: {} },
    images: [], fonts: { ready: Promise.resolve() },
  };
  const source = fs.readFileSync(path.join(__dirname, 'preview/browser.js'), 'utf8');
  const boot = vm.runInNewContext(source.replace('export async function', 'async function') + '\nboot;', {
    window, parent, document, location: { href: 'http://127.0.0.1/preview?state=' + state, origin: 'http://127.0.0.1' },
    URL, AbortController, structuredClone,
    CustomEvent: class { constructor(name, options) { this.type = name; this.detail = options.detail; } },
    requestAnimationFrame: painted ? queueMicrotask : () => {},
    fetch: (_url, options) => { diagnostics.push(JSON.parse(options.body)); return Promise.resolve(); },
    // No images in these lifecycle tests; the settling-window branch never wins.
    setTimeout: (fn, ms) => ms === 100 ? setTimeout(fn, ms) : undefined,
    clearTimeout,
    clearInterval() {},
    setInterval() {},
  });
  return { boot, window, document, canvas, messages, diagnostics, handlers,
    async command(data) {
      handlers.get('message')({ source: parent, origin: 'http://127.0.0.1', data: { type: 'workbench-preview-command', ...data } });
      for (let i = 0; i < 8; i++) await new Promise(setImmediate);
    },
  };
}

test('a reusable renderer disposes its adapter, listeners and canvas exactly once', async () => {
  const r = runtime();
  let cleaned = 0;
  const mounted = await r.boot({ id: 'page' }, { default: 'page' }, { mount: () => () => cleaned++ }, {});
  await Promise.all([mounted.dispose(), mounted.dispose()]);
  assert.equal(cleaned, 1);
  assert.equal(r.handlers.size, 0);
  assert.equal(r.canvas.removed, true);
  assert.equal(r.window.workbench, null);
});

test('disposing a renderer cancels a pending font wait instead of blocking its next mount', async () => {
  const r = runtime();
  r.document.fonts.ready = new Promise(() => {});
  const booting = r.boot({ id: 'page' }, { default: 'page' }, { mount() {} }, {});
  await new Promise(setImmediate);
  await r.window.__workbenchStop();
  await booting;
  assert.equal(r.canvas.removed, true);
  assert.equal(r.handlers.size, 0);
  assert.equal(r.window.__workbenchReady, false);
});

test('a hidden iframe becomes ready even when the browser suspends animation frames', async () => {
  const r = runtime('default', false);
  let played = false;
  await r.boot({ id: 'page', play: () => { played = true; } }, { default: 'page' }, { mount() {} }, {});
  assert.equal(played, true);
  assert.equal(r.window.__workbenchReady, true);
});

test('offscreen lazy images do not hold preview readiness behind the image timeout', async () => {
  const r = runtime();
  r.window.innerWidth = 1512;
  r.window.innerHeight = 982;
  let observed = false;
  r.document.images = [{ loading: 'lazy', complete: false,
    getBoundingClientRect: () => ({ top: 2000, bottom: 2200, left: 0, right: 300 }),
    addEventListener() { observed = true; },
  }];
  await r.boot({ id: 'page', inputs: { secret: 'PRIVATE_INPUT' } }, { default: 'page' }, { mount() {} }, {}, { development: true });
  assert.equal(r.window.__workbenchReady, true);
  assert.equal(observed, false);
  assert.deepEqual(r.diagnostics.map(entry => entry.details.phase),
    ['started', 'setup.completed', 'mount.completed', 'fonts.waiting', 'images.waiting', 'ready']);
  const images = r.diagnostics.find(entry => entry.details.phase === 'images.waiting');
  assert.equal(images.details.deferredImages, 1);
  assert.equal(images.details.pendingImages, 0);
  assert.ok(r.diagnostics.at(-1).details.elapsedMs >= 0);
  assert.doesNotMatch(JSON.stringify(r.diagnostics), /PRIVATE_INPUT/);
});

test('visible lazy images still load before preview readiness', async () => {
  const r = runtime();
  r.window.innerWidth = 1512;
  r.window.innerHeight = 982;
  const handlers = {};
  r.document.images = [{ loading: 'lazy', complete: false,
    getBoundingClientRect: () => ({ top: 200, bottom: 400, left: 0, right: 300 }),
    addEventListener(name, handler) { handlers[name] = handler; },
  }];
  const booting = r.boot({ id: 'page' }, { default: 'page' }, { mount() {} }, {});
  await new Promise(setImmediate);
  assert.equal(r.window.__workbenchReady, false);
  handlers.load();
  await booting;
  assert.equal(r.window.__workbenchReady, true);
});

test('preview lifecycle merges state data, aborts and cleans up before resetting', async () => {
  const r = runtime('disabled');
  const order = [];
  const contexts = [];
  const preview = { id: 'button', inputs: { label: 'Continue', disabled: false }, fixtures: { root: true },
    setup: () => { order.push('preview'); return () => order.push('stop preview'); },
    states: { disabled: { inputs: { disabled: true }, fixtures: { state: true },
      setup: () => { order.push('state'); return () => order.push('stop state'); } } } };
  const adapter = { mount: (_canvas, source, context) => { assert.equal(source, 'component'); contexts.push(context); order.push('mount'); return () => order.push('stop mount'); } };
  await r.boot(preview, { default: 'component' }, adapter, { setup: () => { order.push('environment'); return () => order.push('stop environment'); } });
  assert.equal(r.window.__workbenchReady, true);
  assert.deepEqual(contexts[0].inputs, { label: 'Continue', disabled: true });
  assert.deepEqual(contexts[0].fixtures, { root: true, state: true });
  await r.command({ inputs: { label: 'Edited' } });
  assert.equal(contexts[0].signal.aborted, true);
  assert.deepEqual(order.slice(4, 8), ['stop mount', 'stop state', 'stop preview', 'stop environment']);
  assert.equal(contexts[1].inputs.label, 'Edited');
  await r.command({ command: 'reset' });
  assert.equal(contexts[2].inputs.label, 'Continue');
});

test('actions from initial play remain available when the host inspects the ready preview', async () => {
  const r = runtime();
  await r.boot({ id: 'button', states: { default: { play: context => context.action('clicked', 'Continue') } } },
    { default: 'component' }, { mount() {} }, {});
  await r.command({ command: 'inspect' });
  const ready = r.messages.at(-1);
  assert.equal(ready.event, 'ready');
  assert.deepEqual(JSON.parse(JSON.stringify(ready.actions)), [{ name: 'clicked', values: ['Continue'] }]);
  assert.equal(r.window.__workbenchReady, true);
});

test('missing state exports and render failures prevent capture readiness', async () => {
  const r = runtime('missing');
  let mounted = false;
  await r.boot({ id: 'button', states: { missing: {} } }, { default: 'component', missing: undefined },
    { mount() { mounted = true; } }, {});
  assert.equal(mounted, false);
  assert.match(r.window.__workbenchError, /selected source export does not exist/);
  assert.equal(r.window.__workbenchReady, false);
  const broken = runtime();
  await broken.boot({ id: 'button' }, { default: 'component' }, { mount() { throw new Error('Render failed'); } }, {});
  assert.match(broken.window.__workbenchError, /Render failed/);
  assert.equal(broken.messages.at(-1).event, 'error');
});

test('a failed cleanup still releases other resources and a subsequent reset recovers', async () => {
  const r = runtime();
  const stopped = [];
  let attempt = 0;
  await r.boot({ id: 'button', setup: () => () => stopped.push('setup') }, { default: 'component' },
    { mount() { attempt++; return () => { stopped.push('mount'); if (attempt === 1) throw new Error('Cleanup failed'); }; } }, {});
  await r.command({ command: 'reset' });
  assert.deepEqual(stopped, ['mount', 'setup']);
  assert.match(r.window.__workbenchError, /Cleanup failed/);
  await r.command({ command: 'reset' });
  assert.equal(r.window.__workbenchReady, true);
  assert.equal(attempt, 2);
});

test('host commands during initial setup wait rather than mounting two runtimes', async () => {
  const r = runtime();
  let release;
  const setup = new Promise(resolve => { release = resolve; });
  const inputs = [];
  let first = true;
  const started = r.boot({ id: 'button', inputs: { label: 'Initial' }, setup: () => { if (first) { first = false; return setup; } } },
    { default: 'component' }, { mount(_canvas, _source, context) { inputs.push(context.inputs.label); } }, {});
  await new Promise(setImmediate);
  await r.command({ inputs: { label: 'Edited' } });
  assert.deepEqual(inputs, []);
  release(); await started;
  await new Promise(setImmediate);
  assert.deepEqual(inputs, ['Initial', 'Edited']);
});

test('async adapter errors invalidate readiness, release resources and allow reset', async () => {
  const r = runtime();
  let context;
  let stopped = 0;
  await r.boot({ id: 'button' }, { default: 'component' }, { mount(_canvas, _source, next) { context = next; return () => stopped++; } }, {});
  context.error(new Error('Async render failed'));
  await new Promise(setImmediate);
  assert.equal(r.window.__workbenchReady, false);
  assert.match(r.window.__workbenchError, /Async render failed/);
  assert.equal(stopped, 1);
  await r.command({ command: 'reset' });
  assert.equal(r.window.__workbenchReady, true);
});
