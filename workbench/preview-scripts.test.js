var test = require('node:test');
var assert = require('node:assert/strict');
var vm = require('node:vm');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var scripts = require('./preview-scripts');

test('the bundle initializes keyboard forwarding, actions and page states together', function () {
  var handlers = {};
  var attributes = {};
  var messages = [];
  var form = {};
  var removed = false;
  var window = { parent: { wbEmbedded: true, postMessage: function (message) { messages.push(message); } },
    addEventListener: function (name, handler) { handlers[name] = handler; } };
  var document = {
    readyState: 'complete', forms: [form],
    documentElement: { setAttribute: function (name, value) { attributes[name] = value; } },
    addEventListener: function (name, handler) { handlers[name] = handler; },
    querySelectorAll: function (selector) {
      return selector === '[data-wb-state-only]' ? [{ getAttribute: function () { return 'default'; },
        remove: function () { removed = true; } }] : [];
    },
  };
  vm.runInNewContext(scripts.source, { window: window, document: document,
    location: { search: '?actions=off&state=error' }, URL: URL });
  assert.equal(attributes['data-wb-actions'], 'off');
  assert.equal(attributes['data-wb-state'], 'error');
  assert.equal(form.noValidate, true);
  assert.equal(removed, true);
  var prevented = false;
  handlers.click({ target: { tagName: 'A', hasAttribute: function () { return true; } },
    preventDefault: function () { prevented = true; } });
  assert.equal(prevented, true);
  handlers.keydown({ type: 'keydown', key: 'F5', preventDefault: function () {} });
  assert.equal(messages[0].type, 'wb-keyboard');
  assert.equal(messages[0].event.key, 'F5');
});

test('served HTML receives the same single compatibility injection as compiled previews', async function (t) {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-compat-'));
  t.after(function () { fs.rmSync(root, { recursive: true, force: true }); });
  fs.writeFileSync(path.join(root, 'index.html'), '<html><head><title>Acme</title></head><body></body></html>');
  var running = await require('./server').start({ root: root, capture: { close: function () {} } });
  t.after(function () { return running.close(); });
  var base = 'http://127.0.0.1:' + running.port;
  var page = await (await fetch(base + '/')).text();
  assert.equal(page, scripts.withPreviewScripts(fs.readFileSync(path.join(root, 'index.html'), 'utf8')));
  var bundle = await fetch(base + '/_workbench/preview-compat.js');
  assert.equal(bundle.status, 200);
  assert.equal(await bundle.text(), scripts.source);
});
