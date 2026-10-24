var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

function load() {
  var window = {};
  var file = path.join(__dirname, "./address.js");
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
    src: 'pages/sign-in.html', scenario: 'error', example: null, size: 'mobile', lens: 'staging',
  });
  assert.deepEqual(plain(address.parse('#pages/sign-in.html~staging')), {
    src: 'pages/sign-in.html', scenario: null, example: null, size: null, lens: 'staging',
  });
  assert.deepEqual(plain(address.parse('#pages/sign-in.html:error@fit')), {
    src: 'pages/sign-in.html', scenario: 'error', example: null, size: 'fit', lens: null,
  });
  assert.deepEqual(plain(address.parse('#pages/sign-in.html')), {
    src: 'pages/sign-in.html', scenario: null, example: null, size: null, lens: null,
  });
  assert.deepEqual(plain(address.parse('')), { src: '', scenario: null, example: null, size: null, lens: null });
});

test('reads a docs page address with its example, scenario, and lens', function () {
  var address = load();
  assert.deepEqual(plain(address.parse('#docs/card.md!with-custom-style~native')), {
    src: 'docs/card.md', scenario: null, example: 'with-custom-style', size: null, lens: 'native',
  });
  assert.deepEqual(plain(address.parse('#docs/card.md:loading!basic')), {
    src: 'docs/card.md', scenario: 'loading', example: 'basic', size: null, lens: null,
  });
  assert.deepEqual(plain(address.parse('#docs/card.md!basic@mobile')), {
    src: 'docs/card.md', scenario: null, example: 'basic', size: 'mobile', lens: null,
  });
});

test('decodes what the browser encoded, and tolerates what it can’t', function () {
  var address = load();
  assert.equal(address.parse('#pages/sign%20in.html@fit').src, 'pages/sign in.html');
  assert.equal(address.parse('#pages/%E0%A4%A@fit').src, 'pages/%E0%A4%A');
});

test('writes the address the shell always wrote, plus a lens when there is one', function () {
  var address = load();
  assert.equal(address.write({ src: 'pages/sign-in.html', scenario: null, size: 'fit', lens: null }), 'pages/sign-in.html@fit');
  assert.equal(address.write({ src: 'pages/sign-in.html', scenario: 'error', size: 'mobile', lens: null }), 'pages/sign-in.html:error@mobile');
  assert.equal(
    address.write({ src: 'pages/sign-in.html', scenario: 'error', size: 'mobile', lens: 'staging' }),
    'pages/sign-in.html:error@mobile~staging'
  );
  assert.equal(address.write({ src: '', scenario: 'error', size: 'mobile', lens: 'staging' }), '');
  assert.equal(address.write(null), '');
});

test('writes a docs page without a size, with its example', function () {
  var address = load();
  assert.equal(address.write({ src: 'docs/card.md', scenario: null, example: 'basic', size: null, lens: 'native' }), 'docs/card.md!basic~native');
  assert.equal(address.write({ src: 'docs/card.md', scenario: null, example: null, size: null, lens: null }), 'docs/card.md');
});

test('reads each artboard’s iteration, the implicit artboard under ""', function () {
  var address = load();
  assert.deepEqual(plain(address.parse('#jobs.workbench.ts:empty+illustrated@laptop')), {
    src: 'jobs.workbench.ts', scenario: 'empty', example: null, size: 'laptop', lens: null,
    iterations: { '': 'illustrated' },
  });
  assert.deepEqual(plain(address.parse('#feedback.workbench.ts+warning/bold+info/calm@laptop~staging')), {
    src: 'feedback.workbench.ts', scenario: null, example: null, size: 'laptop', lens: 'staging',
    iterations: { warning: 'bold', info: 'calm' },
  });
  assert.deepEqual(
    plain(address.parse('#feedback.workbench.ts+warning/bold+warning/calm++/x+info/@laptop').iterations),
    { warning: 'bold' }
  );
  assert.equal(address.parse('#feedback.workbench.ts+@laptop').iterations, undefined);
});

test('writes iterations after the scenario, in the map’s order', function () {
  var address = load();
  assert.equal(
    address.write({ src: 'feedback.workbench.ts', scenario: 'default', iterations: { info: 'calm', warning: 'bold' }, size: 'laptop', lens: null }),
    'feedback.workbench.ts:default+info/calm+warning/bold@laptop'
  );
  assert.equal(
    address.write({ src: 'jobs.workbench.ts', scenario: 'empty', iterations: { '': 'illustrated' }, size: 'laptop' }),
    'jobs.workbench.ts:empty+illustrated@laptop'
  );
  assert.equal(address.write({ src: 'jobs.workbench.ts', iterations: {}, size: 'laptop' }), 'jobs.workbench.ts@laptop');
});

test('round-trips', function () {
  var address = load();
  var target = { src: 'preview/components-button.html', scenario: 'icon-only', example: null, size: 'resizable', lens: 'storybook' };
  assert.deepEqual(plain(address.parse('#' + address.write(target))), target);
  var docs = { src: 'docs/card.md', scenario: 'loading', example: 'with-custom-style', size: null, lens: 'native' };
  assert.deepEqual(plain(address.parse('#' + address.write(docs))), docs);
  var iterated = {
    src: 'feedback.workbench.ts', scenario: 'default', example: null, size: 'laptop', lens: null,
    iterations: { warning: 'bold', info: 'calm' },
  };
  assert.deepEqual(plain(address.parse('#' + address.write(iterated))), iterated);
});
