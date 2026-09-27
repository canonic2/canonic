var test = require('node:test');
var assert = require('node:assert');
var fs = require('fs');
var os = require('os');
var path = require('path');

var designExport = require('./export');

function names(zip) {
  var out = [];
  var at = 0;
  while (zip.readUInt32LE(at) === 0x04034b50) {
    var size = zip.readUInt32LE(at + 18);
    var nameLength = zip.readUInt16LE(at + 26);
    var extraLength = zip.readUInt16LE(at + 28);
    out.push(zip.slice(at + 30, at + 30 + nameLength).toString());
    at += 30 + nameLength + extraLength + size;
  }
  return out;
}

function bodies(zip) {
  var out = {};
  var at = 0;
  while (zip.readUInt32LE(at) === 0x04034b50) {
    var size = zip.readUInt32LE(at + 18);
    var nameLength = zip.readUInt16LE(at + 26);
    var extraLength = zip.readUInt16LE(at + 28);
    var name = zip.slice(at + 30, at + 30 + nameLength).toString();
    var bodyAt = at + 30 + nameLength + extraLength;
    out[name] = zip.slice(bodyAt, bodyAt + size).toString();
    at = bodyAt + size;
  }
  return out;
}

test('exports workbench entries and transitive UI dependencies without unrelated code', function () {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-export-test-'));
  try {
    fs.mkdirSync(path.join(root, 'src', 'button'), { recursive: true });
    fs.mkdirSync(path.join(root, 'src', 'shared'), { recursive: true });
    fs.mkdirSync(path.join(root, 'src', 'billing'), { recursive: true });
    fs.mkdirSync(path.join(root, 'pages'), { recursive: true });
    fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\n');
    fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify({ dependencies: { react: '^19.0.0', stripe: '^18.0.0' } }));
    fs.writeFileSync(path.join(root, 'src', 'button', 'button.stories.tsx'), "import './button';\n");
    fs.writeFileSync(path.join(root, 'src', 'button', 'button.tsx'), "import React from 'react'; import '../shared/tokens.css'; // import '../billing/charge';\n");
    fs.writeFileSync(path.join(root, 'src', 'shared', 'tokens.css'), '.button{background:url(./dot.svg)}\n');
    fs.writeFileSync(path.join(root, 'src', 'shared', 'dot.svg'), '<svg/>');
    fs.writeFileSync(path.join(root, 'src', 'button', 'button.test.tsx'), "import '../billing/charge';\n");
    fs.writeFileSync(path.join(root, 'src', 'billing', 'charge.ts'), "import Stripe from 'stripe';\n");
    fs.writeFileSync(path.join(root, 'pages', 'home.html'), '<link href="./home.css"><img src="../src/shared/dot.svg">');
    fs.writeFileSync(path.join(root, 'pages', 'home.css'), 'main { color: rebeccapurple }\n');
    var view = {
      name: 'Acme UI', implementations: { storybook: { root: root } },
      screens: { home: { label: 'Home', design: path.join(root, 'pages', 'home.html'), code: [] }, button: { label: 'Button', design: null, code: [
        { implementation: 'storybook', path: path.join(root, 'src', 'button', 'button.stories.tsx'), exists: true },
      ] } },
    };
    var result = designExport.create(root, view, { screenshots: [{
      screen: 'button', state: 'default', variant: 'default-desktop', label: 'Default', viewport: 'desktop',
      width: 1512, height: 982, body: Buffer.from('reference jpeg'),
    }] });
    var files = result.report.files;
    assert.ok(files.includes('src/button/button.stories.tsx'));
    assert.ok(files.includes('src/button/button.tsx'));
    assert.ok(files.includes('src/shared/tokens.css'));
    assert.ok(files.includes('src/shared/dot.svg'));
    assert.ok(files.includes('pages/home.html'));
    assert.ok(files.includes('pages/home.css'));
    assert.ok(!files.some(function (file) { return file.indexOf('project/') === 0; }));
    assert.ok(!files.some(function (file) { return file.indexOf('billing') > -1 || file.indexOf('.test.') > -1; }));
    assert.deepStrictEqual(result.report.dependencies, [{ name: 'react', version: '^19.0.0' }]);
    assert.ok(names(result.body).includes('acme-ui-design-system/canonic-export.json'));
    assert.ok(names(result.body).includes('acme-ui-design-system/pages/README.md'));
    assert.ok(names(result.body).includes('acme-ui-design-system/src/button/README.md'));
    assert.deepStrictEqual(result.report.screens.map(function (screen) { return screen.id; }), ['button', 'home']);
    var button = result.report.screens[0];
    var home = result.report.screens[1];
    assert.match(button.hash, /^sha256:[a-f0-9]{64}$/);
    assert.ok(button.files.includes('src/button/button.tsx'));
    assert.ok(button.files.includes('src/shared/dot.svg'));
    assert.ok(home.files.includes('pages/home.html'));
    assert.ok(!home.files.includes('src/button/button.tsx'));
    var archived = bodies(result.body);
    assert.strictEqual(names(result.body).filter(function (name) { return name === 'acme-ui-design-system/package.json'; }).length, 1);
    assert.match(archived['acme-ui-design-system/package.json'], /stripe/);
    assert.match(archived['acme-ui-design-system/src/button/README.md'], new RegExp(button.hash));
    assert.match(archived['acme-ui-design-system/src/button/README.md'], /Source \(storybook\)/);
    assert.match(archived['acme-ui-design-system/src/button/README.md'], /Default · Desktop/);
    assert.match(archived['acme-ui-design-system/src/button/README.md'], /screenshots\/default-desktop\.jpg/);
    assert.strictEqual(archived['acme-ui-design-system/src/button/screenshots/default-desktop.jpg'], 'reference jpeg');
    assert.deepStrictEqual(button.screenshots, [{
      state: 'default', label: 'Default', viewport: 'desktop',
      path: 'src/button/screenshots/default-desktop.jpg', width: 1512, height: 982,
    }]);

    fs.writeFileSync(path.join(root, 'src', 'billing', 'charge.ts'), 'unrelated change\n');
    var unrelated = designExport.create(root, view);
    assert.strictEqual(unrelated.report.screens[0].hash, button.hash);
    fs.writeFileSync(path.join(root, 'src', 'shared', 'dot.svg'), '<svg>changed</svg>');
    var changed = designExport.create(root, view);
    assert.notStrictEqual(changed.report.screens[0].hash, button.hash);
    assert.notStrictEqual(changed.report.screens[1].hash, home.hash);
    assert.strictEqual(result.body.readUInt32LE(0), 0x04034b50);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('keeps generated guides and screenshots distinct when pages share a folder', function () {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-export-pages-'));
  try {
    fs.mkdirSync(path.join(root, 'pages'));
    fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Pages\n');
    fs.writeFileSync(path.join(root, 'pages', 'sign-in.html'), '<main>Sign in</main>');
    fs.writeFileSync(path.join(root, 'pages', 'account.html'), '<main>Account</main>');
    var result = designExport.create(root, { name: 'Pages', implementations: {}, screens: {
      signin: { label: 'Sign in', design: path.join(root, 'pages', 'sign-in.html'), code: [] },
      account: { label: 'Account', design: path.join(root, 'pages', 'account.html'), code: [] },
    } }, { screenshots: [
      { screen: 'signin', variant: 'default-mobile', label: 'Default', viewport: 'mobile', width: 393, height: 852, body: Buffer.from('signin') },
      { screen: 'account', variant: 'default-mobile', label: 'Default', viewport: 'mobile', width: 393, height: 852, body: Buffer.from('account') },
    ] });
    var archived = names(result.body);
    assert.ok(archived.includes('pages-design-system/pages/sign-in.README.md'));
    assert.ok(archived.includes('pages-design-system/pages/account.README.md'));
    assert.ok(archived.includes('pages-design-system/pages/screenshots/sign-in/default-mobile.jpg'));
    assert.ok(archived.includes('pages-design-system/pages/screenshots/account/default-mobile.jpg'));
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('downloads one ZIP containing attachment-sized ZIP parts', function () {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-export-split-'));
  try {
    fs.mkdirSync(path.join(root, 'pages'));
    fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Split\n');
    fs.writeFileSync(path.join(root, 'pages', 'home.html'), '<main>Home</main>');
    var screenshots = Array.from({ length: 4 }, function (_, index) {
      return { screen: 'home', variant: 'state-' + index + '-desktop', label: 'State ' + index,
        viewport: 'desktop', width: 1512, height: 982, body: Buffer.alloc(25000, index) };
    });
    var result = designExport.create(root, { name: 'Split', implementations: {}, screens: {
      home: { label: 'Home', design: path.join(root, 'pages', 'home.html'), code: [] },
    } }, { screenshots: screenshots, maxArchiveBytes: 70000 });
    assert.equal(result.body, null);
    assert.ok(result.archives.length > 1);
    assert.equal(result.download.filename, 'split-design-system-parts.zip');
    assert.deepStrictEqual(names(result.download.body).filter(function (name) { return /\.zip$/.test(name); }),
      result.archives.map(function (archive) { return archive.filename; }));
    assert.equal(result.report.parts.length, result.archives.length);
    result.archives.forEach(function (archive, index) {
      assert.ok(archive.body.length <= 70000);
      assert.match(archive.filename, new RegExp('part-' + String(index + 1).padStart(2, '0') + '-of-'));
      var archived = names(archive.body);
      assert.ok(archived.includes('split-design-system/README.md'));
      assert.ok(archived.includes('split-design-system/canonic-export.json'));
      assert.ok(archived.some(function (name) { return /canonic-export-part-\d+\.json$/.test(name); }));
    });
    var payload = result.archives.flatMap(function (archive) { return archive.files; });
    screenshots.forEach(function (shot) {
      assert.ok(payload.includes('pages/screenshots/' + shot.variant + '.jpg'));
    });
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('extracts imports, assets, and package names', function () {
  assert.deepStrictEqual(
    designExport.references('x.ts', "import x from './x'; export { y } from '@scope/pkg/y'; require('plain/z')"),
    ['./x', '@scope/pkg/y', 'plain/z']
  );
  assert.strictEqual(designExport.packageName('@scope/pkg/x'), '@scope/pkg');
  assert.strictEqual(designExport.packageName('plain/x'), 'plain');
  assert.strictEqual(designExport.packageName('./local'), null);
  assert.strictEqual(designExport.packageName('virtual:svg-icons-register'), null);
});

test('resolves a local package theme imported from CSS through package exports', function () {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-export-theme-'));
  try {
    var frontend = path.join(root, 'packages', 'frontend-web');
    var ui = path.join(root, 'packages', 'ui');
    var globals = path.join(frontend, 'src', 'globals.css');
    var story = path.join(ui, 'src', 'button.stories.tsx');
    var theme = path.join(ui, 'src', 'styles', 'theme.css');
    var icon = path.join(ui, 'src', 'components', 'icons-next', 'assets', 'plus.svg');
    var iconComponent = path.join(ui, 'src', 'components', 'icons-next', 'icon-next.web.tsx');
    var progress = path.join(ui, 'src', 'components', 'progress-bar', 'progress-bar.web.tsx');
    var progressProps = path.join(ui, 'src', 'components', 'progress-bar', 'types', 'progress-bar-props.ts');
    fs.mkdirSync(path.dirname(globals), { recursive: true });
    fs.mkdirSync(path.dirname(story), { recursive: true });
    fs.mkdirSync(path.dirname(theme), { recursive: true });
    fs.mkdirSync(path.dirname(icon), { recursive: true });
    fs.mkdirSync(path.dirname(progressProps), { recursive: true });
    fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Theme\n');
    fs.writeFileSync(path.join(frontend, 'package.json'), JSON.stringify({ name: '@acme/frontend', dependencies: { '@acme/ui': '1.0.0' } }));
    fs.writeFileSync(path.join(ui, 'package.json'), JSON.stringify({ name: '@acme/ui', exports: { './theme.css': './src/styles/theme.css' } }));
    fs.writeFileSync(path.join(frontend, 'vite.config.ts'), 'import path from "node:path";\nexport default { plugins: [createSvgIconsPlugin({ iconDirs: [path.resolve(process.cwd(), "../ui/src/components/icons-next/assets")] })] };\n');
    fs.writeFileSync(globals, '@import "@acme/ui/theme.css";\n');
    fs.writeFileSync(story, 'import "./components/progress-bar/progress-bar.web.tsx";\nimport "./components/icons-next/icon-next.web.tsx";\nexport const Button = () => null;\n');
    fs.writeFileSync(theme, ':root { --color-primary: blue; }\n');
    fs.writeFileSync(icon, '<svg id="plus"/>\n');
    fs.writeFileSync(iconComponent, 'export const Icon = () => <use href="#icon-plus" />;\n');
    fs.writeFileSync(progress, 'import type { Props } from "./types/progress-bar-props.js";\n');
    fs.writeFileSync(progressProps, 'export type Props = { current: number };\n');
    var view = { name: 'Theme', implementations: { storybook: { root: ui } }, screens: {
      button: { label: 'Button', design: globals, code: [
        { implementation: 'storybook', path: story, exists: true },
      ] },
    } };
    var result = designExport.create(root, view);
    assert.ok(result.report.files.includes('packages/ui/src/styles/theme.css'));
    assert.ok(result.report.files.includes('packages/ui/src/components/icons-next/assets/plus.svg'));
    assert.ok(result.report.files.includes('packages/ui/src/components/progress-bar/types/progress-bar-props.ts'));
    assert.ok(result.report.screens[0].files.includes('packages/ui/src/styles/theme.css'));
    assert.ok(result.report.screens[0].files.includes('packages/ui/src/components/icons-next/assets/plus.svg'));
    assert.ok(result.report.screens[0].files.includes('packages/ui/src/components/progress-bar/types/progress-bar-props.ts'));
    assert.ok(!result.report.warnings.some(function (warning) { return warning.includes('@acme/ui/theme.css'); }));
    assert.ok(!result.report.warnings.some(function (warning) { return warning.includes('progress-bar-props.js'); }));
    var before = result.report.screens[0].hash;
    fs.writeFileSync(theme, ':root { --color-primary: red; }\n');
    assert.notStrictEqual(designExport.create(root, view).report.screens[0].hash, before);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('exports custom Storybook config and assets discovered through a linked package plugin', function () {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-export-storybook-'));
  try {
    var storybook = path.join(root, 'apps', 'storybook');
    var ui = path.join(root, 'libs', 'ui');
    var story = path.join(ui, 'src', 'button.stories.tsx');
    var icon = path.join(ui, 'src', 'icon.tsx');
    var assets = path.join(ui, 'src', 'icons', 'assets');
    fs.mkdirSync(path.join(storybook, 'web'), { recursive: true });
    fs.mkdirSync(path.join(ui, 'vite'), { recursive: true });
    fs.mkdirSync(assets, { recursive: true });
    fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Sprite\n');
    fs.writeFileSync(path.join(storybook, 'package.json'), JSON.stringify({
      name: '@acme/storybook',
      scripts: { storybook: 'storybook dev --config-dir web --port 6006' },
      dependencies: { '@acme/ui': 'link:../../libs/ui' },
    }));
    fs.writeFileSync(path.join(storybook, 'web', 'main.ts'), "import { iconSpriteOptions } from '@acme/ui/vite';\nexport default iconSpriteOptions();\n");
    fs.writeFileSync(path.join(storybook, 'web', 'preview.ts'), "import 'virtual:svg-icons-register';\n");
    fs.writeFileSync(path.join(ui, 'package.json'), JSON.stringify({
      name: '@acme/ui',
      exports: { './vite': { default: './vite/index.mjs' } },
    }));
    fs.writeFileSync(path.join(ui, 'vite', 'index.mjs'), "export const icons = new URL('../src/icons/assets/', import.meta.url);\n");
    fs.writeFileSync(story, "import { Icon } from './icon';\nexport const Play = () => Icon('play');\n");
    fs.writeFileSync(icon, "export const Icon = name => '<use href=#icon-' + name + ' />';\n");
    fs.writeFileSync(path.join(assets, 'play.svg'), '<svg id="play"/>');
    fs.writeFileSync(path.join(assets, 'skip.svg'), '<svg id="skip"/>');
    var view = {
      name: 'Sprite',
      implementations: { storybook: { kind: 'storybook', root: storybook } },
      screens: { button: { label: 'Button', design: null, code: [
        { implementation: 'storybook', path: story, exists: true },
      ] } },
    };

    var result = designExport.create(root, view);
    assert.ok(result.report.files.includes('apps/storybook/web/main.ts'));
    assert.ok(result.report.files.includes('apps/storybook/web/preview.ts'));
    assert.ok(result.report.files.includes('libs/ui/package.json'));
    assert.ok(result.report.files.includes('libs/ui/vite/index.mjs'));
    assert.ok(result.report.files.includes('libs/ui/src/icons/assets/play.svg'));
    assert.ok(result.report.files.includes('libs/ui/src/icons/assets/skip.svg'));
    assert.ok(result.report.screens[0].files.includes('libs/ui/src/icons/assets/play.svg'));
    assert.ok(!result.report.dependencies.some(function (dependency) {
      return dependency.name === '@acme/ui' || dependency.name === 'virtual:svg-icons-register';
    }));
    var before = result.report.screens[0].hash;
    fs.writeFileSync(path.join(assets, 'play.svg'), '<svg id="play-changed"/>');
    assert.notStrictEqual(designExport.create(root, view).report.screens[0].hash, before);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
