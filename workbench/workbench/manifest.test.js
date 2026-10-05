var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

var manifest = require('./manifest');

test('authored lens labels validate and retain defaults on invalid configuration', function () {
  var problems = [];
  var item = { src: 'button.html' };
  manifest.pageLensLabel(' Reference ', item, 'Pages › Button', problems);
  assert.equal(manifest.authoredLensLabel(item), 'Reference');
  assert.equal(manifest.authoredLensLabel({ workbench: true }), 'Workbench');
  assert.equal(manifest.authoredLensLabel({}), 'Design');
  assert.deepEqual(manifest.previews({ lensLabel: ' Design ' }, problems), { lensLabel: 'Design' });
  assert.deepEqual(problems, []);
  for (var invalid of ['', '  ', 12, null, {}]) {
    var invalidItem = { src: 'button.html' };
    manifest.pageLensLabel(invalid, invalidItem, 'Pages › Button', problems);
    assert.equal(manifest.authoredLensLabel(invalidItem), 'Design');
    assert.deepEqual(manifest.previews({ lensLabel: invalid }, problems), {});
  }
  assert.equal(problems.length, 10);
  assert.ok(problems.every(function (problem) { return /lensLabel: must be a nonempty string/.test(problem); }));
  var docs = { src: 'button.md' };
  manifest.pageLensLabel('Design', docs, 'Docs › Button', problems);
  assert.equal(docs.lensLabel, undefined);
  assert.match(problems[10], /docs lenses use implementation labels/);
});

test('manual preview placement overrides the discovery lens label and inherits it when omitted', function () {
  var imported = [{ name: 'Components', items: [{ src: 'button.workbench.ts', workbench: true, lensLabel: 'Design', states: [{ id: 'default', label: 'Default' }] }] }];
  for (var label of [undefined, 'Reference']) {
    var original = { src: 'button.workbench.ts', label: 'Button', implementations: { live: { path: '/button' } } };
    if (label) original.lensLabel = label;
    var base = [{ name: 'Components', items: [original] }];
    var merged = manifest.mergeCollections(base, imported);
    assert.equal(merged[0].items.length, 1);
    assert.equal(merged[0].items[0].lensLabel, label || 'Design');
    assert.equal(merged[0].items[0].workbench, true);
    assert.deepEqual(merged[0].items[0].implementations, original.implementations);
    assert.equal(original.workbench, undefined);
    assert.equal(original.lensLabel, label);
  }
});

test('preview icon maps validate and match complete title prefixes', function () {
  var problems = [];
  var settings = manifest.previews({ icon: 'boxes', icons: { 'Web App/': 'app-window', 'Web App/Pages': 'monitor' } }, problems);
  assert.deepEqual(problems, []);
  assert.equal(manifest.titleIcon(settings, 'Web App/Pages/Jobs', 'component').icon, 'monitor');
  assert.equal(manifest.titleIcon(settings, 'Web App/PagesExtra/Jobs', 'component').icon, 'app-window');
  assert.equal(manifest.titleIcon(settings, 'Other', 'component').icon, 'boxes');
  assert.equal(manifest.titleIcon({}, 'Other', 'component').icon, 'component');
  var invalid = manifest.previews({ icon: 'Bad Icon', icons: { '/': 'monitor', Pages: 'BAD' } }, problems);
  assert.deepEqual(invalid, { icons: {} });
  assert.equal(problems.length, 3);
});

test('shared collection icons use precedence in either import order and retain manual overrides', function () {
  var collection = function (icon, priority, src) { return { name: 'Web App', icon: icon, iconPriority: priority, items: [{ src: src }] }; };
  var preview = collection('component', 1, 'jobs.workbench.ts');
  var catalog = collection('monitor', 5, 'customers.html');
  for (var pair of [[preview, catalog], [catalog, preview]]) {
    assert.equal(manifest.mergeCollections([pair[0]], [pair[1]])[0].icon, 'monitor');
  }
  assert.equal(manifest.mergeCollections([collection('app-window', 6, 'jobs.workbench.ts')], [catalog])[0].icon, 'app-window');
  assert.equal(manifest.mergeCollections([preview], [collection('book-open', 0, 'customers.html')])[0].icon, 'component');
  assert.equal(manifest.mergeCollections([collection('boxes', 4, 'jobs.workbench.ts')], [catalog])[0].icon, 'monitor');
  assert.equal(manifest.mergeCollections([collection('file-text', 2, 'authored.html')], [preview])[0].icon, 'file-text');
  assert.equal(manifest.mergeCollections([collection('file-text', 2, 'authored.html')], [catalog])[0].icon, 'monitor');
  var manual = { name: 'Web App', icon: 'star', items: [] };
  assert.equal(manifest.mergeCollections([manual], [preview, catalog])[0].icon, 'star');
  assert.deepEqual(manifest.mergeCollections([manual], []), []);
  assert.deepEqual(manual.items, []);
  var first = collection('monitor', 5, 'first.html');
  var second = collection('app-window', 5, 'second.html');
  assert.equal(manifest.mergeCollections([first], [second])[0].icon, manifest.mergeCollections([second], [first])[0].icon);
});

/* The same file runs in the browser, where it has no `module` to export to. */
test('hands itself to a window when there is no module', function () {
  var window = {};
  var file = path.join(__dirname, 'manifest.js');
  vm.runInNewContext(fs.readFileSync(file, 'utf8'), { window: window }, { filename: file });
  assert.equal(typeof window.wbManifest.implementations, 'function');
  assert.equal(window.wbManifest.labelOf('local-dev'), 'Local dev');
});

test('merges a local file over the committed one, implementations by name', function () {
  var merged = manifest.merge(
    {
      name: 'Acme',
      collections: [{ name: 'Pages' }],
      implementations: {
        dev: { kind: 'url', base: 'http://localhost:3000', root: '../acme' },
        storybook: { kind: 'storybook', url: 'http://localhost:6006' },
      },
    },
    {
      name: 'Acme (mine)',
      implementations: {
        dev: { base: 'http://localhost:4000' },
        staging: { kind: 'url', base: 'https://staging.example.com' },
      },
    }
  );

  assert.equal(merged.name, 'Acme (mine)');
  assert.deepEqual(merged.collections, [{ name: 'Pages' }]);
  assert.deepEqual(merged.implementations.dev, { kind: 'url', base: 'http://localhost:4000', root: '../acme' });
  assert.deepEqual(merged.implementations.storybook, { kind: 'storybook', url: 'http://localhost:6006' });
  assert.equal(merged.implementations.staging.base, 'https://staging.example.com');
});

test('merge copes with a missing side', function () {
  assert.deepEqual(manifest.merge({ name: 'Acme' }, null), { name: 'Acme' });
  assert.deepEqual(manifest.merge(null, { name: 'Acme' }), { name: 'Acme' });
});

test('merges imported items into a manual collection with the same name', function () {
  var manual = [{ name: 'Components', icon: 'star', items: [{ src: 'button.html' }] }];
  var imported = [
    { name: 'Components', icon: 'component', items: [{ src: '__storybook/card.html' }] },
    { name: 'Foundations', icon: 'component', items: [{ src: '__storybook/color.html' }] },
  ];
  assert.deepEqual(manifest.mergeCollections(manual, imported), [
    { name: 'Components', icon: 'star', items: [{ src: 'button.html' }, { src: '__storybook/card.html' }] },
    { name: 'Foundations', icon: 'component', items: [{ src: '__storybook/color.html' }] },
  ]);
  assert.equal(manual[0].items.length, 1);
});

test('merges preview and Storybook groups by exact name within their collection without mutating inputs', function () {
  var previews = [{ name: 'Web App', icon: 'component', items: [
    { group: 'Pages', icon: 'folder', items: [{ label: 'Jobs', src: 'jobs.workbench.ts', workbench: true }] },
    { label: 'Welcome', src: 'welcome.html' },
  ] }];
  var stories = [{ name: 'Web App', icon: 'monitor', items: [
    { group: 'Pages', icon: 'book-open', items: [{ label: 'Account', src: '__storybook/account.html' }] },
    { group: 'Forms', items: [{ label: 'Contact', src: '__storybook/contact.html' }] },
    { group: 'pages', items: [{ label: 'Other', src: '__storybook/other.html' }] },
  ] }, { name: 'Auth', items: [
    { group: 'Pages', items: [{ label: 'Sign in', src: '__storybook/sign-in.html' }] },
  ] }];
  var before = JSON.stringify([previews, stories]);
  var merged = manifest.mergeCollections(previews, stories);
  assert.equal(merged[0].icon, 'component');
  assert.deepEqual(merged[0].items.map(function (item) { return item.group || item.label; }), ['Pages', 'Welcome', 'Forms', 'pages']);
  assert.equal(merged[0].items[0].icon, 'folder');
  assert.deepEqual(merged[0].items[0].items.map(function (item) { return item.label; }), ['Jobs', 'Account']);
  assert.equal(merged[1].items[0].items[0].label, 'Sign in');
  assert.equal(JSON.stringify([previews, stories]), before);
  var again = manifest.mergeCollections(merged, [{ name: 'Web App', items: [
    { group: 'Forms', items: [{ label: 'Search', src: 'search.workbench.ts', workbench: true }] },
  ] }]);
  assert.deepEqual(again[0].items[2].items.map(function (item) { return item.label; }), ['Contact', 'Search']);
  assert.equal(merged[0].items[2].items.length, 1);
  assert.equal(stories[0].items[1].items.length, 1);
});

test('reads the declared implementations', function () {
  var problems = [];
  var impls = manifest.implementations(
    {
      storybook: { kind: 'storybook', url: 'http://localhost:6006/', root: '../acme/packages/ui', label: 'Stories', catalog: true },
      dev: { kind: 'url', base: 'http://localhost:3710' },
      staging: { kind: 'url', base: 'https://staging.example.com' },
    },
    problems
  );

  assert.deepEqual(problems, []);
  assert.deepEqual(impls.storybook, {
    key: 'storybook', label: 'Stories', kind: 'storybook', url: 'http://localhost:6006',
    root: '../acme/packages/ui', catalog: true, catalogIcon: 'book-open', catalogIcons: {},
  });
  assert.deepEqual(impls.dev, {
    key: 'dev', label: 'Dev', kind: 'url', base: 'http://localhost:3710', root: null,
  });
  assert.equal(impls.staging.base, 'https://staging.example.com');
  assert.deepEqual(Object.keys(impls), ['storybook', 'dev', 'staging']);
});

test('names every problem with an implementation and keeps the rest', function () {
  var problems = [];
  var impls = manifest.implementations(
    {
      'Dev Server': { kind: 'url', base: 'http://localhost:3000' },
      dev: { kind: 'url', base: 'localhost:3000' },
      stories: { kind: 'storybook', base: 'http://localhost:6006' },
      docs: { kind: 'docs', url: 'http://localhost:1' },
      bare: 'http://localhost:2',
      staging: { kind: 'url', base: 'https://staging.example.com', render: 'popup', root: 'file:///x', catalog: true },
      odd: { kind: 'storybook', url: 'http://localhost:3', catalog: 'yes' },
    },
    problems
  );

  assert.deepEqual(Object.keys(impls), ['staging', 'odd']);
  assert.equal(impls.staging.render, undefined);
  assert.equal(impls.staging.root, null);
  assert.deepEqual(problems, [
    'implementations: “Dev Server” must be kebab-case — it travels in a URL.',
    'implementations › dev: needs a base starting with http:// or https://.',
    'implementations › stories: needs a url starting with http:// or https://, or url: auto.',
    'implementations › docs: kind must be url, storybook, workbench, examples, ios-simulator, or window.',
    'implementations › bare: needs a kind, and a url or base.',
    'implementations › staging: catalog is only available for Storybook and iOS Simulator implementations.',
    'implementations › staging: render is no longer used; remove it. URL and Storybook implementations use iframes.',
    'implementations › staging: root must be a folder path, not a URL.',
    'implementations › odd: catalog must be true, false, or a map of icon settings.',
  ]);
});

test('accepts automatic Storybook and catalogued iOS Simulator implementations', function () {
  var problems = [];
  var implementations = manifest.implementations({
    stories: { kind: 'storybook', url: 'auto', catalog: true },
    simulator: { kind: 'ios-simulator', device: 'booted', catalog: true },
  }, problems);
  assert.deepEqual(problems, []);
  assert.deepEqual(implementations.stories, {
    key: 'stories', label: 'Stories', kind: 'storybook', root: null,
    url: 'auto', auto: true, catalog: true, catalogIcon: 'book-open', catalogIcons: {},
  });
  assert.deepEqual(implementations.simulator, {
    key: 'simulator', label: 'Simulator', kind: 'ios-simulator', root: null,
    device: 'booted', catalog: true, catalogIcon: 'smartphone', catalogIcons: {},
  });
});

test('accepts a window implementation by application, and pages map it to a window title', function () {
  var problems = [];
  var implementations = manifest.implementations({
    emulator: { kind: 'window', app: 'com.example.emulator' },
    blank: { kind: 'window' },
    spaced: { kind: 'window', app: 'Example App' },
    started: { kind: 'window', app: 'example', start: { command: 'run', check: { port: 3000 } }, catalog: true },
  }, problems);
  assert.deepEqual(implementations.emulator, {
    key: 'emulator', label: 'Emulator', kind: 'window', root: null, app: 'com.example.emulator',
  });
  assert.equal(implementations.blank, undefined);
  assert.equal(implementations.spaced, undefined);
  assert.deepEqual(problems, [
    'implementations › blank: needs an app — the application’s bundle ID, or part of it, such as com.example.app.',
    'implementations › spaced: needs an app — the application’s bundle ID, or part of it, such as com.example.app.',
    'implementations › started: catalog is only available for Storybook and iOS Simulator implementations.',
    'implementations › started: start is available for URL and Storybook implementations.',
  ]);

  problems = [];
  var lenses = manifest.pageLenses(
    { emulator: 'Example Phone', started: '' },
    { label: 'Sign in', src: 'pages/sign-in.html' },
    implementations,
    'Pages › Sign in',
    problems
  );
  assert.deepEqual(lenses, { emulator: { window: 'Example Phone' } });
  assert.deepEqual(problems, ['Pages › Sign in: implementation “started” needs the window’s title, or part of it.']);
  assert.equal(manifest.streamed(implementations.emulator), true);
  assert.equal(manifest.streamed({ kind: 'ios-simulator' }), true);
  assert.equal(manifest.streamed({ kind: 'url' }), false);
  assert.equal(manifest.streamed(null), false);
});

test('refuses an implementations block that is not a map', function () {
  var problems = [];
  assert.deepEqual(manifest.implementations(['dev'], problems), {});
  assert.deepEqual(problems, ['implementations: must be a map of names to implementations.']);
  assert.deepEqual(manifest.implementations(undefined, problems), {});
  assert.equal(problems.length, 1);
});

test('legacy renderer settings keep implementations usable and report how to migrate', function () {
  var window = {};
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'manifest.js'), 'utf8'), { window: window });
  var raw = manifest.merge({ implementations: {
    dev: { kind: 'url', base: 'http://localhost:3000', render: 'browser' },
    stories: { kind: 'storybook', url: 'http://localhost:6006', render: 'iframe' },
  } }, { implementations: { dev: { base: 'http://localhost:4000' } } });
  var problems = [];
  var implementations = manifest.implementations(raw.implementations, problems);
  var browserProblems = [];
  var browserImplementations = window.wbManifest.implementations(raw.implementations, browserProblems);
  assert.deepEqual(JSON.parse(JSON.stringify(browserImplementations)), implementations);
  assert.deepEqual(Array.from(browserProblems), problems);
  assert.equal(implementations.dev.base, 'http://localhost:4000');
  assert.equal(implementations.dev.render, undefined);
  assert.equal(implementations.stories.render, undefined);
  assert.equal(problems.length, 2);
  assert.ok(problems.every(function (problem) { return /remove it.*iframes/.test(problem); }));
  assert.deepEqual(manifest.pageLenses({ dev: '/login' }, { states: [] }, implementations, 'Login', problems), {
    dev: { path: '/login' },
  });
});

function impls() {
  var problems = [];
  var out = manifest.implementations(
    {
      storybook: { kind: 'storybook', url: 'http://localhost:6006' },
      dev: { kind: 'url', base: 'http://localhost:3710' },
    },
    problems
  );
  assert.deepEqual(problems, []);
  return out;
}

var signIn = {
  label: 'Sign in',
  src: 'pages/sign-in.html',
  states: [{ id: 'default', label: 'Default' }, { id: 'error', label: 'Wrong password' }],
};

test('a local file merges sizes by key and keeps the committed order', function () {
  var merged = manifest.merge(
    { sizes: { fit: true, sidebar: { width: 340, height: 'fill', icon: 'panel-left' }, mobile: true } },
    { sizes: { sidebar: { width: 360 }, tablet: { width: 1024, height: 1366 } } }
  );
  assert.deepEqual(Object.keys(merged.sizes), ['fit', 'sidebar', 'mobile', 'tablet']);
  assert.deepEqual(merged.sizes.sidebar, { width: 360, height: 'fill', icon: 'panel-left' });
});

test('sizes belong to each space: a file’s top-level sizes are not shared with its spaces', function () {
  var raw = { sizes: { fit: true }, spaces: { web: { sizes: { mobile: true } }, docs: {} } };
  assert.deepEqual(manifest.selectSpace(raw, 'web', []).raw.sizes, { mobile: true });
  assert.equal(manifest.selectSpace(raw, 'docs', []).raw.sizes, undefined);
  var local = manifest.merge(raw, { spaces: { web: { sizes: { mobile: { label: 'Phone' } } } } });
  assert.deepEqual(manifest.selectSpace(local, 'web', []).raw.sizes, { mobile: { label: 'Phone' } });
});

test('reads a page’s lenses in both forms', function () {
  var problems = [];
  var lenses = manifest.pageLenses(
    { dev: '/', storybook: 'Components/Button' },
    signIn, impls(), 'Pages › Sign in', problems
  );
  assert.deepEqual(problems, []);
  assert.deepEqual(lenses, { dev: { path: '/' }, storybook: { title: 'Components/Button' } });

  var mapped = manifest.pageLenses(
    { dev: { default: '/', error: '/?error=1' } },
    signIn, impls(), 'Pages › Sign in', problems
  );
  assert.deepEqual(problems, []);
  assert.deepEqual(mapped, { dev: { path: '/', states: { error: '/?error=1' } } });
});

test('a state map may name the first state by its own id', function () {
  var problems = [];
  var item = { label: 'Library', src: 'library.html', states: [{ id: 'filled', label: 'Filled' }, { id: 'empty', label: 'Empty' }] };
  var lenses = manifest.pageLenses({ dev: { filled: '/library', empty: '/library?empty' } }, item, impls(), 'App › Library', problems);
  assert.deepEqual(problems, []);
  assert.deepEqual(lenses, { dev: { path: '/library', states: { empty: '/library?empty' } } });
});

test('names every problem with a page’s lenses', function () {
  var problems = [];
  var lenses = manifest.pageLenses(
    {
      prod: '/',
      dev: 'sign-in',
      storybook: '',
    },
    signIn, impls(), 'Pages › Sign in', problems
  );
  assert.equal(lenses, undefined);
  assert.deepEqual(problems, [
    'Pages › Sign in: implementation “prod” isn’t declared under implementations.',
    'Pages › Sign in: implementation “dev” paths must start with / — they’re appended to http://localhost:3710.',
    'Pages › Sign in: implementation “storybook” needs a story title.',
  ]);

  problems = [];
  lenses = manifest.pageLenses(
    { dev: { error: '/?error=1', locked: '/locked' } },
    signIn, impls(), 'Pages › Sign in', problems
  );
  assert.equal(lenses, undefined);
  assert.deepEqual(problems, [
    'Pages › Sign in: implementation “dev” maps state “locked”, which this page doesn’t declare.',
    'Pages › Sign in: implementation “dev” needs a path for the default state (“default”).',
  ]);

  problems = [];
  assert.equal(manifest.pageLenses({ dev: ['/a'] }, signIn, impls(), 'Pages › Sign in', problems), undefined);
  assert.deepEqual(problems, [
    'Pages › Sign in: implementation “dev” needs a path, or a map of this page’s state ids to paths.',
  ]);

  problems = [];
  assert.equal(manifest.pageLenses('dev', signIn, impls(), 'Pages › Sign in', problems), undefined);
  assert.deepEqual(problems, ['Pages › Sign in: implementations must be a map of implementation names to paths.']);
});

test('reads a page’s code pointers', function () {
  var problems = [];
  var code = manifest.pageCode(
    {
      dev: 'packages/auth/src/pages/login',
      storybook: ['src/button.tsx', 'src/button.stories.tsx'],
    },
    impls(), 'Pages › Sign in', problems
  );
  assert.deepEqual(problems, []);
  assert.deepEqual(code, [
    { implementation: 'dev', path: 'packages/auth/src/pages/login' },
    { implementation: 'storybook', path: 'src/button.tsx' },
    { implementation: 'storybook', path: 'src/button.stories.tsx' },
  ]);
});

test('names every problem with a page’s code', function () {
  var problems = [];
  var code = manifest.pageCode({ prod: 'x', dev: '' }, impls(), 'Pages › Sign in', problems);
  assert.equal(code, undefined);
  assert.deepEqual(problems, [
    'Pages › Sign in: code names implementation “prod”, which isn’t declared under implementations.',
    'Pages › Sign in: code for “dev” needs a path relative to that implementation’s root.',
  ]);

  problems = [];
  assert.equal(manifest.pageCode(['x'], impls(), 'Pages › Sign in', problems), undefined);
  assert.deepEqual(problems, ['Pages › Sign in: code must be a map of implementation names to paths.']);
});

test('derives a story’s state from its id', function () {
  assert.equal(manifest.storyState('components-button--icon-only'), 'icon-only');
  assert.equal(manifest.storyState('button--default'), 'default');
  assert.equal(manifest.storyState('odd'), 'odd');
});

test('a space mark takes a named or hex colour, and a Lucide icon or a project image', function () {
  var problems = [];
  assert.deepEqual(manifest.spaceMark({ color: 'Green', icon: 'rocket' }, problems), { color: 'green', icon: 'rocket', image: null });
  assert.deepEqual(manifest.spaceMark({ color: '#2F7D55', icon: 'assets/logo.svg' }, problems), { color: '#2f7d55', icon: null, image: 'assets/logo.svg' });
  assert.deepEqual(manifest.spaceMark({ color: '#abc' }, problems), { color: '#abc', icon: null, image: null });
  assert.deepEqual(manifest.spaceMark({}, problems), { color: null, icon: null, image: null });
  assert.deepEqual(problems, []);
});

test('an unusable space colour or icon is named and left out', function () {
  var problems = [];
  var mark = manifest.spaceMark({ color: 'chartreuse', icon: '../logo.svg' }, problems);
  assert.deepEqual(mark, { color: null, icon: null, image: null });
  assert.equal(problems.length, 2);
  assert.match(problems[0], /^color: must be one of blue/);
  assert.match(problems[1], /^icon: must be a kebab-case Lucide icon name/);

  ['/abs/logo.svg', 'https://example.com/logo.png', 'logo.pdf', 'Rocket Ship'].forEach(function (icon) {
    var found = [];
    assert.equal(manifest.spaceMark({ icon: icon }, found).image, null, icon);
    assert.equal(found.length, 1, icon);
  });
});

test('a file without spaces is one space', function () {
  var problems = [];
  var picked = manifest.selectSpace({ name: 'Acme', collections: [{ name: 'Pages' }] }, null, problems);
  assert.equal(picked.key, null);
  assert.equal(picked.root, null);
  assert.equal(picked.raw.name, 'Acme');
  assert.deepEqual(problems, []);
});

test('each space inherits the shared keys but not the name, colour, icon or root', function () {
  var raw = {
    name: 'Acme workspace',
    color: 'red',
    implementations: { storybook: { kind: 'storybook', url: 'http://localhost:6006' }, dev: { kind: 'url', base: 'http://localhost:3000' } },
    collections: [{ name: 'Shared' }],
    spaces: {
      web: { name: 'Acme Web', color: 'blue', implementations: { dev: { base: 'http://localhost:4000' } } },
      'design-system': { root: '../ui', icon: 'palette', collections: [{ name: 'Components' }] },
    },
  };
  var problems = [];
  var web = manifest.selectSpace(raw, 'web', problems);
  assert.equal(web.key, 'web');
  assert.equal(web.raw.name, 'Acme Web');
  assert.equal(web.raw.color, 'blue');
  assert.deepEqual(web.raw.collections, [{ name: 'Shared' }]);
  assert.deepEqual(web.raw.implementations.dev, { kind: 'url', base: 'http://localhost:4000' });
  assert.equal(web.raw.implementations.storybook.url, 'http://localhost:6006');
  assert.equal(web.raw.spaces, undefined);

  var ds = manifest.selectSpace(raw, 'design-system', problems);
  assert.equal(ds.root, '../ui');
  assert.equal(ds.raw.name, 'Design system');
  assert.equal(ds.raw.color, undefined);
  assert.equal(ds.raw.icon, 'palette');
  assert.deepEqual(ds.raw.collections, [{ name: 'Components' }]);
  assert.equal(ds.raw.root, undefined);
  assert.deepEqual(problems, []);
});

test('an unknown space falls back to the first, and malformed spaces are named', function () {
  var problems = [];
  var raw = { spaces: { web: { name: 'Web' }, 'Bad Key': { name: 'x' }, empty: 'nope' } };
  assert.deepEqual(manifest.spaceKeys(raw, problems), ['web']);
  assert.equal(problems.length, 2);

  var found = [];
  var picked = manifest.selectSpace(raw, 'gone', found);
  assert.equal(picked.key, 'web');
  assert.match(found.pop(), /there is no space “gone”/);

  var listed = [];
  assert.deepEqual(manifest.spaceKeys({ spaces: ['web'] }, listed), []);
  assert.match(listed[0], /must be a map of space ids/);
});

test('a local file merges into one space by key', function () {
  var merged = manifest.merge(
    { spaces: { web: { name: 'Web', implementations: { dev: { kind: 'url', base: 'http://localhost:3000' } } }, ui: { name: 'UI' } } },
    { spaces: { web: { implementations: { dev: { base: 'http://localhost:4000' } } } } }
  );
  assert.equal(merged.spaces.web.name, 'Web');
  assert.deepEqual(merged.spaces.web.implementations.dev, { kind: 'url', base: 'http://localhost:4000' });
  assert.deepEqual(merged.spaces.ui, { name: 'UI' });
});

test('reads examples implementations, which render a docs page’s examples', function () {
  var problems = [];
  var impls = manifest.implementations({
    web: { kind: 'examples', adapter: 'react', styles: ['src/theme.css', 'src/web.css'], environment: 'src/docs/environment.tsx' },
    native: { kind: 'examples', label: 'React Native Web', adapter: 'react-native-web', styles: 'src/native.css' },
    bare: { kind: 'examples' },
    away: { kind: 'examples', adapter: 'html', styles: ['../acme/theme.css', '/etc/theme.css'], environment: 'http://example.com/env.js',
      base: 'http://localhost:1', root: '../acme', start: { command: 'npm start' } },
  }, problems);
  assert.deepEqual(impls.web, {
    key: 'web', label: 'Web', kind: 'examples', root: '.', adapter: 'react',
    styles: ['src/theme.css', 'src/web.css'], environment: 'src/docs/environment.tsx',
  });
  assert.deepEqual(impls.native.styles, ['src/native.css']);
  assert.equal(impls.native.label, 'React Native Web');
  assert.deepEqual(Object.keys(impls), ['web', 'native']);
  assert.deepEqual(problems, [
    'implementations › bare: needs an adapter — html, react, vue, react-native-web, or one registered in workbench.config.ts.',
    'implementations › away: styles must be paths inside the project, relative to its root.',
    'implementations › away: styles must be paths inside the project, relative to its root.',
    'implementations › away: environment must be a path inside the project, relative to its root.',
    'implementations › away: base doesn’t apply to examples, which Workbench compiles itself.',
    'implementations › away: start is available for URL and Storybook implementations.',
    'implementations › away: Examples use this project root.',
  ]);
});

test('a docs page maps examples lenses to example sources and opens with its lens', function () {
  var problems = [];
  var impls = manifest.implementations({
    light: { kind: 'examples', adapter: 'html' },
    dark: { kind: 'examples', adapter: 'html' },
    dev: { kind: 'url', base: 'http://localhost:3000' },
  }, problems);
  var raw = { label: 'Card', src: 'docs/card.md', lens: 'dark',
    implementations: { light: 'src/card/examples/', dark: 'src/card/card.examples.ts', dev: '/card', away: '../x/' } };
  var item = { label: 'Card', src: raw.src };
  item.implementations = manifest.pageLenses(raw.implementations, item, impls, 'Docs › Card', problems);
  manifest.docsPageEntry(raw, item, 'Docs › Card', problems);
  assert.deepEqual(item.implementations, {
    light: { examples: 'src/card/examples/' },
    dark: { examples: 'src/card/card.examples.ts' },
  });
  assert.equal(item.docs, true);
  assert.equal(item.lens, 'dark');
  assert.deepEqual(problems, [
    'Docs › Card: docs pages take examples lenses; “dev” is a url implementation.',
    'Docs › Card: implementation “away” isn’t declared under implementations.',
  ]);

  problems = [];
  var unnamed = { label: 'Button', src: 'docs/button.md' };
  unnamed.implementations = manifest.pageLenses({ dark: '../outside/', light: 'src/button/' }, unnamed, impls, 'Docs › Button', problems);
  manifest.docsPageEntry({ lens: 'sepia', sizes: ['mobile'] }, unnamed, 'Docs › Button', problems);
  assert.equal(unnamed.lens, 'light', 'without a valid lens, the first mapped lens opens');
  assert.deepEqual(problems, [
    'Docs › Button: examples lens “dark” needs an example folder (ending in /) or file, relative to the project root.',
    'Docs › Button: sizes don’t apply to a docs page, which uses the whole canvas.',
    'Docs › Button: lens “sepia” isn’t one of this page’s lenses (light).',
  ]);

  problems = [];
  var guide = { label: 'Colors', src: 'docs/colors.MD' };
  manifest.docsPageEntry({}, guide, 'Docs › Colors', problems);
  assert.deepEqual(guide, { label: 'Colors', src: 'docs/colors.MD', docs: true }, 'a docs page needs no lenses');
  assert.deepEqual(problems, []);
});

test('examples lenses and lens belong to docs pages only', function () {
  var problems = [];
  var impls = manifest.implementations({ light: { kind: 'examples', adapter: 'html' } }, problems);
  var item = { label: 'Sign in', src: 'pages/sign-in.html' };
  assert.equal(manifest.pageLenses({ light: 'src/examples/' }, item, impls, 'Pages › Sign in', problems), undefined);
  manifest.docsPageEntry({ lens: 'light' }, item, 'Pages › Sign in', problems);
  assert.equal(item.docs, undefined);
  assert.deepEqual(problems, [
    'Pages › Sign in: implementation “light” renders examples, which only docs pages (a .md src) have.',
    'Pages › Sign in: lens is only for docs pages (a .md src).',
  ]);
});

test('a src may not contain the address marks', function () {
  assert.equal(manifest.srcProblem('docs/card.md'), null);
  assert.equal(manifest.srcProblem('docs/card!.md'),
    'src “docs/card!.md” can’t contain “!” — “:”, “!”, and “~” mark the state, the example, and the lens in the address.');
  assert.match(manifest.srcProblem('a:b~c.html'), /can’t contain “:” or “~”/);
  assert.equal(manifest.projectPath('src/card/'), 'src/card/');
  assert.equal(manifest.projectPath('src/../../etc'), null);
  assert.equal(manifest.projectPath('C:\\acme\\card.ts'), null);
});
