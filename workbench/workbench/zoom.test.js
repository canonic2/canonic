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
