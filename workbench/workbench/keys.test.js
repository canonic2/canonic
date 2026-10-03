var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function load(options) {
  var listeners = {};
  var sent = [];
  var commands = [];
  var document = {
    execCommand: function (command) { commands.push(command); return true; },
    getSelection: function () { return 'Selected preview text'; },
  };
  var window = {
    addEventListener: function (type, fn) { listeners[type] = fn; },
  };
  var destination = {
    postMessage: function (message) { sent.push(message); },
  };
  var file = path.join(__dirname, 'keys.js');
  vm.runInNewContext(fs.readFileSync(file, 'utf8'), { window: window, document: document, URL: URL }, { filename: file });
  window.wbKeys.relay(destination, options);
  return { destination: destination, listeners: listeners, sent: sent, window: window, commands: commands, document: document };
}

function event(overrides) {
  return Object.assign({
    type: 'keydown', key: '', code: '', keyCode: 0, location: 0,
    shiftKey: false, altKey: false, ctrlKey: false, metaKey: false, repeat: false,
    target: { nodeType: 1, tagName: 'DIV', isContentEditable: false },
    prevented: false,
    preventDefault: function () { this.prevented = true; },
  }, overrides || {});
}

function input() {
  return {
    nodeType: 1,
    tagName: 'INPUT',
    isContentEditable: false,
    selected: false,
    select: function () { this.selected = true; },
  };
}

test('relays editor chords and blocks their browser default where needed', function () {
  var f = load();
  var e = event({ key: 'P', code: 'KeyP', keyCode: 80, metaKey: true, shiftKey: true });
  f.listeners.keydown(e);

  assert.equal(e.prevented, true);
  assert.equal(f.sent.length, 1);
  assert.equal(f.sent[0].type, 'wb-keyboard');
  assert.equal(f.sent[0].eventType, 'keydown');
  assert.equal(f.sent[0].event.key, 'P');
  assert.equal(f.sent[0].event.metaKey, true);
  assert.equal(f.sent[0].event.shiftKey, true);
});

test('keeps canvas zoom chords out of the editor relay', function () {
  var f = load();
  [
    event({ key: '0', code: 'Digit0', metaKey: true }),
    event({ key: '=', code: 'Equal', metaKey: true }),
    event({ key: '+', code: 'Equal', metaKey: true, shiftKey: true }),
    event({ key: '-', code: 'Minus', ctrlKey: true }),
  ].forEach(function (e) { f.listeners.keydown(e); });
  assert.equal(f.sent.length, 0);

  /* Other digits still reach the editor, e.g. ⌘1 to focus an editor group. */
  f.listeners.keydown(event({ key: '1', code: 'Digit1', metaKey: true }));
  assert.equal(f.sent.length, 1);
});

test('keeps editing commands local and forwards ordinary keys without cancelling typing', function () {
  var f = load();
  var typed = event({ key: 'a', code: 'KeyA' });
  f.listeners.keydown(typed);
  f.listeners.keydown(event({ key: 'c', code: 'KeyC', metaKey: true }));
  f.listeners.keydown(event({
    key: 'x', code: 'KeyX', metaKey: true,
    target: { nodeType: 1, tagName: 'INPUT', isContentEditable: false },
  }));
  assert.equal(typed.prevented, false);
  assert.deepEqual(f.sent.map(function (message) { return message.event.key; }), ['a']);
  assert.deepEqual(f.commands, ['copy', 'cut']);
});

test('selects the real input inside a shadow-root event path', function () {
  var f = load();
  var field = input();
  var host = { nodeType: 1, tagName: 'BZ-TEXT-INPUT', isContentEditable: false };
  var e = event({
    key: 'a', code: 'KeyA', metaKey: true, target: host,
    composedPath: function () { return [field, host]; },
  });

  f.listeners.keydown(e);

  assert.equal(field.selected, true);
  assert.equal(e.prevented, true);
  assert.deepEqual(f.sent, []);
});

test('leaves select-all native when no editable control has focus', function () {
  var f = load();
  var e = event({ key: 'a', code: 'KeyA', metaKey: true });

  f.listeners.keydown(e);

  assert.equal(e.prevented, false);
  assert.deepEqual(f.sent, []);
});

test('relays function keys and non-editing alt shortcuts', function () {
  var f = load();
  f.listeners.keydown(event({ key: 'F1', code: 'F1', keyCode: 112 }));
  f.listeners.keydown(event({ key: 'f', code: 'KeyF', altKey: true }));
  assert.equal(f.sent.length, 2);
});

test('does not swallow browser shortcuts when no editor is listening', function () {
  var f = load({ enabled: function () { return false; } });
  for (var key of ['p', 'f', 's', 'c']) {
    var e = event({ key: key, metaKey: true });
    f.listeners.keydown(e);
    assert.equal(e.prevented, false);
  }
  assert.deepEqual(f.sent, []);
  assert.deepEqual(f.commands, []);
});

test('forwards modified editor commands that share letters with copy and select all', function () {
  var f = load();
  f.listeners.keydown(event({ key: 'C', metaKey: true, shiftKey: true }));
  f.listeners.keydown(event({ key: 'A', metaKey: true, shiftKey: true, target: input() }));
  assert.deepEqual(f.sent.map(function (message) { return message.event.key; }), ['C', 'A']);
  assert.deepEqual(f.commands, []);
});

test('forwards an unmodified chord continuation without cancelling its browser behavior', function () {
  var f = load();
  f.listeners.keydown(event({ key: 'k', metaKey: true }));
  var second = event({ key: 'u' });
  f.listeners.keydown(second);
  assert.equal(second.prevented, false);
  assert.deepEqual(f.sent.map(function (message) { return message.event.key; }), ['k', 'u']);
});

test('leaves modified caret movement and AltGraph typing in shadow-root inputs', function () {
  var f = load();
  var field = input();
  var host = { tagName: 'ACME-INPUT', nodeType: 1 };
  for (var value of [
    { key: 'ArrowLeft', metaKey: true }, { key: 'ArrowRight', altKey: true },
    { key: '@', ctrlKey: true, altKey: true, getModifierState: function () { return true; } },
  ]) {
    var e = event(Object.assign({ target: host, composedPath: function () { return [field, host]; } }, value));
    f.listeners.keydown(e);
    assert.equal(e.prevented, false);
  }
  assert.deepEqual(f.sent, []);
});

test('forwards the preview selection with a context-menu request and clears stale context', function () {
  var f = load();
  var e = event({ clientX: 20, clientY: 30, defaultPrevented: false });
  f.listeners.contextmenu(e);
  assert.equal(e.prevented, true);
  assert.deepEqual(JSON.parse(JSON.stringify(f.sent[0])), {
    type: 'wb-context-menu', x: 20, y: 30, selection: 'Selected preview text', editable: false,
  });
  f.listeners.pointerdown();
  assert.equal(f.sent[1].type, 'wb-context-reset');
});

test('preserves page-provided menus and standalone browser menus', function () {
  for (var options of [{}, { enabled: function () { return false; } }]) {
    var f = load(options);
    var e = event({ defaultPrevented: !options.enabled });
    f.listeners.contextmenu(e);
    assert.equal(e.prevented, false);
    assert.deepEqual(f.sent, []);
  }
});

test('context-menu paste reaches the focused shadow-root input only from the parent', function () {
  var f = load();
  f.document.activeElement = { shadowRoot: { activeElement: input() } };
  f.listeners.message({ source: {}, data: { type: 'wb-edit', command: 'paste', text: 'no' } });
  assert.deepEqual(f.commands, []);
  f.listeners.message({ source: f.destination, data: { type: 'wb-edit', command: 'paste', text: 'pasted' } });
  assert.deepEqual(f.commands, ['insertText']);
});

test('password menus omit the value and reject cut while still allowing paste', function () {
  var f = load();
  var field = Object.assign(input(), { type: 'password', selectionStart: 0, selectionEnd: 6 });
  Object.defineProperty(field, 'value', { get: function () { throw new Error('Password value must not be read'); } });
  var host = { tagName: 'ACME-INPUT', nodeType: 1 };
  f.document.activeElement = { shadowRoot: { activeElement: field } };
  f.listeners.contextmenu(event({ target: host, composedPath: function () { return [field, host]; } }));
  assert.equal(f.sent[0].selection, '');
  assert.equal(f.sent[0].editable, true);
  f.listeners.message({ source: f.destination, data: { type: 'wb-edit', command: 'cut' } });
  assert.deepEqual(f.commands, []);
  f.listeners.message({ source: f.destination, data: { type: 'wb-edit', command: 'paste', text: 'replacement' } });
  assert.deepEqual(f.commands, ['insertText']);
});

test('cut rechecks text-field selection after the menu opens', function () {
  for (var tagName of ['INPUT', 'TEXTAREA']) {
    var f = load();
    var field = Object.assign(input(), { tagName: tagName, value: 'Preview', selectionStart: 0, selectionEnd: 7 });
    f.document.activeElement = field;
    f.listeners.contextmenu(event({ target: field }));
    assert.equal(f.sent[0].selection, 'Preview');
    field.selectionEnd = 0;
    var message = { source: f.destination, data: { type: 'wb-edit', command: 'cut' } };
    f.listeners.message(message);
    assert.deepEqual(f.commands, []);
    field.selectionEnd = 7;
    f.listeners.message(message);
    assert.deepEqual(f.commands, ['delete']);
  }
});

test('cut requires selected text in contenteditable fields', function () {
  var f = load();
  f.document.activeElement = { nodeType: 1, tagName: 'DIV', isContentEditable: true };
  var selected = '';
  f.document.getSelection = function () { return selected; };
  var message = { source: f.destination, data: { type: 'wb-edit', command: 'cut' } };
  f.listeners.message(message);
  assert.deepEqual(f.commands, []);
  selected = 'Selected note';
  f.listeners.message(message);
  assert.deepEqual(f.commands, ['delete']);
});

test('accepts editor shortcuts only from the active Storybook iframe and its origin', function () {
  var keys = load().window.wbKeys;
  var frame = { contentWindow: {} };
  var message = {
    source: frame.contentWindow, origin: 'http://localhost:6006',
    data: JSON.stringify({ key: 'storybook-channel', event: { type: 'previewKeydown', args: [
      { event: { key: 'P', code: 'KeyP', keyCode: 80, metaKey: true, shiftKey: true } },
    ] } }),
  };
  assert.equal(keys.storybook(message, frame, 'http://localhost:6006').event.key, 'P');
  assert.equal(keys.storybook(Object.assign({}, message, { source: {} }), frame, 'http://localhost:6006'), null);
  assert.equal(keys.storybook(Object.assign({}, message, { origin: 'https://example.com' }), frame, 'http://localhost:6006'), null);
  assert.equal(keys.storybook(Object.assign({}, message, { data: 'bad json' }), frame, 'http://localhost:6006'), null);
});

test('the shell cancels select-all over its chrome and leaves fields and notes to it', function () {
  var keys = load().window.wbKeys;
  var source = fs.readFileSync(path.join(__dirname, 'workbench.js'), 'utf8');
  var start = source.indexOf('  /* ⌘A is a text command.');
  var handler = source.slice(source.indexOf("  document.addEventListener('keydown'", start), source.indexOf('}, true);', start) + 9);
  var listener = null;
  vm.runInNewContext(handler, {
    window: { wbKeys: keys },
    document: { addEventListener: function (type, fn, capture) { assert.equal(capture, true); listener = fn; } },
  });

  var chrome = event({ key: 'a', metaKey: true, target: { nodeType: 1, tagName: 'BUTTON', isContentEditable: false } });
  listener(chrome);
  assert.equal(chrome.prevented, true);

  var note = { nodeType: 1, tagName: 'DIV', isContentEditable: true };
  var inNote = event({ key: 'a', metaKey: true, target: note, composedPath: function () { return [note]; } });
  listener(inNote);
  assert.equal(inNote.prevented, false);

  var field = event({ key: 'a', ctrlKey: true, target: input() });
  listener(field);
  assert.equal(field.prevented, false);

  var chord = event({ key: 'a', metaKey: true, shiftKey: true });
  listener(chord);
  assert.equal(chord.prevented, false, 'a chord that is not select-all still relays');
  assert.equal(keys.editingTarget(field), field.target);
});
