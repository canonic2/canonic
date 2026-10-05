var assert = require('node:assert/strict');
var test = require('node:test');
var fs = require('node:fs');
var path = require('node:path');
var vm = require('node:vm');

test('live mirror revisions include the pointer that controls CSS hover', function () {
  var listeners = {};
  var doc = { baseURI: 'http://localhost/page', activeElement: null,
    addEventListener: function (name, listener) { listeners[name] = listener; },
    removeEventListener: function (name) { delete listeners[name]; } };
  doc.defaultView = { MutationObserver: class {
    observe() {}
    takeRecords() { return []; }
    disconnect() {}
  }, customElements: { get: function () { return null; } } };
  doc.documentElement = { nodeType: 1, localName: 'html', namespaceURI: 'http://www.w3.org/1999/xhtml',
    attributes: [], childNodes: [], scrollLeft: 0, scrollTop: 0,
    hasAttribute: function () { return false; }, getRootNode: function () { return doc; } };
  var window = {};
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'dom-mirror.js'), 'utf8'), { window: window });
  var mirror = window.wbDOMMirror.create(doc);
  var initial = mirror.read();
  assert.equal(initial.pointer, null);
  listeners.pointerover({ type: 'pointerover', clientX: 45, clientY: 28 });
  var hovered = mirror.read();
  assert.equal(JSON.stringify(hovered.pointer), JSON.stringify({ x: 45, y: 28 }));
  assert.notEqual(hovered.revision, initial.revision);
  listeners.pointerout({ relatedTarget: null });
  var cleared = mirror.read();
  assert.equal(cleared.pointer, null);
  assert.notEqual(cleared.revision, hovered.revision);
  mirror.stop();
  assert.equal(listeners.pointerover, undefined);
});
