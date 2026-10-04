var assert = require('node:assert/strict');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var test = require('node:test');
var yaml = require('./yaml');
var config = require('./config');

/* The shapes a workbench.yaml is actually written in. The tree in the sidebar
   is only ever as right as this is. */

test('reads sections, folders, screens and states', function () {
  var doc = yaml.parse([
    'name: Acme',
    '',
    'sections:',
    '  - name: Pages    # the rail button',
    '    icon: file-text',
    '    items:',
    '      - folder: Auth',
    '        items:',
    '          - label: Sign in',
    '            src: pages/sign-in.html',
    '            states:',
    '              - id: default',
    '                label: Default',
    '              - id: error',
    '                label: Wrong password',
      '      - label: Loose screen',
      '        src: pages/loose.html',
      '        icon: panel-top',
  ].join('\n'));

  assert.equal(doc.name, 'Acme');
  assert.equal(doc.sections.length, 1);

  var section = doc.sections[0];
  assert.equal(section.name, 'Pages');
  assert.equal(section.icon, 'file-text');

  var folder = section.items[0];
  assert.equal(folder.folder, 'Auth');
  assert.equal(folder.items[0].label, 'Sign in');
  assert.deepEqual(folder.items[0].states[1], { id: 'error', label: 'Wrong password' });

  assert.equal(section.items[1].src, 'pages/loose.html');
  assert.equal(section.items[1].icon, 'panel-top');
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
      yaml.parse(['name: Acme', 'sections:', '  - name: Pages', '  no colon here'].join('\n'));
    },
    /line 4/
  );
});

test('refuses a tab where an indent belongs', function () {
  assert.throws(function () {
    yaml.parse(['sections:', '\t- name: Pages'].join('\n'));
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
  'sections:',
  '  - name: Pages',
  '    items:',
  '      - label: Sign in',
  '        src: pages/sign-in.html',
  '        viewports:',
  '          - desktop',
  '          - mobile',
  '        states:',
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

    var signIn = read.sections[0].items[0];
    assert.deepEqual(signIn.viewports, ['desktop', 'mobile']);
    assert.deepEqual(signIn.implementations, {
      dev: { path: '/', states: { error: '/?error=1' } },
      staging: { path: '/' },
    });
    assert.deepEqual(signIn.code, [
      { implementation: 'dev', path: 'src/pages/login' },
      { implementation: 'staging', path: 'src/pages/login' },
    ]);

    var button = read.sections[1].items[0];
    assert.deepEqual(button.viewports, ['fit', 'desktop', 'mobile', 'responsive']);
    assert.equal(button.icon, 'square-mouse-pointer');
    assert.deepEqual(button.implementations, { storybook: { title: 'Components/Button' } });
    assert.deepEqual(read.problems, []);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('updates the sections block without rewriting the rest of the YAML file', function () {
  var original = [
    'name: Acme # keep this comment',
    'implementations:',
    '  dev:',
    '    kind: url',
    '    base: http://localhost:3000',
    'sections:',
    '  - name: Old',
    '    items:',
    '      - label: Old page',
    '        src: old.html',
    '',
  ].join('\n');
  var root = project({ 'workbench.yaml': original });
  try {
    config.updateSections(root, [{ name: 'Pages', icon: 'file-text', items: [{
      label: 'Sign in', src: 'pages/sign-in.html', viewports: ['desktop', 'mobile'],
    }] }]);
    var body = fs.readFileSync(path.join(root, 'workbench.yaml'), 'utf8');
    assert.match(body, /name: Acme # keep this comment/);
    assert.match(body, /base: http:\/\/localhost:3000/);
    assert.match(body, /viewports:\n\s+- desktop\n\s+- mobile/);
    assert.deepEqual(config.read(root).sections[0].items[0].viewports, ['desktop', 'mobile']);
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

test('accepts a Storybook catalog without handwritten sections', function () {
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
    assert.equal(read.implementations.storybook.catalog, true);
    assert.equal(read.implementations.storybook.catalogIcon, 'book-open');
    assert.deepEqual(read.implementations.storybook.catalogIcons, {});
    assert.deepEqual(read.sections, []);
    assert.deepEqual(read.problems, []);
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

test('accepts automatic Storybook and Simulator catalogs without handwritten sections', function () {
  var root = project({
    'workbench.yaml': [
      'implementations:',
      '  storybook:',
      '    kind: storybook',
      '    url: auto',
      '    catalog: true',
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
    assert.deepEqual(read.sections, []);
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

    var signIn = view.screens['pages/sign-in.html'];
    assert.equal(signIn.label, 'Sign in');
    assert.equal(signIn.design, path.join(root, 'pages/sign-in.html'));
    assert.deepEqual(signIn.code, [
      { implementation: 'dev', path: path.join(product, 'src/pages/login'), relative: 'src/pages/login', exists: true },
      { implementation: 'staging', path: null, relative: 'src/pages/login', exists: false },
    ]);

    var button = view.screens['preview/components-button.html'];
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

test('reports a project icon image that isn’t in the project, and resolves the mark', function () {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-mark-'));
  try {
    fs.writeFileSync(path.join(root, 'workbench.yaml'), [
      'name: Acme',
      'color: purple',
      'icon: brand/logo.svg',
      'sections:',
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

function multiProject(body) {
  var dir = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-multi-'));
  fs.writeFileSync(path.join(dir, 'workbench.yaml'), body);
  return dir;
}

var MULTI = [
  '# Two projects in one file',
  'implementations:',
  '  dev:',
  '    kind: url',
  '    base: http://localhost:3000',
  'projects:',
  '  web:',
  '    name: Acme Web',
  '    sections:',
  '      - name: Pages',
  '        items:',
  '          - label: Home',
  '            src: index.html',
  '  ui:',
  '    name: Acme UI',
  '    root: packages/ui',
  '    # its own sections come later',
  '',
  'previews: false',
  '',
].join('\n');

test('lists one project per entry, each with its root', function () {
  var dir = multiProject(MULTI);
  try {
    assert.deepEqual(config.list(dir), [
      { key: 'web', root: dir },
      { key: 'ui', root: path.join(dir, 'packages', 'ui') },
    ]);
    var single = multiProject('name: Acme\nsections: []\n');
    assert.deepEqual(config.list(single), [{ key: null, root: single }]);
    var broken = multiProject('name: Acme\n  sections: []\n');
    assert.deepEqual(config.list(broken), [{ key: null, root: broken }]);
    assert.deepEqual(config.list(path.join(dir, 'packages')), []);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('reads one project of a file, with the shared keys and its own root', function () {
  var dir = multiProject(MULTI);
  try {
    var web = config.read({ dir: dir, key: 'web' });
    assert.equal(web.name, 'Acme Web');
    assert.equal(web.key, 'web');
    assert.equal(web.root, dir);
    assert.equal(web.sections[0].items[0].src, 'index.html');
    assert.equal(web.implementations.dev.base, 'http://localhost:3000');
    assert.equal(web.previews, false);

    var ui = config.read({ dir: dir, key: 'ui' });
    assert.equal(ui.root, path.join(dir, 'packages', 'ui'));
    assert.deepEqual(ui.sections, []);
    /* A folder alone reads the first project. */
    assert.equal(config.read(dir).key, 'web');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('saving pages for one project rewrites only that project, adding its sections when it has none', function () {
  var dir = multiProject(MULTI);
  try {
    var sections = [{ name: 'Components', items: [{ label: 'Button', src: 'button.html' }] }];
    var saved = config.updateSections({ dir: dir, key: 'ui' }, sections);
    assert.deepEqual(saved.sections, sections);
    var body = fs.readFileSync(path.join(dir, 'workbench.yaml'), 'utf8');
    assert.match(body, /^# Two projects in one file/);
    assert.match(body, /    # its own sections come later\n    sections:\n      - name: Components\n/);
    assert.match(body, /\n\npreviews: false\n$/);
    assert.equal(config.read({ dir: dir, key: 'web' }).sections[0].items[0].src, 'index.html');
    assert.equal(config.read({ dir: dir, key: 'ui' }).sections[0].items[0].src, 'button.html');

    config.updateSections({ dir: dir, key: 'web' }, [{ name: 'Pages', items: [{ label: 'About', src: 'about.html' }] }]);
    assert.equal(config.read({ dir: dir, key: 'web' }).sections[0].items[0].label, 'About');
    assert.equal(config.read({ dir: dir, key: 'ui' }).sections[0].items[0].label, 'Button');
    assert.equal(config.read({ dir: dir, key: 'web' }).implementations.dev.base, 'http://localhost:3000');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
