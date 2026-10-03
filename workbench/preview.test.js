const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { Compiler, discover } = require('./preview/compiler.cjs');
const server = require('./server');
const exporter = require('./export');
const manifest = require('./workbench/manifest');
const portable = require('./preview/portable.cjs');

function fixture(t, files) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-preview-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const [file, contents] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.writeFileSync(path.join(root, file), contents);
  }
  fs.symlinkSync(path.join(__dirname, 'node_modules'), path.join(root, 'node_modules'), 'junction');
  return root;
}
const definition = (adapter, entry, extra = '') => `import { definePreview } from '@canonic/workbench';
export default definePreview({ id: 'components/button', title: 'Components/Button', adapter: '${adapter}', source: { entry: '${entry}' },
inputs: { label: 'Continue' }, states: { default: {}, disabled: { inputs: { label: 'Disabled' } } }, ${extra} });`;

test('discovers TypeScript previews, validates IDs and states, and isolates invalid definitions', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas, ctx) { canvas.textContent = ctx.inputs.label; }',
    'broken.workbench.ts': 'export default { id: "INVALID" };',
    'dist/ignored.workbench.ts': definition('html', './button.ts'),
  });
  assert.equal(discover(root).length, 2);
  const index = await new Compiler(root).index();
  assert.equal(index.previews.length, 1);
  assert.deepEqual(index.previews[0].states.map(state => state.id), ['default', 'disabled']);
  assert.equal(index.errors.length, 1);
  assert.match(index.errors[0], /kebab-case/);
  await assert.rejects(new Compiler(root, { include: ['src/**'] }).compile('button.workbench.ts'), /declared/);
});

test('builds HTML scripts, inline modules, CSS, srcset and assets into portable output', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.html'),
    'button.html': '<!doctype html><html><head><style>button{background:url(./dot.svg)}</style></head><body><a href="/account">Account</a><button>Continue</button><img srcset="./dot.svg 1x, ./dot.svg 2x"><script type="module">import "./button.css"; import { label } from "./label.ts"; document.querySelector("button").textContent=label;</script></body></html>',
    'button.css': 'button { color: purple; }',
    'label.ts': 'export const label: string = "Bundled";',
    'dot.svg': '<svg xmlns="http://www.w3.org/2000/svg"/>',
  });
  const compiler = new Compiler(root);
  const output = await compiler.compile('button.workbench.ts', false);
  assert.ok(Array.from(output.outputs.keys()).some(name => name.endsWith('.css')));
  assert.ok(Array.from(output.outputs.keys()).some(name => name.endsWith('.svg')));
  assert.ok(output.localFiles.includes(path.join(root, 'label.ts')));
  const html = output.outputs.get('index.html').toString();
  assert.doesNotMatch(html, /<base|\/_workbench\/actions|\/_workbench\/keys/);
  assert.match(output.outputs.get('preview.js').toString(), /srcset/);
  assert.match(output.outputs.get('preview.js').toString(), /href=["']\/account/);
  assert.ok(output.localFiles.includes(path.join(root, 'button.css')));
  const css = Array.from(output.outputs.keys()).find(name => name.endsWith('.css') && output.outputs.get(name).includes(Buffer.from('purple')));
  assert.ok(css);
  assert.ok(output.outputs.get('preview.js').toString().includes(css));
  fs.writeFileSync(path.join(root, 'label.ts'), 'export const label = "Changed";');
  const changed = await compiler.compile('button.workbench.ts', false);
  assert.notEqual(changed.revision, output.revision);
});

test('bundles React and Vue SFCs with scoped styles using project framework versions', async t => {
  for (const [adapter, entry, source] of [
    ['react', './Button.tsx', 'export default function Button({label}:{label:string}) { return <button>{label}</button>; }'],
    ['vue', './Button.vue', '<script setup lang="ts">defineProps<{label:string}>()</script><template><button>{{label}}</button></template><style scoped>button{color:red}</style>'],
  ]) {
    const root = fixture(t, { 'button.workbench.ts': definition(adapter, entry), [entry.slice(2)]: source });
    const built = await new Compiler(root).compile('button.workbench.ts', false);
    assert.ok(built.outputs.get('preview.js').length > 1000);
    assert.ok(built.localFiles.includes(path.join(root, entry)));
    if (adapter === 'vue') assert.match(built.outputs.get('preview.css').toString(), /data-v-/);
  }
});

test('custom adapters and compiler plugins use the same browser export path', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('acme', './button.acme'),
    'button.acme': 'Custom adapter',
    'adapter.ts': 'export function mount(canvas, source, ctx) { canvas.textContent = source + ctx.inputs.label; }',
    'workbench.config.ts': `import fs from 'node:fs'; import { defineConfig } from '@canonic/workbench';
export default defineConfig({ adapters: { acme: { runtime: './adapter.ts', plugins: [{ name: 'acme', setup(build) { build.onLoad({filter:/\\.acme$/}, args => ({ contents: JSON.stringify(fs.readFileSync(args.path,'utf8')), loader:'json' })); } }] } } });`,
  });
  const built = await new Compiler(root).compile('button.workbench.ts', false);
  assert.match(built.outputs.get('preview.js').toString(), /Custom adapter/);
  assert.ok(built.localFiles.includes(path.join(root, 'workbench.config.ts')));
});

test('preview sources cannot escape the project or compile excluded definitions', async t => {
  const root = fixture(t, { 'button.workbench.ts': definition('html', '../outside.ts') });
  const compiler = new Compiler(root);
  assert.match((await compiler.index()).errors[0], /inside the project/);
  await assert.rejects(compiler.compile('../outside.workbench.ts'), /declared/);
});

test('server imports default previews, serves compiled pages, exports runnable output and closes its worker', async t => {
  const diagnostics = [];
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\n',
    'button.workbench.ts': definition('html', './button.ts', "viewports: ['mobile'], controls: { label: { type: 'text' } },"),
    'button.ts': 'export default function(canvas, ctx) { canvas.textContent = ctx.inputs.label; }',
  });
  const running = await server.start({ root, onLog: entry => diagnostics.push(entry), capture: { capture: async () => Buffer.from('jpeg'), close: () => Promise.resolve() } });
  t.after(() => running.close());
  const base = `http://127.0.0.1:${running.port}`;
  const config = await (await fetch(base + '/_workbench/config')).json();
  assert.deepEqual(config.problems, []);
  const preview = config.catalogSections[0].items[0];
  assert.equal(preview.src, 'button.workbench.ts');
  assert.equal(preview.workbench, true);
  const page = await fetch(base + '/button.workbench.ts?state=disabled');
  const html = await page.text();
  assert.match(page.headers.get('content-type'), /text\/html/);
  assert.match(html, /<base href="\/_workbench\/previews/);
  assert.match(html, /<head><script src="\/_workbench\/preview-compat\.js"><\/script>/);
  assert.equal((html.match(/preview-compat\.js/g) || []).length, 1);
  assert.doesNotMatch(html, /<script src="\/_workbench\/(keys|actions|states)\.js"/);
  const compatibility = await fetch(base + '/_workbench/preview-compat.js');
  assert.equal(compatibility.status, 200);
  assert.match(compatibility.headers.get('content-type'), /javascript/);
  assert.equal(await compatibility.text(), require('./preview-scripts').source);
  const assetBase = /<base href="([^"]+)"/.exec(html)[1];
  const asset = await fetch(base + assetBase + 'preview.js');
  assert.equal(asset.status, 200);
  const etag = asset.headers.get('etag');
  assert.ok(etag);
  assert.equal((await fetch(base + assetBase + 'preview.js', { headers: { 'If-None-Match': etag } })).status, 304);
  fs.writeFileSync(path.join(root, 'button.ts'), 'export default function(canvas) { canvas.textContent = "Changed"; }');
  const changedAsset = await fetch(base + assetBase + 'preview.js', { headers: { 'If-None-Match': etag } });
  assert.equal(changedAsset.status, 200);
  assert.notEqual(changedAsset.headers.get('etag'), etag);
  const timing = diagnostics.find(entry => entry.event === 'preview.request.completed' && entry.details.route === '/page');
  assert.ok(timing);
  assert.equal(timing.details.file, 'button.workbench.ts');
  assert.ok(timing.details.queueMs >= 0);
  assert.ok(timing.details.workMs >= 0);
  assert.ok(timing.details.elapsedMs >= timing.details.queueMs);
  const bundle = await fetch(base + '/_workbench/export');
  assert.equal(bundle.status, 200);
  assert.ok((await bundle.arrayBuffer()).byteLength > 1000);
  fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\npreviews: false\n');
  const disabled = await (await fetch(base + '/_workbench/config')).json();
  assert.equal(disabled.catalogSections.length, 0);
  assert.equal((await fetch(base + '/button.workbench.ts')).status, 404);
  assert.equal((await fetch(base + assetBase + 'preview.js')).status, 404);
});

test('untrusted and disabled workspaces never execute preview definitions', async t => {
  for (const disabled of [true, false]) {
    const root = fixture(t, {
      'workbench.yaml': 'name: Acme\n' + (disabled ? 'previews: false\n' : ''),
      'button.workbench.ts': 'import fs from "node:fs"; fs.writeFileSync(__dirname + "/executed", "yes"); export default {};',
    });
    const running = await server.start({ root, isTrusted: disabled, capture: { close: () => Promise.resolve() } });
    try {
      const config = await running.config();
      assert.equal(fs.existsSync(path.join(root, 'executed')), false);
      assert.equal(config.catalogSections.length, 0);
    } finally { await running.close(); }
  }
});

test('manual preview placement receives definition states and stays unique', () => {
  const original = [{ group: 'Authored', items: [{ label: 'My button', src: 'button.workbench.ts', implementations: { storybook: { title: 'Button' } } }] }];
  const imported = [{ group: 'Components', items: [{ label: 'Button', src: 'button.workbench.ts', workbench: true, states: [{ id: 'default', label: 'Default' }] }] }];
  const merged = manifest.mergeSections(original, imported);
  assert.equal(merged.length, 1);
  assert.equal(merged[0].items[0].label, 'My button');
  assert.equal(merged[0].items[0].workbench, true);
  assert.ok(merged[0].items[0].implementations.storybook);
  assert.equal(original[0].items[0].workbench, undefined);
});

test('a Workbench implementation maps a design to managed preview states', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\nimplementations:\n  built:\n    kind: workbench\nsections:\n  - name: Pages\n    items:\n      - label: Design\n        src: design.html\n        implementations:\n          built: components/button\n',
    'design.html': '<button>Design</button>',
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas, ctx) { canvas.textContent = ctx.inputs.label; }',
  });
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  try {
    const config = await running.config();
    assert.deepEqual(config.problems, []);
    assert.equal(config.sections[0].items[0].implementations.built.states.disabled, '/button.workbench.ts?state=disabled');
    assert.equal(config.screens['design.html'].code[0].relative, 'button.ts');
    const plan = server.exportCapturePlan(config, `http://127.0.0.1:${running.port}/`);
    assert.ok(plan.captures.some(capture => capture.state === 'default' && capture.url.includes('state=default')));
  } finally { await running.close(); }
});

test('portable compiler source closure is hashed and browser files are included in the archive', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\n',
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'import { label } from "./fixture.ts"; export default function(canvas) { canvas.textContent = label; }',
    'fixture.ts': 'export const label = "Hello";',
  });
  const compiler = new Compiler(root);
  const built = await compiler.compile('button.workbench.ts', false);
  const view = { name: 'Acme', screens: { 'button.workbench.ts': { label: 'Button', design: path.join(root, 'button.workbench.ts'), code: [] } }, implementations: {} };
  const portable = { previews: [{ id: built.id, file: built.file, files: built.localFiles, packages: built.packages, directory: 'browser/' + built.slug, states: [{ id: 'default' }] }],
    files: Array.from(built.outputs, ([name, body]) => ({ path: 'browser/' + built.slug + '/' + name, data: Buffer.from(body).toString('base64') })), warnings: [] };
  const output = exporter.create(root, view, { portable });
  assert.ok(output.report.screens[0].files.includes('fixture.ts'));
  assert.ok(!output.report.dependencies.some(pkg => pkg.name === '@canonic/workbench'));
  assert.equal(output.report.browser.previews[0].source, 'button.workbench.ts');
  assert.ok(output.body.includes(Buffer.from('browser/' + built.slug + '/index.html')));
  const first = output.report.screens[0].hash;
  fs.writeFileSync(path.join(root, 'fixture.ts'), 'export const label = "Changed";');
  assert.notEqual(exporter.create(root, view, { portable }).report.screens[0].hash, first);
});

test('authoring declarations infer input types and reject invalid state inputs', t => {
  const cp = require('node:child_process');
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas: HTMLElement) { canvas.textContent = "Hello"; }',
    'tsconfig.json': JSON.stringify({ compilerOptions: { strict: true, noEmit: true, target: 'ES2022', module: 'ESNext', moduleResolution: 'Bundler', types: [] }, include: ['*.ts'] }),
  });
  cp.execFileSync(process.execPath, [path.join(__dirname, 'preview', 'cli.cjs'), 'init', root]);
  const tsc = path.join(path.dirname(require.resolve('typescript')), '..', 'bin', 'tsc');
  const check = () => cp.spawnSync(process.execPath, [tsc, '-p', root], { encoding: 'utf8' });
  const valid = check();
  assert.equal(valid.status, 0, valid.stdout + valid.stderr);
  fs.writeFileSync(path.join(root, 'button.workbench.ts'), definition('html', './button.ts').replace("label: 'Disabled'", 'label: 123'));
  assert.notEqual(check().status, 0);
});

test('portable build includes the interactive viewer and shared controls while retaining successful previews', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas, context) { canvas.textContent = context.inputs.label; }',
    'broken.workbench.ts': "export default { id: 'broken', adapter: 'unknown', source: { entry: './button.ts' } }",
  });
  const result = await portable.create(new Compiler(root), { name: 'Acme' });
  assert.equal(result.previews.length, 1);
  assert.equal(result.warnings.length, 1);
  const contents = name => Buffer.from(result.files.find(file => file.path === 'browser/' + name).data, 'base64');
  const catalog = JSON.parse(contents('workbench.json'));
  assert.equal(catalog.name, 'Acme');
  assert.deepEqual(catalog.previews[0].states.map(state => state.id), ['default', 'disabled']);
  assert.equal(catalog.previews[0].files, undefined);
  assert.ok(result.files.some(file => file.path === result.previews[0].directory + '/index.html'));
  assert.deepEqual(contents('preview-controls.js'), fs.readFileSync(path.join(__dirname, 'workbench/preview-controls.js')));
  assert.deepEqual(contents('preview-controls.css'), fs.readFileSync(path.join(__dirname, 'workbench/preview-controls.css')));
});

test('CLI builds a static viewer without starting a server and refuses to overwrite existing content', t => {
  const cp = require('node:child_process');
  const root = fixture(t, { 'workbench.yaml': 'name: Acme\n', 'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas) { canvas.textContent = "Hello"; }' });
  const output = path.join(root, 'output');
  const args = [path.join(__dirname, 'preview/cli.cjs'), 'build', root, output];
  const result = cp.spawnSync(process.execPath, args, { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(JSON.parse(fs.readFileSync(path.join(output, 'workbench.json'))).name, 'Acme');
  assert.ok(fs.existsSync(path.join(output, 'index.html')));
  fs.writeFileSync(path.join(output, 'preserve.txt'), 'User content');
  const repeat = cp.spawnSync(process.execPath, args, { encoding: 'utf8' });
  assert.notEqual(repeat.status, 0);
  assert.match(repeat.stderr, /must be empty/);
  assert.equal(fs.readFileSync(path.join(output, 'preserve.txt'), 'utf8'), 'User content');
});
