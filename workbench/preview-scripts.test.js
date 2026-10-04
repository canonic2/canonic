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
  handlers.click({ target: { tagName: 'A', hasAttribute: function () { return true; }, getAttribute: function () { return 'next.html'; } },
    preventDefault: function () { prevented = true; } });
  assert.equal(prevented, true);
  handlers.keydown({ type: 'keydown', key: 'F5', preventDefault: function () {} });
  assert.equal(messages[0].type, 'wb-keyboard');
  assert.equal(messages[0].event.key, 'F5');
});

test('in-page anchors scroll in either position and a mounted preview claims live links and forms', function () {
  var handlers = {};
  var scrolled = [];
  var window = { parent: {}, addEventListener: function () {} };
  var document = {
    readyState: 'complete', forms: [],
    documentElement: { setAttribute: function () {} },
    addEventListener: function (name, handler) { handlers[name] = handler; },
    querySelectorAll: function () { return []; },
    getElementById: function (id) { return id === 'tour' ? { scrollIntoView: function () { scrolled.push(id); } } : null; },
    getElementsByName: function () { return []; },
  };
  vm.runInNewContext(scripts.source, { window: window, document: document,
    location: { search: '?actions=off', href: 'http://127.0.0.1/page.html', protocol: 'http:', origin: 'http://127.0.0.1' }, URL: URL });
  function click(href) {
    var prevented = false;
    handlers.click({ target: { tagName: 'A', hasAttribute: function () { return true; }, getAttribute: function () { return href; } },
      preventDefault: function () { prevented = true; } });
    return prevented;
  }
  var actions = window.wbPreviewActions;
  assert.equal(click('#tour'), true);
  assert.deepEqual(scrolled, ['tour']);

  var followed = [];
  actions.follow = function (href, kind) { followed.push(kind + ' ' + href); return true; };
  assert.equal(click('https://example.com/acme.zip'), true);
  assert.deepEqual(followed, [], 'off stops links before a preview sees them');

  actions.configure('?actions=on');
  assert.equal(actions.on(), true);
  click('#tour');
  assert.deepEqual(scrolled, ['tour', 'tour']);
  assert.equal(click('https://example.com/acme.zip'), true);
  var submitted = false;
  handlers.submit({ target: { tagName: 'FORM', getAttribute: function () { return '/sign-up'; } }, preventDefault: function () { submitted = true; } });
  assert.equal(submitted, true);
  assert.deepEqual(followed, ['link https://example.com/acme.zip', 'submit /sign-up']);
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
