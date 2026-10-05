const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const { Compiler, docsRequest } = require('./preview/compiler.cjs');

function fixture(t, files) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-docs-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const [file, contents] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.writeFileSync(path.join(root, file), contents);
  }
  fs.symlinkSync(path.join(__dirname, 'node_modules'), path.join(root, 'node_modules'), 'junction');
  return root;
}

/* The bundle is an ES module for the browser; HTML examples only touch the
   DOM when mounted, so Node can import it and read what it exports. */
async function load(t, compiled) {
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-docs-bundle-')), 'examples.mjs');
  t.after(() => fs.rmSync(path.dirname(file), { recursive: true, force: true }));
  fs.writeFileSync(file, compiled.outputs.get('examples.js'));
  return import(pathToFileURL(file).href);
}

const canvas = () => ({ textContent: '' });

test('a folder lens bundles one example per file, by file name, with its styles', async t => {
  const root = fixture(t, {
    'docs/button/primary.ts': 'export default (canvas) => { canvas.textContent = "Primary"; };',
    'docs/button/with-icon.ts': 'import { label } from "../label.ts"; export default (canvas) => { canvas.textContent = label; };',
    'docs/button/WithShadow.ts': 'export default () => {};',
    'docs/button/notes.md': '# Not an example',
    'docs/label.ts': 'export const label: string = "With icon";',
    'theme.css': '.acme-button { color: purple; }',
  });
  const compiled = await new Compiler(root).compileDocs({ adapter: 'html', examples: 'docs/button/', styles: ['theme.css'] });
  assert.deepEqual(compiled.examples, [
    { id: 'primary', file: 'docs/button/primary.ts', export: 'default' },
    { id: 'with-icon', file: 'docs/button/with-icon.ts', export: 'default' },
  ]);
  assert.deepEqual(compiled.problems, ['WithShadow.ts: example file names must be kebab-case.']);
  assert.equal(compiled.stylesheet, true);
  assert.match(compiled.outputs.get('examples.css').toString(), /purple/);
  const bundle = await load(t, compiled);
  assert.deepEqual(Object.keys(bundle.examples), ['primary', 'with-icon']);
  const target = canvas();
  bundle.examples['with-icon'](target);
  assert.equal(target.textContent, 'With icon');
  assert.equal(typeof bundle.adapter.mount, 'function');
  assert.deepEqual(bundle.environment, {});
});

test('a file lens bundles each named export as a kebab-case example and skips the default', async t => {
  const root = fixture(t, {
    'card.examples.ts': [
      'export const basic = (canvas) => { canvas.textContent = "Basic"; };',
      'export function withCustomStyle(canvas) { canvas.textContent = "Custom"; }',
      'export const with_custom_style = () => {};',
      'export default () => {};',
    ].join('\n'),
  });
  const compiled = await new Compiler(root).compileDocs({ adapter: 'html', examples: 'card.examples.ts' });
  assert.deepEqual(compiled.examples.map(example => [example.id, example.export]), [['basic', 'basic'], ['with-custom-style', 'withCustomStyle']]);
  assert.deepEqual(compiled.problems, ['with_custom_style: gives the same example ID as withCustomStyle (“with-custom-style”).']);
  const bundle = await load(t, compiled);
  const target = canvas();
  bundle.examples['with-custom-style'](target);
  assert.equal(target.textContent, 'Custom');
});

test('the project environment and the lens environment wrap the examples together', async t => {
  const root = fixture(t, {
    'workbench.config.ts': 'export default { environment: "project-environment.ts" };',
    'project-environment.ts': 'export const setup = () => { globalThis.acmeOrder.push("project"); };',
    'lens-environment.ts': 'export const setup = () => { globalThis.acmeOrder.push("lens"); };',
    'examples/basic.ts': 'export default () => {};',
  });
  const compiled = await new Compiler(root).compileDocs({ adapter: 'html', examples: 'examples/', environment: 'lens-environment.ts' });
  const bundle = await load(t, compiled);
  globalThis.acmeOrder = [];
  t.after(() => { delete globalThis.acmeOrder; });
  await bundle.environment.setup({});
  assert.deepEqual(globalThis.acmeOrder, ['project', 'lens']);
});

test('adding an example to a folder rebuilds the bundle', async t => {
  const root = fixture(t, { 'examples/basic.ts': 'export default () => {};' });
  const compiler = new Compiler(root);
  const request = { adapter: 'html', examples: 'examples/' };
  const first = await compiler.compileDocs(request);
  assert.equal(await compiler.compileDocs(request), first, 'unchanged sources reuse the build');
  // Directory times have coarse resolution on some file systems.
  await new Promise(resolve => setTimeout(resolve, 20));
  fs.writeFileSync(path.join(root, 'examples/large.ts'), 'export default () => {};');
  const second = await compiler.compileDocs(request);
  assert.deepEqual(second.examples.map(example => example.id), ['basic', 'large']);
  assert.notEqual(second.revision, first.revision);
});

test('a lens’s listing is kept until a file it read changes', async t => {
  const root = fixture(t, { 'card.examples.ts': 'export * from "./more.ts";\nexport const basic = () => {};\n', 'more.ts': 'export const large = () => {};\n' });
  const compiler = new Compiler(root);
  const request = docsRequest({ adapter: 'html', examples: 'card.examples.ts' }, root);
  const first = await compiler.docsExamples(request);
  assert.deepEqual(first.examples.map(example => example.id).sort(), ['basic', 'large']);
  assert.equal(await compiler.docsExamples(request), first, 'unchanged sources reuse the listing');
  await new Promise(resolve => setTimeout(resolve, 20));
  fs.writeFileSync(path.join(root, 'more.ts'), 'export const large = () => {};\nexport const small = () => {};\n');
  assert.deepEqual((await compiler.docsExamples(request)).examples.map(example => example.id).sort(), ['basic', 'large', 'small'], 'a re-exported file is watched too');
});

test('docs requests stay inside the project and name what is wrong', async t => {
  const root = fixture(t, { 'examples/basic.ts': 'export default () => {};', 'card.astro': '---\n---\n<p>Card</p>' });
  const compiler = new Compiler(root);
  await assert.rejects(compiler.compileDocs({ adapter: 'html', examples: '../outside/' }), /inside the project/);
  await assert.rejects(compiler.compileDocs({ adapter: 'html', examples: 'missing/' }), /doesn’t exist: missing\//);
  await assert.rejects(compiler.compileDocs({ adapter: 'html', examples: 'examples/', styles: ['gone.css'] }), /A style doesn’t exist: gone.css/);
  await assert.rejects(compiler.compileDocs({ adapter: 'Acme', examples: 'examples/' }), /kebab-case adapter/);
  await assert.rejects(compiler.compileDocs({ adapter: 'acme', examples: 'examples/' }), /Unknown adapter “acme”/);
  await assert.rejects(compiler.compileDocs({ adapter: 'astro', examples: 'card.astro' }), /point the lens at a folder of \.astro files/);
});

const server = require('./server');

const DOCS_YAML = [
  'name: Acme',
  'implementations:',
  '  web:',
  '    kind: examples',
  '    adapter: html',
  '    styles:',
  '      - theme.css',
  '  native:',
  '    kind: examples',
  '    adapter: html',
  'collections:',
  '  - name: Design system',
  '    items:',
  '      - label: Button',
  '        src: docs/button.md',
  '        implementations:',
  '          web: docs/button/',
  '          native: docs/button.examples.ts',
].join('\n');

function docsProject(t) {
  return fixture(t, {
    'workbench.yaml': DOCS_YAML,
    'theme.css': '.acme-button { color: purple; }',
    'docs/button.md': ['# Button', '', 'See [Card](card.md) and the [notes](notes.md).', '', '## Primary', '```example primary', 'caption: label', '```', '', '## Secondary', '```example secondary', '```'].join('\n'),
    'docs/notes.md': '# Not a docs page',
    'docs/button/primary.ts': 'export default (canvas: HTMLElement) => { canvas.textContent = "Primary"; };',
    'docs/button/secondary.ts': 'export default (canvas: HTMLElement) => { canvas.textContent = "Secondary"; };',
    'docs/button/ghost.ts': 'export default () => {};',
    'docs/button.examples.ts': 'export const primary = (canvas: HTMLElement) => { canvas.textContent = "Native primary"; };\n',
  });
}

test('the server serves a declared docs page at its Markdown path, in the lens asked for', async t => {
  const root = docsProject(t);
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  t.after(() => running.close());
  const base = `http://127.0.0.1:${running.port}`;

  const page = await fetch(base + '/docs/button.md');
  assert.equal(page.status, 200);
  assert.match(page.headers.get('content-type'), /text\/html/);
  const html = await page.text();
  assert.match(html, /<link rel="stylesheet" href="\/_workbench\/src\/docs\/page\/docs-page.css">/);
  // The Markdown answers at once; its panels wait for the page's script to ask for the bundle.
  assert.match(html, /data-wb-example="primary" data-status="pending"/);
  assert.match(html, /data-wb-example="secondary" data-status="pending"/);
  assert.match(html, /<p class="wb-docs-caption">label<\/p>/);
  assert.match(html, /<a href="\/docs\/notes.md">notes<\/a>/, 'a Markdown file that isn’t a docs page is a plain link');
  assert.match(html, /preview-compat\.js|wbPreviewActions|actions/);
  const options = JSON.parse(/window\.__workbenchDocs=(.*?)<\/script>/.exec(html)[1]);
  assert.equal(options.lens, 'web');
  assert.equal(options.lensLabel, 'Web');
  assert.equal(options.revision, '', 'a deferred page learns its revision with its bundle');

  const info = await (await fetch(base + options.bundle.info)).json();
  assert.deepEqual(info.examples.sort(), ['ghost', 'primary', 'secondary']);
  assert.equal(info.error, null);
  const bundle = await fetch(base + info.module);
  assert.equal(bundle.status, 200);
  assert.match(await bundle.text(), /Secondary/);
  assert.ok(info.stylesheet, 'the lens’s styles come with the bundle');
  assert.match(await (await fetch(base + info.stylesheet)).text(), /purple/);

  const native = await (await fetch(base + '/docs/button.md?lens=native&state=loading')).text();
  const nativeOptions = JSON.parse(/window\.__workbenchDocs=(.*?)<\/script>/.exec(native)[1]);
  assert.deepEqual((await (await fetch(base + nativeOptions.bundle.info)).json()).examples, ['primary'], 'Native lacks secondary, so its panel says so');

  const code = await (await fetch(base + '/_workbench/docs/source?page=docs%2Fbutton.md&lens=native&example=primary')).json();
  assert.equal(code.text, 'export const primary = (canvas: HTMLElement) => { canvas.textContent = "Native primary"; };');
  assert.equal(code.file, 'docs/button.examples.ts');
  const missing = await fetch(base + '/_workbench/docs/source?page=docs%2Fbutton.md&lens=native&example=secondary');
  assert.equal(missing.status, 404);

  const revision = (await (await fetch(base + '/_workbench/docs/revision?page=docs%2Fbutton.md&lens=web')).json()).revision;
  assert.equal(revision, info.revision);
  fs.appendFileSync(path.join(root, 'docs/button.md'), '\nMore notes.\n');
  assert.notEqual((await (await fetch(base + '/_workbench/docs/revision?page=docs%2Fbutton.md&lens=web')).json()).revision, revision);

  const raw = await fetch(base + '/docs/notes.md');
  assert.match(raw.headers.get('content-type'), /text\/markdown/);
  assert.equal(await raw.text(), '# Not a docs page');
});

test('docs problems join the config’s problems once listed, and the host hears when they do', async t => {
  const root = docsProject(t);
  let changed;
  const noticed = new Promise(resolve => { changed = resolve; });
  const running = await server.start({ root, capture: { close: () => Promise.resolve() }, onCatalogChanged: () => changed() });
  t.after(() => running.close());
  // The first config doesn't wait for the examples to be listed.
  await running.config();
  await noticed;
  assert.deepEqual((await running.config()).problems, ['Button (docs/button.md): example “ghost” in Web is not placed in docs/button.md.']);
});

test('the page script and stylesheet are served from src/ with types stripped', async t => {
  const root = docsProject(t);
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  t.after(() => running.close());
  const base = `http://127.0.0.1:${running.port}`;
  const script = await fetch(base + '/_workbench/src/docs/page/docs-page.ts');
  assert.equal(script.status, 200);
  assert.match(script.headers.get('content-type'), /text\/javascript/);
  const body = await script.text();
  assert.match(body, /import \{ mountExamples \} from "\.\/mount-examples\.ts"/);
  assert.doesNotMatch(body, /interface ExampleSource|: string\)/);
  assert.equal((await fetch(base + '/_workbench/src/docs/docs-service.ts')).status, 404);
  assert.equal((await fetch(base + '/_workbench/src/docs/page/docs-page.css')).status, 200);
});

test('without trust, a docs page renders its Markdown and says why its examples are missing', async t => {
  const root = docsProject(t);
  const running = await server.start({ root, isTrusted: false, capture: { close: () => Promise.resolve() } });
  t.after(() => running.close());
  // A fresh connection: earlier tests' servers held this port, and fetch keeps sockets alive.
  const html = await new Promise((resolve, reject) => require('node:http').get(`http://127.0.0.1:${running.port}/docs/button.md`, { agent: false }, answer => {
    let body = ''; answer.setEncoding('utf8'); answer.on('data', chunk => { body += chunk; }); answer.on('end', () => resolve(body));
  }).on('error', reject));
  assert.match(html, /<h2 id="primary">Primary<\/h2>/);
  assert.match(html, /data-status="unavailable"><p class="wb-docs-example-note">Examples run project code, so they appear in a trusted workspace only.<\/p>/);
  assert.deepEqual((await running.config()).problems, ['Docs page examples: Examples run project code, so they appear in a trusted workspace only.']);
});

test('defineDocs definitions are discovered, placed by title, and served like declared docs pages', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\n',
    'docs/colors.workbench.ts': [
      "import { defineDocs } from '@canonic2/workbench';",
      'export default defineDocs({',
      "  id: 'foundations/colors', title: 'Design system/Foundations/Colors', docs: './colors.md',",
      "  lenses: { web: { adapter: 'html', examples: './colors.examples.ts' }, 'react-native': { label: 'React Native Web', adapter: 'html', examples: './colors.examples.ts' } },",
      "  lens: 'react-native',",
      '});',
    ].join('\n'),
    'docs/broken.workbench.ts': "import { defineDocs } from '@canonic2/workbench';\nexport default defineDocs({ id: 'broken', docs: './missing.md' });",
    'docs/type.workbench.ts': "import { defineDocs } from '@canonic2/workbench';\nexport default defineDocs({ id: 'type', title: 'Design system/Type', icon: 'type', docs: './colors.md' });",
    'docs/odd.workbench.ts': "import { defineDocs } from '@canonic2/workbench';\nexport default defineDocs({ id: 'odd', icon: 'Odd Icon', docs: './colors.md' });",
    'docs/colors.md': '# Colors\n\n```example swatch\n```\n',
    'docs/colors.examples.ts': 'export const swatch = (canvas: HTMLElement) => { canvas.textContent = "Swatch"; };\n',
  });
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  t.after(() => running.close());
  const config = await running.config();
  assert.deepEqual(config.problems.sort(), ['docs/broken.workbench.ts: docs must exist inside the project: ./missing.md',
    'docs/odd.workbench.ts: icon must be a kebab-case Lucide icon name.']);
  const collection = config.catalogCollections.find(candidate => candidate.name === 'Design system');
  assert.equal(collection.items.find(item => item.label === 'Type').icon, 'type', 'a definition can name its icon');
  assert.deepEqual(collection.items.filter(item => item.group), [{ group: 'Foundations', items: [{
    src: 'docs/colors.md', label: 'Colors', docs: true, icon: 'book-open', lens: 'react-native',
    docsLenses: [{ key: 'web', label: 'Web' }, { key: 'react-native', label: 'React Native Web' }],
    implementations: { web: { examples: 'docs/colors.examples.ts' }, 'react-native': { examples: 'docs/colors.examples.ts' } },
  }] }]);
  const html = await new Promise((resolve, reject) => require('node:http').get(`http://127.0.0.1:${running.port}/docs/colors.md`, { agent: false }, answer => {
    let body = ''; answer.setEncoding('utf8'); answer.on('data', chunk => { body += chunk; }); answer.on('end', () => resolve(body));
  }).on('error', reject));
  assert.match(html, /data-wb-example="swatch" data-status="pending"/);
  const options = JSON.parse(/window\.__workbenchDocs=(.*?)<\/script>/.exec(html)[1]);
  assert.equal(options.lens, 'react-native');
  assert.deepEqual((await (await fetch(`http://127.0.0.1:${running.port}` + options.bundle.info)).json()).examples, ['swatch']);
});

test('a file lens lists examples it re-exports, with export * included', async t => {
  const root = fixture(t, {
    'examples/basic.ts': 'export const basic = () => {};\nexport const withIcon = () => {};',
    'examples/sizes.ts': 'export const large = () => {};',
    'examples/index.ts': "export * from './basic.ts';\nexport { large as huge } from './sizes.ts';\nimport './theme.css';",
    'examples/theme.css': '.acme { color: red; }',
  });
  const compiler = new Compiler(root);
  await compiler.settings();
  const { docsRequest } = require('./preview/compiler.cjs');
  const listed = await compiler.docsExamples(docsRequest({ adapter: 'html', examples: 'examples/index.ts' }, root));
  assert.deepEqual(listed.examples.map(example => [example.id, example.export]).sort(), [['basic', 'basic'], ['huge', 'huge'], ['with-icon', 'withIcon']]);
});

test('the design-system export captures each docs page whole and each example its lens renders', async t => {
  const root = docsProject(t);
  const pages = [];
  const artboards = [];
  const capture = {
    captureExportPage: async payload => { pages.push(payload); return { image: Buffer.from('jpeg') }; },
    capture: async (base, payload) => { artboards.push(payload.url); return { image: Buffer.from('jpeg') }; },
    close: () => Promise.resolve(),
  };
  const running = await server.start({ root, capture });
  t.after(() => running.close());
  const answer = await new Promise((resolve, reject) => require('node:http').get(`http://127.0.0.1:${running.port}/_workbench/export`, { agent: false }, response => {
    const chunks = []; response.on('data', chunk => chunks.push(chunk)); response.on('end', () => resolve({ status: response.statusCode, body: Buffer.concat(chunks) }));
  }).on('error', reject));
  assert.equal(answer.status, 200);
  assert.deepEqual(artboards.filter(url => url.includes('.md')), [], 'a docs page gets no artboard references');
  assert.deepEqual(pages.map(payload => [new URL(payload.url).search, payload.selector || 'whole page', payload.fullPage, payload.width]), [
    ['?lens=web', 'whole page', true, 1056],
    ['?lens=web', '#example-primary [data-wb-example-stage]', true, 1056],
    ['?lens=web', '#example-secondary [data-wb-example-stage]', true, 1056],
    ['?lens=native', 'whole page', true, 1056],
    ['?lens=native', '#example-primary [data-wb-example-stage]', true, 1056],
  ]);
  for (const name of ['web-page', 'web-primary', 'native-primary']) assert.ok(answer.body.includes(Buffer.from(name)), name);
});

test('the portable export builds each docs page in each lens as a static page with its code', async t => {
  const root = docsProject(t);
  fs.writeFileSync(path.join(root, 'docs/button.md'), fs.readFileSync(path.join(root, 'docs/button.md'), 'utf8') + '\n![Anatomy](anatomy.svg)\n');
  fs.writeFileSync(path.join(root, 'docs/anatomy.svg'), '<svg xmlns="http://www.w3.org/2000/svg"/>');
  const portable = require('./preview/portable.cjs');
  const built = await portable.create(new Compiler(root), { name: 'Acme' });
  const files = new Map(built.files.map(file => [file.path, Buffer.from(file.data, 'base64')]));
  const catalog = JSON.parse(files.get('browser/workbench.json'));
  assert.deepEqual(catalog.docs, [{ id: 'docs/button.md', title: 'Button', src: 'docs/button.md', lens: 'web',
    lenses: [{ key: 'web', label: 'Web', directory: 'browser/docs/docs-button/web' }, { key: 'native', label: 'Native', directory: 'browser/docs/docs-button/native' }],
    examples: [{ id: 'primary', label: 'Primary' }, { id: 'secondary', label: 'Secondary' }] }]);
  const page = files.get('browser/docs/docs-button/web/index.html').toString();
  assert.match(page, /<script type="module" src="..\/..\/page.js"><\/script>/);
  assert.match(page, /<link rel="stylesheet" href="..\/..\/docs-page.css">/);
  assert.match(page, /<img src=".\/files\/docs\/anatomy.svg" alt="Anatomy">/);
  const options = JSON.parse(/window\.__workbenchDocs=(.*?)<\/script>/.exec(page)[1]);
  assert.deepEqual([options.bundle, options.sourceUrl, options.revisionUrl], [{ module: './examples.js' }, './sources/', '']);
  assert.ok(files.has('browser/docs/docs-button/web/examples.js'));
  assert.ok(files.has('browser/docs/docs-button/web/files/docs/anatomy.svg'));
  assert.ok(files.has('browser/docs/page.js') && !files.get('browser/docs/page.js').toString().includes('interface '));
  assert.equal(JSON.parse(files.get('browser/docs/docs-button/native/sources/primary')).text,
    'export const primary = (canvas: HTMLElement) => { canvas.textContent = "Native primary"; };');
  assert.match(files.get('browser/docs/docs-button/native/index.html').toString(), /data-wb-example="secondary" data-status="missing"/);
});
