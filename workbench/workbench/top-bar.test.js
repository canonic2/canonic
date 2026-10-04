var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function topBar() {
  var context = {};
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'top-bar.js'), 'utf8'), context);
  return context.wbTopBar;
}

test('compacts when equal side tracks would make top bar regions collide', function () {
  var needsCompact = topBar().needsCompact;
  assert.equal(needsCompact(1200, 330, 290, 390, 12), false);
  assert.equal(needsCompact(980, 330, 290, 390, 12), true);
});

test('the widest side determines the symmetric top bar space', function () {
  var needsCompact = topBar().needsCompact;
  assert.equal(needsCompact(900, 180, 260, 300, 12), false);
  assert.equal(needsCompact(880, 180, 260, 300, 12), true);
});

test('an unset CSS gap measures as zero instead of poisoning the breakpoint', function () {
  var pixels = topBar().pixels;
  assert.equal(pixels('normal'), 0);
  assert.equal(pixels('12px'), 12);
});
