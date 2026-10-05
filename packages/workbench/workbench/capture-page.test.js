var assert = require('node:assert/strict');
var test = require('node:test');
var fs = require('node:fs');
var path = require('node:path');
var vm = require('node:vm');
function page(extra) {
  var loads = []; var paints = 0; var scrolls = []; var mirrors = []; var sandbox;
  var listeners = {};
  var behavior = 'smooth';
  var style = {
    getPropertyValue: function () { return behavior; },
    getPropertyPriority: function () { return ''; },
    setProperty: function (_, value) { behavior = value; },
    removeProperty: function () { behavior = ''; },
  };
  var doc = { readyState: 'complete', fonts: { ready: Promise.resolve() }, images: [], documentElement: { style: style } };
  var inner = { document: doc, location: { href: 'about:blank' }, scrollX: 0, scrollY: 0,
    scrollTo: function (x, y) { this.scrollX = x; this.scrollY = y; scrolls.push([x, y, behavior]); }, requestAnimationFrame: paint };
  doc.defaultView = inner;
  function paint(fn) { paints++; fn(); }
  var frame = { contentWindow: inner, contentDocument: doc,
    addEventListener: function (name, fn) { listeners[name] = fn; }, removeEventListener: function (name) { delete listeners[name]; },
    set src(url) { loads.push(url); inner.location.href = url; listeners.load(); },
    set srcdoc(html) { loads.push('srcdoc'); inner.location.href = 'about:srcdoc'; listeners.load(); },
    setAttribute: function (name, value) { if (name === 'sandbox') sandbox = value; },
    removeAttribute: function () {},
  };
  var annotations = { innerHTML: '' };
  var window = Object.assign({ setTimeout: setTimeout, requestAnimationFrame: paint, wbDOMMirror: { apply: async function (_, snapshot) { mirrors.push(snapshot.revision); } } }, extra);
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'capture-page.js'), 'utf8'), {
    window: window, document: { getElementById: function (id) { return id === 'captureFrame' ? frame : annotations; } },
  });
  return { inner: inner, prepare: window.wbCapture.prepare, loads: loads, mirrors: mirrors, sandbox: function () { return sandbox; }, paints: function () { return paints; }, annotations: annotations, scrolls: scrolls, behavior: function () { return behavior; } };
}

test('an exact prepared view needs no navigation or extra settling before capture', async function () {
  var p = page(); var request = { url: 'http://localhost/one', revision: '1', width: 960, height: 720, annotations: '<svg/>', scroll: { x: 0, y: 100 } };
  var result = await p.prepare(request); var painted = p.paints();
  var reused = await p.prepare(request);
  assert.equal(p.loads.length, 1); assert.equal(p.paints(), painted);
  assert.equal(p.annotations.innerHTML, '<svg/>'); assert.deepEqual(p.scrolls.at(-1), [0, 100, 'auto']);
  assert.equal(p.behavior(), 'smooth');
  assert.equal(JSON.stringify(result.scroll), JSON.stringify({ requestedX: 0, requestedY: 100, appliedX: 0, appliedY: 100 }));
  assert.equal(JSON.stringify(reused.scroll), JSON.stringify(result.scroll));
});

test('live updates reuse an inert document, exact captures skip apply, and navigation resets it', async function () {
  var p = page(); var request = { url: 'http://localhost/one', revision: '1', mirror: { revision: 'snapshot-1' } };
  await p.prepare(request); var painted = p.paints();
  await p.prepare(request);
  assert.deepEqual(p.mirrors, ['snapshot-1']); assert.equal(p.paints(), painted);
  assert.equal(p.sandbox(), 'allow-same-origin');
  request.mirror = { revision: 'snapshot-2' }; await p.prepare(request);
  assert.deepEqual(p.loads, ['srcdoc']); assert.deepEqual(p.mirrors, ['snapshot-1', 'snapshot-2']);
  request.revision = '2'; await p.prepare(request);
  assert.deepEqual(p.loads, ['srcdoc', 'srcdoc']);
  delete request.mirror; await p.prepare(request);
  assert.equal(p.loads.at(-1), request.url);
});

test('a mirrored capture names the element under each annotation from its own copy', async function () {
  var asked = [];
  var p = page({ wbDescribe: { at: function (doc, x, y) { asked.push([doc, x, y]); return 'button.create “Create Customer”'; } } });
  var result = await p.prepare({ url: 'http://localhost/one', revision: '1', mirror: { revision: 'snapshot-1' },
    anchors: [{ x: 120, y: 40 }, null] });
  assert.deepEqual(Array.from(result.targets), ['button.create “Create Customer”', null]);
  assert.equal(asked.length, 1); assert.equal(asked[0][0], p.inner.document); assert.deepEqual(asked[0].slice(1), [120, 40]);
  var plain = await p.prepare({ url: 'http://localhost/one', revision: '1', mirror: { revision: 'snapshot-1' } });
  assert.equal(plain.targets, undefined);
});

test('a reload revision refreshes even the same URL; geometry and annotations settle without reloading', async function () {
  var p = page(); var request = { url: 'http://localhost/one', revision: '1', width: 960, height: 720 };
  await p.prepare(request); await p.prepare(Object.assign({}, request, { width: 393, annotations: 'new annotations' }));
  assert.equal(p.loads.length, 1); assert.equal(p.annotations.innerHTML, 'new annotations');
  await p.prepare(Object.assign({}, request, { revision: '2' }));
  assert.equal(p.loads.length, 2);
});

test('a request without a revision loads the page afresh every time', async function () {
  var p = page(); var request = { url: 'http://localhost/one', width: 960, height: 720 };
  await p.prepare(request); await p.prepare(request);
  assert.equal(p.loads.length, 2);
  await p.prepare(Object.assign({}, request, { revision: '' }));
  assert.equal(p.loads.length, 3);
});

test('reference capture waits for Workbench setup and interaction playback', async function () {
  var p = page();
  p.inner.__workbenchOptions = {};
  p.inner.__workbenchReady = false;
  var complete = false;
  var ready = p.prepare({ url: 'http://localhost/button.workbench.ts' }).then(function () { complete = true; });
  await new Promise(setImmediate);
  assert.equal(complete, false);
  assert.equal(p.paints(), 0);
  p.inner.__workbenchReady = true;
  await ready;
  assert.equal(complete, true);
});

test('Workbench render errors fail reference capture instead of exporting an error screenshot', async function () {
  var p = page();
  p.inner.__workbenchOptions = {};
  p.inner.__workbenchError = 'Expected render failure';
  await assert.rejects(p.prepare({ url: 'http://localhost/button.workbench.ts' }), /Expected render failure/);
  assert.equal(p.paints(), 0);
});
