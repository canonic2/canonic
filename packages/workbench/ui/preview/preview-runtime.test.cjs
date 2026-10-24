const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function runtime(scenario = 'default', painted = true, actions = null, trace = false) {
  const handlers = new Map();
  const documentHandlers = new Map();
  const messages = [];
  const diagnostics = [];
  const canvas = { children: [], removed: false, remove() { this.removed = true; }, replaceChildren(...children) { this.children = children; } };
  const window = {
    addEventListener(name, handler) { handlers.set(name, handler); },
    removeEventListener(name, handler) { if (handlers.get(name) === handler) handlers.delete(name); },
    dispatchEvent() {},
    wbPreviewActions: actions,
  };
  const parent = { postMessage(message) { messages.push(message); } };
  const document = {
    createElement: () => canvas,
    body: { append() {} },
    documentElement: { dataset: {} },
    baseURI: 'http://127.0.0.1/previews/page.workbench.ts?scenario=' + scenario,
    addEventListener(name, handler) { documentHandlers.set(name, handler); },
    removeEventListener(name, handler) { if (documentHandlers.get(name) === handler) documentHandlers.delete(name); },
    images: [], fonts: { ready: Promise.resolve() },
  };
  const source = fs.readFileSync(path.join(__dirname, "./browser.js"), 'utf8');
  const location = { href: 'http://127.0.0.1/preview?scenario=' + scenario + (trace ? '&wbTrace=acme-selection' : ''), pathname: '/preview', origin: 'http://127.0.0.1' };
  const boot = vm.runInNewContext(source.replace(/^export /gm, '') + '\nboot;', {
    window, parent, document, location,
    URL, AbortController, structuredClone, FormData: class { constructor(form) { return Object.entries(form.fields); } },
    CustomEvent: class { constructor(name, options) { this.type = name; this.detail = options.detail; } },
    requestAnimationFrame: painted ? queueMicrotask : () => {},
    fetch: (_url, options) => { diagnostics.push(JSON.parse(options.body)); return Promise.resolve(); },
    // No images in these lifecycle tests; the settling-window branch never wins.
    setTimeout: (fn, ms) => ms === 100 ? setTimeout(fn, ms) : undefined,
    clearTimeout,
    clearInterval() {},
    setInterval() {},
  });
  return { boot, window, document, location, canvas, messages, diagnostics, handlers, documentHandlers,
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

test('scenario artboard inputs apply over the scenario and survive control reset', async () => {
  const r = runtime('default');
  let context;
  await r.boot({ id: 'feedback', inputs: { tone: 'info', title: 'Base' }, scenarios: { default: { inputs: { tone: 'neutral' },
    artboards: { success: { width: 560, height: 119, inputs: { tone: 'success' } } } } },
  }, { default: 'component' }, { mount(_canvas, _source, value) { context = value; } }, {}, { artboard: 'success' });
  assert.equal(context.scenario, 'default');
  assert.equal(context.artboard, 'success');
  assert.equal('iteration' in context, false);
  assert.equal(context.inputs.tone, 'success');
  assert.equal(r.document.documentElement.dataset.wbIteration, undefined);
  await r.command({ inputs: { title: 'Edited' } });
  assert.equal(context.inputs.title, 'Edited');
  await r.command({ command: 'reset' });
  assert.equal(context.inputs.title, 'Base');
  assert.equal(context.inputs.tone, 'success');
  await r.window.__workbenchStop();
});

test('an iteration of a declared artboard merges after it and uses its own source', async () => {
  const r = runtime('default');
  r.location.href += '&artboard=warning&iteration=bold';
  const order = [];
  const activations = [];
  r.window.__workbenchRequests = { activate: levels => activations.push(levels), deactivate() {} };
  let mounted;
  const preview = { id: 'feedback', inputs: { tone: 'info', title: 'Base' }, fixtures: { user: 'Ada' }, globals: { theme: 'light' },
    requests: { '/api/items': { body: ['preview'] } },
    setup: () => { order.push('preview setup'); },
    scenarios: { default: { fixtures: { plan: 'free' }, requests: { '/api/items': { body: ['scenario'] } },
      setup: () => { order.push('scenario setup'); }, play: () => { order.push('scenario play'); }, ready: () => { order.push('scenario ready'); },
      artboards: { warning: { width: 480, height: 320, inputs: { tone: 'warning' },
        iterations: { bold: { inputs: { title: 'Bold' }, fixtures: { plan: 'pro' }, globals: { theme: 'dark' },
          requests: { '/api/items': { body: ['iteration'] } },
          setup: () => { order.push('iteration setup'); }, play: () => { order.push('iteration play'); }, ready: () => { order.push('iteration ready'); } } } } } } } };
  await r.boot(preview, { default: 'component', 'default+warning/bold': 'bold component' },
    { mount(_canvas, source, context) { mounted = { source, context }; } }, {});
  assert.equal(r.window.__workbenchReady, true);
  assert.equal(mounted.source, 'bold component');
  assert.deepEqual([mounted.context.scenario, mounted.context.artboard, mounted.context.iteration], ['default', 'warning', 'bold']);
  assert.deepEqual(JSON.parse(JSON.stringify(mounted.context.inputs)), { tone: 'warning', title: 'Bold' });
  assert.deepEqual(JSON.parse(JSON.stringify(mounted.context.fixtures)), { user: 'Ada', plan: 'pro' });
  assert.deepEqual(JSON.parse(JSON.stringify(mounted.context.globals)), { theme: 'dark' });
  assert.deepEqual(JSON.parse(JSON.stringify(activations[0])).map(level => level['/api/items'].body[0]), ['iteration', 'scenario', 'preview']);
  assert.deepEqual(order, ['preview setup', 'scenario setup', 'iteration setup', 'scenario play', 'iteration play', 'scenario ready', 'iteration ready']);
  assert.equal(r.document.documentElement.dataset.wbIteration, 'bold');
  const ready = r.messages.findLast(message => message.event === 'ready');
  assert.deepEqual([ready.scenario, ready.iteration], ['default', 'bold']);
  mounted.context.action('clicked');
  assert.equal(r.messages.at(-1).iteration, 'bold');
  await r.window.__workbenchStop();
});

test('an iteration of the implicit artboard falls back to the scenario source', async () => {
  const r = runtime('empty');
  let mounted;
  await r.boot({ id: 'jobs', scenarios: { default: {}, empty: { iterations: { illustrated: { inputs: { art: true } }, plain: {} } } } },
    { default: 'component', empty: 'empty component', 'empty+illustrated': 'illustrated component' },
    { mount(_canvas, source, context) { mounted = { source, context }; } }, {}, { iteration: 'plain' });
  assert.equal(mounted.source, 'empty component');
  assert.equal(mounted.context.iteration, 'plain');
  assert.equal('artboard' in mounted.context, false);
  const illustrated = runtime('empty');
  await illustrated.boot({ id: 'jobs', scenarios: { empty: { iterations: { illustrated: { inputs: { art: true } } } } } },
    { default: 'component', 'empty+illustrated': 'illustrated component' },
    { mount(_canvas, source, context) { mounted = { source, context }; } }, {}, { iteration: 'illustrated' });
  assert.equal(mounted.source, 'illustrated component');
  assert.equal(mounted.context.inputs.art, true);
});

test('a declared artboard looks up its iteration, then its own source, then the scenario and default', async () => {
  const preview = { id: 'feedback', scenarios: { cards: { artboards: { warning: { width: 480, height: 320, inputs: { tone: 'warning' },
    iterations: { bold: { inputs: { title: 'Bold' } }, calm: {} } } } } } };
  const mountedWith = async (sources, options) => {
    const r = runtime('cards');
    let mounted;
    await r.boot(preview, sources, { mount(_canvas, source) { mounted = source; } }, {}, options);
    await r.window.__workbenchStop();
    return mounted;
  };
  const all = { default: 'default', cards: 'cards', 'cards+warning/': 'warning', 'cards+warning/bold': 'bold' };
  assert.equal(await mountedWith(all, { artboard: 'warning', iteration: 'bold' }), 'bold');
  assert.equal(await mountedWith(all, { artboard: 'warning', iteration: 'calm' }), 'warning');
  assert.equal(await mountedWith(all, { artboard: 'warning' }), 'warning');
  assert.equal(await mountedWith({ default: 'default', cards: 'cards' }, { artboard: 'warning', iteration: 'calm' }), 'cards');
  assert.equal(await mountedWith({ default: 'default' }, { artboard: 'warning' }), 'default');
  // The implicit artboard's iteration key is never mistaken for a declared artboard's.
  assert.equal(await mountedWith({ default: 'default', cards: 'cards', 'cards+bold': 'implicit' }, { artboard: 'warning', iteration: 'bold' }), 'cards');
});

test('unknown artboards and iterations render errors, also after a scenario switch', async () => {
  const unknownArtboard = runtime();
  await unknownArtboard.boot({ id: 'feedback', scenarios: { default: {} } }, { default: 'component' }, { mount() {} }, {}, { artboard: 'missing' });
  assert.match(unknownArtboard.window.__workbenchError, /Unknown preview artboard: missing/);
  assert.equal(unknownArtboard.messages.at(-1).event, 'error');
  const r = runtime('default');
  let mounts = 0;
  await r.boot({ id: 'feedback', scenarios: {
    default: { artboards: { warning: { width: 480, height: 320, iterations: { bold: {} } } } },
    empty: { iterations: { bold: {} } },
  } }, { default: 'component' }, { mount() { mounts++; } }, {}, { artboard: 'warning', iteration: 'bold' });
  assert.equal(r.window.__workbenchReady, true);
  // The empty scenario has no declared artboards, so this instance's artboard is unknown there.
  await r.command({ command: 'scenario', scenario: 'empty' });
  assert.match(r.window.__workbenchError, /Unknown preview artboard: warning/);
  assert.deepEqual([r.messages.at(-1).event, r.messages.at(-1).scenario, r.messages.at(-1).iteration], ['error', 'empty', 'bold']);
  await r.command({ command: 'scenario', scenario: 'default' });
  assert.equal(r.window.__workbenchReady, true);
  assert.equal(mounts, 2);
  const unknownIteration = runtime();
  await unknownIteration.boot({ id: 'jobs', scenarios: { default: { iterations: { bold: {} } } } }, { default: 'component' }, { mount() {} }, {}, { iteration: 'calm' });
  assert.match(unknownIteration.window.__workbenchError, /Unknown preview iteration: calm/);
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
  const r = runtime('default', true, null, true);
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
    ['started', 'setup.completed', 'mount.completed', 'hooks.completed', 'fonts.completed', 'images.completed', 'ready']);
  // Each phase's time is on the event that ends it.
  const images = r.diagnostics.find(entry => entry.details.phase === 'images.completed');
  assert.equal(images.details.deferredImages, 1);
  assert.equal(images.details.pendingImages, 0);
  assert.ok(r.diagnostics.at(-1).details.elapsedMs >= 0);
  assert.doesNotMatch(JSON.stringify(r.diagnostics), /PRIVATE_INPUT/);
  assert.ok(r.diagnostics.every(entry => entry.details.selection === 'acme-selection'));
});

test('development previews do not log browser phases with trace disabled', async () => {
  const r = runtime();
  await r.boot({ id: 'page' }, { default: 'page' }, { mount() {} }, {}, { development: true });
  assert.equal(r.window.__workbenchReady, true);
  assert.deepEqual(r.diagnostics, []);
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

test('preview lifecycle merges scenario data, aborts and cleans up before resetting', async () => {
  const r = runtime('disabled');
  const order = [];
  const contexts = [];
  const preview = { id: 'button', inputs: { label: 'Continue', disabled: false }, fixtures: { root: true },
    setup: () => { order.push('preview'); return () => order.push('stop preview'); },
    scenarios: { disabled: { inputs: { disabled: true }, fixtures: { scenario: true },
      setup: () => { order.push('scenario'); return () => order.push('stop scenario'); } } } };
  const adapter = { mount: (_canvas, source, context) => { assert.equal(source, 'component'); contexts.push(context); order.push('mount'); return () => order.push('stop mount'); } };
  await r.boot(preview, { default: 'component' }, adapter, { setup: () => { order.push('environment'); return () => order.push('stop environment'); } });
  assert.equal(r.window.__workbenchReady, true);
  assert.deepEqual(contexts[0].inputs, { label: 'Continue', disabled: true });
  assert.deepEqual(contexts[0].fixtures, { root: true, scenario: true });
  await r.command({ inputs: { label: 'Edited' } });
  assert.equal(contexts[0].signal.aborted, true);
  assert.deepEqual(order.slice(4, 8), ['stop mount', 'stop scenario', 'stop preview', 'stop environment']);
  assert.equal(contexts[1].inputs.label, 'Edited');
  await r.command({ command: 'reset' });
  assert.equal(contexts[2].inputs.label, 'Continue');
});

test('actions from initial play remain available when the host inspects the ready preview', async () => {
  const r = runtime();
  await r.boot({ id: 'button', scenarios: { default: { play: context => context.action('clicked', 'Continue') } } },
    { default: 'component' }, { mount() {} }, {});
  await r.command({ command: 'inspect' });
  const ready = r.messages.at(-1);
  assert.equal(ready.event, 'ready');
  assert.deepEqual(JSON.parse(JSON.stringify(ready.actions)), [{ name: 'clicked', values: ['Continue'] }]);
  assert.equal(r.window.__workbenchReady, true);
});

test('links open mapped previews and log every other destination as an action', async () => {
  let on = true;
  const actions = { configure() {}, on: () => on, follow: null };
  const r = runtime('default', true, actions);
  const preview = { id: 'pages/start', links: { '/next/': 'pages/next', 'done.html': { scenario: 'done' } }, scenarios: { default: {}, done: {} } };
  const renderer = await r.boot(preview, { default: 'page', done: 'page' }, { mount() {} }, {});
  const sent = () => r.messages.at(-1);
  assert.equal(actions.follow('/next/', 'link'), true);
  assert.deepEqual({ ...sent(), scenario: sent().scenario }, { type: 'workbench-preview', event: 'navigate', id: 'pages/start', preview: 'pages/next', scenario: null });
  actions.follow('http://127.0.0.1/previews/done.html#top', 'link');
  assert.deepEqual([sent().preview, sent().scenario], ['pages/start', 'done']);
  actions.follow('https://example.com/acme.zip', 'link');
  assert.deepEqual([sent().event, sent().name, [...sent().values]], ['action', 'navigate', ['https://example.com/acme.zip']]);
  actions.follow('/sign-up', 'submit', { fields: { email: 'ada@example.com' } });
  assert.deepEqual([sent().name, [...sent().values]], ['submit', ['/sign-up', '{"email":"ada@example.com"}']]);
  on = false;
  const count = r.messages.length;
  r.window.workbench.navigate('pages/next');
  assert.equal(r.messages.length, count);
  await renderer.dispose();
  assert.equal(actions.follow, null);
});

test('without the actions switch a portable preview still claims links but not in-page anchors', async () => {
  const r = runtime();
  await r.boot({ id: 'pages/start', links: { '/next/': 'pages/next' } }, { default: 'page' }, { mount() {} }, {});
  const click = href => {
    const link = { tagName: 'A', hasAttribute: () => true, getAttribute: () => href };
    let prevented = false;
    r.documentHandlers.get('click')({ composedPath: () => [link], preventDefault: () => { prevented = true; } });
    return prevented;
  };
  assert.equal(click('#install'), false);
  assert.equal(click('/next/'), true);
  assert.equal(r.messages.at(-1).preview, 'pages/next');
});

test('each render activates its scenario request mocks before setup and disposal deactivates them', async () => {
  const r = runtime('empty');
  const activations = [];
  r.window.__workbenchRequests = {
    activate: (levels, context) => activations.push([levels, context.scenario]),
    deactivate: () => activations.push('off'),
  };
  const preview = { id: 'pages/customers', requests: { '/api/customers': { body: ['Ada'] } },
    setup: () => { activations.push('setup'); },
    scenarios: { default: {}, empty: { requests: { '/api/customers': { body: [] } } } } };
  const renderer = await r.boot(preview, { default: 'page' }, { mount() {} }, {});
  assert.deepEqual(JSON.parse(JSON.stringify(activations)), [
    [[{ '/api/customers': { body: [] } }, { '/api/customers': { body: ['Ada'] } }], 'empty'], 'setup']);
  await renderer.dispose();
  assert.equal(activations.at(-1), 'off');
});

test('a project environment wraps outside a preview environment and its hooks run first', async () => {
  const source = fs.readFileSync(path.join(__dirname, "./browser.js"), 'utf8');
  const combine = vm.runInNewContext(source.replace(/^export /gm, '') + '\ncombine;', {});
  const order = [];
  const environment = name => ({
    setup: () => { order.push(name + ' setup'); return () => order.push(name + ' cleanup'); },
    wrap: tree => name + '(' + tree + ')',
  });
  const combined = combine({ ...environment('project'), theme: 'dark', locale: 'en' }, { ...environment('preview'), theme: 'light' });
  assert.equal(combined.wrap('page', {}), 'project(preview(page))');
  assert.deepEqual([combined.theme, combined.locale], ['light', 'en']);
  const cleanup = await combined.setup({});
  await cleanup();
  assert.deepEqual(order, ['project setup', 'preview setup', 'preview cleanup', 'project cleanup']);
  const only = { wrap: tree => tree };
  assert.equal(combine({}, only), only);
});

test('missing scenario exports and render failures prevent capture readiness', async () => {
  const r = runtime('missing');
  let mounted = false;
  await r.boot({ id: 'button', scenarios: { missing: {} } }, { default: 'component', missing: undefined },
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
