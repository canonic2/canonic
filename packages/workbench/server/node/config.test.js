var assert = require('node:assert/strict');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var test = require('node:test');
var yaml = require('./yaml');
var config = require('./config');

test('browser reader preserves icon-only declarations and hides them after failed imports', async function () {
  var vm = require('node:vm');
  var manifest = require('../../contracts/manifest');
  var warnings = [];
  var body = 'name: Acme\ncollections:\n  - name: Web App\n    icon: app-window\n';
  var window = { wbManifest: manifest, wbYaml: yaml.parse };
  function Request() {}
  Request.prototype.open = function (_, url) { this.url = url; };
  Request.prototype.send = function () {
    this.status = this.url.endsWith('workbench.local.yaml') ? 404 : 200;
    this.responseText = body;
    this.onload();
  };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, "../../ui/legacy/config.js"), 'utf8'), {
    window: window, URL: URL, URLSearchParams: URLSearchParams,
    location: { protocol: 'http:', search: '' },
    document: { baseURI: 'http://127.0.0.1/', querySelector: () => null },
    XMLHttpRequest: Request, console: { warn: message => warnings.push(message) },
  });
  var loaded = await new Promise((resolve, reject) => window.wbConfig.load(resolve, reject));
  assert.equal(loaded.collections[0].icon, 'app-window');
  assert.equal(warnings.length, 0);
  assert.equal(manifest.mergeCollections(loaded.collections, []).length, 0);
  assert.equal(manifest.mergeCollections(loaded.collections, [{ name: 'Web App', icon: 'component', iconPriority: 1, items: [{ src: 'jobs.workbench.ts' }] }])[0].icon, 'app-window');
  body += 'previews: false\n';
  await assert.rejects(new Promise((resolve, reject) => window.wbConfig.load(resolve, reject)), /nothing to show/);
});

test('icon-only collections survive reading', function () {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-collection-icon-'));
  try {
    fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\ncollections:\n  - name: Web App\n    icon: app-window\n');
    assert.deepEqual(config.read(root).collections, [{ name: 'Web App', icon: 'app-window', items: [] }]);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('server and browser readers retain authored lens labels', async function () {
  var body = 'name: Acme\npreviews:\n  lensLabel: Design\ncollections:\n  - name: Pages\n    items:\n      - label: Button\n        src: button.workbench.ts\n        lensLabel: Reference\n      - label: Invalid\n        src: invalid.html\n        lensLabel: false\n';
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-lens-label-'));
  try {
    fs.writeFileSync(path.join(root, 'workbench.yaml'), body);
    var read = config.read(root);
    assert.equal(read.previews.lensLabel, 'Design');
    assert.equal(read.collections[0].items[0].lensLabel, 'Reference');
    assert.equal(read.collections[0].items[1].lensLabel, undefined);
    assert.deepEqual(read.problems, ['Pages › Invalid: lensLabel: must be a nonempty string.']);
    var warnings = [];
    var window = { wbManifest: require('../../contracts/manifest'), wbYaml: yaml.parse };
    function Request() {}
    Request.prototype.open = function (_, url) { this.url = url; };
    Request.prototype.send = function () {
      this.status = this.url.endsWith('workbench.local.yaml') ? 404 : 200;
      this.responseText = body;
      this.onload();
    };
    require('node:vm').runInNewContext(fs.readFileSync(path.join(__dirname, "../../ui/legacy/config.js"), 'utf8'), {
      window: window, URL: URL, URLSearchParams: URLSearchParams,
      location: { protocol: 'http:', search: '' },
      document: { baseURI: 'http://127.0.0.1/', querySelector: () => null },
      XMLHttpRequest: Request, console: { warn: message => warnings.push(message) },
    });
    var browser = await new Promise((resolve, reject) => window.wbConfig.load(resolve, reject));
    assert.equal(browser.collections[0].items[0].lensLabel, 'Reference');
    assert.equal(browser.previews.lensLabel, 'Design');
    assert.deepEqual(warnings, ['[workbench] workbench.yaml:\nPages › item 2 › Invalid: lensLabel: must be a nonempty string.']);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

/* The shapes a workbench.yaml is actually written in. The tree in the sidebar
   is only ever as right as this is. */

test('reads collections, groups, pages and scenarios', function () {
  var doc = yaml.parse([
    'name: Acme',
    '',
    'collections:',
    '  - name: Pages    # the rail button',
    '    icon: file-text',
    '    items:',
    '      - group: Auth',
    '        items:',
    '          - label: Sign in',
    '            src: pages/sign-in.html',
    '            scenarios:',
    '              - id: default',
    '                label: Default',
    '              - id: error',
    '                label: Wrong password',
      '      - label: Loose page',
      '        src: pages/loose.html',
      '        icon: panel-top',
  ].join('\n'));

  assert.equal(doc.name, 'Acme');
  assert.equal(doc.collections.length, 1);

  var collection = doc.collections[0];
  assert.equal(collection.name, 'Pages');
  assert.equal(collection.icon, 'file-text');

  var group = collection.items[0];
  assert.equal(group.group, 'Auth');
  assert.equal(group.items[0].label, 'Sign in');
  assert.deepEqual(group.items[0].scenarios[1], { id: 'error', label: 'Wrong password' });

  assert.equal(collection.items[1].src, 'pages/loose.html');
  assert.equal(collection.items[1].icon, 'panel-top');
});

test('keeps a # that is part of a value', function () {
  assert.equal(yaml.parse('name: "#00a1ff"').name, '#00a1ff');
});

test('reads quoted strings, numbers and booleans', function () {
  var doc = yaml.parse(['name: "Acme: the good one"', 'width: 1512', 'live: false'].join('\n'));
  assert.equal(doc.name, 'Acme: the good one');
  assert.equal(doc.width, 1512);
  assert.equal(doc.live, false);
});

test('points at the line it choked on', function () {
  assert.throws(
    function () {
      yaml.parse(['name: Acme', 'collections:', '  - name: Pages', '  no colon here'].join('\n'));
    },
    /line 4/
  );
});

test('refuses a tab where an indent belongs', function () {
  assert.throws(function () {
    yaml.parse(['collections:', '\t- name: Pages'].join('\n'));
  }, /tab/);
});

/* The reader on the server's side: the same file, plus what only this
   machine can answer. */

var MAIN = [
  'name: Acme',
  'implementations:',
  '  storybook:',
  '    kind: storybook',
  '    url: http://localhost:6006',
  '    root: ../product/packages/ui',
  '  dev:',
  '    kind: url',
  '    base: http://localhost:3000',
  '  staging:',
  '    kind: url',
  '    base: https://staging.example.com',
  'collections:',
  '  - name: Pages',
  '    items:',
  '      - label: Sign in',
  '        src: pages/sign-in.html',
  '        sizes:',
  '          - laptop',
  '          - mobile',
  '        scenarios:',
  '          - id: default',
  '            label: Default',
  '          - id: error',
  '            label: Wrong password',
  '        implementations:',
  '          dev:',
  '            default: /',
  '            error: /?error=1',
  '          staging: /',
  '        code:',
  '          dev: src/pages/login',
  '          staging: src/pages/login',
  '  - name: Components',
  '    items:',
  '      - label: Button',
  '        src: preview/components-button.html',
  '        icon: square-mouse-pointer',
  '        implementations:',
  '          storybook: Components/Button',
  '        code:',
  '          storybook:',
  '            - src/button.tsx',
  '            - src/missing.tsx',
].join('\n');

function project(files) {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-config-test-'));
  Object.keys(files).forEach(function (name) {
    var file = path.join(root, name);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, files[name]);
  });
  return root;
}

test('reads implementations, lenses and code pointers', function () {
  var root = project({ 'workbench.yaml': MAIN });
  try {
    var read = config.read(root);
    assert.equal(read.name, 'Acme');
    assert.deepEqual(Object.keys(read.implementations), ['storybook', 'dev', 'staging']);
    assert.deepEqual(read.files, { main: true, local: false });

    var signIn = read.collections[0].items[0];
    assert.deepEqual(signIn.sizes, ['laptop', 'mobile']);
    assert.deepEqual(signIn.implementations, {
      dev: { path: '/', scenarios: { error: '/?error=1' } },
      staging: { path: '/' },
    });
    assert.deepEqual(signIn.code, [
      { implementation: 'dev', path: 'src/pages/login' },
      { implementation: 'staging', path: 'src/pages/login' },
    ]);

    var button = read.collections[1].items[0];
    assert.equal(button.sizes, undefined, 'a page without sizes supports every size of the space');
    assert.equal(button.icon, 'square-mouse-pointer');
    assert.deepEqual(button.implementations, { storybook: { title: 'Components/Button' } });
    assert.deepEqual(read.problems, []);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});


test('answers null for a project without a workbench.yaml', function () {
  var root = project({});
  try {
    assert.equal(config.read(root), null);
    assert.equal(config.resolve(root, null), null);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('reports a catalog on a Storybook implementation and keeps the implementation', function () {
  var root = project({
    'workbench.yaml': [
      'name: Acme stories',
      'implementations:',
      '  storybook:',
      '    kind: storybook',
      '    url: http://localhost:6006',
      '    catalog: true',
    ].join('\n'),
  });
  try {
    var read = config.read(root);
    assert.equal(read.implementations.storybook.url, 'http://localhost:6006');
    assert.equal(read.implementations.storybook.catalog, undefined);
    assert.deepEqual(read.collections, []);
    assert.deepEqual(read.problems, ['implementations › storybook: catalog is only available for iOS Simulator implementations.']);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('reads a configurable implementation command and readiness check', function () {
  var root = project({
    'workbench.yaml': [
      'implementations:',
      '  storybook:',
      '    kind: storybook',
      '    url: http://localhost:6006',
      '    start:',
      '      command: yarn workspace @example/storybook storybook:web',
      '      cwd: .',
      '      check:',
      '        port: 6006',
      '      ready:',
      '        url: http://localhost:6006/index.json',
      '      timeout: 90',
    ].join('\n'),
  });
  try {
    var read = config.read(root);
    assert.deepEqual(read.implementations.storybook.start, {
      command: 'yarn workspace @example/storybook storybook:web',
      cwd: '.', check: { port: 6006, host: '127.0.0.1' }, timeout: 90,
      ready: { url: 'http://localhost:6006/index.json' },
    });
    assert.deepEqual(read.problems, []);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('accepts an automatic Storybook and a Simulator catalog without handwritten collections', function () {
  var root = project({
    'workbench.yaml': [
      'implementations:',
      '  storybook:',
      '    kind: storybook',
      '    url: auto',
      '  simulator:',
      '    kind: ios-simulator',
      '    device: booted',
      '    catalog: true',
    ].join('\n'),
  });
  try {
    var read = config.read(root);
    assert.equal(read.implementations.storybook.auto, true);
    assert.equal(read.implementations.simulator.device, 'booted');
    assert.equal(read.implementations.simulator.catalogIcon, 'smartphone');
    assert.deepEqual(read.collections, []);
    assert.deepEqual(read.problems, []);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('merges workbench.local.yaml over the committed file', function () {
  var root = project({
    'workbench.yaml': MAIN,
    'workbench.local.yaml': [
      'implementations:',
      '  dev:',
      '    base: http://localhost:4000',
      '    root: ../product',
    ].join('\n'),
  });
  try {
    var read = config.read(root);
    assert.equal(read.implementations.dev.base, 'http://localhost:4000');
    assert.equal(read.implementations.dev.root, '../product');
    assert.equal(read.implementations.staging.base, 'https://staging.example.com');
    assert.deepEqual(read.files, { main: true, local: true });
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('names the local file when it is the one that is wrong', function () {
  var root = project({ 'workbench.yaml': MAIN, 'workbench.local.yaml': 'implementations:\n\tdev: x' });
  try {
    assert.throws(function () {
      config.read(root);
    }, /workbench\.local\.yaml, line 2/);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('resolves every path for this machine and says what it could not', function () {
  var root = project({
    'workbench.yaml': MAIN,
    'workbench.local.yaml': ['implementations:', '  dev:', '    root: ../product'].join('\n'),
    '../product/src/pages/login/index.tsx': '',
    '../product/packages/ui/src/button.tsx': '',
  });
  var product = path.resolve(root, '../product');
  try {
    var view = config.resolve(root, config.read(root));
    assert.equal(view.name, 'Acme');
    assert.equal(view.implementations.dev.root, product);
    assert.equal(view.implementations.storybook.root, path.join(product, 'packages/ui'));
    assert.equal(view.implementations.staging.root, null);

    var signIn = view.pages['pages/sign-in.html'];
    assert.equal(signIn.label, 'Sign in');
    assert.equal(signIn.design, path.join(root, 'pages/sign-in.html'));
    assert.deepEqual(signIn.code, [
      { implementation: 'dev', path: path.join(product, 'src/pages/login'), relative: 'src/pages/login', exists: true },
      { implementation: 'staging', path: null, relative: 'src/pages/login', exists: false },
    ]);

    var button = view.pages['preview/components-button.html'];
    assert.deepEqual(button.code, [
      { implementation: 'storybook', path: path.join(product, 'packages/ui/src/button.tsx'), relative: 'src/button.tsx', exists: true },
      { implementation: 'storybook', path: path.join(product, 'packages/ui/src/missing.tsx'), relative: 'src/missing.tsx', exists: false },
    ]);

    assert.deepEqual(view.problems, [
      'Pages › Sign in: code for “staging” can’t resolve — implementation “staging” has no root.',
    ]);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
    fs.rmSync(product, { recursive: true, force: true });
  }
});

test('reports a space icon image that isn’t in the project, and resolves the mark', function () {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-mark-'));
  try {
    fs.writeFileSync(path.join(root, 'workbench.yaml'), [
      'name: Acme',
      'color: purple',
      'icon: brand/logo.svg',
      'collections:',
      '  - name: Pages',
      '    items:',
      '      - label: Home',
      '        src: index.html',
    ].join('\n'));
    var view = config.resolve(root, config.read(root));
    assert.deepEqual(view.mark, { color: 'purple', icon: null, image: 'brand/logo.svg' });
    assert.deepEqual(view.problems, ['icon: brand/logo.svg isn’t in the project.']);

    fs.mkdirSync(path.join(root, 'brand'));
    fs.writeFileSync(path.join(root, 'brand', 'logo.svg'), '<svg/>');
    assert.deepEqual(config.resolve(root, config.read(root)).problems, []);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

function multiSpace(body) {
  var dir = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-multi-'));
  fs.writeFileSync(path.join(dir, 'workbench.yaml'), body);
  return dir;
}

var MULTI = [
  '# Two spaces in one file',
  'implementations:',
  '  dev:',
  '    kind: url',
  '    base: http://localhost:3000',
  'spaces:',
  '  web:',
  '    name: Acme Web',
  '    collections:',
  '      - name: Pages',
  '        items:',
  '          - label: Home',
  '            src: index.html',
  '  ui:',
  '    name: Acme UI',
  '    root: packages/ui',
  '    # its own collections come later',
  '',
  'previews: false',
  '',
].join('\n');

test('lists one space per entry, each with its root', function () {
  var dir = multiSpace(MULTI);
  try {
    assert.deepEqual(config.list(dir), [
      { key: 'web', root: dir },
      { key: 'ui', root: path.join(dir, 'packages', 'ui') },
    ]);
    var single = multiSpace('name: Acme\ncollections: []\n');
    assert.deepEqual(config.list(single), [{ key: null, root: single }]);
    var broken = multiSpace('name: Acme\n  collections: []\n');
    assert.deepEqual(config.list(broken), [{ key: null, root: broken }]);
    assert.deepEqual(config.list(path.join(dir, 'packages')), []);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('reads one space of a file, with the shared keys and its own root', function () {
  var dir = multiSpace(MULTI);
  try {
    var web = config.read({ dir: dir, key: 'web' });
    assert.equal(web.name, 'Acme Web');
    assert.equal(web.key, 'web');
    assert.equal(web.root, dir);
    assert.equal(web.collections[0].items[0].src, 'index.html');
    assert.equal(web.implementations.dev.base, 'http://localhost:3000');
    assert.equal(web.previews, false);

    var ui = config.read({ dir: dir, key: 'ui' });
    assert.equal(ui.root, path.join(dir, 'packages', 'ui'));
    assert.deepEqual(ui.collections, []);
    /* A folder alone reads the first space. */
    assert.equal(config.read(dir).key, 'web');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});


test('reads Markdown pages, pages with docs, their docs lenses, and the lens a Markdown page opens with', function () {
  var root = project({ 'workbench.yaml': [
    'implementations:',
    '  web:',
    '    kind: docs',
    '    adapter: html',
    '    styles:',
    '      - demo/demo.css',
    '  native:',
    '    kind: docs',
    '    adapter: html',
    'collections:',
    '  - name: Design system',
    '    items:',
    '      - label: Card',
    '        src: docs/card.md',
    '        lens: native',
    '        implementations:',
    '          web: docs/card/',
    '          native: docs/card.examples.ts',
    '      - label: Colors',
    '        src: docs/colors.md',
    '      - label: Marked',
    '        src: docs/bad!name.md',
    '      - label: Button',
    '        src: pages/button.html',
    '        docs: docs/button.md',
    '        implementations:',
    '          web: docs/button/',
  ].join('\n') });
  try {
    var read = config.read(root);
    var items = read.collections[0].items;
    assert.deepEqual(items.map(function (item) { return item.label; }), ['Card', 'Colors', 'Button']);
    assert.deepEqual(items[0].implementations, { web: { examples: 'docs/card/' }, native: { examples: 'docs/card.examples.ts' } });
    assert.equal(items[0].markdown, 'docs/card.md');
    assert.equal(items[0].lens, 'native');
    assert.equal(items[1].markdown, 'docs/colors.md');
    assert.equal(items[1].lens, undefined);
    assert.equal(items[2].markdown, 'docs/button.md');
    assert.deepEqual(items[2].implementations, { web: { examples: 'docs/button/' } });
    assert.deepEqual(read.implementations.web.styles, ['demo/demo.css']);
    assert.deepEqual(read.problems, []);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

var SIZED = [
  'name: Acme # keep this comment',
  'sizes:',
  '  fit: true',
  '  sidebar:',
  '    width: 340',
  '    height: fill',
  '  tablet:',
  '    width: 1024',
  '    height: 1366',
  'collections:',
  '  - name: Interface',
  '    items:',
  '      # the sidebar is 340 wide',
  '      - label: Sidebar',
  '        src: sidebar.html',
  '        sizes:',
  '          - sidebar',
  '          - laptop',
  '      - label: Menu',
  '        src: menu.html',
  '        sizes:',
  '          - popover:',
  '              width: 280',
  '              height: 360',
  '          - fit',
  '      - label: Card',
  '        src: card.md',
  '        sizes:',
  '          - fit',
  '      - label: Dashboard',
  '        src: dashboard.html',
  '',
].join('\n');

test('reads a space’s sizes and each page’s, with what the local file sets marked', function () {
  var root = project({ 'workbench.yaml': SIZED, 'workbench.local.yaml': 'sizes:\n  sidebar:\n    width: 360\n' });
  try {
    var read = config.read(root);
    assert.deepEqual(read.sizes.map(function (size) { return [size.key, size.width, size.height, !!size.local]; }),
      [['fit', null, null, false], ['sidebar', 360, 'fill', true], ['tablet', 1024, 1366, false]]);
    var items = read.collections[0].items;
    assert.deepEqual(items[0].sizes, ['sidebar']);
    assert.deepEqual(items[1].sizes, ['popover', 'fit']);
    assert.deepEqual(items[1].ownSizes.map(function (size) { return [size.key, size.width, size.height, size.button]; }), [['popover', 280, 360, false]]);
    assert.equal(items[2].sizes, undefined, 'a docs page has no size');
    assert.equal(items[3].sizes, undefined, 'a page without sizes supports every size of the space');
    assert.deepEqual(read.problems, [
      'Interface › Sidebar: size “laptop” isn’t one of the space’s sizes.',
      'Interface › Card: sizes don’t apply to a Markdown page, which uses the whole canvas.',
    ]);
    var resolved = config.resolve(root, read);
    assert.equal(resolved.sizes, read.sizes);
    assert.deepEqual(resolved.pages['sidebar.html'].sizes, ['sidebar']);
    assert.equal(resolved.pages['menu.html'].ownSizes[0].key, 'popover');
    assert.equal(resolved.pages['dashboard.html'].sizes, undefined);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('sizes belong to each space of a file that lists several', function () {
  var dir = multiSpace(MULTI.replace('spaces:\n  web:\n', 'sizes:\n  fit: true\nspaces:\n  web:\n    sizes:\n      mobile: true\n'));
  try {
    var web = config.read({ dir: dir, key: 'web' });
    assert.deepEqual(web.sizes.map(function (size) { return size.key; }), ['mobile']);
    assert.match(web.problems.join('\n'), /Sizes: sizes go in each space, under spaces.<key>, when the file lists spaces\./);
    var ui = config.read({ dir: dir, key: 'ui' });
    assert.deepEqual(ui.sizes.map(function (size) { return size.key; }), ['fit', 'laptop', 'mobile', 'resizable']);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
