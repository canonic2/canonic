/* What the server puts into a page on the way out. This is the whole of what a
   project has to do to preview well — nothing — so it is worth pinning. */

var test = require('node:test');
var assert = require('node:assert');
var fs = require('node:fs');
var http = require('node:http');
var os = require('node:os');
var path = require('node:path');

var server = require('./server');
var config = require('./config');
var withPreviewScripts = server.withPreviewScripts;

var SCRIPTS = '<script src="/_workbench/preview-compat.js"></script>';

test('puts one compatibility script first inside head', function () {
  var out = withPreviewScripts('<!doctype html><html><head><title>x</title></head><body></body></html>');

  assert.ok(out.indexOf('<head>' + SCRIPTS) !== -1);
  /* Before the page's own head, which is what "before its own scripts" means. */
  assert.ok(out.indexOf(SCRIPTS) < out.indexOf('<title>'));
});

test('reads a head with attributes', function () {
  var out = withPreviewScripts('<html><head lang="en"><title>x</title></head></html>');

  assert.ok(out.indexOf('<head lang="en">' + SCRIPTS) !== -1);
});

test('is not fooled by header', function () {
  var out = withPreviewScripts('<html><body><header class="a">hi</header></body></html>');

  /* No <head> to speak of, so the scripts belong to <html> — and <header> is
     left exactly where it was. */
  assert.ok(out.indexOf('<html>' + SCRIPTS) !== -1);
  assert.ok(out.indexOf('<header class="a">hi</header>') !== -1);
});

test('falls back to the top of a fragment', function () {
  var out = withPreviewScripts('<div>just a fragment</div>');

  assert.strictEqual(out, SCRIPTS + '<div>just a fragment</div>');
});

test('leaves the rest of the document alone', function () {
  var page = '<html><head></head><body><p>unchanged</p></body></html>';

  assert.strictEqual(withPreviewScripts(page).replace(SCRIPTS, ''), page);
});

test('accepts a local native capture payload and normalizes its geometry', function () {
  var payload = server.capturePayload(Buffer.from(JSON.stringify({
    url: '/pages/sign-in.html?state=error',
    width: 1511.7,
    height: '982',
    scroll: { x: 12, y: 40 },
    markup: '<svg></svg>',
    name: 'sign-in.png',
  })), 'http://127.0.0.1:3579/');

  assert.strictEqual(payload.url, 'http://127.0.0.1:3579/pages/sign-in.html?state=error');
  assert.strictEqual(payload.width, 1512);
  assert.strictEqual(payload.height, 982);
  assert.deepStrictEqual(payload.scroll, { x: 12, y: 40 });
  assert.strictEqual(payload.markup, '<svg></svg>');
});

test('refuses capture URLs outside the workbench origin', function () {
  assert.throws(function () {
    server.capturePayload(Buffer.from(JSON.stringify({
      url: 'https://example.com/', width: 393, height: 852,
    })), 'http://127.0.0.1:3579/');
  }, /must belong to this workbench/);
});

test('local capture carries live mirror state and rejects malformed snapshots', function () {
  var mirror = { revision: 'session-2', html: '<html><body>Edited</body></html>', states: [{ id: '1', value: 'typed' }], defined: ['x-card'], animations: [] };
  function payload(value) {
    return server.capturePayload(Buffer.from(JSON.stringify({ url: '/page.html', width: 1440, height: 1000, mirror: value })), 'http://127.0.0.1:3579/');
  }
  assert.deepStrictEqual(payload(mirror).mirror, mirror);
  assert.equal(payload(undefined).mirror, undefined);
  assert.throws(function () { payload(Object.assign({}, mirror, { states: null })); }, /Invalid live DOM snapshot/);
  assert.throws(function () { payload(Object.assign({}, mirror, { revision: '' })); }, /Invalid live DOM snapshot/);
});

test('refuses invalid native capture dimensions', function () {
  assert.throws(function () {
    server.capturePayload(Buffer.from(JSON.stringify({
      url: '/page.html', width: 0, height: 852,
    })), 'http://127.0.0.1:3579/');
  }, /capture width/);
});

test('local capture accepts incremental records and rejects malformed or executable nodes', function () {
  var mirror = { version: 1, revision: 'session-2', base: 'session-1', root: '1', nodes: [
    { id: '2', type: 1, tag: 'p', ns: 'http://www.w3.org/1999/xhtml', attrs: [['class', 'edited', null]], children: ['3'] },
    { id: '3', type: 3, text: 'Changed text' },
  ], removed: ['4'], cleared: ['4'], states: [{ id: '5', value: 'typed' }], defined: [], animations: [] };
  function payload(value) {
    return server.capturePayload(Buffer.from(JSON.stringify({ url: '/page.html', width: 100, height: 100, mirror: value })), 'http://127.0.0.1:3579/');
  }
  assert.deepEqual(payload(mirror).mirror, mirror);
  assert.deepEqual(payload(Object.assign({}, mirror, { pointer: { x: 32, y: 16 } })).mirror.pointer, { x: 32, y: 16 });
  assert.throws(function () { payload(Object.assign({}, mirror, { pointer: { x: Infinity, y: 16 } })); }, /Invalid live DOM pointer/);
  assert.throws(function () { payload(Object.assign({}, mirror, { pointer: { x: -1, y: 16 } })); }, /Invalid live DOM pointer/);
  assert.throws(function () { payload(Object.assign({}, mirror, { base: 12 })); }, /Invalid live DOM patch/);
  assert.throws(function () { payload(Object.assign({}, mirror, { nodes: [{ id: '2', type: 1, tag: 'script', ns: '', attrs: [], children: [] }] })); }, /Invalid live DOM patch/);
  assert.throws(function () { payload(Object.assign({}, mirror, { removed: [null] })); }, /Invalid live DOM patch/);
});

function post(port, pathname, payload) {
  var body = JSON.stringify(payload || {});
  return new Promise(function (resolve, reject) {
    var req = http.request({
      host: '127.0.0.1',
      port: port,
      path: pathname,
      method: 'POST',
      /* A fresh socket each time: the servers under test come and go on the
         same port, and a kept-alive one would be reused against the wrong one. */
      agent: false,
      headers: {
        'Content-Type': 'application/json',
        'Content-Length': Buffer.byteLength(body),
      },
    }, function (res) {
      var chunks = [];
      res.on('data', function (chunk) { chunks.push(chunk); });
      res.on('end', function () {
        resolve({ status: res.statusCode, body: JSON.parse(Buffer.concat(chunks).toString('utf8')) });
      });
    });
    req.on('error', reject);
    req.end(body);
  });
}

function get(port, pathname) {
  return new Promise(function (resolve, reject) {
    http.get({ host: '127.0.0.1', port: port, path: pathname, agent: false }, function (res) {
      var chunks = [];
      res.on('data', function (chunk) { chunks.push(chunk); });
      res.on('end', function () {
        resolve({ status: res.statusCode, body: JSON.parse(Buffer.concat(chunks).toString('utf8')) });
      });
    }).on('error', reject);
  });
}

function getRaw(port, pathname) {
  return new Promise(function (resolve, reject) {
    http.get({ host: '127.0.0.1', port: port, path: pathname, agent: false }, function (res) {
      var chunks = [];
      res.on('data', function (chunk) { chunks.push(chunk); });
      res.on('end', function () {
        resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) });
      });
    }).on('error', reject);
  });
}

/* A stand-in for a Storybook or a dev server: whatever the test hands it. */
function stub(handler) {
  return new Promise(function (resolve) {
    var server = http.createServer(handler);
    server.listen(0, '127.0.0.1', function () {
      resolve({
        url: 'http://127.0.0.1:' + server.address().port,
        close: function () {
          return new Promise(function (done) { server.close(done); });
        },
      });
    });
  });
}

test('funnels bounded browser diagnostics into the server session logger', async function () {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-log-test-'));
  var entries = [];
  var running = await server.start({
    root: root,
    capture: { close: function () {} },
    onLog: function (entry) { entries.push(entry); },
  });
  try {
    var answer = await post(running.port, server.LOG_PATH, {
      level: 'error',
      event: 'handoff.browser.failed',
      details: { phase: 'send', message: 'x'.repeat(3000), count: 2 },
    });
    assert.strictEqual(answer.status, 200);
    assert.deepStrictEqual(entries.map(function (entry) { return entry.event; }), [
      'session.started', 'handoff.browser.failed',
    ]);
    assert.strictEqual(entries[1].level, 'error');
    assert.strictEqual(entries[1].details.phase, 'send');
    assert.strictEqual(entries[1].details.message.length, 2000);
    assert.strictEqual(entries[1].details.count, 2);
  } finally {
    await running.close();
    fs.rmSync(root, { recursive: true, force: true });
  }
  assert.strictEqual(entries[entries.length - 1].event, 'session.stopping');
});

test('stops accepting diagnostics after the per-session byte budget', async function () {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-log-limit-test-'));
  var entries = [];
  var running = await server.start({
    root: root,
    capture: { close: function () {} },
    logLimit: 300,
    onLog: function (entry) { entries.push(entry); },
  });
  try {
    await post(running.port, server.LOG_PATH, {
      level: 'error', event: 'large.failure', details: { message: 'x'.repeat(2000) },
    });
    await post(running.port, server.LOG_PATH, {
      level: 'error', event: 'another.failure', details: { message: 'y'.repeat(2000) },
    });
  } finally {
    await running.close();
    fs.rmSync(root, { recursive: true, force: true });
  }
  assert.strictEqual(entries.filter(function (entry) { return entry.event === 'log.limit.reached'; }).length, 1);
  assert.strictEqual(entries.some(function (entry) { return entry.event === 'large.failure'; }), false);
  assert.strictEqual(entries.some(function (entry) { return entry.event === 'another.failure'; }), false);
});

test('discovers Storybook ports from project package scripts', function () {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-storybook-port-'));
  try {
    fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify({
      scripts: { storybook: 'storybook dev --port 7123' },
    }));
    assert.strictEqual(server.storybookPorts(root)[0], 7123);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('turns matching booted simulators into implementation-only screens', function () {
  var imported = server.simulatorCatalog([
    { name: 'iPhone 17', udid: 'SIM-1', runtime: 'iOS 26.2' },
    { name: 'iPad Pro', udid: 'SIM-2', runtime: 'iOS 26.2' },
  ], 'ios', { label: 'Simulator', device: 'iPhone 17', catalogIcon: 'smartphone' });
  assert.deepStrictEqual(imported.sections, [{
    group: 'Simulator', icon: 'smartphone', items: [{
      label: 'iPhone 17', src: '__ios-simulator/ios/SIM-1.html', icon: 'smartphone',
      implementations: { ios: { device: 'SIM-1' } }, implementationOnly: 'ios',
    }],
  }]);
  assert.strictEqual(imported.screens['__ios-simulator/ios/SIM-1.html'].simulator.udid, 'SIM-1');
});

test('negotiates a native Simulator stream only for a configured device', async function () {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-simulator-stream-'));
  fs.writeFileSync(path.join(root, 'workbench.yaml'), [
    'name: Simulator stream',
    'implementations:',
    '  ios:',
    '    kind: ios-simulator',
    '    device: booted',
    '    catalog: true',
  ].join('\n'));
  var calls = [];
  var nativeStream = {
    start: function (request) {
      calls.push(['start', request]);
      return { token: 'stream-token', source: 'iPhone 17' };
    },
    stop: function () { calls.push(['stop']); },
    accept: function () { return false; },
    close: function () {},
  };
  var running = await server.start({
    root: root,
    capture: { close: function () {} },
    windowStream: nativeStream,
    simulators: function () { return [{ name: 'iPhone 17', udid: 'SIM-1', runtime: 'iOS 26.2' }]; },
  });
  try {
    var started = await post(running.port, server.SIMULATOR_STREAM_PATH, {
      implementation: 'ios', udid: 'SIM-1',
    });
    assert.strictEqual(started.status, 200);
    assert.strictEqual(started.body.stream, server.WINDOW_STREAM_PATH + '?token=stream-token');
    assert.strictEqual(started.body.codec, 'h264');
    assert.deepStrictEqual(calls[0], ['start', {
      source: 'iPhone 17', app: 'simulator', id: 'SIM-1', codec: 'h264',
    }]);
    var refused = await post(running.port, server.SIMULATOR_STREAM_PATH, {
      implementation: 'ios', udid: 'SIM-2', offer: 'offer-sdp',
    });
    assert.strictEqual(refused.status, 403);
    var stopped = await post(running.port, server.SIMULATOR_STREAM_PATH, {
      implementation: 'ios', udid: 'SIM-1', stop: true,
    });
    assert.strictEqual(stopped.status, 200);
    assert.deepStrictEqual(calls[1], ['stop']);
  } finally {
    await running.close();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('streams only the window a screen names, from its implementation’s app', async function () {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-window-stream-'));
  fs.mkdirSync(path.join(root, 'pages'));
  fs.writeFileSync(path.join(root, 'pages', 'sign-in.html'), '<!doctype html><title>Sign in</title>');
  fs.writeFileSync(path.join(root, 'workbench.yaml'), [
    'name: Window stream',
    'implementations:',
    '  emulator:',
    '    kind: window',
    '    app: com.example.emulator',
    '  dev:',
    '    kind: url',
    '    base: http://localhost:3000',
    'sections:',
    '  - name: Pages',
    '    items:',
    '      - label: Sign in',
    '        src: pages/sign-in.html',
    '        implementations:',
    '          emulator: Example Phone',
    '          dev: /sign-in',
  ].join('\n'));
  var calls = [];
  var nativeStream = {
    start: function (request) {
      calls.push(['start', request]);
      return { token: 'window-token', source: request.source, codec: request.codec };
    },
    stop: function () { calls.push(['stop']); },
    accept: function () { return false; },
    close: function () {},
  };
  var running = await server.start({ root: root, capture: { close: function () {} }, windowStream: nativeStream });
  try {
    var started = await post(running.port, server.WINDOW_STREAM_PATH, {
      implementation: 'emulator', src: 'pages/sign-in.html', codec: 'jpeg',
    });
    assert.strictEqual(started.status, 200);
    assert.strictEqual(started.body.stream, server.WINDOW_STREAM_PATH + '?token=window-token');
    assert.strictEqual(started.body.codec, 'jpeg');
    assert.deepStrictEqual(calls[0], ['start', {
      app: 'com.example.emulator', source: 'Example Phone', id: 'emulator\npages/sign-in.html', codec: 'jpeg',
    }]);

    var undeclared = await post(running.port, server.WINDOW_STREAM_PATH, {
      implementation: 'emulator', src: 'pages/other.html',
    });
    assert.strictEqual(undeclared.status, 403);
    var notWindow = await post(running.port, server.WINDOW_STREAM_PATH, {
      implementation: 'dev', src: 'pages/sign-in.html',
    });
    assert.strictEqual(notWindow.status, 403);
    assert.strictEqual(calls.length, 1);

    var stopped = await post(running.port, server.WINDOW_STREAM_PATH, {
      implementation: 'emulator', src: 'pages/sign-in.html', stop: true,
    });
    assert.strictEqual(stopped.status, 200);
    assert.deepStrictEqual(calls[1], ['stop']);
  } finally {
    await running.close();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

/* A project with two implementations pointed at a stub, and one screen of
   each kind. `product` is where the code is. */
function project(implementationUrl) {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-server-test-'));
  var product = path.join(root, 'product');
  fs.mkdirSync(path.join(product, 'src', 'button'), { recursive: true });
  fs.writeFileSync(path.join(product, 'src', 'button', 'button.tsx'), '');
  fs.writeFileSync(path.join(product, 'src', 'button', 'button.stories.tsx'), '');
  fs.mkdirSync(path.join(root, 'pages'), { recursive: true });
  fs.mkdirSync(path.join(root, 'preview'), { recursive: true });
  fs.writeFileSync(path.join(root, 'pages', 'sign-in.html'), '<html></html>');
  fs.writeFileSync(path.join(root, 'preview', 'components-button.html'), '<html></html>');
  fs.writeFileSync(path.join(root, 'workbench.yaml'), [
    'name: Acme',
    'implementations:',
    '  storybook:',
    '    kind: storybook',
    '    url: ' + implementationUrl,
    '    root: product',
    '  dev:',
    '    kind: url',
    '    base: ' + implementationUrl,
    '    root: product',
    'sections:',
    '  - name: Pages',
    '    items:',
    '      - label: Sign in',
    '        src: pages/sign-in.html',
    '        implementations:',
    '          dev: /login',
    '        code:',
    '          dev: src/button',
    '      - label: Button',
    '        src: preview/components-button.html',
    '        implementations:',
    '          storybook: Components/Button',
  ].join('\n'));
  return { root: root, product: product };
}

test('answers the config as this machine resolves it', async function () {
  var made = project('http://localhost:6006');
  var running = await server.start({ root: made.root, capture: { close: function () {} } });
  try {
    var answer = await get(running.port, server.CONFIG_PATH);
    assert.strictEqual(answer.status, 200);
    assert.strictEqual(answer.body.ok, true);
    assert.strictEqual(answer.body.implementations.dev.root, made.product);
    assert.deepStrictEqual(answer.body.screens['pages/sign-in.html'].code, [
      { implementation: 'dev', path: path.join(made.product, 'src', 'button'), relative: 'src/button', exists: true },
    ]);
    assert.deepStrictEqual(answer.body.problems, []);
  } finally {
    await running.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('reads and updates page configuration through the local form endpoint', async function () {
  var made = project('http://localhost:6006');
  var running = await server.start({ root: made.root, capture: { close: function () {} } });
  try {
    var before = await get(running.port, server.CONFIG_FILE_PATH);
    assert.strictEqual(before.status, 200);
    assert.strictEqual(before.body.sections[0].items[0].label, 'Sign in');
    var updated = await post(running.port, server.CONFIG_FILE_PATH, { sections: [{
      name: 'Pages', icon: 'file-text', items: [{
        label: 'Home', src: 'pages/home.html', viewports: ['desktop', 'mobile'],
      }],
    }] });
    assert.strictEqual(updated.status, 200);
    assert.deepStrictEqual(config.read(made.root).sections[0].items[0].viewports, ['desktop', 'mobile']);
    assert.match(fs.readFileSync(path.join(made.root, 'workbench.yaml'), 'utf8'), /label: Home/);
  } finally {
    await running.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('downloads the resolved workbench as a ZIP', async function () {
  var made = project('http://localhost:6006');
  var captures = [];
  var running = await server.start({ root: made.root, capture: {
    capture: function (base, payload) { captures.push([base, payload]); return Buffer.from('reference jpeg'); },
    close: function () {},
  } });
  try {
    var answer = await getRaw(running.port, server.EXPORT_PATH);
    assert.strictEqual(answer.status, 200);
    assert.strictEqual(answer.headers['content-type'], 'application/zip');
    assert.match(answer.headers['content-disposition'], /^attachment; filename="acme-design-system\.zip"$/);
    assert.strictEqual(answer.body.readUInt32LE(0), 0x04034b50);
    assert.ok(answer.body.includes(Buffer.from('product/src/button/button.stories.tsx')));
    assert.ok(answer.body.includes(Buffer.from('pages/screenshots/default-fit.jpg')));
    assert.ok(answer.body.includes(Buffer.from('preview/screenshots/default-fit.jpg')));
    assert.strictEqual(captures.length, 6);
    assert.strictEqual(captures[0][1].width, 1440);
    assert.strictEqual(captures[0][1].height, 900);
  } finally {
    await running.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('plans every local state and imported Storybook story for export capture', function () {
  var view = {
    implementations: { storybook: { kind: 'storybook', url: 'http://127.0.0.1:6006' } },
    sections: [{ items: [{
      label: 'Sign in', src: 'pages/sign-in.html',
      viewports: ['responsive', 'fit'],
      states: [{ id: 'default', label: 'Default' }, { id: 'error', label: 'Error' }],
    }] }],
    catalogSections: [{ items: [{
      label: 'Button', src: '__storybook/storybook/button.html', implementationOnly: 'storybook',
      viewports: ['mobile'],
      states: [{ id: 'default', label: 'Default' }, { id: 'icon-only', label: 'Icon Only' }],
    }] }],
    screens: {
      '__storybook/storybook/button.html': { stories: [
        { id: 'button--default', state: 'default', label: 'Default' },
        { id: 'button--icon-only', state: 'icon-only', label: 'Icon Only' },
      ] },
    },
  };
  var plan = server.exportCapturePlan(view, 'http://127.0.0.1:3579/');
  assert.deepStrictEqual(plan.captures.map(function (capture) {
    return [capture.screen, capture.state, capture.viewport, capture.width, capture.height, capture.external];
  }), [
    ['pages/sign-in.html', 'default', 'desktop', 1512, 982, false],
    ['pages/sign-in.html', 'default', 'mobile', 393, 852, false],
    ['pages/sign-in.html', 'default', 'fit', 1440, 900, false],
    ['pages/sign-in.html', 'error', 'desktop', 1512, 982, false],
    ['pages/sign-in.html', 'error', 'mobile', 393, 852, false],
    ['pages/sign-in.html', 'error', 'fit', 1440, 900, false],
    ['__storybook/storybook/button.html', 'default', 'mobile', 393, 852, true],
    ['__storybook/storybook/button.html', 'icon-only', 'mobile', 393, 852, true],
  ]);
  assert.match(plan.captures[3].url, /state=error/);
  assert.match(plan.captures[7].url, /button--icon-only/);
  assert.deepStrictEqual(plan.warnings, []);
});

test('captures export references through four reusable workers', async function () {
  var active = 0;
  var peak = 0;
  var closed = false;
  var payloads = [];
  var workers = Array.from({ length: 4 }, function () {
    return { captureExportPage: async function (payload) {
      payloads.push(payload);
      active += 1;
      peak = Math.max(peak, active);
      await new Promise(function (resolve) { setTimeout(resolve, 10); });
      active -= 1;
      return { image: Buffer.from('jpeg') };
    } };
  });
  var capture = { createPool: function (size) {
    assert.equal(size, 4);
    return { workers: workers, close: async function () { closed = true; } };
  } };
  var captures = Array.from({ length: 7 }, function (_, index) {
    return { screen: 'story-' + index, state: 'default', variant: 'default-mobile', label: 'Story ' + index,
      viewport: 'mobile', viewportLabel: 'Mobile', url: 'http://localhost:6006/iframe.html?id=story--' + index,
      external: true, width: 393, height: 852 };
  });
  var result = await server.captureExportReferences(capture, { captures: captures, warnings: [] }, 'http://localhost:3579/', function () {});
  assert.equal(peak, 4);
  assert.equal(result.screenshots.length, 7);
  assert.ok(payloads.every(function (payload) { return payload.reuse === 'storybook'; }));
  assert.equal(closed, true);
});

test('keeps successful export captures when pool cleanup fails', async function () {
  var worker = { captureExportPage: async function () { return { image: Buffer.from('jpeg') }; } };
  var capture = { createPool: function () {
    return { workers: [worker, worker], close: async function () { throw new Error('close failed'); } };
  } };
  var captures = ['button', 'card'].map(function (story) {
    return { screen: story, state: 'default', variant: 'default-mobile', label: story,
      viewport: 'mobile', viewportLabel: 'Mobile', url: 'http://localhost:6006/iframe.html?id=' + story,
      external: true, width: 393, height: 852 };
  });
  var result = await server.captureExportReferences(capture, { captures: captures, warnings: [] }, 'http://localhost:3579/', function () {});
  assert.equal(result.screenshots.length, 2);
  assert.deepEqual(result.warnings, ['Capture worker cleanup: close failed']);
});

test('keeps every viewport for a story on one warm capture worker', async function () {
  var assignments = new Map();
  var workers = Array.from({ length: 2 }, function (_, worker) {
    return { captureExportPage: async function (payload) {
      if (!assignments.has(payload.url)) assignments.set(payload.url, new Set());
      assignments.get(payload.url).add(worker);
      return { image: Buffer.from('jpeg') };
    } };
  });
  var capture = { createPool: function (size) {
    assert.equal(size, 2);
    return { workers: workers, close: async function () {} };
  } };
  var captures = ['button', 'card'].flatMap(function (story) {
    return ['mobile', 'desktop', 'fit'].map(function (viewport) {
      return { screen: story, state: 'default', variant: 'default-' + viewport, label: story,
        viewport: viewport, viewportLabel: viewport, url: 'http://localhost:6006/iframe.html?id=' + story,
        external: true, width: viewport === 'mobile' ? 393 : 1512, height: 852 };
    });
  });
  var result = await server.captureExportReferences(capture, { captures: captures, warnings: [] }, 'http://localhost:3579/', function () {});
  assert.equal(result.screenshots.length, 6);
  assert.deepEqual(Array.from(assignments.values(), function (workerIds) { return workerIds.size; }), [1, 1]);
});

test('reports export capture progress before serving the completed job', async function () {
  var made = project('http://localhost:6006');
  var running = await server.start({ root: made.root, capture: {
    capture: function () { return Promise.resolve(Buffer.alloc(20000, 1)); },
    close: function () {},
  }, exportMaxArchiveBytes: 90000 });
  try {
    var started = await post(running.port, server.EXPORT_PATH, {});
    assert.strictEqual(started.status, 202);
    assert.strictEqual(started.body.total, 6);
    var status = started.body;
    for (var i = 0; i < 20 && status.status !== 'complete'; i += 1) {
      status = (await get(running.port, server.EXPORT_PATH + '?job=' + started.body.id)).body;
    }
    assert.strictEqual(status.status, 'complete');
    assert.strictEqual(status.completed, 6);
    assert.ok(status.parts.length > 1);
    assert.ok(status.parts.every(function (part) { return part.bytes <= 90000; }));
    var bundle = await getRaw(running.port, server.EXPORT_PATH + '?job=' + started.body.id + '&download=1');
    assert.strictEqual(bundle.status, 200);
    assert.strictEqual(bundle.headers['content-type'], 'application/zip');
    assert.match(bundle.headers['content-disposition'], /design-system-parts\.zip/);
    assert.ok(bundle.body.length > status.parts.reduce(function (total, part) { return total + part.bytes; }, 0));
    var download = await getRaw(running.port, server.EXPORT_PATH + '?job=' + started.body.id + '&download=1&part=1');
    assert.strictEqual(download.status, 200);
    assert.strictEqual(download.headers['content-type'], 'application/zip');
    assert.match(download.headers['content-disposition'], /part-01-of-/);
    assert.ok(download.body.length <= 90000);
  } finally {
    await running.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('waits for eager capture warm-up before starting export screenshots', async function () {
  var made = project('http://localhost:6006');
  var releaseWarm;
  var warmed = new Promise(function (resolve) { releaseWarm = resolve; });
  var captures = 0;
  var running = await server.start({ root: made.root, eagerCapture: true, capture: {
    warm: function () { return warmed; },
    capture: function () { captures += 1; return Buffer.from('reference jpeg'); },
    close: function () {},
  } });
  try {
    var started = await post(running.port, server.EXPORT_PATH, {});
    await new Promise(function (resolve) { setImmediate(resolve); });
    assert.equal(captures, 0);
    assert.equal((await get(running.port, server.EXPORT_PATH + '?job=' + started.body.id)).body.completed, 0);
    releaseWarm();
    await warmed;
    var status;
    for (var i = 0; i < 20; i += 1) {
      status = (await get(running.port, server.EXPORT_PATH + '?job=' + started.body.id)).body;
      if (status.status === 'complete') break;
    }
    assert.equal(status.status, 'complete');
    assert.equal(captures, 6);
  } finally {
    await running.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('coalesces overlapping exports into one capture job', async function () {
  var made = project('http://localhost:6006');
  var releaseCapture;
  var firstCapture = new Promise(function (resolve) { releaseCapture = resolve; });
  var captures = 0;
  var running = await server.start({ root: made.root, capture: {
    capture: function () {
      captures += 1;
      return captures === 1 ? firstCapture.then(function () { return Buffer.from('reference jpeg'); }) : Buffer.from('reference jpeg');
    },
    close: function () {},
  } });
  try {
    var first = await post(running.port, server.EXPORT_PATH, {});
    var second = await post(running.port, server.EXPORT_PATH, {});
    assert.equal(second.body.id, first.body.id);
    releaseCapture();
    var status;
    for (var i = 0; i < 20; i += 1) {
      status = (await get(running.port, server.EXPORT_PATH + '?job=' + first.body.id)).body;
      if (status.status === 'complete') break;
    }
    assert.equal(status.status, 'complete');
    assert.equal(captures, 6);
  } finally {
    await running.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('retains a slow export job until after it settles', async function () {
  var made = project('http://localhost:6006');
  var releaseCapture;
  var firstCapture = new Promise(function (resolve) { releaseCapture = resolve; });
  var captures = 0;
  var running = await server.start({ root: made.root, exportRetentionMs: 100, capture: {
    capture: function () {
      captures += 1;
      return captures === 1 ? firstCapture.then(function () { return Buffer.from('reference jpeg'); }) : Buffer.from('reference jpeg');
    },
    close: function () {},
  } });
  try {
    var started = await post(running.port, server.EXPORT_PATH, {});
    await new Promise(function (resolve) { setTimeout(resolve, 120); });
    assert.equal((await get(running.port, server.EXPORT_PATH + '?job=' + started.body.id)).status, 200);
    releaseCapture();
    var status;
    for (var i = 0; i < 20; i += 1) {
      status = await get(running.port, server.EXPORT_PATH + '?job=' + started.body.id);
      if (status.body.status === 'complete') break;
    }
    assert.equal(status.body.status, 'complete');
    await new Promise(function (resolve) { setTimeout(resolve, 120); });
    assert.equal((await get(running.port, server.EXPORT_PATH + '?job=' + started.body.id)).status, 404);
  } finally {
    await running.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('bounds retained completed export bundles', async function () {
  var made = project('http://localhost:6006');
  var running = await server.start({ root: made.root, capture: {
    capture: function () { return Buffer.from('reference jpeg'); }, close: function () {},
  } });
  async function completeExport() {
    var started = await post(running.port, server.EXPORT_PATH, {});
    var status;
    for (var i = 0; i < 20; i += 1) {
      status = await get(running.port, server.EXPORT_PATH + '?job=' + started.body.id);
      if (status.body.status === 'complete') break;
    }
    assert.equal(status.body.status, 'complete');
    return started.body.id;
  }
  try {
    var first = await completeExport();
    var second = await completeExport();
    var third = await completeExport();
    assert.equal((await get(running.port, server.EXPORT_PATH + '?job=' + first)).status, 404);
    assert.equal((await get(running.port, server.EXPORT_PATH + '?job=' + second)).status, 200);
    assert.equal((await get(running.port, server.EXPORT_PATH + '?job=' + third)).status, 200);
  } finally {
    await running.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('imports an opted-in Storybook catalog as screens and states', async function () {
  var storybook = await stub(function (req, res) {
    if (req.url !== '/index.json') {
      res.writeHead(404);
      res.end();
      return;
    }
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ entries: {
      'components-inputs-button--default': {
        id: 'components-inputs-button--default', title: 'Components/Inputs/Button', name: 'Default', type: 'story',
        importPath: './src/button/button.stories.tsx', componentPath: './src/button/button.tsx',
      },
      'components-inputs-button--icon-only': {
        id: 'components-inputs-button--icon-only', title: 'Components/Inputs/Button', name: 'Icon Only', type: 'story',
        importPath: './src/button/button.stories.tsx', componentPath: './src/button/button.tsx',
      },
      'components-inputs-button--docs': {
        id: 'components-inputs-button--docs', title: 'Components/Inputs/Button', name: 'Docs', type: 'docs',
      },
    } }));
  });
  var made = project(storybook.url);
  fs.writeFileSync(path.join(made.root, 'workbench.yaml'), [
    'name: Acme catalog',
    'implementations:',
    '  storybook:',
    '    kind: storybook',
    '    url: ' + storybook.url,
    '    root: product',
    '    catalog:',
    '      icon: book-open',
    '      icons:',
    '        Components: layout-grid',
    '        Components/Inputs: form-input',
  ].join('\n'));
  var opened = [];
  var running = await server.start({
    root: made.root,
    capture: { close: function () {} },
    onOpen: function (file) { opened.push(file); },
  });
  try {
    var answer = await get(running.port, server.CONFIG_PATH);
    assert.strictEqual(answer.status, 200);
    assert.deepStrictEqual(answer.body.catalogSections, [{
      group: 'Components',
      icon: 'layout-grid',
      items: [{
        folder: 'Inputs',
        items: [{
          label: 'Button',
          src: '__storybook/storybook/components-inputs-button.html',
          icon: 'form-input',
          states: [{ id: 'default', label: 'Default' }, { id: 'icon-only', label: 'Icon Only' }],
          implementations: { storybook: { title: 'Components/Inputs/Button' } },
          implementationOnly: 'storybook',
        }],
      }],
    }]);
    var screen = answer.body.screens['__storybook/storybook/components-inputs-button.html'];
    assert.strictEqual(screen.design, null);
    assert.deepStrictEqual(screen.code.map(function (entry) { return [entry.relative, entry.exists]; }), [
      ['./src/button/button.tsx', true],
      ['./src/button/button.stories.tsx', true],
    ]);
    assert.deepStrictEqual(answer.body.problems, []);

    var source = await post(running.port, server.OPEN_PATH, {
      src: '__storybook/storybook/components-inputs-button.html',
      path: path.join(made.product, 'src', 'button', 'button.tsx'),
    });
    assert.strictEqual(source.status, 200);
    assert.deepStrictEqual(opened, [path.join(made.product, 'src', 'button', 'button.tsx')]);
  } finally {
    await running.close();
    await storybook.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('automatically detects a running Storybook from the project script', async function () {
  var storybook = await stub(function (req, res) {
    if (req.url !== '/index.json') return res.writeHead(404).end();
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ entries: {
      'components-button--primary': {
        id: 'components-button--primary', title: 'Components/Button', name: 'Primary', type: 'story',
      },
    } }));
  });
  var made = project(storybook.url);
  fs.writeFileSync(path.join(made.root, 'package.json'), JSON.stringify({
    scripts: { storybook: 'storybook dev -p ' + new URL(storybook.url).port },
  }));
  fs.writeFileSync(path.join(made.root, 'workbench.yaml'), [
    'name: Automatic Storybook',
    'implementations:',
    '  storybook:',
    '    kind: storybook',
    '    url: auto',
    '    catalog: true',
  ].join('\n'));
  var running = await server.start({ root: made.root, capture: { close: function () {} } });
  try {
    var answer = await get(running.port, server.CONFIG_PATH);
    assert.strictEqual(answer.status, 200);
    assert.strictEqual(answer.body.implementations.storybook.url, storybook.url);
    assert.strictEqual(answer.body.catalogSections[0].items[0].label, 'Button');
  } finally {
    await running.close();
    await storybook.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('opens only what the config resolves to', async function () {
  var made = project('http://localhost:6006');
  var opened = [];
  var running = await server.start({
    root: made.root,
    capture: { close: function () {} },
    onOpen: function (file) { opened.push(file); },
  });
  try {
    var design = await post(running.port, server.OPEN_PATH, { src: 'pages/sign-in.html' });
    assert.strictEqual(design.status, 200);
    assert.strictEqual(design.body.opened, path.join(made.root, 'pages', 'sign-in.html'));

    var code = await post(running.port, server.OPEN_PATH, { src: 'pages/sign-in.html', path: path.join(made.product, 'src', 'button') });
    assert.strictEqual(code.status, 200);

    var elsewhere = await post(running.port, server.OPEN_PATH, { src: 'pages/sign-in.html', path: '/etc/passwd' });
    assert.strictEqual(elsewhere.status, 404);

    var unknown = await post(running.port, server.OPEN_PATH, { src: 'pages/nope.html' });
    assert.strictEqual(unknown.status, 404);

    assert.deepStrictEqual(opened, [
      path.join(made.root, 'pages', 'sign-in.html'),
      path.join(made.product, 'src', 'button'),
    ]);
  } finally {
    await running.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('captures a page only at an implementation’s origin', async function () {
  var made = project('http://localhost:6006');
  var calls = [];
  var fake = {
    capturePage: function (payload) { calls.push(payload); return Buffer.from('page png'); },
    close: function () {},
  };
  var running = await server.start({ root: made.root, capture: fake });
  try {
    var refused = await post(running.port, server.CAPTURE_PAGE_PATH, {
      url: 'https://example.com/', width: 393, height: 852, name: 'x.png',
    });
    assert.strictEqual(refused.status, 403);
    assert.deepStrictEqual(calls, []);

    var taken = await post(running.port, server.CAPTURE_PAGE_PATH, {
      url: 'http://localhost:6006/login', width: 393, height: 852, markup: '<svg></svg>', name: 'sign-in-dev.png',
    });
    assert.strictEqual(taken.status, 200);
    assert.strictEqual(taken.body.file, path.join('.canonic', '.handoffs', 'sign-in-dev.png'));
    assert.strictEqual(fs.readFileSync(path.join(made.root, taken.body.file), 'utf8'), 'page png');
    assert.strictEqual(calls.length, 1);
    assert.strictEqual(calls[0].url, 'http://localhost:6006/login');
    assert.strictEqual(calls[0].markup, '<svg></svg>');
    assert.ok(calls[0].css.indexOf('.wb-markup-layer') > -1);
  } finally {
    await running.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('lists a title’s stories from the Storybook’s index, with their code', async function () {
  var storybook = await stub(function (req, res) {
    if (req.url !== '/index.json') {
      res.writeHead(404);
      res.end();
      return;
    }
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({
      v: 5,
      entries: {
        'components-button--default': {
          id: 'components-button--default', title: 'Components/Button', name: 'Default', type: 'story',
          importPath: './src/button/button.stories.tsx', componentPath: './src/button/button.tsx',
        },
        'components-button--icon-only': {
          id: 'components-button--icon-only', title: 'Components/Button', name: 'Icon Only', type: 'story',
          importPath: './src/button/button.stories.tsx', componentPath: './src/button/nowhere.tsx',
        },
        'components-button--docs': { id: 'components-button--docs', title: 'Components/Button', name: 'Docs', type: 'docs' },
        'button--default': { id: 'button--default', title: 'Button', name: 'Default', type: 'story' },
      },
    }));
  });
  var made = project(storybook.url);
  var running = await server.start({ root: made.root, capture: { close: function () {} } });
  try {
    var answer = await get(running.port, server.STORIES_PATH + '?implementation=storybook&title=Components%2FButton');
    assert.strictEqual(answer.status, 200);
    assert.deepStrictEqual(answer.body.stories.map(function (s) { return [s.id, s.name, s.state]; }), [
      ['components-button--default', 'Default', 'default'],
      ['components-button--icon-only', 'Icon Only', 'icon-only'],
    ]);
    assert.deepStrictEqual(answer.body.stories[0].code, [
      path.join(made.product, 'src', 'button', 'button.tsx'),
      path.join(made.product, 'src', 'button', 'button.stories.tsx'),
    ]);
    assert.deepStrictEqual(answer.body.stories[1].code, [
      path.join(made.product, 'src', 'button', 'button.stories.tsx'),
    ]);

    var missing = await get(running.port, server.STORIES_PATH + '?implementation=storybook&title=Button%2FPrimary');
    assert.strictEqual(missing.status, 404);
    assert.match(missing.body.error, /No stories titled “Button\/Primary”/);

    var near = await get(running.port, server.STORIES_PATH + '?implementation=storybook&title=Button');
    assert.strictEqual(near.status, 200);
    assert.strictEqual(near.body.stories.length, 1);

    var wrongKind = await get(running.port, server.STORIES_PATH + '?implementation=dev&title=Button');
    assert.strictEqual(wrongKind.status, 400);
  } finally {
    await running.close();
    await storybook.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('suggests the prefixed title when only the prefix differs', async function () {
  var storybook = await stub(function (req, res) {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ v: 5, entries: {
      'components-button--default': { id: 'components-button--default', title: 'Components/Button', name: 'Default', type: 'story' },
    } }));
  });
  var made = project(storybook.url);
  var running = await server.start({ root: made.root, capture: { close: function () {} } });
  try {
    var answer = await get(running.port, server.STORIES_PATH + '?implementation=storybook&title=Button');
    assert.strictEqual(answer.status, 404);
    assert.strictEqual(answer.body.error, 'No stories titled “Button” — did you mean “Components/Button”?');
  } finally {
    await running.close();
    await storybook.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('says when the Storybook is not running or is not one', async function () {
  var gone = await stub(function () {});
  var url = gone.url;
  await gone.close();
  var made = project(url);
  var running = await server.start({ root: made.root, capture: { close: function () {} } });
  try {
    var answer = await get(running.port, server.STORIES_PATH + '?implementation=storybook&title=Button');
    assert.strictEqual(answer.status, 503);
    assert.strictEqual(answer.body.error, 'Storybook at ' + url + ' isn’t answering — is it running?');
  } finally {
    await running.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }

  var notOne = await stub(function (req, res) {
    res.writeHead(200, { 'Content-Type': 'text/html' });
    res.end('<html>');
  });
  made = project(notOne.url);
  running = await server.start({ root: made.root, capture: { close: function () {} } });
  try {
    var odd = await get(running.port, server.STORIES_PATH + '?implementation=storybook&title=Button');
    assert.strictEqual(odd.status, 502);
    assert.match(odd.body.error, /isn’t a Storybook index/);
  } finally {
    await running.close();
    await notOne.close();
    fs.rmSync(made.root, { recursive: true, force: true });
  }
});

test('warms, prepares, saves and closes the native capture service', async function () {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-server-test-'));
  var calls = [];
  var fake = {
    warm: function (base) { calls.push(['warm', base]); },
    prepare: function (base, payload) { calls.push(['prepare', base, payload]); },
    capture: function (base, payload) {
      calls.push(['capture', base, payload]);
      return Buffer.from('native png');
    },
    close: function () { calls.push(['close']); },
  };
  var running = await server.start({ root: root, capture: fake });

  try {
    var warmed = await post(running.port, server.CAPTURE_WARM_PATH, {});
    assert.strictEqual(warmed.status, 200);
    assert.strictEqual(warmed.body.warmed, true);

    var payload = {
      url: 'http://127.0.0.1:' + running.port + '/page.html',
      width: 393,
      height: 852,
      name: 'page.png',
    };
    var prepared = await post(running.port, server.CAPTURE_PREPARE_PATH, payload);
    assert.strictEqual(prepared.status, 200);
    assert.strictEqual(prepared.body.prepared, true);

    var captured = await post(running.port, server.CAPTURE_PATH, payload);
    assert.strictEqual(captured.status, 200);
    assert.strictEqual(captured.body.file, path.join('.canonic', '.handoffs', 'page.png'));
    assert.strictEqual(fs.readFileSync(path.join(root, captured.body.file), 'utf8'), 'native png');
  } finally {
    await running.close();
    fs.rmSync(root, { recursive: true, force: true });
  }

  assert.deepStrictEqual(calls.map(function (call) { return call[0]; }), [
    'warm', 'prepare', 'capture', 'close',
  ]);
});

test('starts warming when the extension server starts, without a browser request', async function (t) {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-eager-test-'));
  var calls = [];
  var running = await server.start({ root: root, eagerCapture: true, capture: {
    warm: function (base) { calls.push(base); }, close: function () {},
  } });
  t.after(async function () { await running.close(); fs.rmSync(root, { recursive: true, force: true }); });
  await new Promise(function (resolve) { setImmediate(resolve); });
  assert.deepEqual(calls, ['http://127.0.0.1:' + running.port + '/']);
});

test('advertises clipboard handoff only when the editor provides it', async function (t) {
  var roots = [];
  var servers = [];
  async function available(onHandoff) {
    var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-handoff-test-'));
    roots.push(root);
    var running = await server.start({ root: root, onHandoff: onHandoff, capture: { close: function () {} } });
    servers.push(running);
    var response = await fetch('http://127.0.0.1:' + running.port + server.HANDOFF_PATH);
    return response.json();
  }
  t.after(async function () {
    await Promise.all(servers.map(function (running) { return running.close(); }));
    roots.forEach(function (root) { fs.rmSync(root, { recursive: true, force: true }); });
  });
  assert.deepEqual(await available(undefined), { ok: true, available: false });
  assert.deepEqual(await available(function () {}), { ok: true, available: true });
});

test('JPEG captures keep their format through saving, readback, upload and handoff', async function (t) {
  var made = project('http://localhost:6006');
  var image = Buffer.from([0xff, 0xd8, 0xff, 0xd9]);
  var calls = [];
  var copiedPrompt;
  var running = await server.start({ root: made.root, capture: {
    capture: function (_base, payload) { calls.push(payload); return image; },
    capturePage: function (payload) { calls.push(payload); return { image: image, targets: ['h1'] }; },
    close: function () {},
  }, onHandoff: function (text) { copiedPrompt = text; } });
  t.after(async function () { await running.close(); fs.rmSync(made.root, { recursive: true, force: true }); });
  var base = 'http://127.0.0.1:' + running.port;
  var payload = { url: base + '/page.html', width: 393, height: 852, name: 'page.png', format: 'jpeg' };
  var shot = await post(running.port, server.CAPTURE_PATH, payload);
  assert.equal(shot.status, 200);
  assert.equal(shot.body.file, path.join('.canonic', '.handoffs', 'page.jpg'));
  var read = await fetch(base + '/' + shot.body.file);
  assert.equal(read.headers.get('content-type'), 'image/jpeg');
  assert.deepEqual(Buffer.from(await read.arrayBuffer()), image);
  var external = await post(running.port, server.CAPTURE_PAGE_PATH, Object.assign({}, payload, { url: 'http://localhost:6006/' }));
  assert.equal(external.status, 200);
  assert.equal(external.body.file, path.join('.canonic', '.handoffs', 'page-2.jpg'));
  assert.deepEqual(external.body.targets, ['h1']);
  var downloaded = await fetch(base + server.CAPTURE_IMAGE_PATH, {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(payload),
  });
  assert.equal(downloaded.status, 200);
  assert.equal(downloaded.headers.get('content-type'), 'image/jpeg');
  assert.deepEqual(Buffer.from(await downloaded.arrayBuffer()), image);
  var downloadedExternal = await fetch(base + server.CAPTURE_PAGE_IMAGE_PATH, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(Object.assign({}, payload, { url: 'http://localhost:6006/' })),
  });
  assert.equal(downloadedExternal.status, 200);
  assert.equal(downloadedExternal.headers.get('content-type'), 'image/jpeg');
  assert.deepEqual(Buffer.from(await downloadedExternal.arrayBuffer()), image);
  assert.deepEqual(
    fs.readdirSync(path.join(made.root, '.canonic', '.handoffs')).sort(),
    ['page-2.jpg', 'page.jpg']
  );
  assert.ok(calls.every(function (call) { return call.format === 'jpeg'; }));
  var uploaded = await fetch(base + server.SHOT_PATH + '?name=upload.png', {
    method: 'POST', headers: { 'Content-Type': 'image/jpeg' }, body: image,
  });
  assert.equal((await uploaded.json()).file, path.join('.canonic', '.handoffs', 'upload.jpg'));
  var handed = await post(running.port, '/_workbench/handoff', {
    file: shot.body.file, src: 'page.html', label: 'Page', width: 393, frame: { w: 393, h: 852 }, marks: [],
  });
  assert.equal(handed.status, 200, JSON.stringify(handed.body));
  assert.match(copiedPrompt, /Screenshot: `\.canonic\/\.handoffs\/page\.jpg`/);
  assert.equal(fs.existsSync(path.join(made.root, '.canonic', '.handoffs', 'page.md')), false);
  var invalid = await post(running.port, server.CAPTURE_PATH, Object.assign({}, payload, { format: 'gif' }));
  assert.match(invalid.body.error, /format must be/);
});

test('logs requested and renderer-applied capture scroll coordinates', async function (t) {
  var made = project('http://localhost:6006');
  var entries = [];
  var image = Buffer.from([1, 2, 3]);
  var running = await server.start({ root: made.root, onLog: function (entry) { entries.push(entry); }, capture: {
    capturePage: function () {
      return { image: image, details: { scroll: { requestedX: 0, requestedY: 280, appliedX: 0, appliedY: 0 } } };
    },
    close: function () {},
  } });
  t.after(async function () { await running.close(); fs.rmSync(made.root, { recursive: true, force: true }); });
  var shot = await post(running.port, server.CAPTURE_PAGE_PATH, {
    url: 'http://localhost:6006/iframe.html', width: 393, height: 852,
    scroll: { x: 0, y: 280 }, name: 'scroll-diagnostic',
  });
  assert.equal(shot.status, 200);
  var captures = entries.filter(function (entry) { return /^capture\./.test(entry.event); });
  assert.deepEqual(captures.map(function (entry) { return [
    entry.event, entry.details.surface, entry.details.mirrored,
    entry.details.requestedY, entry.details.rendererReported, entry.details.appliedY,
  ]; }), [
    ['capture.requested', 'implementation', false, 280, false, undefined],
    ['capture.completed', 'implementation', false, 280, true, 0],
  ]);
});

test('prepares only configured implementation origins and carries reload revisions', async function (t) {
  var made = project('http://localhost:6006');
  var calls = [];
  var running = await server.start({ root: made.root, capture: {
    preparePage: function (payload) { calls.push(payload); }, close: function () {},
  } });
  t.after(async function () { await running.close(); fs.rmSync(made.root, { recursive: true, force: true }); });
  var denied = await post(running.port, server.CAPTURE_PAGE_PREPARE_PATH, { url: 'https://example.com/', width: 960, height: 720 });
  assert.equal(denied.status, 403); assert.equal(calls.length, 0);
  var prepared = await post(running.port, server.CAPTURE_PAGE_PREPARE_PATH, {
    url: 'http://localhost:6006/login', width: 393, height: 852, revision: 'reload-2',
  });
  assert.equal(prepared.status, 200); assert.equal(prepared.body.prepared, true);
  assert.equal(calls[0].revision, 'reload-2');
});

test('bridged implementation captures use the supplied live DOM instead of reloading the page', async function (t) {
  var made = project('http://localhost:6006');
  var calls = [];
  var image = Buffer.from([1, 2, 3]);
  var running = await server.start({ root: made.root, capture: {
    prepare: function (base, payload) { calls.push(['prepare', base, payload]); },
    preparePage: function (payload) { calls.push(['preparePage', payload]); },
    capture: function (base, payload) { calls.push(['capture', base, payload]); return image; },
    capturePage: function (payload) { calls.push(['capturePage', payload]); return image; },
    close: function () {},
  } });
  t.after(async function () { await running.close(); fs.rmSync(made.root, { recursive: true, force: true }); });
  var mirror = { version: 1, revision: 'bridge-1', base: null, root: '1', nodes: [
    { id: '1', type: 1, tag: 'html', ns: 'http://www.w3.org/1999/xhtml', attrs: [], children: [] },
  ], removed: [], states: [], cleared: [], defined: [], animations: [] };
  var payload = { url: 'http://localhost:6006/iframe.html', width: 393, height: 852, revision: 'view-1', mirror: mirror };
  var prepared = await post(running.port, server.CAPTURE_PAGE_PREPARE_PATH, payload);
  assert.equal(prepared.status, 200);
  var shot = await post(running.port, server.CAPTURE_PAGE_PATH, payload);
  assert.equal(shot.status, 200);
  assert.deepEqual(calls.map(function (call) { return call[0]; }), ['prepare', 'capture']);
  assert.equal(calls[0][2].mirror.revision, 'bridge-1');
  assert.match(calls[0][2].url, /\/_workbench\/bridged-preview$/);
  assert.match(calls[0][1], /^http:\/\/127\.0\.0\.1:/);
});
