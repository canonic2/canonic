var test = require('node:test');
var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var vm = require('node:vm');

function host() {
  var mounted = []; var cleaned = []; var fetched = []; var styles = new Map();
  var rootListeners = []; var cancelledTimers = []; var timerId = 0;
  function element(name) {
    var attributes = {};
    return { tagName: String(name || '').toUpperCase(), attributes: [], children: [],
      removeAttribute: function (name) { delete attributes[name]; }, setAttribute: function (name, value) { attributes[name] = value; },
      remove: function () { if (this.parent) this.parent.children = this.parent.children.filter(child => child !== this); },
      querySelectorAll: function () { return this.children.filter(child => child.tagName === 'STYLE' && !Object.hasOwn(child.attributesMap || {}, 'data-wb-host-style')); },
      attributesMap: attributes,
      replaceChildren: function () { this.children = []; },
      append: function (child) { child.parent = this; this.children.push(child); if (child.onload) queueMicrotask(child.onload); } };
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
    importModule: async function (name) { return { mount: async function () {
      mounted.push(name);
      if (!styles.has(name)) {
        var style = element('style'); style.sheet = { disabled: false, cssRules: [] };
        styles.set(name, style); document.head.append(style);
      }
      window.__workbenchReady = true;
      var disposed = false;
      window.__workbenchStop = async function () { if (!disposed) { disposed = true; cleaned.push(name); } };
    } }; },
  };
  var source = fs.readFileSync(path.join(__dirname, 'preview-host.js'), 'utf8');
  vm.runInNewContext(source.replace("import '/_workbench/preview-runtime.js';", '').replace('import(content.module)', 'importModule(content.module)'), context);
  return { renderer: window.wbPreviewHost, window: window, mounted: mounted, cleaned: cleaned, fetched: fetched,
    styles: styles, document: document, listeners: function () { return rootListeners; }, cancelledTimers: cancelledTimers };
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
