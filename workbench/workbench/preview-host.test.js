var test = require('node:test');
var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var vm = require('node:vm');

function host(options = {}) {
  var mounted = []; var cleaned = []; var fetched = []; var styles = new Map();
  var modules = new Map(); var imported = [];
  var rootListeners = []; var cancelledTimers = []; var timerId = 0;
  function element(name) {
    var attributes = {};
    return { tagName: String(name || '').toUpperCase(), attributes: [], children: [],
      removeAttribute: function (name) { delete attributes[name]; }, setAttribute: function (name, value) { attributes[name] = value; },
      get childNodes() { return this.children; },
      remove: function () { if (this.parent) this.parent.children = this.parent.children.filter(child => child !== this); this.parent = null; },
      querySelectorAll: function () { return this.children.filter(child => child.tagName === 'STYLE' && !Object.hasOwn(child.attributesMap || {}, 'data-wb-host-style')); },
      attributesMap: attributes,
      replaceChildren: function (...children) { this.children.forEach(child => { child.parent = null; }); this.children = []; children.forEach(child => this.append(child)); },
      append: function (child) { child.remove(); child.parent = this; this.children.push(child); if (child.onload) queueMicrotask(child.onload); } };
  }
  var document = { head: element(), body: element(), documentElement: element(), createElement: element };
  var window = { wbPreviewActions: { configure: function () {} }, clearTimeout: function (id) { cancelledTimers.push(id); }, clearInterval: function () {},
    cancelAnimationFrame: function () {}, setTimeout: function () { return ++timerId; }, setInterval: function () {}, requestAnimationFrame: function () {} };
  class FakeEventTarget {
    addEventListener(type, listener) { rootListeners.push([this, type, listener]); }
    removeEventListener(type, listener) { rootListeners = rootListeners.filter(entry => entry[0] !== this || entry[1] !== type || entry[2] !== listener); }
  }
  Object.setPrototypeOf(window, FakeEventTarget.prototype);
  Object.setPrototypeOf(document, FakeEventTarget.prototype);
  var context = { window: window, document: document, EventTarget: FakeEventTarget, URL: URL, AbortController: AbortController,
    location: { href: 'http://127.0.0.1/_workbench/preview-host.html', origin: 'http://127.0.0.1' },
    history: { replaceState: function () {} },
    fetch: async function (address) {
      fetched.push(address);
      var file = new URL(address, 'http://127.0.0.1').searchParams.get('file');
      return { ok: true, json: async function () { return { base: '/assets/', module: file, title: file, options: {} }; } };
    },
    importModule: function (name) {
      if (!modules.has(name)) modules.set(name, (async function () {
        imported.push(name);
        await options.import?.(name, document);
        return { mount: async function () {
          mounted.push(name);
          await options.mount?.(name, document);
          if (!styles.has(name)) {
            var style = element('style'); style.sheet = { disabled: false, cssRules: [] };
            styles.set(name, style); document.head.append(style);
          }
          window.__workbenchReady = true;
          var disposed = false;
          window.__workbenchStop = async function () { if (!disposed) { disposed = true; cleaned.push(name); } };
        } };
      })());
      return modules.get(name);
    },
  };
  var source = fs.readFileSync(path.join(__dirname, 'preview-host.js'), 'utf8');
  vm.runInNewContext(source.replace("import '/_workbench/preview-runtime.js';", '').replace('import(content.module)', 'importModule(content.module)'), context);
  return { renderer: window.wbPreviewHost, window: window, mounted: mounted, cleaned: cleaned, fetched: fetched,
    styles: styles, document: document, imported: imported, listeners: function () { return rootListeners; }, cancelledTimers: cancelledTimers };
}

test('a warm host mounts different previews and resets without replacing its compatibility bridge', async function () {
  var h = host(); var bridge = h.window.wbPreviewActions;
  await h.renderer.load('/one.workbench.ts?state=default');
  await h.renderer.reset();
  await h.renderer.load('/two.workbench.tsx');
  assert.deepEqual(h.mounted, ['one.workbench.ts', 'two.workbench.tsx']);
  assert.deepEqual(h.cleaned, ['one.workbench.ts']);
  assert.equal(h.window.wbPreviewActions, bridge);
  assert.equal(h.window.__workbenchReady, true);
});

test('a newer load supersedes a queued preview before it can mount', async function () {
  var h = host();
  await Promise.all([h.renderer.load('/old.workbench.ts'), h.renderer.load('/new.workbench.ts')]);
  assert.deepEqual(h.mounted, ['new.workbench.ts']);
});

test('library-owned styles remain connected, isolated and reusable on return', async function () {
  var h = host();
  await h.renderer.load('/one.workbench.ts');
  var one = h.styles.get('one.workbench.ts');
  await h.renderer.load('/two.workbench.ts');
  var two = h.styles.get('two.workbench.ts');
  assert.equal(one.sheet.disabled, true);
  assert.equal(two.sheet.disabled, false);
  await h.renderer.load('/one.workbench.ts');
  assert.equal(one.sheet.disabled, false);
  assert.equal(two.sheet.disabled, true);
  assert.ok(h.document.head.children.includes(one));
  assert.ok(h.document.head.children.includes(two));
});

test('cached imports restore their SVG sprite before every mount without retaining portals', async function () {
  var sprite; var portals = [];
  var h = host({
    import: function (_name, document) {
      sprite = document.createElement('svg'); sprite.id = '__svg__icons__dom__';
      var symbol = document.createElement('symbol'); symbol.id = 'icon-acme';
      sprite.append(symbol); document.body.append(sprite);
    },
    mount: function (_name, document) {
      assert.ok(document.body.children.includes(sprite));
      assert.equal(sprite.children[0].id, 'icon-acme');
      assert.equal(document.body.children.length, 1);
      var portal = document.createElement('div');
      portals.push(portal); document.body.append(portal);
    },
  });
  await h.renderer.load('/one.workbench.ts?state=default');
  await h.renderer.load('/one.workbench.ts?state=with-filters');
  await h.renderer.reset();
  assert.equal(h.document.body.children.length, 0);
  await h.renderer.load('/one.workbench.ts?state=default');
  assert.deepEqual(h.imported, ['one.workbench.ts']);
  assert.equal(h.mounted.length, 3);
  assert.equal(h.window.__workbenchError, null);
  assert.deepEqual(h.document.body.children, [sprite, portals[2]]);
  assert.equal(portals[0].parent, null);
  assert.equal(portals[1].parent, null);
});

test('import-time body nodes belong to their module and preserve identity and order on return', async function () {
  var nodes = new Map();
  var h = host({
    import: function (name, document) {
      var children = [document.createElement('svg'), document.createElement('span')];
      nodes.set(name, children); children.forEach(child => document.body.append(child));
    },
    mount: function (name, document) { assert.deepEqual(document.body.childNodes, nodes.get(name)); },
  });
  await h.renderer.load('/one.workbench.ts');
  await h.renderer.load('/two.workbench.ts');
  assert.ok(nodes.get('one.workbench.ts').every(node => node.parent === null));
  await h.renderer.load('/one.workbench.ts');
  assert.deepEqual(h.document.body.childNodes, nodes.get('one.workbench.ts'));
  assert.ok(nodes.get('two.workbench.ts').every(node => node.parent === null));
  assert.equal(h.window.__workbenchError, null);
  assert.deepEqual(h.imported, ['one.workbench.ts', 'two.workbench.ts']);
});

test('a failed mount does not discard the cached module sprite', async function () {
  var sprite; var attempts = 0;
  var h = host({
    import: function (_name, document) { sprite = document.createElement('svg'); document.body.append(sprite); },
    mount: function (_name, document) {
      assert.ok(document.body.children.includes(sprite));
      if (++attempts === 1) throw new Error('Mount failed');
    },
  });
  await h.renderer.load('/one.workbench.ts');
  assert.match(h.window.__workbenchError, /Mount failed/);
  assert.equal(sprite.parent, null);
  await h.renderer.load('/one.workbench.ts');
  assert.equal(h.window.__workbenchError, null);
  assert.equal(h.window.__workbenchReady, true);
  assert.deepEqual(h.imported, ['one.workbench.ts']);
  assert.ok(h.document.body.children.includes(sprite));
});

test('superseding an in-flight import retains its body nodes for a later return', async function () {
  var started; var finish; var sprite;
  var importing = new Promise(resolve => { started = resolve; });
  var blocked = new Promise(resolve => { finish = resolve; });
  var h = host({ import: async function (name, document) {
    if (name !== 'one.workbench.ts') return;
    started(); await blocked;
    sprite = document.createElement('svg'); document.body.append(sprite);
  } });
  var first = h.renderer.load('/one.workbench.ts');
  await importing;
  var second = h.renderer.load('/two.workbench.ts');
  finish(); await Promise.all([first, second]);
  assert.deepEqual(h.mounted, ['two.workbench.ts']);
  assert.equal(sprite.parent, null);
  await h.renderer.load('/one.workbench.ts');
  assert.deepEqual(h.imported, ['one.workbench.ts', 'two.workbench.ts']);
  assert.ok(h.document.body.children.includes(sprite));
});

test('page root listeners and timers are released while compatibility listeners stay installed', async function () {
  var h = host();
  h.window.addEventListener('compatibility', function () {});
  await h.renderer.load('/one.workbench.ts');
  h.window.addEventListener('page', function () {});
  h.document.addEventListener('page', function () {});
  var timer = h.window.setTimeout(function () {}, 5000);
  await h.renderer.reset();
  assert.deepEqual(h.listeners().map(entry => entry[1]), ['compatibility']);
  assert.ok(h.cancelledTimers.includes(timer));
});

test('warm hosts reject external pages and undeclared document types before fetching', async function () {
  var h = host();
  await assert.rejects(h.renderer.load('https://example.com/one.workbench.ts'), /local Workbench/);
  await assert.rejects(h.renderer.load('/one.html'), /local Workbench/);
  assert.equal(h.fetched.length, 0);
});
