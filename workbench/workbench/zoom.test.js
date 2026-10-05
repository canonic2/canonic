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

function canvas(globals) {
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
  var canvas = element('canvas'); canvas.clientWidth = 1200; canvas.clientHeight = 900;
  var frame = element('artboard'); frame.offsetWidth = 800; frame.offsetHeight = 600;
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
  Object.assign(context, globals);
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'zoom.js'), 'utf8'), context);
  return { zoom: context.wbZoom, canvas: canvas, frame: frame, element: element, resize: function () { resize(); } };
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
  c.canvas.clientWidth = 1000; c.resize();
  assert.equal(c.zoom.scale(), 2);
});

test('zoom to fit still follows canvas resizes after centering', function () {
  var c = canvas();
  c.element('zoomIn').click();
  c.element('zoomCenter').click();
  c.zoom.fit();
  c.canvas.clientWidth = 500; c.resize();
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

test('a docs page fills the canvas at 100%, keeps its layout width when zoomed, and scrolls instead of panning', function () {
  var c = canvas({ wbDocsLayout: require('../src/docs/canvas/docs-layout.ts') });
  var page = { scrollY: 1000, scrollTo: function (x, y) { this.scrollY = y; }, scrollBy: function (x, y) { this.scrollY += y; } };
  c.frame.querySelector = function () { return { contentWindow: page }; };
  c.zoom.panTo(-50, -50);
  c.zoom.mode('docs');
  assert.equal(c.zoom.scale(), 1);
  assert.deepEqual([c.frame.style.width, c.frame.style.height, c.frame.style.transform], ['1200px', '900px', 'translate(0px, 0px) scale(1)']);

  c.element('zoomOut').click();
  assert.equal(c.zoom.scale(), 0.5);
  assert.deepEqual([c.frame.style.width, c.frame.style.height], ['1200px', '1800px'], 'zooming out shows more page without reflowing it');
  assert.equal(c.zoom.pan().x, 300, 'a page narrower than the canvas is centered');
  assert.equal(page.scrollY, 550, 'the content at the canvas middle stays there');

  c.zoom.panTo(300, 0);
  page.scrollY = 0;
  c.zoom.fit();
  assert.equal(c.zoom.scale(), 1, 'zoom to fit returns a docs page to 100%');

  c.zoom.mode('default');
  assert.equal(c.frame.style.width, '', 'an artboard page gets its own size back');
});

test('fitting fills only the flagged axes with the canvas’s room, never below 320', function () {
  var c = canvas();
  c.canvas.dataset.fillHeight = 'true';
  c.frame.style.width = '340px';
  c.zoom.fit();
  assert.equal(c.frame.style.width, '340px', 'a fixed width is left as it is');
  assert.equal(c.frame.style.height, (900 - 40 - 80) + 'px');
  c.canvas.dataset.fillWidth = 'true';
  c.canvas.clientWidth = 200;
  c.zoom.fit();
  assert.equal(c.frame.style.width, '320px');
  c.canvas.dataset.fillWidth = 'false'; c.canvas.dataset.fillHeight = 'false';
  c.frame.style.height = '90px';
  c.zoom.fit();
  assert.equal(c.frame.style.height, '90px', 'nothing filled, nothing changed');
});
