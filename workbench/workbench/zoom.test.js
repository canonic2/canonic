var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function rules() {
  var file = path.join(__dirname, 'zoom.js');
  var context = { Math: Math };
  vm.runInNewContext(fs.readFileSync(file, 'utf8'), context, { filename: file });
  return context.wbZoomRules;
}

function canvas() {
  var elements = new Map();
  function element(id) {
    if (!elements.has(id)) elements.set(id, {
      style: { setProperty: function () {} }, dataset: {}, hidden: false,
      classList: { add: function () {}, remove: function () {} },
      listeners: {}, setAttribute: function () {},
      addEventListener: function (type, callback) { this.listeners[type] = callback; },
      click: function () { this.listeners.click(); },
    });
    return elements.get(id);
  }
  var stage = element('stage'); stage.clientWidth = 1200; stage.clientHeight = 900;
  var frame = element('frameShell'); frame.offsetWidth = 800; frame.offsetHeight = 600;
  var resize;
  var context = {
    document: {
      querySelector: element, getElementById: element, querySelectorAll: function () { return []; },
      addEventListener: function () {},
    },
    addEventListener: function () {},
    ResizeObserver: function (callback) { resize = callback; this.observe = function () {}; },
    MutationObserver: function () { this.observe = function () {}; },
  };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'zoom.js'), 'utf8'), context);
  return { zoom: context.wbZoom, stage: stage, frame: frame, element: element, resize: function () { resize(); } };
}

test('recenter restores a panned frame at the current zoom without changing its layout size', function () {
  var c = canvas();
  for (var zoomIn of [false, true]) {
    if (zoomIn) c.element('zoomIn').click();
    var scale = c.zoom.scale();
    c.zoom.panTo(-900, -700);
    c.element('zoomCenter').click();
    assert.equal(c.zoom.scale(), scale);
    assert.equal(c.zoom.pan().x + c.frame.offsetWidth * scale / 2, 600);
    assert.equal(c.zoom.pan().y + c.frame.offsetHeight * scale / 2, 430);
    assert.equal(c.frame.offsetWidth, 800);
    assert.equal(c.frame.offsetHeight, 600);
    assert.equal(c.element('zoomValue').textContent, Math.round(scale * 100) + '%');
  }
  c.stage.clientWidth = 1000; c.resize();
  assert.equal(c.zoom.scale(), 2);
});

test('zoom to fit still follows canvas resizes after centering', function () {
  var c = canvas();
  c.element('zoomIn').click();
  c.element('zoomCenter').click();
  c.zoom.fit();
  c.stage.clientWidth = 500; c.resize();
  assert.equal(c.zoom.scale(), (500 - 64) / 800);
  assert.ok(Math.abs(c.zoom.pan().x - 32) < 1e-9);
});

test('zoom steps by powers of two from wherever the canvas is', function () {
  var r = rules();
  assert.equal(r.stepIn(0.69), 1);
  assert.equal(r.stepIn(1), 2);
  assert.equal(r.stepIn(1.2), 2);
  assert.equal(r.stepOut(0.69), 0.5);
  assert.equal(r.stepOut(1), 0.5);
  assert.equal(r.stepOut(2.5), 2);
});

test('zoom stays between 10% and 800%', function () {
  var r = rules();
  assert.equal(r.stepOut(0.1), 0.1);
  assert.equal(r.stepIn(8), 8);
  assert.equal(r.clamp(40), 8);
});

test('Figma shortcuts map to zoom commands, by key or by physical key', function () {
  var r = rules();
  assert.equal(r.command({ metaKey: true, key: '0', code: 'Digit0' }), 'actual');
  assert.equal(r.command({ ctrlKey: true, key: '=', code: 'Equal' }), 'in');
  assert.equal(r.command({ metaKey: true, key: '+', code: 'Equal', shiftKey: true }), 'in');
  assert.equal(r.command({ metaKey: true, key: 'ß', code: 'Minus' }), 'out');
  assert.equal(r.command({ shiftKey: true, key: '!', code: 'Digit1' }), 'fit');
  assert.equal(r.command({ shiftKey: true, key: ')', code: 'Digit0' }), 'actual');
  assert.equal(r.command({ key: '0', code: 'Digit0' }), null);
  assert.equal(r.command({ metaKey: true, altKey: true, key: '0', code: 'Digit0' }), null);
  assert.equal(r.command({ metaKey: true, key: '1', code: 'Digit1' }), null);
});

test('a mouse notch zooms by a step while a pinch stays fine-grained', function () {
  var r = rules();
  var notch = r.wheelFactor({ deltaY: -100, deltaMode: 0 });
  var pinch = r.wheelFactor({ deltaY: -2, deltaMode: 0 });
  assert.ok(notch > 1.2 && notch < 1.35);
  assert.ok(pinch > 1 && pinch < 1.03);
  assert.ok(r.wheelFactor({ deltaY: 100, deltaMode: 0 }) < 1);
});
