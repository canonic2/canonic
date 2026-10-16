import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { docsEntrySource, entrySlug, previewEntrySource } from './entries.ts';
import { htmlPart, runnableScript, transformHtml } from './html-loader.ts';
import { compilerOptions, projectExtensions } from './options.ts';

const root = path.resolve('/acme');
const page = path.join(root, 'pages/landing.html');

test('an HTML page becomes a module: local styles and scripts enter the build, external ones and data stay', () => {
  const html = '<!doctype html><html><head><title>Acme</title><meta charset="utf-8"><link rel="stylesheet" href="./landing.css">' +
    '<link rel="stylesheet" href="https://example.com/theme.css"><style>.hero { background: url(./hero.svg); }</style></head>' +
    '<body class="acme"><img src="./hero.svg" srcset="./hero.svg 1x, https://example.com/hero@2x.png 2x"><a href="/account">Account</a>' +
    '<script data-acme="kept">document.body.dataset.classic = "yes";</script><script type="application/json">{"plans": []}</script><script type="module">import { label } from "./label.ts"; document.body.dataset.label = label;</script>' +
    '<script type="module" src="./widget.ts"></script><script src="https://example.com/analytics.js"></script></body></html>';
  const module = transformHtml(html, page, root);
  assert.match(module, new RegExp('import ' + JSON.stringify(path.join(root, 'pages/landing.css')).replace(/[\\.]/g, '\\$&') + ';'));
  assert.ok(module.includes(JSON.stringify(page + '?workbench-style=0')), 'the inline style is imported as CSS');
  assert.ok(module.includes(JSON.stringify(page + '?workbench-script=0')), 'a classic inline script is a module of the build');
  assert.ok(module.includes(JSON.stringify(page + '?workbench-script=1')), 'an inline module script is a module of the build');
  assert.ok(module.includes('<script data-acme=\\"kept\\" data-workbench-script='), 'other attributes stay on the marker');
  assert.ok(module.includes('{\\"plans\\": []}'), 'JSON data stays in the page');
  assert.ok(module.includes(JSON.stringify(path.join(root, 'pages/widget.ts') + '?workbench-run')), 'a local script file is a module of the build');
  assert.ok(module.includes('https://example.com/theme.css'), 'external stylesheets stay linked');
  assert.ok(module.includes('https://example.com/analytics.js'), 'external scripts stay');
  assert.ok(module.includes('import url0 from ' + JSON.stringify(path.join(root, 'pages/hero.svg'))));
  assert.ok(module.includes('https://example.com/hero@2x.png 2x'));
  assert.ok(module.includes('href=\\"/account\\"'), 'links are navigation, not assets');
  assert.doesNotMatch(module, /<title>|<meta|<style/);
  assert.equal((module.match(/data-workbench-script=/g) || []).length, 3);
});

test('an inline style or script is served by query, and scripts run again on each render', () => {
  const html = '<style>a { color: red; }</style><style>b { color: blue; }</style>' +
    '<script type="module">import "./one.css";\nimport { two } from "./two.ts"; console.log(two);</script>' +
    '<script type="module">import "wbfile:/acme/.astro/hoisted.ts";</script>';
  assert.equal(htmlPart(html, '?workbench-style=1'), 'b { color: blue; }');
  const script = htmlPart(html, '?workbench-script=0');
  assert.match(script, /^import "\.\/one\.css";\nimport \{ two \} from "\.\/two\.ts";\nexport default async function workbenchScript\(\) \{\n console\.log\(two\);\n\}/);
  assert.throws(() => htmlPart(html, '?other'), /Unknown HTML part/);
  // Astro's hoisted client script is imported once, not run per render.
  const module = transformHtml(html, page, root);
  assert.ok(module.includes('import "/acme/.astro/hoisted.ts";'));
  assert.equal((module.match(/data-workbench-script=/g) || []).length, 1);
});

test('only leading imports stay outside the per-render function', () => {
  assert.equal(runnableScript('import "./a.css"; import { b } from "./b.ts"; run(b); import("./lazy.ts");'),
    'import "./a.css";\nimport { b } from "./b.ts";\nexport default async function workbenchScript() {\n run(b); import("./lazy.ts");\n}\n');
});

test('preview entries register for the live pipeline and export for portable output', () => {
  const input = {
    slug: entrySlug('pages/landing'), definition: '/acme/landing.workbench.ts', adapter: { file: '/workbench/react.js', native: false },
    sources: [{ state: 'default', kind: 'module' as const, file: '/acme/Landing.tsx', exportName: 'default' }],
    projectEnvironment: '/acme/environment.tsx', ownEnvironment: '/acme/landing-environment.tsx', styles: ['/acme/theme.css'],
  };
  const live = previewEntrySource(input);
  assert.match(live, /from "\/_workbench\/preview-runtime\.js"/);
  assert.match(live, /combine\(projectEnvironment, ownEnvironment\)/);
  assert.match(live, /window\.__wbRspack/);
  assert.match(live, /module\.hot\.accept\(\["\/acme\/Landing\.tsx"\]/);
  const portable = previewEntrySource({ ...input, portable: { runtime: '/workbench/browser.js', requests: '/workbench/requests.js', note: 'Static.' } });
  assert.ok(portable.startsWith('import "/workbench/requests.js";'), 'request mocks load before any project module');
  assert.match(portable, /export function mount/);
  assert.match(portable, /controls: \{\}, docs: .*"Static\."/);
  assert.doesNotMatch(portable, /__wbRspack|module\.hot|preview-runtime/);
  assert.notEqual(entrySlug('a/b'), entrySlug('a--b'));
});

test('docs entries list examples by ID for each source kind', () => {
  const base = { slug: 'docs-1', adapter: { file: '/workbench/react.js', native: true, nativeStyles: '/workbench/react-native-web.css' }, styles: [] };
  const folder = docsEntrySource({ ...base, examples: { kind: 'folder', examples: [{ id: 'primary', file: '/acme/primary.tsx' }] } });
  assert.match(folder, /import "\/workbench\/react-native-web\.css";/);
  assert.match(folder, /const adapter = \{ mount: mountNative \};/);
  assert.match(folder, /"primary": example0\.default/);
  const exports = docsEntrySource({ ...base, portable: { runtime: '/workbench/browser.js', requests: '/workbench/requests.js' },
    examples: { kind: 'exports', file: '/acme/card.examples.ts', examples: [{ id: 'with-icon', exportName: 'withIcon' }] } });
  assert.match(exports, /export const examples = \{"with-icon": source\["withIcon"\]\};/);
  const astro = docsEntrySource({ ...base, examples: { kind: 'astro', examples: [{ id: 'card', module: '/acme/.astro/card.js' }] } });
  assert.match(astro, /"card": \{ html: example0\.html, inputs: \{\}, renderUrl: null \}/);
});

test('project plugins must be Rspack plugins; an adapter’s apply only to its compiler', () => {
  const rspack = { apply() {} };
  const native = { apply() {} };
  const result = projectExtensions({
    plugins: [rspack, { name: 'acme-old', setup() {} }, 'nonsense'],
    rules: [{ test: /\.acme$/, type: 'asset/source' }],
    adapters: { 'react-native-web': { plugins: [native], rules: [{ test: /\.native$/ }] }, 'acme-canvas': { runtime: './canvas.ts', plugins: [{ apply() {} }] } },
  }, 'web');
  assert.equal(result.plugins.length, 2);
  assert.ok(result.plugins.includes(rspack) && !result.plugins.includes(native));
  assert.equal(result.rules.length, 1);
  assert.deepEqual(result.problems, [
    'workbench.config.ts: “acme-old” is an esbuild plugin. Workbench compiles previews with Rspack: use an Rspack plugin or a loader rule.',
    'workbench.config.ts: a plugin is not an Rspack plugin (an object with apply(compiler)).',
  ]);
  assert.deepEqual(projectExtensions({ adapters: { 'react-native-web': { plugins: [native] } } }, 'react-native-web').plugins, [native]);
});

test('compiler options keep the cache and output out of the project, and portable builds are self-contained modules', () => {
  const common = { profile: 'web' as const, generation: 2, root, config: {}, previewDir: '/workbench/preview', cacheDir: '/tmp/cache', entry: () => ({}), plugins: [],
    loaders: { vue: 'vue', style: 'style', css: 'css', html: 'html', htmlPart: 'part' } };
  const live = compilerOptions({ ...common, development: true }) as Record<string, any>;
  assert.equal(live.output.path, '/out/web/2');
  assert.equal(live.output.publicPath, '/_workbench/previews/rspack/web/2/');
  assert.equal(live.output.cssFilename, '[name].css', 'stylesheets keep stable names for CSS hot updates');
  assert.equal(live.cache.storage.directory, '/tmp/cache');
  assert.equal(live.optimization.runtimeChunk, 'single');
  const portable = compilerOptions({ ...common, development: false, portable: true }) as Record<string, any>;
  assert.equal(portable.output.library.type, 'module');
  assert.equal(portable.output.publicPath, 'auto');
  assert.equal(portable.optimization.splitChunks, false);
  assert.equal(portable.mode, 'production');
  const sideEffects = portable.module.rules.find((rule: any) => rule.sideEffects === true);
  assert.deepEqual(sideEffects.include, ['/workbench/preview/requests.js', '/workbench/preview/react-native-web.css'],
    'request mocks and native host styles survive the package’s sideEffects: false');
});
