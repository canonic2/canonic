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
  assert.deepEqual(plain(address.parse('#pages/sign-in.html:error@393~staging')), {
    src: 'pages/sign-in.html', state: 'error', width: '393', lens: 'staging',
  });
  assert.deepEqual(plain(address.parse('#pages/sign-in.html~staging')), {
    src: 'pages/sign-in.html', state: null, width: null, lens: 'staging',
  });
  assert.deepEqual(plain(address.parse('#pages/sign-in.html:error@fit')), {
    src: 'pages/sign-in.html', state: 'error', width: 'fit', lens: null,
  });
  assert.deepEqual(plain(address.parse('#pages/sign-in.html')), {
    src: 'pages/sign-in.html', state: null, width: null, lens: null,
  });
  assert.deepEqual(plain(address.parse('')), { src: '', state: null, width: null, lens: null });
});

test('decodes what the browser encoded, and tolerates what it can’t', function () {
  var address = load();
  assert.equal(address.parse('#pages/sign%20in.html@fit').src, 'pages/sign in.html');
  assert.equal(address.parse('#pages/%E0%A4%A@fit').src, 'pages/%E0%A4%A');
});

test('writes the address the shell always wrote, plus a lens when there is one', function () {
  var address = load();
  assert.equal(address.write({ src: 'pages/sign-in.html', state: null, width: 'fit', lens: null }), 'pages/sign-in.html@fit');
  assert.equal(address.write({ src: 'pages/sign-in.html', state: 'error', width: '393', lens: null }), 'pages/sign-in.html:error@393');
  assert.equal(
    address.write({ src: 'pages/sign-in.html', state: 'error', width: '393', lens: 'staging' }),
    'pages/sign-in.html:error@393~staging'
  );
  assert.equal(address.write({ src: '', state: 'error', width: '393', lens: 'staging' }), '');
  assert.equal(address.write(null), '');
});

test('round-trips', function () {
  var address = load();
  var target = { src: 'preview/components-button.html', state: 'icon-only', width: 'resizable', lens: 'storybook' };
  assert.deepEqual(plain(address.parse('#' + address.write(target))), target);
});
