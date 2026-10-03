var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var test = require('node:test');
var vm = require('node:vm');

var manifest = require('./manifest');

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
      sections: [{ name: 'Pages' }],
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
  assert.deepEqual(merged.sections, [{ name: 'Pages' }]);
  assert.deepEqual(merged.implementations.dev, { kind: 'url', base: 'http://localhost:4000', root: '../acme' });
  assert.deepEqual(merged.implementations.storybook, { kind: 'storybook', url: 'http://localhost:6006' });
  assert.equal(merged.implementations.staging.base, 'https://staging.example.com');
});

test('merge copes with a missing side', function () {
  assert.deepEqual(manifest.merge({ name: 'Acme' }, null), { name: 'Acme' });
  assert.deepEqual(manifest.merge(null, { name: 'Acme' }), { name: 'Acme' });
});

test('merges imported items into a manual section with the same name', function () {
  var manual = [{ group: 'Components', icon: 'star', items: [{ src: 'button.html' }] }];
  var imported = [
    { group: 'Components', icon: 'component', items: [{ src: '__storybook/card.html' }] },
    { group: 'Foundations', icon: 'component', items: [{ src: '__storybook/color.html' }] },
  ];
  assert.deepEqual(manifest.mergeSections(manual, imported), [
    { group: 'Components', icon: 'star', items: [{ src: 'button.html' }, { src: '__storybook/card.html' }] },
    { group: 'Foundations', icon: 'component', items: [{ src: '__storybook/color.html' }] },
  ]);
  assert.equal(manual[0].items.length, 1);
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
    'implementations › docs: kind must be url, storybook, ios-simulator, or window.',
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

test('accepts a window implementation by application, and screens map it to a window title', function () {
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
  var lenses = manifest.screenLenses(
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
  assert.deepEqual(manifest.screenLenses({ dev: '/login' }, { states: [] }, implementations, 'Login', problems), {
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

test('normalizes screen viewports and enables every mode when omitted', function () {
  var problems = [];
  assert.deepEqual(manifest.screenViewports(undefined, 'Pages › Sign in', problems), ['fit', 'desktop', 'mobile', 'responsive']);
  assert.deepEqual(manifest.screenViewports(['desktop', 'mobile', 'desktop'], 'Pages › Sign in', problems), ['desktop', 'mobile']);
  assert.deepEqual(manifest.screenViewports(['tablet'], 'Pages › Sign in', problems), ['fit', 'desktop', 'mobile', 'responsive']);
  assert.deepEqual(problems, [
    'Pages › Sign in: viewports may only contain desktop, mobile, responsive, or fit.',
  ]);
});

test('maps declared viewports to the workbench width controls', function () {
  assert.deepEqual(manifest.viewportWidths(['desktop', 'mobile', 'responsive']), ['1512', '393', 'resizable']);
  assert.deepEqual(manifest.viewportWidths(['fit']), ['fit']);
  assert.deepEqual(manifest.viewportWidths(), ['fit', '1512', '393', 'resizable']);
});

test('reads a screen’s lenses in both forms', function () {
  var problems = [];
  var lenses = manifest.screenLenses(
    { dev: '/', storybook: 'Components/Button' },
    signIn, impls(), 'Pages › Sign in', problems
  );
  assert.deepEqual(problems, []);
  assert.deepEqual(lenses, { dev: { path: '/' }, storybook: { title: 'Components/Button' } });

  var mapped = manifest.screenLenses(
    { dev: { default: '/', error: '/?error=1' } },
    signIn, impls(), 'Pages › Sign in', problems
  );
  assert.deepEqual(problems, []);
  assert.deepEqual(mapped, { dev: { path: '/', states: { error: '/?error=1' } } });
});

test('a state map may name the first state by its own id', function () {
  var problems = [];
  var item = { label: 'Library', src: 'library.html', states: [{ id: 'filled', label: 'Filled' }, { id: 'empty', label: 'Empty' }] };
  var lenses = manifest.screenLenses({ dev: { filled: '/library', empty: '/library?empty' } }, item, impls(), 'App › Library', problems);
  assert.deepEqual(problems, []);
  assert.deepEqual(lenses, { dev: { path: '/library', states: { empty: '/library?empty' } } });
});

test('names every problem with a screen’s lenses', function () {
  var problems = [];
  var lenses = manifest.screenLenses(
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
  lenses = manifest.screenLenses(
    { dev: { error: '/?error=1', locked: '/locked' } },
    signIn, impls(), 'Pages › Sign in', problems
  );
  assert.equal(lenses, undefined);
  assert.deepEqual(problems, [
    'Pages › Sign in: implementation “dev” maps state “locked”, which this screen doesn’t declare.',
    'Pages › Sign in: implementation “dev” needs a path for the default state (“default”).',
  ]);

  problems = [];
  assert.equal(manifest.screenLenses({ dev: ['/a'] }, signIn, impls(), 'Pages › Sign in', problems), undefined);
  assert.deepEqual(problems, [
    'Pages › Sign in: implementation “dev” needs a path, or a map of this screen’s state ids to paths.',
  ]);

  problems = [];
  assert.equal(manifest.screenLenses('dev', signIn, impls(), 'Pages › Sign in', problems), undefined);
  assert.deepEqual(problems, ['Pages › Sign in: implementations must be a map of implementation names to paths.']);
});

test('reads a screen’s code pointers', function () {
  var problems = [];
  var code = manifest.screenCode(
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

test('names every problem with a screen’s code', function () {
  var problems = [];
  var code = manifest.screenCode({ prod: 'x', dev: '' }, impls(), 'Pages › Sign in', problems);
  assert.equal(code, undefined);
  assert.deepEqual(problems, [
    'Pages › Sign in: code names implementation “prod”, which isn’t declared under implementations.',
    'Pages › Sign in: code for “dev” needs a path relative to that implementation’s root.',
  ]);

  problems = [];
  assert.equal(manifest.screenCode(['x'], impls(), 'Pages › Sign in', problems), undefined);
  assert.deepEqual(problems, ['Pages › Sign in: code must be a map of implementation names to paths.']);
});

test('derives a story’s state from its id', function () {
  assert.equal(manifest.storyState('components-button--icon-only'), 'icon-only');
  assert.equal(manifest.storyState('button--default'), 'default');
  assert.equal(manifest.storyState('odd'), 'odd');
});
