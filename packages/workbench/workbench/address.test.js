var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function load() {
  var window = {};
  var file = path.join(__dirname, 'address.js');
  vm.runInNewContext(fs.readFileSync(file, 'utf8'), { window: window }, { filename: file });
  return window.wbAddress;
}

/* Objects made inside the vm have that realm's Object behind them; compare
   the values, not the prototypes. */
function plain(value) {
  return JSON.parse(JSON.stringify(value));
}

test('reads every part of an address', function () {
  var address = load();
  assert.deepEqual(plain(address.parse('#pages/sign-in.html:error@mobile~staging')), {
    src: 'pages/sign-in.html', state: 'error', example: null, size: 'mobile', lens: 'staging',
  });
  assert.deepEqual(plain(address.parse('#pages/sign-in.html~staging')), {
    src: 'pages/sign-in.html', state: null, example: null, size: null, lens: 'staging',
  });
  assert.deepEqual(plain(address.parse('#pages/sign-in.html:error@fit')), {
    src: 'pages/sign-in.html', state: 'error', example: null, size: 'fit', lens: null,
  });
  assert.deepEqual(plain(address.parse('#pages/sign-in.html')), {
    src: 'pages/sign-in.html', state: null, example: null, size: null, lens: null,
  });
  assert.deepEqual(plain(address.parse('')), { src: '', state: null, example: null, size: null, lens: null });
});

test('reads a docs page address with its example, state, and lens', function () {
  var address = load();
  assert.deepEqual(plain(address.parse('#docs/card.md!with-custom-style~native')), {
    src: 'docs/card.md', state: null, example: 'with-custom-style', size: null, lens: 'native',
  });
  assert.deepEqual(plain(address.parse('#docs/card.md:loading!basic')), {
    src: 'docs/card.md', state: 'loading', example: 'basic', size: null, lens: null,
  });
  assert.deepEqual(plain(address.parse('#docs/card.md!basic@mobile')), {
    src: 'docs/card.md', state: null, example: 'basic', size: 'mobile', lens: null,
  });
});

test('decodes what the browser encoded, and tolerates what it can’t', function () {
  var address = load();
  assert.equal(address.parse('#pages/sign%20in.html@fit').src, 'pages/sign in.html');
  assert.equal(address.parse('#pages/%E0%A4%A@fit').src, 'pages/%E0%A4%A');
});

test('writes the address the shell always wrote, plus a lens when there is one', function () {
  var address = load();
  assert.equal(address.write({ src: 'pages/sign-in.html', state: null, size: 'fit', lens: null }), 'pages/sign-in.html@fit');
  assert.equal(address.write({ src: 'pages/sign-in.html', state: 'error', size: 'mobile', lens: null }), 'pages/sign-in.html:error@mobile');
  assert.equal(
    address.write({ src: 'pages/sign-in.html', state: 'error', size: 'mobile', lens: 'staging' }),
    'pages/sign-in.html:error@mobile~staging'
  );
  assert.equal(address.write({ src: '', state: 'error', size: 'mobile', lens: 'staging' }), '');
  assert.equal(address.write(null), '');
});

test('writes a docs page without a size, with its example', function () {
  var address = load();
  assert.equal(address.write({ src: 'docs/card.md', state: null, example: 'basic', size: null, lens: 'native' }), 'docs/card.md!basic~native');
  assert.equal(address.write({ src: 'docs/card.md', state: null, example: null, size: null, lens: null }), 'docs/card.md');
});

test('round-trips', function () {
  var address = load();
  var target = { src: 'preview/components-button.html', state: 'icon-only', example: null, size: 'resizable', lens: 'storybook' };
  assert.deepEqual(plain(address.parse('#' + address.write(target))), target);
  var docs = { src: 'docs/card.md', state: 'loading', example: 'with-custom-style', size: null, lens: 'native' };
  assert.deepEqual(plain(address.parse('#' + address.write(docs))), docs);
});
