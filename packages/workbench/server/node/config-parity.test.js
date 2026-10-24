/* The Rust config reader (`workbench-server config`) gives the Node reader's
   answer, `config.resolve(dir, config.read({ dir, key }))`, for every
   fixture: a handwritten corpus that reaches each rule and problem, and
   seeded random configs built from the same keys. specs/system/server.md, phase 2.
   The browser's reader (ui/legacy/config.js, which the canvas and the
   sidebar use) reads the same page tree with the same problems, apart from
   what only the server reads: sizes, and paths on this machine. */
var test = require('node:test');
var assert = require('node:assert/strict');
var cp = require('node:child_process');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var url = require('node:url');
var vm = require('node:vm');

var config = require('./config');
var yaml = require('./yaml');
var manifest = require('../../contracts/manifest');
var host = require('./src/modules/host/index.ts');

var BINARY = host.binaryPath();
var BROWSER = fs.readFileSync(path.join(__dirname, '../../ui/legacy/config.js'), 'utf8');

function node(dir, key) {
  try { return JSON.parse(JSON.stringify(config.resolve(dir, config.read({ dir: dir, key: key })))); }
  catch (error) { return { error: String(error.message || error) }; }
}

/* The browser's reader, given the space's folder as the editor's sidebar
   gives it, and its key as the server does. Its problems are what it warns
   about, or what it refuses with. */
function browser(dir, key) {
  var warnings = [];
  var window = { wbManifest: manifest, wbYaml: yaml.parse };
  function Request() {}
  Request.prototype.open = function (_, address) { this.url = address; };
  Request.prototype.send = function () {
    var file = path.join(dir, path.basename(decodeURIComponent(new URL(this.url).pathname)));
    this.status = fs.existsSync(file) ? 200 : 404;
    this.responseText = this.status === 200 ? fs.readFileSync(file, 'utf8') : '';
    this.onload();
  };
  vm.runInNewContext(BROWSER, {
    window: window, URL: URL,
    location: { protocol: 'http:', search: '' },
    document: {
      baseURI: 'http://127.0.0.1/',
      querySelector: function (selector) {
        if (selector.indexOf('canonic-root') !== -1) return { content: url.pathToFileURL(dir).href + '/' };
        if (key && selector.indexOf('canonic-space') !== -1) return { content: key };
        return null;
      },
    },
    XMLHttpRequest: Request,
    console: { warn: function (message) { warnings.push(message); } },
  });
  var out = null;
  window.wbConfig.load(
    function (loaded) { out = { config: JSON.parse(JSON.stringify(loaded)), problems: warnings.length ? warnings[0].split('\n').slice(1) : [] }; },
    function (error) { out = { error: String(error.message || error) }; }
  );
  return out;
}

/* What only the server reads: sizes, an icon file, code on this disk. */
var SERVER_ONLY = new RegExp([
  '^Sizes', '^icon: .* isn’t in the project\\.$', 'can’t resolve — implementation',
  ': true only includes a default size', ': must be true or a map of width', 'isn’t a size field',
  ', so it takes no width or height\\.', 'must be a whole number from 1 to 8192, or fill\\.',
  'width and height can’t both be fill', ': icon “[^”]*” must be a kebab-case Lucide icon name\\.$',
  'button doesn’t apply to a page’s own size', ': button must be true or false\\.',
  'isn’t one of the space’s sizes\\.', 'each size is a key of the space’s sizes',
  'must be a kebab-case key that isn’t all digits\\.', 'is one of the space’s sizes; a page can list it',
  '” is listed twice\\.',
].join('|'));

/* The page tree without what only the server adds to it. */
function tree(collections) {
  function items(list) {
    return list.map(function (item) {
      if (item.group) return { group: item.group, items: items(item.items) };
      var out = Object.assign({}, item);
      delete out.sizes;
      delete out.ownSizes;
      return out;
    });
  }
  return collections.map(function (collection) {
    return { name: collection.name, icon: collection.icon || 'file-text', items: items(collection.items) };
  });
}

function sameInBrowser(dir, key, label) {
  var expected = node(dir, key);
  var seen = browser(dir, key);
  if (expected.error) {
    assert.ok(seen.error, 'the browser refuses what the server can’t read: ' + label);
    return;
  }
  var problems = expected.problems.filter(function (problem) { return !SERVER_ONLY.test(problem); }).sort();
  if (seen.error) {
    assert.ok(expected.problems.indexOf(manifest.NOTHING_TO_SHOW) !== -1, 'the browser refuses only a space with nothing to show: ' + seen.error + '\n' + label);
    assert.deepEqual(seen.error.slice(seen.error.indexOf(' — ') + 3).split('\n').sort(), problems, label);
    return;
  }
  assert.deepEqual(seen.problems.slice().sort(), problems, label);
  assert.deepEqual(tree(seen.config.collections), tree(expected.collections), label);
  assert.equal(seen.config.name, expected.name, label);
}

function rust(dir, key) {
  var run = cp.spawnSync(BINARY, ['config', dir].concat(key ? ['--space', key] : []), { encoding: 'utf8' });
  if (run.status !== 0) return { error: run.stderr.trim() };
  return JSON.parse(run.stdout);
}

/* A project from { 'workbench.yaml': text, 'workbench.local.yaml': text, other files }. */
function project(t, files) {
  var dir = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-config-parity-'));
  t.after(function () { fs.rmSync(dir, { recursive: true, force: true }); });
  Object.keys(files).forEach(function (name) {
    fs.mkdirSync(path.dirname(path.join(dir, name)), { recursive: true });
    fs.writeFileSync(path.join(dir, name), files[name]);
  });
  return fs.realpathSync(dir);
}

function same(t, files, keys) {
  var dir = project(t, files);
  (keys || [null]).forEach(function (key) {
    assert.deepEqual(rust(dir, key), node(dir, key), 'space ' + key + ' of\n' + files['workbench.yaml']);
  });
}

function sameBrowser(t, files, keys) {
  var dir = project(t, files);
  (keys || [null]).forEach(function (key) {
    sameInBrowser(dir, key, 'space ' + key + ' of\n' + files['workbench.yaml']);
  });
}

var CORPUS = [
  // YAML: scalars, quotes, comments, duplicates, index keys, and each syntax error.
  { 'workbench.yaml': 'name: "Acme # demo" # a comment\ncolor: \'#2F7D55\'\nicon: rocket\nflag: true\nnothing: ~\nport: 007\nratio: -1.50\n"10": ten\n2: two\nname: Acme again\n' },
  { 'workbench.yaml': 'name: Acme\n\tcolor: blue\n' },
  { 'workbench.yaml': '  name: Acme\n' },
  { 'workbench.yaml': 'name: Acme\njust words\n' },
  { 'workbench.yaml': 'collections:\n  -oops\n' },
  { 'workbench.yaml': 'collections:\n  - name: Pages\n      items: []\n' },
  { 'workbench.yaml': ': no key\n' },
  { 'workbench.yaml': '' },
  { 'workbench.yaml': '# only a comment\n\n' },
  { 'workbench.yaml': 'name: Acme\r\ncolor: green\r\n' },
  // The mark.
  { 'workbench.yaml': 'name: Acme\ncolor: magenta\nicon: Not An Icon\n' },
  { 'workbench.yaml': 'color: "#abc"\nicon: brand/mark.svg\n', 'brand/mark.svg': '<svg/>' },
  { 'workbench.yaml': 'icon: missing.png\n' },
  { 'workbench.yaml': 'icon: ../outside.png\ncolor: 12\n' },
  // Spaces.
  { 'workbench.yaml': 'implementations:\n  dev:\n    kind: url\n    base: http://localhost:3000\nsizes:\n  laptop: true\nspaces:\n  web:\n    name: Web\n    root: apps/web\n  docs:\n    color: teal\n    implementations:\n      dev:\n        base: http://localhost:4000\n  Bad_Key:\n    name: x\n  list-space: [nope]\n', keys: [null, 'web', 'docs', 'missing'] },
  { 'workbench.yaml': 'spaces: nope\n' },
  { 'workbench.yaml': 'spaces:\n  only:\n    root: https://example.com\n    name: 42\n', keys: [null, 'only'] },
  // Local overrides.
  { 'workbench.yaml': 'name: Acme\nimplementations:\n  dev:\n    kind: url\n    base: http://localhost:3000\n    label: Dev\n  staging:\n    kind: url\n    base: https://staging.example.com\nsizes:\n  laptop: true\n  tablet:\n    width: 820\n    height: 1180\n', 'workbench.local.yaml': 'implementations:\n  dev:\n    base: http://localhost:4000\n    root: ../app\n  extra:\n    kind: url\n    base: http://127.0.0.1:9000\nsizes:\n  tablet:\n    width: 768\n  phone: true\nname: Local Acme\n' },
  { 'workbench.yaml': 'spaces:\n  a:\n    name: A\n    sizes:\n      mobile: true\n  b:\n    name: B\n', 'workbench.local.yaml': 'spaces:\n  a:\n    sizes:\n      laptop: true\n  c:\n    name: C\n', keys: ['a', 'b', 'c'] },
  { 'workbench.yaml': 'name: Acme\n', 'workbench.local.yaml': '\tbroken\n' },
  // Implementations, every kind and problem.
  { 'workbench.yaml': [
    'implementations:',
    '  dev:', '    kind: url', '    base: http://localhost:3000///', '    root: ../acme', '    render: popup',
    '    start:', '      command: pnpm dev', '      cwd: app', '      check:', '        port: "3000"', '      ready:', '        url: http://localhost:3000/health', '      timeout: 30',
    '  staging:', '    kind: url', '    base: localhost:3000',
    '  Bad Name:', '    kind: url',
    '  design:', '    kind: url', '    base: http://localhost:1',
    '  plain: just text',
    '  wiki:', '    kind: wiki',
    '  stories:', '    kind: storybook', '    url: auto',
    '  sb:', '    kind: storybook',
    '  sb-start:', '    kind: storybook', '    start:', '      command: pnpm storybook',
    '  sb-base:', '    kind: storybook', '    base: http://localhost:6006',
    '  sb-url:', '    kind: storybook', '    url: http://localhost:6006/', '    root: packages/ui', '    catalog: true',
    '  sim:', '    kind: ios-simulator', '    catalog:', '      icon: Tablet Icon', '    start:', '      command: x',
    '  sim2:', '    kind: ios-simulator', '    device: iPhone 17', '    catalog: true',
    '  sim3:', '    kind: ios-simulator', '    catalog: maybe',
    '  win:', '    kind: window', '    app: com.example.app',
    '  win-bad:', '    kind: window', '    app: not ok!',
    '  previews:', '    kind: workbench', '    root: elsewhere',
    '  docs-ok:', '    kind: docs', '    adapter: react', '    styles:', '      - src/theme.css', '      - /abs.css', '      - ../up.css', '    environment: src/env.ts', '    url: http://x',
    '  docs-bad:', '    kind: docs',
    '  docs-root:', '    kind: docs', '    adapter: html', '    root: other',
    '  probe:', '    kind: url', '    base: https://example.com', '    start:', '      command: ""', '      check:', '        port: 0', '      timeout: 999',
    '  probe2:', '    kind: url', '    base: https://example.com', '    start:', '      command: go', '      check:', '        port: 80', '        url: http://x', '      ready: nope',
    '  probe3:', '    kind: url', '    base: https://example.com', '    start: yes',
    '  rooted:', '    kind: url', '    base: http://localhost:2', '    root: ftp://example.com/x',
    ''].join('\n') },
  { 'workbench.yaml': 'implementations: [a, b]\n' },
  { 'workbench.yaml': 'implementations:\n' },
  // Previews.
  { 'workbench.yaml': 'previews: false\n' },
  { 'workbench.yaml': 'previews: nope\n' },
  { 'workbench.yaml': 'previews:\n  lensLabel: "  Preview  "\n  icon: Bad Icon\n  icons:\n    Components/: component\n    Pages: Not Kebab\n    "": file\n  include:\n    - src/**/*.workbench.ts\n  config: tools/workbench.config.ts\n' },
  { 'workbench.yaml': 'previews:\n  lensLabel: ""\n  icons: list\n  include: []\n' },
  { 'workbench.yaml': 'previews:\n  include:\n    - ../outside/*.ts\n' },
  { 'workbench.yaml': 'previews:\n  config: https://example.com/c.ts\n' },
  { 'workbench.yaml': 'previews:\n  icon: rocket\n  lensLabel: 12\n' },
  // Sizes.
  { 'workbench.yaml': 'sizes:\n  fit: true\n  laptop:\n    width: 1440\n    label: Small laptop\n    icon: laptop\n    button: false\n  mobile: true\n  resizable:\n    width: 10\n  tablet:\n    width: 820\n    height: 1180\n    color: blue\n  wide:\n    width: fill\n    height: 600\n  full:\n    width: fill\n    height: fill\n  tiny:\n    width: 0\n    height: 10\n  "123": true\n  Bad: true\n  custom: true\n  ok-icon:\n    width: 300\n    height: 300\n    icon: Not Kebab\n    button: maybe\n' },
  { 'workbench.yaml': 'sizes: [laptop]\n' },
  { 'workbench.yaml': 'sizes:\n  nothing: false\n' },
  // Pages, groups and the tree's dropped entries.
  { 'workbench.yaml': [
    'implementations:',
    '  dev:', '    kind: url', '    base: http://localhost:3000', '    root: app',
    '  staging:', '    kind: url', '    base: https://staging.example.com',
    '  sb:', '    kind: storybook', '    url: http://localhost:6006',
    '  web:', '    kind: docs', '    adapter: react',
    '  native:', '    kind: docs', '    adapter: react-native-web',
    '  sim:', '    kind: ios-simulator',
    '  win:', '    kind: window', '    app: com.example.app',
    '  wb:', '    kind: workbench',
    '  docs:', '    kind: url', '    base: http://localhost:7',
    'sizes:', '  laptop: true', '  mobile: true',
    'collections:',
    '  - name: Pages', '    icon: file-text', '    items:',
    '      - group: Auth', '        items:',
    '          - label: Sign in', '            src: pages/sign-in.html', '            lensLabel: Reference',
    '            sizes:', '              - mobile', '              - tablet', '              - phone:', '                  width: 360', '                  height: 740', '              - laptop:', '                  width: 1', '              - mobile', '              - phone:', '                  width: 1', '                  height: 1', '              - Bad:', '                  width: 1', '                  height: 1', '              - a: 1', '                b: 2',
    '            scenarios:', '              - id: default', '                label: Default', '              - id: error', '                label: Wrong password',
    '                iterations:', '                  - id: inline-hint', '                  - id: banner', '                    label: Top banner', '                  - id: Bold', '                  - id: 2-up', '                  - label: No id', '                  - id: banner', '                  - id: 7', '                  - just-text',
    '              - id: Bad Id', '                label: x', '              - id: loading',
    '            implementations:',
    '              dev:', '                default: /sign-in', '                error: /sign-in?error=1', '                missing: /x', '                loading: relative',
    '              staging: /sign-in', '              sb: Components/Button', '              sim: iPhone', '              win: Acme Window', '              wb: pages/sign-in', '              undeclared: /x',
    '            code:', '              dev:', '                - src/sign-in.ts', '                - ""', '              sb: src/button.tsx', '              ghost: x',
    '          - group: Nested', '            items:', '              - label: Hidden', '                src: pages/hidden.html',
    '          - label: Bad src', '            src: pages/a:b.html',
    '          - label: Route', '            src: routes/+page.html',
    '          - label: Lone', '            src: pages/lone.html', '            scenarios:', '              - id: default', '                label: Default', '                iterations:', '                  - id: dense',
    '          - label: Listless', '            src: pages/listless.html', '            scenarios:', '              - id: default', '                label: Default', '                iterations: dense', '              - id: other', '                label: Other', '                iterations: []',
    '          - label: Up', '            src: ../up.html',
    '          - label: Abs', '            src: /abs.html',
    '          - src: pages/no-label.html',
    '      - label: Dashboard', '        src: pages/dashboard.html', '        docs: docs/dashboard.md', '        lens: web',
    '        implementations:', '          web: docs/dashboard.examples.ts', '          native: /abs/examples', '          dev: 42', '          staging:', '            other: /x', '          sb: ""', '          wb: Not An Id', '          win: ""', '          sim: ""',
    '        code: just-text',
    '      - label: Bad docs', '        src: pages/x.html', '        docs: docs/x.txt', '        implementations:', '          web: x/',
    '      - label: List lenses', '        src: pages/y.html', '        implementations: [a]', '        code: [a]',
    '  - name: Docs', '    items:',
    '      - label: Card', '        src: docs/card.md', '        lens: native', '        lensLabel: Nope', '        docs: docs/other.md', '        sizes: [mobile]',
    '        implementations:', '          web: docs/card.examples.ts', '          native: docs/card/',
    '      - label: Guide', '        src: docs/guide.md', '        lens: nothing',
    '      - label: Builtin', '        src: docs/builtin.md', '        implementations:', '          docs: /x',
    '  - name: Empty',
    '  - name: Icon only', '    icon: component',
    '  - icon: no-name', '    items:', '      - label: Lost', '        src: lost.html',
    ''].join('\n'), 'app/src/sign-in.ts': 'export {}' },
  // Scenarios: a first scenario that isn't `default`, lone scenarios with path
  // maps, labels from the id, repeated ids, and a preview definition's page,
  // whose path maps wait for the definition's scenarios.
  { 'workbench.yaml': [
    'implementations:', '  dev:', '    kind: url', '    base: http://localhost:3000',
    'collections:',
    '  - name: App', '    items:',
    '      - label: Library', '        src: library.html',
    '        scenarios:', '          - id: filled', '            label: Filled', '          - id: empty-shelf',
    '        implementations:', '          dev:', '            filled: /library', '            empty-shelf: /library?empty',
    '      - label: Shelf', '        src: shelf.html',
    '        scenarios:', '          - id: filled', '          - id: empty',
    '        implementations:', '          dev:', '            default: /shelf', '            empty: /shelf?empty', '            gone: /gone',
    '      - label: Lone map', '        src: lone-map.html',
    '        scenarios:', '          - id: filled',
    '        implementations:', '          dev:', '            filled: /lone',
    '      - label: Lone default', '        src: lone-default.html',
    '        scenarios:', '          - id: filled', '            label: Filled',
    '        implementations:', '          dev:', '            default: /lone',
    '      - label: Repeated', '        src: repeated.html',
    '        scenarios:', '          - id: default', '          - id: error', '            label: Error', '          - id: error', '            label: Again', '          - id: 2-up', '          - id: a--b', '          - id: 7', '          - label: No id',
    '      - label: Listless', '        src: listless.html', '        scenarios: nope',
    '      - label: Jobs', '        src: jobs.workbench.ts',
    '        implementations:', '          dev:', '            default: /jobs', '            empty: /jobs?empty', '            bad: relative',
    ''].join('\n') },
  { 'workbench.yaml': [
    'previews: false',
    'implementations:', '  dev:', '    kind: url', '    base: http://localhost:3000',
    'collections:',
    '  - name: App', '    items:',
    '      - label: Jobs', '        src: jobs.workbench.ts',
    '        implementations:', '          dev:', '            default: /jobs', '            empty: /jobs?empty',
    ''].join('\n') },
  // Duplicates: collections by name, pages by src, across collections and groups.
  { 'workbench.yaml': [
    'collections:',
    '  - name: Pages', '    items:',
    '      - label: Sign in', '        src: pages/sign-in.html',
    '      - group: Auth', '        items:',
    '          - label: Sign in again', '            src: pages/sign-in.html',
    '          - label: Reset', '            src: pages/reset.html',
    '      - group: Only copies', '        items:',
    '          - label: Reset again', '            src: pages/reset.html',
    '  - name: Pages', '    icon: star', '    items:',
    '      - label: Home', '        src: pages/home.html',
    '  - name: More', '    items:',
    '      - label: Home', '        src: pages/home.html',
    '      - label: Reset elsewhere', '        src: pages/reset.html',
    '      - label: ""', '        src: pages/unnamed.html',
    '      - label: Unlinked',
    ''].join('\n') },
  // Nothing to show: no page, and nothing to import.
  { 'workbench.yaml': 'previews: false\n' },
  { 'workbench.yaml': 'previews: false\ncollections:\n  - name: Placeholder\n    icon: star\n  - name: Empty\n' },
  { 'workbench.yaml': 'previews: false\nimplementations:\n  sim:\n    kind: ios-simulator\n    catalog: true\n' },
  { 'workbench.yaml': 'name: Acme\ncollections:\n  - name: Empty\n' },
];

test('the Rust config reader gives the Node reader’s answer for every fixture in the corpus', { skip: !fs.existsSync(BINARY) && 'build the server first' }, function (t) {
  CORPUS.forEach(function (entry) {
    var files = Object.assign({}, entry);
    var keys = files.keys;
    delete files.keys;
    same(t, files, keys);
  });
});

test('the browser’s reader gives the server reader’s page tree and problems for every fixture in the corpus', function (t) {
  CORPUS.forEach(function (entry) {
    var files = Object.assign({}, entry);
    var keys = files.keys;
    delete files.keys;
    sameBrowser(t, files, keys);
  });
});

test('every reader reports dropped entries, duplicates and nothing to show at the same places', function (t) {
  var duplicates = CORPUS.find(function (entry) { return /Sign in again/.test(entry['workbench.yaml']); });
  var dir = project(t, duplicates);
  var read = node(dir, null);
  assert.deepEqual(read.collections.map(function (collection) { return collection.name; }), ['Pages', 'More']);
  assert.deepEqual(read.problems, [
    'Pages › Auth › Sign in again: src “pages/sign-in.html” is already in the page tree; only its first entry is kept.',
    'Pages › Only copies › Reset again: src “pages/reset.html” is already in the page tree; only its first entry is kept.',
    'Pages: group “Only copies” has nothing in it.',
    'collection “Pages” is declared twice; only the first is kept.',
    'More › Reset elsewhere: src “pages/reset.html” is already in the page tree; only its first entry is kept.',
    'More › pages/unnamed.html: every page needs a label and a src.',
    'More › Unlinked: every page needs a label and a src.',
  ]);
  assert.deepEqual(browser(dir, null).problems, read.problems);

  var scenarios = CORPUS.find(function (entry) { return /Lone map/.test(entry['workbench.yaml']); });
  dir = project(t, scenarios);
  read = node(dir, null);
  var items = read.collections[0].items;
  assert.deepEqual(items[0].scenarios, [{ id: 'filled', label: 'Filled' }, { id: 'empty-shelf', label: 'Empty shelf' }]);
  assert.deepEqual(items[0].implementations, { dev: { path: '/library', scenarios: { 'empty-shelf': '/library?empty' } } });
  assert.deepEqual(items[1].implementations, { dev: { path: '/shelf', scenarios: { empty: '/shelf?empty' } } });
  assert.equal(items[2].scenarios, undefined, 'one scenario offers no choice');
  assert.deepEqual(items[2].implementations, { dev: { path: '/lone', scenarios: {} } });
  assert.deepEqual(items[3].implementations, { dev: { path: '/lone', scenarios: {} } });
  assert.deepEqual(items[4].scenarios, [{ id: 'default', label: 'Default' }, { id: 'error', label: 'Error' }]);
  assert.deepEqual(items[6].implementations, { dev: { pending: { default: '/jobs', empty: '/jobs?empty' } } });
  assert.deepEqual(read.problems, [
    'App › Shelf: implementation “dev” maps scenario “gone”, which this page doesn’t declare.',
    'App › Repeated: scenario id “error” is declared twice.',
    'App › Repeated: scenario id “2-up” must be kebab-case — it travels in a URL.',
    'App › Repeated: scenario id “a--b” must be kebab-case — it travels in a URL.',
    // Entries 6 and 7: `id: 7` is a number, not an id, and the last has none.
    'App › Repeated: scenario 6 needs an id.',
    'App › Repeated: scenario 7 needs an id.',
    'App › Listless: scenarios must be a list of entries with an id and an optional label.',
    'App › Jobs: implementation “dev” paths must start with / — they’re appended to http://localhost:3000.',
  ]);
  assert.deepEqual(browser(dir, null).problems, read.problems);

  // Without previews, a preview definition's page is an authored page like any other.
  var offline = CORPUS.find(function (entry) { return /^previews: false\nimplementations:\n  dev:/.test(entry['workbench.yaml']); });
  dir = project(t, offline);
  assert.deepEqual(node(dir, null).problems, ['App › Jobs: implementation “dev” maps scenario “empty”, which this page doesn’t declare.']);

  dir = project(t, { 'workbench.yaml': 'previews: false\ncollections:\n  - name: Empty\n' });
  assert.deepEqual(node(dir, null).problems, ['collection “Empty” has nothing in it.', manifest.NOTHING_TO_SHOW]);
  assert.match(browser(dir, null).error, /nothing to show/);
});

test('an unnamed single space has its folder’s name in every reader', function (t) {
  var dir = project(t, { 'workbench.yaml': 'collections:\n  - name: Pages\n    items:\n      - label: Home\n        src: index.html\n' });
  assert.equal(node(dir, null).name, path.basename(dir));
  assert.equal(browser(dir, null).config.name, path.basename(dir));
});

test('a folder without workbench.yaml has no config in either reader', { skip: !fs.existsSync(BINARY) && 'build the server first' }, function (t) {
  var dir = project(t, { 'README.md': '# Acme' });
  assert.equal(rust(dir, null), null);
  assert.equal(node(dir, null), null);
});

/* Seeded random configs from the corpus's vocabulary, valid and not. */
function generator(seed) {
  var state = seed >>> 0;
  function next() { state = (state + 0x6D2B79F5) >>> 0; var t = state; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }
  function pick(items) { return items[Math.floor(next() * items.length)]; }
  function maybe(p) { return next() < p; }
  var scalars = ['', 'x', 'dev', 'Dev', '/path', 'pages/a.html', 'docs/a.md', 'http://localhost:3000', 'https://example.com', 'auto', 'rocket', 'Not Kebab', '#2f7d55', 'blue', 'fill', 'true', true, false, null, 0, 1, 393, 9000, -1, '..', '../x', 'a:b', 'web', 'storybook', 'url', 'docs', 'react', 'com.example.app'];
  function scalar() { return pick(scalars); }
  function implementation() {
    var out = { kind: pick(['url', 'storybook', 'docs', 'ios-simulator', 'window', 'workbench', 'nope', scalar()]) };
    ['base', 'url', 'root', 'label', 'adapter', 'device', 'app', 'catalog', 'render', 'styles', 'environment'].forEach(function (key) { if (maybe(0.2)) out[key] = key === 'styles' && maybe(0.5) ? [scalar(), scalar()] : scalar(); });
    if (maybe(0.15)) out.start = maybe(0.3) ? scalar() : { command: scalar(), check: maybe(0.7) ? { port: pick([3000, '3000', 0, 70000, 'x']) } : { url: scalar() }, timeout: pick([10, 0, 400, 'x']) };
    return out;
  }
  function page(names) {
    var out = { label: maybe(0.9) ? pick(['Sign in', 'Card', 'x', '']) : scalar(), src: pick(['pages/a.html', 'pages/b.html', 'docs/a.md', 'docs/b.md', 'a:b.html', 'a+b.html', '/abs.html', scalar()]) };
    if (maybe(0.3)) out.scenarios = [{ id: 'default', label: 'Default' }, { id: pick(['error', 'Bad', '']), label: pick(['Error', '']) }];
    if (out.scenarios && maybe(0.5)) out.scenarios[pick([0, 1])].iterations = maybe(0.2) ? scalar() : [{ id: pick(['bold', 'Bold', '1-up', 'two-words', '', 7]), label: scalar() }, { id: pick(['bold', 'calm']) }];
    if (maybe(0.3)) out.sizes = maybe(0.5) ? [pick(['laptop', 'mobile', 'tablet', 'x'])] : [{ phone: { width: pick([360, 0, 'fill']), height: 740 } }];
    if (maybe(0.2)) out.docs = pick(['docs/a.md', 'docs/a.txt', '../x.md', scalar()]);
    if (maybe(0.2)) out.lens = pick(names.concat(['nope']));
    if (maybe(0.2)) out.lensLabel = scalar();
    if (maybe(0.4)) {
      out.implementations = {};
      names.concat(['ghost']).forEach(function (name) { if (maybe(0.5)) out.implementations[name] = maybe(0.3) ? { default: pick(['/x', 'x']), error: '/e', other: '/o' } : scalar(); });
    }
    if (maybe(0.2)) { out.code = {}; names.concat(['ghost']).forEach(function (name) { if (maybe(0.4)) out.code[name] = maybe(0.3) ? [scalar(), scalar()] : scalar(); }); }
    return out;
  }
  return function config() {
    var names = ['dev', 'sb', 'web', 'sim', 'win', 'docs'].filter(function () { return maybe(0.6); });
    var out = {};
    if (maybe(0.7)) out.name = scalar();
    if (maybe(0.3)) out.color = scalar();
    if (maybe(0.3)) out.icon = scalar();
    if (maybe(0.8)) { out.implementations = {}; names.forEach(function (name) { out.implementations[name] = implementation(); }); }
    if (maybe(0.3)) out.sizes = { laptop: true, tablet: { width: pick([820, 0, 'fill']), height: 1180 }, mobile: maybe(0.5) ? true : { width: 10 } };
    if (maybe(0.3)) out.previews = maybe(0.3) ? false : { include: maybe(0.7) ? ['src/**/*.workbench.ts'] : [], lensLabel: scalar(), icons: { Components: pick(['component', 'Bad']) } };
    var items = [];
    var count = Math.floor(next() * 5);
    for (var i = 0; i < count; i++) items.push(maybe(0.25) ? { group: pick(['Auth', '']), items: [page(names), page(names)] } : page(names));
    if (items.length || maybe(0.5)) out.collections = [{ name: pick(['Pages', '']), icon: scalar(), items: items }];
    return out;
  };
}

test('the Rust config reader gives the Node reader’s answer for seeded random configs', { skip: !fs.existsSync(BINARY) && 'build the server first' }, function (t) {
  var next = generator(20261018);
  for (var i = 0; i < 300; i++) {
    var raw = next();
    var text;
    try { text = yaml.stringify(raw); } catch (error) { continue; }
    same(t, { 'workbench.yaml': text, 'pages/a.html': '<p>Acme</p>', 'docs/a.md': '# Acme' });
  }
});
