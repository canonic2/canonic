const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { Compiler, discover } = require('./preview/compiler.cjs');
const server = require('./server');
const manifest = require('../../contracts/manifest');
const compilerFixture = require('./compiler-fixture.cjs');
const { Browser, findChrome } = require('../../scripts/chrome.cjs');

function fixture(t, files) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-preview-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const [file, contents] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.writeFileSync(path.join(root, file), contents);
  }
  fs.symlinkSync(path.join(__dirname, "../../node_modules"), path.join(root, 'node_modules'), 'junction');
  return root;
}
// Portable builds: the Rspack production compiler the export and the CLI use.
async function build(t, root, file, options) {
  const compiler = await compilerFixture.open(root, options);
  t.after(() => compiler.close());
  return compiler.compile(file);
}
async function buildDocs(t, root, request) {
  const compiler = await compilerFixture.open(root);
  t.after(() => compiler.close());
  return compiler.compileDocs(request);
}
// The compatibility script every project page loads first.
const COMPAT = ['keys.js', 'actions.js', 'scenarios.js'].map(name => fs.readFileSync(path.join(__dirname, '../../ui/legacy', name), 'utf8')).join('\n;\n');
const definition = (adapter, entry, extra = '') => `import { definePreview } from '@canonic2/workbench';
export default definePreview({ id: 'components/button', title: 'Components/Button', adapter: '${adapter}', source: { entry: '${entry}' },
inputs: { label: 'Continue' }, scenarios: { default: {}, disabled: { inputs: { label: 'Disabled' } } }, ${extra} });`;

test('built-in browser adapters resolve framework imports from the previewed project', async t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-project-framework-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const files = {
    'button.ts': 'export default function Button() {}',
    'button.workbench.ts': definition('react', './button.ts'),
    'node_modules/react/package.json': JSON.stringify({ name: 'react', main: 'index.js' }),
    'node_modules/react/index.js': 'export default { createElement: () => "ACME_PROJECT_REACT" };',
    'node_modules/react-dom/package.json': JSON.stringify({ name: 'react-dom', exports: { './client': './client.js' } }),
    'node_modules/react-dom/client.js': 'export const createRoot = () => "ACME_PROJECT_REACT_DOM";',
  };
  for (const [name, body] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true });
    fs.writeFileSync(path.join(root, name), body);
  }
  const result = await build(t, root, 'button.workbench.ts');
  const script = result.outputs.get('preview.js').toString();
  assert.match(script, /ACME_PROJECT_REACT/);
  assert.match(script, /ACME_PROJECT_REACT_DOM/);
});

const FEEDBACK = `{
  default: { artboards: {
    info: { label: 'Info', width: 560, height: 79 },
    warning: { width: 560, height: 119, inputs: { tone: 'warning' }, iterations: {
      bold: { label: 'Bold', source: { entry: './alert.bold.ts' } },
      'compact-table': { inputs: { compact: true } },
    } },
  } },
  empty: { iterations: { illustrated: { source: { entry: './alert.empty.ts', export: 'Illustrated' } } } },
  loading: {},
}`;

test('scenarios declare their own artboards and iterations in the page index', async t => {
  const root = fixture(t, {
    'alert.ts': 'export default () => {};',
    'alert.bold.ts': 'export default () => {};',
    'alert.empty.ts': 'export const Illustrated = () => {};',
    'alert.workbench.ts': `import {definePreview} from '@canonic2/workbench'; export default definePreview({id:'feedback',adapter:'html',source:{entry:'./alert.ts'},scenarios:${FEEDBACK}});`,
  });
  const index = await new Compiler(root).index();
  assert.deepEqual(index.errors, []);
  assert.equal(index.previews[0].artboards, undefined, 'the page has no artboards of its own');
  assert.deepEqual(index.previews[0].scenarios, [
    { id: 'default', label: 'Default', artboards: [
      { id: 'info', label: 'Info', width: 560, height: 79 },
      { id: 'warning', label: 'Warning', width: 560, height: 119, iterations: [{ id: 'bold', label: 'Bold', source: 'alert.bold.ts' }, { id: 'compact-table', label: 'Compact table' }] },
    ] },
    // An iteration with its own source lists it from the project root, for handoffs and references.
    { id: 'empty', label: 'Empty', iterations: [{ id: 'illustrated', label: 'Illustrated', source: 'alert.empty.ts' }] },
    { id: 'loading', label: 'Loading' },
  ]);
  assert.deepEqual(compilerFixture.list(root), JSON.parse(JSON.stringify(index)));
});

test('an iteration with its own source is compiled under its iteration key', async t => {
  const root = fixture(t, {
    'alert.ts': 'export default canvas => { canvas.textContent = "ACME_AUTHORED"; };',
    'alert.bold.ts': 'export default canvas => { canvas.textContent = "ACME_BOLD"; };',
    'alert.empty.ts': 'export const Illustrated = canvas => { canvas.textContent = "ACME_ILLUSTRATED"; };',
    'alert.workbench.ts': `import {definePreview} from '@canonic2/workbench'; export default definePreview({id:'feedback',adapter:'html',source:{entry:'./alert.ts'},scenarios:${FEEDBACK}});`,
  });
  const result = await build(t, root, 'alert.workbench.ts');
  const script = result.outputs.get('preview.js').toString();
  for (const marker of ['ACME_AUTHORED', 'ACME_BOLD', 'ACME_ILLUSTRATED', '"default+warning/bold"', '"empty+illustrated"']) assert.ok(script.includes(marker), marker);
  assert.ok(!script.includes('"default+warning/compact-table"'), 'an iteration without a source uses its scenario’s');
});

test('artboards and iterations are validated where they are declared', async t => {
  const root = fixture(t, { 'button.ts': 'export default () => {};', 'button.workbench.ts': definition('html', './button.ts') });
  const board = "info: { width: 560, height: 79 }";
  const cases = [
    ["artboards: { info: { width: 560, height: 79 } }", 'artboards must be declared inside a scenario'],
    ["iterations: { bold: {} }", 'iterations must be declared inside a scenario'],
    ["scenarios: { default: { artboards: {} } }", 'scenario default: artboards must be a map of 1–32 artboards.'],
    ["scenarios: { default: { artboards: [] } }", 'scenario default: artboards must be a map of 1–32 artboards.'],
    [`scenarios: { default: { artboards: Object.fromEntries(Array.from({ length: 33 }, (_, i) => ['board-' + i, { width: 1, height: 1 }])) } }`, 'scenario default: artboards must be a map of 1–32 artboards.'],
    ["scenarios: { default: { artboards: { 'Bad Id': { width: 560, height: 79 } } } }", 'scenario default: artboard id “Bad Id” must be kebab-case.'],
    ["scenarios: { default: { artboards: { info: { width: 0, height: 79 } } } }", 'scenario default › artboard info: width and height must be integers from 1 to 8192.'],
    ["scenarios: { default: { artboards: { info: { width: 560, height: 8193 } } } }", 'scenario default › artboard info: width and height must be integers from 1 to 8192.'],
    ["scenarios: { default: { artboards: { info: { width: 560, height: 79, label: ' ' } } } }", 'scenario default › artboard info: label must be nonempty text.'],
    ["scenarios: { default: { artboards: { info: { width: 560, height: 79, inputs: [] } } } }", 'scenario default › artboard info: inputs must be a map.'],
    [`scenarios: { default: { artboards: { ${board} }, iterations: { bold: {} } } }`, 'scenario default: iterations of declared artboards belong to each artboard'],
    [`scenarios: { default: { artboards: { ${board.replace('}', ", iterations: { Bold: {} } }")} } } }`, 'scenario default › artboard info: iteration id “Bold” must be kebab-case.'],
    ["scenarios: { empty: { iterations: { Bold: {} } } }", 'scenario empty: iteration id “Bold” must be kebab-case.'],
    ["scenarios: { empty: { iterations: [] } }", 'scenario empty: iterations must be a map.'],
    ["scenarios: { empty: { iterations: { bold: null } } }", 'scenario empty › iteration bold: must be an object.'],
    ["scenarios: { empty: { iterations: { bold: { label: '' } } } }", 'scenario empty › iteration bold: label must be nonempty text.'],
    ["scenarios: { empty: { iterations: { bold: { iterations: {} } } } }", 'scenario empty › iteration bold: cannot declare iterations.'],
    ["scenarios: { empty: { iterations: { bold: { artboards: {} } } } }", 'scenario empty › iteration bold: cannot declare artboards.'],
    ["scenarios: { empty: { iterations: { bold: { inputs: 'bold' } } } }", 'scenario empty › iteration bold: inputs must be a map.'],
    ["scenarios: { empty: { iterations: { bold: { ready: true } } } }", 'scenario empty › iteration bold: ready must be a function.'],
    ["scenarios: { default: { artboards: { info: { width: 560, height: 79, requests: {} } } } }", 'scenario default › artboard info: “requests” is not an artboard field; an artboard takes label, width, height, inputs and iterations.'],
    ["scenarios: { empty: { iterations: { bold: { source: { entry: './missing.ts' } } } } }", 'scenario empty › iteration bold: source must exist inside the project: ./missing.ts'],
    ["scenarios: { empty: { iterations: { bold: { requests: { 'bad request': {} } } } } }", 'scenario empty › iteration bold: request bad request must be'],
    ["scenarios: { empty: { iterations: { bold: { fixtures: { size: 1n } } } } }", 'scenario empty › iteration bold: fixtures must be JSON-serializable'],
    [`scenarios: { default: { artboards: { ${board.replace('}', ", iterations: { bold: { inputs: { size: 1n } } } }")} } } }`, 'scenario default › artboard info › iteration bold: inputs must be JSON-serializable'],
  ];
  for (const [fields, message] of cases) {
    fs.writeFileSync(path.join(root, 'button.workbench.ts'), definition('html', './button.ts', fields));
    const index = await new Compiler(root).index();
    assert.deepEqual(index.previews, [], fields);
    assert.equal(index.errors.length, 1, fields);
    assert.ok(index.errors[0].includes(message), fields + '\n' + index.errors[0]);
    // The file is named once, from the project root.
    assert.ok(index.errors[0].startsWith('button.workbench.ts: ') && !index.errors[0].includes(root) && index.errors[0].indexOf('workbench.ts: ') === index.errors[0].lastIndexOf('workbench.ts: '), index.errors[0]);
  }
});

test('native listing matches the compiler on preview fixtures and validation edge cases', async t => {
  const root = fixture(t, { 'button.ts': 'export default () => {};', 'button.workbench.ts': definition('html', './button.ts') });
  const cases = ['', "sizes: []", "sizes: ['123']", "sizes: ['small--wide']", "scenarios: null", "scenarios: []", "scenarios: { default: null }", "scenarios: { '1': {}, '10': {}, '2': {} }", "title: 42", "controls: { tone: {type: 'select', options: []} }", "controls: { tone: null }", "links: null", "links: {'/next': {scenario: 'ready'}}", "links: {'/next': {preview: 3}}", "requests: {'bad request': {}}", "requests: {'GET /acme': {body: {ok: true}}}", "scenarios: { empty: { requests: {'GET /acme': []} } }", "scenarios: { empty: { label: 42 } }"];
  cases.push("artboards: {info:{width:560,height:79}}", "iterations: {bold:{}}", "scenarios: { default: { artboards: {} } }", "scenarios: { default: { artboards: {info:{width:0,height:79}} } }", "scenarios: { default: { artboards: {info:{width:560,height:79}}, iterations: {bold:{}} } }", "scenarios: { default: { artboards: {info:{width:560,height:79,iterations:{'Bad':{}}}} } }", "scenarios: { default: { artboards: {info:{width:560,height:79}} } }", "scenarios: { empty: { iterations: {illustrated:{label:'Illustrated'}} } }");
  for (const fields of cases) {
    fs.writeFileSync(path.join(root, 'button.workbench.ts'), definition('html', './button.ts', fields));
    const expected = JSON.parse(JSON.stringify(await new Compiler(root).index()));
    assert.deepEqual(compilerFixture.list(root), expected, fields);
  }
});

test('a definition whose path holds an address mark is a problem in both indexes, not a page', async t => {
  const root = fixture(t, {
    'routes/page.ts': 'export default () => {};',
    'routes/+page.workbench.ts': definition('html', './page.ts'),
  });
  const index = JSON.parse(JSON.stringify(await new Compiler(root).index()));
  assert.deepEqual(index.previews, []);
  assert.equal(index.errors.length, 1);
  assert.match(index.errors[0], /^routes\/\+page\.workbench\.ts: a preview’s path can’t contain “:”, “\+”, “!”, or “~”/);
  assert.deepEqual(compilerFixture.list(root), index);
});

test('a reserved path is the problem even when its definition is invalid, and it is not evaluated', async t => {
  const root = fixture(t, {
    'routes/page.ts': 'export default () => {};',
    // Invalid both as a literal the native index reads and once evaluated.
    'routes/+page.workbench.ts': definition('html', './page.ts').replace("id: 'components/button'", "id: 'Not Kebab'"),
    'routes/~layout.workbench.ts': 'import missing from "./missing"; export default missing;',
  });
  const compiler = new Compiler(root);
  const evaluated = [];
  const original = compiler.definition.bind(compiler);
  compiler.definition = file => { evaluated.push(file); return original(file); };
  const index = JSON.parse(JSON.stringify(await compiler.index()));
  assert.deepEqual(evaluated, []);
  assert.deepEqual(index.previews, []);
  assert.equal(index.errors.length, 2);
  for (const problem of index.errors) assert.match(problem, /^routes\/[+~][a-z]+\.workbench\.ts: a preview’s path can’t contain “:”, “\+”, “!”, or “~”/);
  assert.deepEqual(compilerFixture.list(root), index);
});

test('definition problems name the file once, by its path from the project root', async t => {
  const root = fixture(t, {
    'pages/page.ts': 'export default () => {};',
    'pages/literal.workbench.ts': definition('html', './page.ts').replace("scenarios: { default: {}", "scenarios: { '2-up': {}"),
    // Evaluated by Node rather than read as a literal.
    'pages/computed.workbench.ts': definition('html', './page.ts').replace("id: 'components/button'", "id: ['pages', 'Computed'].join('/')"),
  });
  const index = JSON.parse(JSON.stringify(await new Compiler(root).index()));
  assert.deepEqual(index.errors, [
    'pages/computed.workbench.ts: id must use kebab-case segments separated by /.',
    'pages/literal.workbench.ts: scenarios need kebab-case IDs and object definitions.',
  ]);
  assert.deepEqual(compilerFixture.list(root), index);
});

test('scenario IDs follow the artboard and iteration ID rule, and default labels are sentence case', async t => {
  const root = fixture(t, { 'button.ts': 'export default () => {};', 'button.workbench.ts': definition('html', './button.ts') });
  for (const id of ['2-up', 'a--b', '-ready', 'ready-', 'Ready', '10']) {
    fs.writeFileSync(path.join(root, 'button.workbench.ts'), definition('html', './button.ts', `scenarios: { '${id}': {} }`).replace('scenarios: { default: {}, disabled: { inputs: { label: \'Disabled\' } } }, ', ''));
    const index = JSON.parse(JSON.stringify(await new Compiler(root).index()));
    assert.deepEqual(index.errors, ['button.workbench.ts: scenarios need kebab-case IDs and object definitions.'], id);
    assert.deepEqual(compilerFixture.list(root), index, id);
  }
  // A link names a scenario by the same rule.
  fs.writeFileSync(path.join(root, 'button.workbench.ts'), definition('html', './button.ts', "links: { '/next': { scenario: '2-up' } }"));
  const linked = JSON.parse(JSON.stringify(await new Compiler(root).index()));
  assert.match(linked.errors[0], /^button\.workbench\.ts: link \/next must name a preview ID/);
  assert.deepEqual(compilerFixture.list(root), linked);
  // Labels: the literal the native index projects, and one with artboards and iterations, which Node lists.
  const literal = "scenarios: { 'with-filters': {}, step2: {}, empty: { label: 'Nothing yet' } }";
  const nested = "scenarios: { 'with-filters': { artboards: { 'side-panel': { width: 320, height: 640, iterations: { 'compact-table': {} } } } }, 'no-results': { iterations: { 'inline-hint': {} } } }";
  const labels = [
    [literal, [{ id: 'with-filters', label: 'With filters' }, { id: 'step2', label: 'Step2' }, { id: 'empty', label: 'Nothing yet' }]],
    [nested, [
      { id: 'with-filters', label: 'With filters', artboards: [{ id: 'side-panel', label: 'Side panel', width: 320, height: 640, iterations: [{ id: 'compact-table', label: 'Compact table' }] }] },
      { id: 'no-results', label: 'No results', iterations: [{ id: 'inline-hint', label: 'Inline hint' }] },
    ]],
  ];
  for (const [fields, scenarios] of labels) {
    fs.writeFileSync(path.join(root, 'button.workbench.ts'), definition('html', './button.ts', fields).replace('scenarios: { default: {}, disabled: { inputs: { label: \'Disabled\' } } }, ', ''));
    const index = JSON.parse(JSON.stringify(await new Compiler(root).index()));
    assert.deepEqual(index.errors, [], fields);
    assert.deepEqual(index.previews[0].scenarios, scenarios, fields);
    assert.deepEqual(compilerFixture.list(root), index, fields);
  }
});

test('defineDocs scenarios follow the ID rule and default to sentence-case labels', async t => {
  const docs = scenarios => `import { defineDocs } from '@canonic2/workbench';
export default defineDocs({ id: 'guides/colors', docs: './colors.md', scenarios: ${scenarios} });`;
  const root = fixture(t, { 'colors.md': '# Colors\n', 'colors.workbench.ts': docs("{ 'dark-mode': {}, empty: { label: 'Nothing yet' } }") });
  let index = JSON.parse(JSON.stringify(await new Compiler(root).index()));
  assert.deepEqual(index.errors, []);
  assert.deepEqual(index.docs[0].scenarios, [{ id: 'dark-mode', label: 'Dark mode' }, { id: 'empty', label: 'Nothing yet' }]);
  assert.deepEqual(compilerFixture.list(root), index);
  fs.writeFileSync(path.join(root, 'colors.workbench.ts'), docs("{ '2-up': {} }"));
  index = JSON.parse(JSON.stringify(await new Compiler(root).index()));
  assert.deepEqual(index.errors, ['colors.workbench.ts: scenarios need kebab-case IDs.']);
  assert.deepEqual(compilerFixture.list(root), index);
});

test('the server supplies discovered lens labels while retaining per-page overrides', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\npreviews:\n  lensLabel: Design\ncollections:\n  - name: Components\n    items:\n      - label: My button\n        src: button.workbench.ts\n        lensLabel: Reference\n',
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas) { canvas.textContent = "Button"; }',
  });
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  t.after(() => running.close());
  const resolved = await running.config();
  assert.deepEqual(resolved.problems, []);
  assert.equal(resolved.catalogCollections[0].items[0].lensLabel, 'Design');
  assert.equal(resolved.collections[0].items[0].lensLabel, 'Reference');
  const merged = manifest.mergeCollections(resolved.collections, resolved.catalogCollections);
  assert.equal(merged[0].items.length, 1);
  assert.equal(merged[0].items[0].label, 'My button');
  assert.equal(manifest.authoredLensLabel(merged[0].items[0]), 'Reference');
});

test('preview icons pass through discovery, override maps, and isolate invalid names', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\npreviews:\n  icon: boxes\n  icons:\n    Components: app-window\ncollections:\n  - name: Components\n    icon: star\n',
    'button.workbench.ts': definition('html', './button.ts', "icon: 'monitor'"),
    'button.ts': 'export default function(canvas) { canvas.textContent = "Button"; }',
    'bad.workbench.ts': definition('html', './button.ts', "icon: 'Bad Icon'"),
  });
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  t.after(() => running.close());
  const resolved = await running.config();
  assert.equal(resolved.catalogCollections[0].icon, 'app-window');
  assert.equal(resolved.catalogCollections[0].items[0].icon, 'monitor');
  assert.equal(resolved.pages['button.workbench.ts'].preview.icon, 'monitor');
  assert.match(resolved.problems.join('\n'), /bad.workbench.ts: .*icon must be a kebab-case Lucide icon name/);
  const manual = resolved.collections.map(collection => ({ name: collection.name, icon: collection.icon, items: collection.items }));
  assert.equal(manifest.mergeCollections(manual, resolved.catalogCollections)[0].icon, 'star');
});

test('preview collection and page icons respect mappings and configured fallbacks', async t => {
  const root = fixture(t, {
    'jobs.workbench.ts': definition('html', './jobs.ts').replace('Components/Button', 'Web App/Pages/Jobs'),
    'jobs.ts': 'export default function() {}',
  });
  const cases = [
    ['', 'component', 'component'],
    ['previews:\n  icon: boxes\n', 'boxes', 'boxes'],
    ['previews:\n  icons:\n    Web App: app-window\n    Web App/Pages: monitor\n', 'app-window', 'monitor'],
  ];
  fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\n');
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  t.after(() => running.close());
  for (const [previews, collectionIcon, pageIcon] of cases) {
    fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\n' + previews);
    const resolved = await running.config();
    assert.deepEqual(resolved.problems, []);
    assert.equal(resolved.catalogCollections.length, 1);
    assert.equal(resolved.catalogCollections[0].icon, collectionIcon);
    assert.equal(resolved.catalogCollections[0].items[0].items[0].icon, pageIcon);
  }
});

test('discovers TypeScript previews, validates IDs and scenarios, and isolates invalid definitions', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas, ctx) { canvas.textContent = ctx.inputs.label; }',
    'broken.workbench.ts': 'export default { id: "INVALID" };',
    'dist/ignored.workbench.ts': definition('html', './button.ts'),
  });
  assert.equal(discover(root).length, 2);
  const index = await new Compiler(root).index();
  assert.equal(index.previews.length, 1);
  assert.deepEqual(index.previews[0].scenarios.map(scenario => scenario.id), ['default', 'disabled']);
  assert.equal(index.errors.length, 1);
  assert.match(index.errors[0], /kebab-case/);
  assert.throws(() => new Compiler(root, { include: ['src/**'] }).declared('button.workbench.ts'), /declared/);
});

test('lists every definition from one build, follows helper edits and isolates broken definitions', async t => {
  const page = (id, extra = '') => `import { definePreview } from '@canonic2/workbench';
import { section } from '../titles';
export default definePreview({ id: 'pages/${id}', title: section + '/${id}', adapter: 'html', source: { entry: '../button.ts' } });${extra}`;
  const root = fixture(t, {
    'titles.ts': "export const section: string = 'Acme';",
    'button.ts': 'export default function(canvas) { canvas.textContent = "Button"; }',
    'pages/alpha.workbench.ts': page('alpha'),
    'pages/beta.workbench.tsx': page('beta'),
    'pages/gamma.workbench.ts': page('gamma'),
  });
  // Definitions evaluated one build at a time: only those the shared build could not.
  const compiler = new Compiler(root);
  const alone = [];
  const evaluate = compiler.evaluate.bind(compiler);
  compiler.evaluate = file => { alone.push(path.relative(root, file)); return evaluate(file); };
  let index = await compiler.index();
  assert.deepEqual(index.previews.map(preview => preview.title), ['Acme/alpha', 'Acme/beta', 'Acme/gamma']);
  assert.deepEqual(index.errors, []);
  assert.deepEqual(alone, []);
  fs.writeFileSync(path.join(root, 'titles.ts'), "export const section: string = 'Acme Pages';\n");
  index = await compiler.index();
  assert.deepEqual(index.previews.map(preview => preview.title), ['Acme Pages/alpha', 'Acme Pages/beta', 'Acme Pages/gamma']);
  assert.deepEqual(alone, []);

  fs.writeFileSync(path.join(root, 'pages/beta.workbench.tsx'), page('beta', '\nthrow new Error("beta failed");'));
  fs.writeFileSync(path.join(root, 'pages/gamma.workbench.ts'), page('gamma', '\nconst = ;'));
  index = await compiler.index();
  assert.deepEqual(index.previews.map(preview => preview.id), ['pages/alpha']);
  assert.equal(index.errors.length, 2);
  assert.match(index.errors.join('\n'), /pages\/beta\.workbench\.tsx: beta failed/);
  assert.match(index.errors.join('\n'), /pages\/gamma\.workbench\.ts: /);
  assert.deepEqual(alone.sort(), ['pages/beta.workbench.tsx', 'pages/gamma.workbench.ts']);
});

test('callers listing at the same time share one definitions build', async t => {
  const page = id => `import { definePreview } from '@canonic2/workbench';
import { section } from '../titles';
export default definePreview({ id: 'pages/${id}', title: section + '/${id}', adapter: 'html', source: { entry: '../button.ts' } });`;
  const root = fixture(t, {
    'titles.ts': "export const section: string = 'Acme';",
    'button.ts': 'export default function(canvas) { canvas.textContent = "Button"; }',
    'pages/alpha.workbench.ts': page('alpha'),
    'pages/beta.workbench.ts': page('beta'),
  });
  const compiler = new Compiler(root);
  const builds = [];
  const loadBatch = compiler.loadBatch.bind(compiler);
  compiler.loadBatch = files => { builds.push(files.length); return loadBatch(files); };
  const [first, second] = await Promise.all([compiler.index(), compiler.index()]);
  assert.deepEqual(builds, [2]);
  assert.deepEqual(second.previews, first.previews);

  // After an edit, the next callers share one build again and see it.
  fs.writeFileSync(path.join(root, 'titles.ts'), "export const section: string = 'Acme Pages';\n");
  const again = await Promise.all([compiler.index(), compiler.index()]);
  assert.deepEqual(builds, [2, 2]);
  for (const index of again) assert.deepEqual(index.previews.map(preview => preview.title), ['Acme Pages/alpha', 'Acme Pages/beta']);
});

test('indexes a definition’s size keys and rejects entries that aren’t keys', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts', "sizes: ['laptop', 'sidebar']"),
    'wide.workbench.ts': definition('html', './button.ts', "sizes: ['1024']").replace('components/button', 'components/wide').replace('Components/Button', 'Components/Wide'),
    'button.ts': 'export default function(canvas) { canvas.textContent = "Button"; }',
  });
  const index = await new Compiler(root).index();
  assert.deepEqual(index.previews.map(preview => [preview.id, preview.sizes]), [['components/button', ['laptop', 'sidebar']]]);
  assert.match(index.errors.join('\n'), /wide\.workbench\.ts: sizes must be a list of size keys/);
});

test('the server resolves a definition’s sizes against the space and reports unknown keys', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\nsizes:\n  fit: true\n  sidebar:\n    width: 340\n    height: fill\n',
    'button.workbench.ts': definition('html', './button.ts', "sizes: ['sidebar', 'tablet']"),
    'button.ts': 'export default function(canvas) { canvas.textContent = "Button"; }',
  });
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  t.after(() => running.close());
  const resolved = await running.config();
  assert.deepEqual(resolved.sizes.map(size => size.key), ['fit', 'sidebar']);
  assert.deepEqual(resolved.pages['button.workbench.ts'].sizes, ['sidebar']);
  assert.match(resolved.problems.join('\n'), /button\.workbench\.ts: size “tablet” isn’t one of the space’s sizes\./);
});

test('builds HTML scripts, inline modules, CSS, srcset and assets into portable output', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.html'),
    'button.html': '<!doctype html><html><head><style>button{background:url(./dot.svg)}</style></head><body><a href="/account">Account</a><button>Continue</button><img srcset="./dot.svg 1x, ./dot.svg 2x"><script type="module">import "./button.css"; import { label } from "./label.ts"; document.querySelector("button").textContent=label;</script></body></html>',
    'button.css': 'button { color: purple; }',
    'label.ts': 'export const label: string = "Bundled";',
    'dot.svg': '<svg xmlns="http://www.w3.org/2000/svg"/>',
  });
  const output = await build(t, root, 'button.workbench.ts');
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
  // The script's own chunk carries the CSS it imports, loaded when the script runs.
  assert.ok(output.outputs.get('preview.js').toString().includes(path.basename(css, '.css')));
  // The inline module script is bundled with its imports and runs on each render.
  assert.match(output.outputs.get('preview.js').toString(), /data-workbench-script/);
  assert.ok(Array.from(output.outputs).some(([name, body]) => name.endsWith('.js') && body.includes(Buffer.from('Bundled'))));
  fs.writeFileSync(path.join(root, 'label.ts'), 'export const label = "Changed";');
  const changed = await build(t, root, 'button.workbench.ts');
  assert.notEqual(changed.revision, output.revision);
});

test('an inline script keeps attributes that look like replacement patterns', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.html'),
    'button.html': '<!doctype html><html><body><button>Continue</button><script data-price="$&amp;$1"> document.body.dataset.ready = "yes";</script></body></html>',
  });
  const output = await build(t, root, 'button.workbench.ts');
  const preview = output.outputs.get('preview.js').toString();
  // The script's attributes stay in the page; its code is bundled.
  assert.ok(preview.includes('data-price=') && preview.includes('$&amp;$1'));
  assert.ok(Array.from(output.outputs).some(([name, body]) => name.endsWith('.js') && body.includes(Buffer.from('document.body.dataset.ready'))));
});

test('bundles React and Vue SFCs with scoped styles using project framework versions', async t => {
  for (const [adapter, entry, source] of [
    ['react', './Button.tsx', 'export default function Button({label}:{label:string}) { return <button>{label}</button>; }'],
    ['vue', './Button.vue', '<script setup lang="ts">defineProps<{label:string}>()</script><template><button>{{label}}</button></template><style scoped>button{color:red}</style>'],
  ]) {
    const root = fixture(t, { 'button.workbench.ts': definition(adapter, entry), [entry.slice(2)]: source });
    const built = await build(t, root, 'button.workbench.ts');
    assert.ok(built.outputs.get('preview.js').length > 1000);
    assert.ok(built.localFiles.includes(path.join(root, entry)));
    // Vue style blocks are stylesheets of the build, scoped to the component.
    if (adapter === 'vue') {
      const css = Array.from(built.outputs).filter(([name]) => name.endsWith('.css')).map(([, body]) => body.toString()).join('\n');
      assert.match(css, /button\[data-v-[0-9a-f]+\]/);
      assert.match(built.outputs.get('index.html').toString(), /<link rel="stylesheet" href="\.\/preview\.css">/);
    }
  }
});

test('CSS modules scope class names and leave custom properties global', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('react', './Button.tsx'),
    'Button.tsx': `import styles from './button.module.css';
export default function Button({label}:{label:string}) { return <button className={styles.tint}><svg><use href="#icon-dot"/></svg>{label}</button>; }`,
    // An SVG sprite outside the stylesheet reads the variable by its written name.
    'button.module.css': '.tint { --acme-tint: rebeccapurple; color: var(--acme-tint); }',
  });
  const built = await build(t, root, 'button.workbench.ts');
  const css = Array.from(built.outputs).filter(([name]) => name.endsWith('.css')).map(([, body]) => body.toString()).join('\n');
  assert.doesNotMatch(css, /(^|[\s,{])\.tint\s*\{/);
  assert.match(css, /--acme-tint:\s*rebeccapurple/);
  assert.match(css, /var\(--acme-tint\)/);
});

test('preview mounts fill flex-body artboards and React Native Web fills frames and docs stages', {
  timeout: 60000, skip: require('../../capture/electron-runtime').unavailable() || false,
}, async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\n',
    'flex.workbench.ts': `import { definePreview } from '@canonic2/workbench';
      export default definePreview({ id:'pages/jobs', title:'Pages/Jobs', adapter:'react',
        source:{entry:'./Flex.tsx'}, scenarios:{loading:{inputs:{label:'Loading'}},
          empty:{inputs:{label:'No jobs'}}, 'load-error':{inputs:{label:'Load error'}}} });`,
    'Flex.tsx': `import './flex.css';
      export default function Jobs({label}) { return <section data-testid="screen" style={{width:'100%'}}>
        <div style={{width:827,maxWidth:'100%'}}>{label}</div></section>; }`,
    'flex.css': 'body { display:flex; } #app { width:100%; }',
    'native.workbench.ts': definition('react-native-web', './Screen.tsx'),
    'web.workbench.ts': definition('react', './Web.tsx'),
    'Screen.tsx': `import { View, Text } from 'react-native';
      export default function Screen() { return <View testID="screen" style={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}><Text>Acme modal</Text></View>; }`,
    'Web.tsx': 'export default function Web() { return <div data-testid="screen">Acme web</div>; }',
    'examples.ts': 'export { default as screen } from "./Screen.tsx";',
    'web-examples.ts': 'export { default as screen } from "./Web.tsx";',
  });
  const routes = new Map();
  for (const name of ['native', 'web', 'flex']) {
    const built = await build(t, root, name + '.workbench.ts');
    for (const [file, contents] of built.outputs) routes.set('/' + name + '/' + file, contents);
    if (name === 'flex') continue;
    const docs = await buildDocs(t, root, { adapter: name === 'native' ? 'react-native-web' : 'react', examples: name === 'native' ? 'examples.ts' : 'web-examples.ts' });
    for (const [file, contents] of docs.outputs) routes.set('/' + name + '-docs/' + file, contents);
    routes.set('/' + name + '-docs/index.html', Buffer.from(`<link rel="stylesheet" href="./examples.css">
      <style>.stage { min-height:48px; padding:32px; } .sized { height:200px; }</style>
      <div class="stage sized"></div><div class="stage intrinsic"></div>
      <script type="module">import { adapter, examples, environment } from './examples.js';
      window.stops = []; for (const stage of document.querySelectorAll('.stage')) {
        window.stops.push(await adapter.mount(stage, examples.screen, { inputs:{}, error:console.error }, environment));
      } window.ready = true;</script>`));
  }
  const http = require('node:http');
  const listener = http.createServer((req, res) => {
    const contents = routes.get(req.url.split('?')[0]);
    if (!contents) { res.writeHead(404); res.end(); return; }
    res.setHeader('Content-Type', req.url.endsWith('.js') ? 'text/javascript' : req.url.endsWith('.css') ? 'text/css' : 'text/html');
    res.end(contents);
  });
  await new Promise(resolve => listener.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => listener.close(resolve)));
  const base = 'http://127.0.0.1:' + listener.address().port;
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  t.after(() => running.close());
  const runner = path.join(root, 'check.cjs');
  fs.writeFileSync(runner, `const { app, BrowserWindow } = require('electron');
    const assert = require('node:assert/strict');
    app.whenReady().then(async () => {
      const win = new BrowserWindow({ show:false, width:393, height:852, useContentSize:true });
      const evaluate = code => win.webContents.executeJavaScript(code);
      const load = async path => {
        await win.loadURL(${JSON.stringify(base)} + path);
        await evaluate(\`new Promise((resolve, reject) => { const start=Date.now(); const poll=()=> {
          if (window.ready || document.querySelector('[data-testid="screen"]')) resolve();
          else if(Date.now()-start>10000) reject(new Error('Mount timed out')); else setTimeout(poll,20);
        }; poll(); })\`);
      };
      for (const surface of ['portable','live']) {
        if (surface === 'live') await win.loadURL(${JSON.stringify(new URL(running.url).origin)} + '/_workbench/preview-host.html');
        for (const [scenario,label] of [['loading','Loading'],['empty','No jobs'],['load-error','Load error']]) {
          if (surface === 'portable') await load('/flex/index.html?scenario=' + scenario);
          else await evaluate('window.wbPreviewHost.load(' + JSON.stringify(${JSON.stringify(new URL(running.url).origin)} + '/flex.workbench.ts?scenario=' + scenario) + ')');
          assert.equal(await evaluate('document.querySelector("[data-testid=screen]").textContent.trim()'),label);
          for (const [width,height] of [[1512,982],[393,852],[800,600]]) {
            win.setContentSize(width,height);
            await evaluate('new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))');
            const box = await evaluate(\`(() => { const mount=document.querySelector('#workbench-preview').getBoundingClientRect();
              const screen=document.querySelector('[data-testid=screen]').getBoundingClientRect();
              return {mount:mount.width,screen:screen.width,width:innerWidth,height:mount.height,viewportHeight:innerHeight}; })()\`);
            assert.equal(box.mount,box.width,surface + ' ' + scenario);
            assert.equal(box.screen,box.width,surface + ' ' + scenario);
            assert.ok(box.height >= box.viewportHeight);
          }
        }
      }
      await load('/native/index.html');
      for (const [width,height] of [[393,852],[800,600],[320,480]]) {
        win.setContentSize(width,height);
        await evaluate('new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))');
        const box = await evaluate(\`(() => { const root=document.querySelector('[data-testid="screen"]');
          const r=root.getBoundingClientRect(), text=root.firstElementChild.getBoundingClientRect();
          return {height:r.height,width:r.width,viewport:innerHeight,viewportWidth:innerWidth,
            centered:Math.abs(text.top+text.height/2-innerHeight/2)<1}; })()\`);
        assert.equal(box.height,box.viewport); assert.equal(box.width,box.viewportWidth); assert.ok(box.centered);
      }
      await evaluate(\`(() => { const style=document.createElement('style'); style.textContent='#workbench-preview { display:block; }'; document.head.append(style); })()\`);
      assert.equal(await evaluate('getComputedStyle(document.querySelector("#workbench-preview")).display'), 'block');
      await load('/web/index.html');
      assert.equal(await evaluate('getComputedStyle(document.querySelector("#workbench-preview")).display'), 'block');
      assert.ok(await evaluate('document.querySelector("[data-testid=screen]").getBoundingClientRect().height < innerHeight'));
      await load('/native-docs/index.html');
      assert.equal(await evaluate('document.querySelector(".sized [data-testid=screen]").getBoundingClientRect().height'),200);
      assert.equal(await evaluate('getComputedStyle(document.querySelector(".sized")).paddingTop'),'32px');
      assert.ok(await evaluate('document.querySelector(".intrinsic").getBoundingClientRect().height < innerHeight'));
      await evaluate('window.stops.forEach(stop => stop())');
      assert.equal(await evaluate('document.querySelectorAll(".wb-react-native-root").length'),0);
      await load('/web-docs/index.html');
      assert.equal(await evaluate('getComputedStyle(document.querySelector(".stage")).display'),'block');
      win.destroy(); app.exit(0);
    }).catch(error => { console.error(error); app.exit(1); });`);
  // Use a test-owned copy of the packaged app so its entry can run assertions.
  const binary = await require('../../capture/electron-runtime').prepare(path.join(root, 'runtime'));
  const appMain = process.platform === 'darwin'
    ? path.resolve(path.dirname(binary), '../Resources/app/main.cjs')
    : path.join(path.dirname(binary), 'resources/app/main.cjs');
  fs.copyFileSync(runner, appMain);
  const env = { ...process.env }; delete env.ELECTRON_RUN_AS_NODE;
  const child = require('node:child_process').spawn(binary, ['--user-data-dir=' + path.join(root, 'browser-profile'), runner], { env });
  t.after(() => { if (child.exitCode === null) child.kill(); });
  let output = '';
  child.stdout.on('data', data => { output += data; });
  child.stderr.on('data', data => { output += data; });
  const code = await new Promise((resolve, reject) => { child.on('error', reject); child.on('exit', resolve); });
  assert.equal(code, 0, output);
});

test('builds run the project’s plugin capabilities and loader rules, and JavaScript plugins are problems', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('acme', './button.acme'),
    'button.acme': 'Custom adapter',
    'adapter.ts': `import { buildInfo } from 'virtual:acme-build-info';
import { platform } from '@acme/platform';
import { nativePlatform } from './native/screen';
import names from 'virtual:svg-icons-names';
import 'virtual:svg-icons-register';
// import.meta.glob('./missing/*.json') in a comment stays as written
const locales = import.meta.glob('./locales/*.json', { eager: true, import: 'default' });
export function mount(canvas, source, ctx) {
  canvas.textContent = [source, ctx.inputs.label, buildInfo, platform, nativePlatform, names.join(','), Object.keys(locales).join(','), locales['./locales/fr.json'].hello, GREETING].join('|');
}`,
    'native/screen.ts': 'import { platform } from "@acme/platform"; export const nativePlatform = "from " + platform;',
    'platform/web.ts': 'export const platform = "ACME_WEB";',
    'platform/native.ts': 'export const platform = "ACME_NATIVE";',
    'locales/en.json': '{"hello":"Hello"}',
    'locales/fr.json': '{"hello":"ACME_BONJOUR"}',
    'icons/check.svg': '<svg width="16" height="12"><title>check</title><path id="a" stroke="#000"/><use href="#a"/></svg>',
    'greeting-loader.cjs': 'module.exports = function (source) { return source.split("GREETING").join(JSON.stringify(this.getOptions().greeting)); };',
    'workbench.config.ts': `import { defineConfig, virtualModule, resolveImport, importGlob, svgSprite } from '@canonic2/workbench';
export default defineConfig({
  adapters: { acme: { runtime: './adapter.ts', rules: [{ test: /\\.acme$/, type: 'asset/source' }] } },
  rules: [{ test: /adapter\\.ts$/, enforce: 'pre', loader: './greeting-loader.cjs', options: { greeting: 'ACME_GREETING' } }],
  plugins: [
    virtualModule({ id: 'virtual:acme-build-info', source: "export const buildInfo = 'ACME_BUILD_42';" }),
    resolveImport({ request: '@acme/platform', importer: 'native/**', to: './platform/native.ts' }),
    resolveImport({ request: '@acme/platform', to: './platform/web.ts' }),
    importGlob(),
    svgSprite({ dirs: 'icons', symbolId: 'icon-[name]' }),
  ],
});`,
  });
  const built = await build(t, root, 'button.workbench.ts');
  const js = Array.from(built.outputs).filter(([name]) => name.endsWith('.js')).map(([, body]) => body.toString()).join('\n');
  for (const expected of ['Custom adapter', 'ACME_BUILD_42', 'ACME_WEB', 'ACME_NATIVE', 'ACME_BONJOUR', 'ACME_GREETING', './locales/en.json', './locales/fr.json', 'icon-check']) {
    assert.ok(js.includes(expected), expected);
  }
  // The icon is a symbol with its own view box and IDs, and no title.
  assert.match(js, /<symbol id=\\?"icon-check\\?"[^>]*viewBox=\\?"0 0 16 12/);
  assert.match(js, /icon-check_a/);
  assert.doesNotMatch(js, /<title>check/);
  assert.ok(built.localFiles.includes(path.join(root, 'workbench.config.ts')));
  assert.ok(built.localFiles.includes(path.join(root, 'locales/fr.json')));
  // JavaScript plugins do not run in the native compiler; they are problems.
  fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\n');
  fs.writeFileSync(path.join(root, 'workbench.config.ts'), `export default { plugins: [{ name: 'acme-old', setup() {} }, { apply() {} }], adapters: { acme: { runtime: './adapter.ts' } } };`);
  const running = await server.start({ root, capture: { close: async () => {} } });
  t.after(() => running.close());
  let problems = [];
  for (let attempt = 0; attempt < 100 && problems.length < 2; attempt++) {
    problems = (await (await fetch('http://127.0.0.1:' + running.port + '/_workbench/config')).json()).problems;
    if (problems.length < 2) await new Promise(resolve => setTimeout(resolve, 100));
  }
  assert.match(problems.join('\n'), /“acme-old” is an esbuild plugin/);
  assert.match(problems.join('\n'), /a plugin is a JavaScript Rspack plugin/);
});

test('a project loader’s emitted error fails the build and its warning does not', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function (canvas) { canvas.textContent = "ACME_BUTTON"; }',
    'checking-loader.cjs': `module.exports = function (source) {
  const { emit } = this.getOptions();
  if (emit === 'error') this.emitError(new Error('ACME_LOADER_FAILED'));
  this.emitWarning(new Error('ACME_LOADER_WARNED'));
  return source;
};`,
  });
  const configure = emit => fs.writeFileSync(path.join(root, 'workbench.config.ts'), `export default {
  rules: [{ test: /button\\.ts$/, loader: './checking-loader.cjs', options: { emit: '${emit}' } }],
};`);
  configure('warning');
  const warned = await build(t, root, 'button.workbench.ts');
  assert.ok(Array.from(warned.outputs.values()).some(body => body.toString().includes('ACME_BUTTON')));
  configure('error');
  await assert.rejects(build(t, root, 'button.workbench.ts'), /checking-loader\.cjs on .*button\.ts: ACME_LOADER_FAILED/);
});

test('builds follow the evaluated configuration and only discovered definitions build', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas) { canvas.textContent = CONFIG_LABEL; }',
    'label.txt': 'ACME_FIRST_CONFIG',
    'workbench.config.ts': `import fs from 'node:fs'; export default { define: {
      CONFIG_LABEL: JSON.stringify(fs.readFileSync(__dirname + '/label.txt', 'utf8')) } };`,
    '.hidden/button.workbench.ts': definition('html', '../button.ts'),
    'dist/button.workbench.ts': definition('html', '../button.ts'),
  });
  const first = await build(t, root, 'button.workbench.ts');
  assert.match(first.outputs.get('preview.js').toString(), /ACME_FIRST_CONFIG/);
  fs.writeFileSync(path.join(root, 'label.txt'), 'ACME_CHANGED_CONFIG');
  const changed = await build(t, root, 'button.workbench.ts');
  assert.notEqual(changed.revision, first.revision);
  assert.match(changed.outputs.get('preview.js').toString(), /ACME_CHANGED_CONFIG/);
  assert.throws(() => new Compiler(root, { include: ['src/**'] }).declared('button.workbench.ts'), /declared/);
  assert.throws(() => new Compiler(root).declared('.hidden/button.workbench.ts'), /declared/);
  assert.throws(() => new Compiler(root).declared('dist/button.workbench.ts'), /declared/);
  fs.symlinkSync(path.join(root, 'button.workbench.ts'), path.join(root, 'linked.workbench.ts'));
  assert.throws(() => new Compiler(root).declared('linked.workbench.ts'), /declared/);
  await assert.rejects(build(t, root, 'linked.workbench.ts'), /declared/);
  fs.unlinkSync(path.join(root, 'button.ts'));
  await assert.rejects(build(t, root, 'button.workbench.ts'), /source must exist/);
});

// Reads the server's event stream until `test` accepts an event, or the deadline.
function nextEvent(url, test, timeoutMs = 30000) {
  return new Promise((resolve, reject) => {
    const request = require('node:http').get(url, response => {
      let buffer = '';
      response.on('data', chunk => {
        buffer += chunk;
        for (let at; (at = buffer.indexOf('\n\n')) > -1;) {
          const block = buffer.slice(0, at); buffer = buffer.slice(at + 2);
          const data = block.split('\n').find(line => line.startsWith('data: '));
          if (!data) continue;
          const event = JSON.parse(data.slice(6));
          if (test(event)) { clearTimeout(timer); request.destroy(); resolve(event); return; }
        }
      });
    });
    const timer = setTimeout(() => { request.destroy(); reject(new Error('No matching event')); }, timeoutMs);
    request.on('error', error => { if (!request.destroyed) reject(error); });
  });
}

test('the server serves previews from the live pipeline, pushes builds, exports runnable output and closes its worker', async t => {
  const diagnostics = [];
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\n',
    'button.workbench.ts': definition('html', './button.ts', "sizes: ['mobile'], controls: { label: { type: 'text' } },"),
    'button.ts': 'export default function(canvas, ctx) { canvas.textContent = ctx.inputs.label; }',
  });
  const running = await server.start({ root, onLog: entry => diagnostics.push(entry), capture: { capture: async () => Buffer.from('jpeg'), close: () => Promise.resolve() } });
  t.after(() => running.close());
  const base = `http://127.0.0.1:${running.port}`;
  const config = await (await fetch(base + '/_workbench/config')).json();
  assert.deepEqual(config.problems, []);
  const preview = config.catalogCollections[0].items[0];
  assert.equal(preview.src, 'button.workbench.ts');
  assert.equal(preview.workbench, true);
  const page = await fetch(base + '/button.workbench.ts?scenario=disabled');
  const html = await page.text();
  assert.match(page.headers.get('content-type'), /text\/html/);
  assert.match(html, /<head><script src="\/_workbench\/preview-compat\.js"><\/script>/);
  assert.equal((html.match(/preview-compat\.js/g) || []).length, 1);
  assert.doesNotMatch(html, /<script src="\/_workbench\/(keys|actions|scenarios)\.js"/);
  assert.match(html, /"push":true/);
  const compatibility = await fetch(base + '/_workbench/preview-compat.js');
  assert.equal(compatibility.status, 200);
  assert.equal(await compatibility.text(), COMPAT);
  // The preview module names the build's scripts; they are content-hashed and immutable.
  const modulePath = /import \{ mount \} from "([^"]+)"/.exec(html)[1];
  const scriptsOf = async () => {
    const answer = await fetch(base + modulePath);
    assert.equal(answer.status, 200);
    assert.match(answer.headers.get('content-type'), /javascript/);
    return JSON.parse(/const scripts = (\[[^\]]*\]);/.exec(await answer.text())[1]);
  };
  const scripts = await scriptsOf();
  assert.ok(scripts.length >= 2);
  const script = await fetch(base + scripts[scripts.length - 1]);
  assert.equal(script.status, 200);
  assert.match(script.headers.get('cache-control'), /immutable/);
  // An edit is built and pushed; the preview module then names new scripts.
  const pushed = nextEvent(base + '/_workbench/previews/rspack/events', event => event.type === 'built');
  await new Promise(resolve => setTimeout(resolve, 300));
  fs.writeFileSync(path.join(root, 'button.ts'), 'export default function(canvas) { canvas.textContent = "Changed"; }');
  const built = await pushed;
  assert.equal(built.profile, 'web');
  assert.deepEqual(built.errors, {});
  assert.notDeepEqual(await scriptsOf(), scripts);
  const timing = diagnostics.find(entry => entry.event === 'preview.request.completed' && entry.details.route === '/page');
  assert.ok(timing);
  assert.equal(timing.details.file, 'button.workbench.ts');
  assert.ok(timing.details.elapsedMs >= timing.details.queueMs);
  const bundle = await fetch(base + '/_workbench/export');
  assert.equal(bundle.status, 200);
  assert.ok((await bundle.arrayBuffer()).byteLength > 1000);
  fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\npreviews: false\n');
  const disabled = await (await fetch(base + '/_workbench/config')).json();
  assert.equal(disabled.catalogCollections.length, 0);
  assert.equal((await fetch(base + '/button.workbench.ts')).status, 404);
  assert.equal((await fetch(base + modulePath)).status, 404);
});

test('plugin capabilities rebuild live when an icon or a matched file is added, edited or deleted', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\n',
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': `import names from 'virtual:svg-icons-names';
import 'virtual:svg-icons-register';
const locales = import.meta.glob('./locales/*.json', { eager: true, import: 'default' });
export default function (canvas) { canvas.textContent = names.join(',') + '|' + Object.keys(locales).join(','); }`,
    'icons/check.svg': '<svg viewBox="0 0 16 16"><path d="M1 1"/></svg>',
    'locales/en.json': '{"hello":"Hello"}',
    'workbench.config.ts': `import { defineConfig, importGlob, svgSprite } from '@canonic2/workbench';
export default defineConfig({ plugins: [importGlob(), svgSprite({ dirs: 'icons', symbolId: 'icon-[name]' })] });`,
  });
  const running = await server.start({ root, capture: { close: async () => {} } });
  t.after(() => running.close());
  const base = `http://127.0.0.1:${running.port}`;
  await (await fetch(base + '/_workbench/config')).json();
  const html = await (await fetch(base + '/button.workbench.ts')).text();
  const modulePath = /import \{ mount \} from "([^"]+)"/.exec(html)[1];
  const built = async () => {
    const source = await (await fetch(base + modulePath)).text();
    const scripts = JSON.parse(/const scripts = (\[[^\]]*\]);/.exec(source)[1]);
    return (await Promise.all(scripts.map(async script => (await fetch(base + script)).text()))).join('\n');
  };
  const first = await built();
  assert.match(first, /icon-check/);
  assert.match(first, /\.\/locales\/en\.json/);
  const edit = async (change, test) => {
    const pushed = nextEvent(base + '/_workbench/previews/rspack/events', event => event.type === 'built');
    await new Promise(resolve => setTimeout(resolve, 300));
    change();
    await pushed;
    let output = await built();
    for (let attempt = 0; attempt < 50 && !test(output); attempt++) {
      await new Promise(resolve => setTimeout(resolve, 100));
      output = await built();
    }
    assert.ok(test(output), 'the rebuilt preview reflects the change');
  };
  await edit(() => fs.writeFileSync(path.join(root, 'icons/star.svg'), '<svg viewBox="0 0 8 8"><circle r="1"/></svg>'), output => /icon-star/.test(output));
  await edit(() => fs.writeFileSync(path.join(root, 'icons/check.svg'), '<svg viewBox="0 0 24 24"><path d="M2 2"/></svg>'), output => /0 0 24 24/.test(output));
  await edit(() => fs.unlinkSync(path.join(root, 'icons/check.svg')), output => !/icon-check/.test(output) && /icon-star/.test(output));
  await edit(() => fs.writeFileSync(path.join(root, 'locales/fr.json'), '{"hello":"Bonjour"}'), output => /\.\/locales\/fr\.json/.test(output));
});

test('a Vue <style src> file is scoped, bundled and rebuilt when it changes', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\n',
    'card.workbench.ts': definition('vue', './Card.vue'),
    'Card.vue': '<template><p class="card">Card</p></template><style scoped src="./styles/card.css"></style><style src="./missing-free.css"></style>',
    'styles/card.css': '.card { color: rgb(1, 2, 3); }',
    'missing-free.css': '.acme-global { margin: 0; }',
  });
  // Portable: the file is compiled like an inline block and is a file of the build.
  const portable = await build(t, root, 'card.workbench.ts');
  const css = Array.from(portable.outputs).filter(([name]) => name.endsWith('.css')).map(([, body]) => body.toString()).join('\n');
  assert.match(css, /\.card\[data-v-[0-9a-f]+\]\s*\{\s*color: rgb\(1, 2, 3\)/);
  assert.match(css, /\.acme-global/);
  assert.ok(portable.localFiles.includes(path.join(root, 'styles/card.css')));
  // Live: an edit to the file alone rebuilds the component's stylesheet.
  const running = await server.start({ root, capture: { close: async () => {} } });
  t.after(() => running.close());
  const origin = `http://127.0.0.1:${running.port}`;
  const html = await (await fetch(origin + '/card.workbench.ts')).text();
  const modulePath = /import \{ mount \} from "([^"]+)"/.exec(html)[1];
  const stylesheets = async () => {
    const source = await (await fetch(origin + modulePath)).text();
    const styles = JSON.parse(/for \(const style of (\[[^\n]*\])\) \{/.exec(source)[1]);
    return (await Promise.all(styles.map(async style => (await fetch(new URL(style.href, origin))).text()))).join('\n');
  };
  assert.match(await stylesheets(), /rgb\(1, 2, 3\)/);
  const pushed = nextEvent(origin + '/_workbench/previews/rspack/events', event => event.type === 'built');
  await new Promise(resolve => setTimeout(resolve, 300));
  fs.writeFileSync(path.join(root, 'styles/card.css'), '.card { color: rgb(4, 5, 6); }');
  await pushed;
  let live = await stylesheets();
  for (let attempt = 0; attempt < 50 && !/rgb\(4, 5, 6\)/.test(live); attempt++) {
    await new Promise(resolve => setTimeout(resolve, 100));
    live = await stylesheets();
  }
  assert.match(live, /\.card\[data-v-[0-9a-f]+\]\s*\{\s*color: rgb\(4, 5, 6\)/);
  // A src that names no file, or a preprocessor's file, explains itself.
  fs.writeFileSync(path.join(root, 'Card.vue'), '<template><p>Card</p></template><style src="./styles/absent.css"></style>');
  await assert.rejects(build(t, root, 'card.workbench.ts'), /Vue style src not found: \.\/styles\/absent\.css/);
  fs.writeFileSync(path.join(root, 'Card.vue'), '<template><p>Card</p></template><style src="./styles/card.scss"></style>');
  await assert.rejects(build(t, root, 'card.workbench.ts'), /Vue style src must name a \.css file/);
});

test('a Vue template edit re-renders in place and keeps state; a script edit re-creates the component', {
  skip: !findChrome() && 'Chrome is required to apply hot updates',
}, async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\n',
    'counter.workbench.ts': definition('vue', './Counter.vue'),
    'Counter.vue': '<script setup lang="ts">import { ref } from "vue"; const count = ref(0);</script><template><button @click="count++">Count {{ count }}</button></template>',
  });
  const running = await server.start({ root, capture: { close: async () => {} } });
  t.after(() => running.close());
  const browser = new Browser();
  t.after(() => browser.close());
  await browser.launch();
  const page = await browser.createTarget();
  await page.navigate(`http://127.0.0.1:${running.port}/counter.workbench.ts`);
  const text = () => page.evaluate('document.querySelector("button")?.textContent || ""');
  const settle = async expected => {
    let seen = '';
    for (let attempt = 0; attempt < 300 && seen !== expected; attempt++) {
      seen = await text();
      if (seen !== expected) await new Promise(resolve => setTimeout(resolve, 50));
    }
    assert.equal(seen, expected);
  };
  await settle('Count 0');
  await page.evaluate('for (let click = 0; click < 7; click++) document.querySelector("button").click()');
  await settle('Count 7');
  fs.writeFileSync(path.join(root, 'Counter.vue'), '<script setup lang="ts">import { ref } from "vue"; const count = ref(0);</script><template><button @click="count++">Total {{ count }}</button></template>');
  await settle('Total 7');
  fs.writeFileSync(path.join(root, 'Counter.vue'), '<script setup lang="ts">import { ref } from "vue"; const count = ref(0); const label = "Again";</script><template><button @click="count++">{{ label }} {{ count }}</button></template>');
  await settle('Again 0');
});

test('a file read from outside the project root rebuilds the preview when it changes', async t => {
  // A monorepo app whose preview imports a sibling package.
  const base = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-monorepo-'));
  t.after(() => fs.rmSync(base, { recursive: true, force: true }));
  const root = path.join(base, 'apps/web');
  const label = path.join(base, 'packages/ui/label.js');
  fs.mkdirSync(root, { recursive: true });
  fs.mkdirSync(path.dirname(label), { recursive: true });
  fs.symlinkSync(path.join(__dirname, '../../node_modules'), path.join(root, 'node_modules'), 'junction');
  fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\n');
  fs.writeFileSync(path.join(root, 'button.workbench.ts'), definition('html', './button.ts'));
  fs.writeFileSync(path.join(root, 'button.ts'), `import { label } from '../../packages/ui/label.js';
export default function (canvas) { canvas.textContent = label; }`);
  fs.writeFileSync(label, 'export const label = "ACME_UI_FIRST";');
  const running = await server.start({ root, capture: { close: async () => {} } });
  t.after(() => running.close());
  const origin = `http://127.0.0.1:${running.port}`;
  const html = await (await fetch(origin + '/button.workbench.ts')).text();
  const modulePath = /import \{ mount \} from "([^"]+)"/.exec(html)[1];
  const built = async () => {
    const source = await (await fetch(origin + modulePath)).text();
    const scripts = JSON.parse(/const scripts = (\[[^\]]*\]);/.exec(source)[1]);
    return (await Promise.all(scripts.map(async script => (await fetch(origin + script)).text()))).join('\n');
  };
  assert.ok((await built()).includes('ACME_UI_FIRST'));
  const pushed = nextEvent(origin + '/_workbench/previews/rspack/events', event => event.type === 'built');
  await new Promise(resolve => setTimeout(resolve, 300));
  fs.writeFileSync(label, 'export const label = "ACME_UI_SECOND";');
  await pushed;
  let output = await built();
  for (let attempt = 0; attempt < 50 && !output.includes('ACME_UI_SECOND'); attempt++) {
    await new Promise(resolve => setTimeout(resolve, 100));
    output = await built();
  }
  assert.ok(output.includes('ACME_UI_SECOND'), 'the rebuilt preview reads the edited package');
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
      assert.equal(config.catalogCollections.length, 0);
    } finally { await running.close(); }
  }
});

test('manual preview placement receives definition scenarios and stays unique', () => {
  const original = [{ name: 'Authored', items: [{ label: 'My button', src: 'button.workbench.ts', implementations: { storybook: { title: 'Button' } } }] }];
  const imported = [{ name: 'Components', items: [{ label: 'Button', src: 'button.workbench.ts', workbench: true, scenarios: [{ id: 'default', label: 'Default' }] }] }];
  const merged = manifest.mergeCollections(original, imported);
  assert.equal(merged.length, 1);
  assert.equal(merged[0].items[0].label, 'My button');
  assert.equal(merged[0].items[0].workbench, true);
  assert.ok(merged[0].items[0].implementations.storybook);
  assert.equal(original[0].items[0].workbench, undefined);
});

test('a Workbench implementation maps a design to managed preview scenarios', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\nimplementations:\n  built:\n    kind: workbench\ncollections:\n  - name: Pages\n    items:\n      - label: Design\n        src: design.html\n        implementations:\n          built: components/button\n',
    'design.html': '<button>Design</button>',
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas, ctx) { canvas.textContent = ctx.inputs.label; }',
  });
  const loaded = [];
  const running = await server.start({ root, capture: {
    capture: (base, payload) => { loaded.push(payload.url); return Buffer.from('reference jpeg'); },
    close: () => Promise.resolve(),
  } });
  try {
    const config = await running.config();
    assert.deepEqual(config.problems, []);
    assert.equal(config.collections[0].items[0].implementations.built.scenarios.disabled, '/button.workbench.ts?scenario=disabled');
    assert.equal(config.pages['design.html'].code[0].relative, 'button.ts');
    // An export in the Workbench lens captures the managed preview's scenarios.
    const base = `http://127.0.0.1:${running.port}`;
    const started = await (await fetch(base + '/_workbench/export', { method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ format: 'zip', scope: 'pages', pages: ['design.html'], lens: 'built' }) })).json();
    let status = started;
    while (status.status === 'running') {
      await new Promise(resolve => setTimeout(resolve, 25));
      status = await (await fetch(base + '/_workbench/export?job=' + started.id)).json();
    }
    assert.equal(status.status, 'complete', JSON.stringify(status));
    assert.ok(loaded.some(url => url.includes('/button.workbench.ts?scenario=default')), loaded.join(' '));
  } finally { await running.close(); }
});

test('the project environment applies to every preview, or per adapter name', async t => {
  const files = (config) => ({
    'workbench.config.ts': `export default ${config};`,
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas, ctx) { canvas.textContent = ctx.inputs.label; }',
    'environment.ts': 'export function setup() { document.title = "ACME_PROJECT_ENVIRONMENT"; }',
  });
  const compiled = async config => (await build(t, fixture(t, files(config)), 'button.workbench.ts')).outputs.get('preview.js').toString();
  assert.match(await compiled(`{ environment: './environment.ts' }`), /ACME_PROJECT_ENVIRONMENT/);
  assert.match(await compiled(`{ environment: { html: './environment.ts' } }`), /ACME_PROJECT_ENVIRONMENT/);
  assert.doesNotMatch(await compiled(`{ environment: { react: './environment.ts' } }`), /ACME_PROJECT_ENVIRONMENT/);
  await assert.rejects(build(t, fixture(t, files(`{ environment: './missing.ts' }`)), 'button.workbench.ts'), /must be a file inside the project/);
});

test('preview links are validated and name previews and scenarios that exist', async t => {
  const page = (id, links) => `import { definePreview } from '@canonic2/workbench';
export default definePreview({ id: '${id}', adapter: 'html', source: { entry: './page.ts' }, scenarios: { default: {}, sent: {} }, links: ${links} });`;
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\n',
    'page.ts': 'export default function(canvas) { canvas.textContent = "Acme"; }',
    'start.workbench.ts': page('pages/start', `{ '/next/': 'pages/next', '/reset/': { scenario: 'sent' }, '/gone/': 'pages/gone', '/lost/': { preview: 'pages/next', scenario: 'lost' } }`),
    'next.workbench.ts': page('pages/next', `{ '/': 'pages/start' }`),
    'broken.workbench.ts': page('pages/broken', `{ '/next/': { label: 'Next' } }`),
  });
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  try {
    const config = await running.config();
    assert.deepEqual(config.pages['start.workbench.ts'].preview.links['/reset/'], { preview: 'pages/start', scenario: 'sent' });
    assert.equal(config.problems.length, 3);
    assert.match(config.problems[0], /^broken\.workbench\.ts: .*link \/next\/ must name a preview ID, or \{ preview, scenario \}\.$/);
    assert.deepEqual(config.problems.slice(1), [
      'start.workbench.ts: link /gone/ names unknown Workbench preview “pages/gone”.',
      'start.workbench.ts: link /lost/ names unknown scenario “lost” of pages/next.',
    ]);
  } finally { await running.close(); }
});


test('authoring declarations infer input types and reject invalid scenario inputs', t => {
  const cp = require('node:child_process');
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas: HTMLElement) { canvas.textContent = "Hello"; }',
    'tsconfig.json': JSON.stringify({ compilerOptions: { strict: true, noEmit: true, target: 'ES2022', module: 'ESNext', moduleResolution: 'Bundler', types: [] }, include: ['*.ts'] }),
  });
  cp.execFileSync(server.binaryPath(), ['init', root, '--product', path.resolve(__dirname, '../..')]);
  const tsc = path.join(path.dirname(require.resolve('@typescript/native')), '..', 'bin', 'tsc');
  const check = () => cp.spawnSync(process.execPath, [tsc, '-p', root], { encoding: 'utf8' });
  const valid = check();
  assert.equal(valid.status, 0, valid.stdout + valid.stderr);
  fs.writeFileSync(path.join(root, 'button.workbench.ts'), definition('html', './button.ts').replace("label: 'Disabled'", 'label: 123'));
  assert.notEqual(check().status, 0);
});

test('the published package types definitions in installed projects', t => {
  const cp = require('node:child_process');
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-preview-package-'));
  t.after(() => fs.rmSync(work, { recursive: true, force: true }));
  const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
  const [packed] = JSON.parse(cp.execFileSync(npm, ['pack', '--json', '--pack-destination', work], { cwd: path.join(__dirname, '../../sdk'), encoding: 'utf8', shell: process.platform === 'win32' }));
  assert.deepEqual(packed.files.map(file => file.path).sort(), ['LICENSE', 'README.md', 'api.d.ts', 'api.js', 'package.json']);
  const tsc = path.join(path.dirname(require.resolve('@typescript/native')), '..', 'bin', 'tsc');
  for (const [type, resolution] of [['module', 'Bundler'], ['module', 'NodeNext'], ['commonjs', 'NodeNext']]) {
    const root = path.join(work, type + '-' + resolution);
    const installed = path.join(root, 'node_modules', '@canonic2', 'workbench');
    fs.mkdirSync(installed, { recursive: true });
    require('tar').x({ file: path.join(work, packed.filename), cwd: installed, strip: 1, sync: true });
    const module = resolution === 'NodeNext' ? 'NodeNext' : 'ESNext';
    fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify({ type }));
    fs.writeFileSync(path.join(root, 'tsconfig.json'), JSON.stringify({ compilerOptions: { strict: true, noEmit: true, target: 'ES2022', module, moduleResolution: resolution, types: [] }, include: ['*.ts'] }));
    fs.writeFileSync(path.join(root, 'button.ts'), 'export default function(canvas: HTMLElement) { canvas.textContent = "Hello"; }');
    fs.writeFileSync(path.join(root, 'button.workbench.ts'), definition('html', './button.ts'));
    const check = () => cp.spawnSync(process.execPath, [tsc, '-p', root], { encoding: 'utf8' });
    const valid = check();
    assert.equal(valid.status, 0, type + ' ' + resolution + ': ' + valid.stdout + valid.stderr);
    fs.writeFileSync(path.join(root, 'button.workbench.ts'), definition('html', './button.ts').replace("label: 'Disabled'", 'label: 123'));
    assert.match(check().stdout, /button\.workbench\.ts.*TS2322/);
  }
});

test('the portable build includes the interactive viewer and shared controls while retaining successful previews', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts', "controls: { label: { type: 'text' } }"),
    'button.ts': 'export default function(canvas, context) { canvas.textContent = context.inputs.label; }',
    'broken.workbench.ts': "export default { id: 'broken', adapter: 'unknown', source: { entry: './button.ts' } }",
  });
  fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\n');
  const output = path.join(root, 'output');
  const result = compilerFixture.build(root, output);
  assert.notEqual(result.status, 0, 'a broken preview is reported');
  assert.match(result.stderr, /broken\.workbench\.ts/);
  const contents = name => fs.readFileSync(path.join(output, name));
  const catalog = JSON.parse(contents('workbench.json'));
  assert.equal(catalog.name, 'Acme');
  assert.equal(catalog.previews.length, 1);
  assert.equal(catalog.warnings.length, 1);
  assert.deepEqual(catalog.previews[0].scenarios.map(scenario => scenario.id), ['default', 'disabled']);
  assert.equal(catalog.previews[0].files, undefined);
  assert.ok(fs.existsSync(path.join(output, catalog.previews[0].directory.replace(/^browser\//, ''), 'index.html')));
  for (const file of ['ui/src/app/viewer/viewer.js', 'ui/src/modules/preview-controls/index.js',
    'ui/src/views/top-bar/preview-bindings.js', 'ui/src/components/preview-controls/element.js',
    'ui/src/components/preview-controls/styles.js', 'ui/src/components/preview-controls/register.js',
    'node_modules/@canonic2/lattice/src/index.js', 'node_modules/@canonic2/lattice/src/report.js',
    'node_modules/@canonic2/lattice/src/modules/store/store.js', 'node_modules/@canonic2/lattice/src/modules/store/shallow.js',
    'node_modules/@canonic2/lattice/src/modules/elements/elements.js'])
    assert.deepEqual(contents(file), fs.readFileSync(path.join(__dirname, '../..', file)), file);
  assert.deepEqual(contents('CANONIC-LICENSE.txt'), fs.readFileSync(path.join(__dirname, "../../LICENSE")));
  if (findChrome()) {
    const listener = require('node:http').createServer((request, response) => {
      const file = path.resolve(output, '.' + new URL(request.url, 'http://example.com').pathname.replace(/\/$/, '/index.html'));
      if (!file.startsWith(output + path.sep)) { response.writeHead(404).end(); return; }
      try {
        response.setHeader('Content-Type', file.endsWith('.js') ? 'text/javascript' : file.endsWith('.css') ? 'text/css' : 'text/html');
        response.end(fs.readFileSync(file));
      } catch { response.writeHead(404).end(); }
    });
    await new Promise(resolve => listener.listen(0, '127.0.0.1', resolve));
    const browser = new Browser();
    t.after(async () => { await browser.close(); await new Promise(resolve => listener.close(resolve)); });
    await browser.launch(); const target = await browser.createTarget();
    const errors = [];
    target.on('Runtime.exceptionThrown', event => errors.push(event.exceptionDetails.text));
    await target.navigate('http://127.0.0.1:' + listener.address().port + '/');
    async function until(expression) {
      const deadline = Date.now() + 10000;
      while (!await target.evaluate(expression)) {
        if (Date.now() > deadline) throw Error('Portable controls did not settle: ' + expression + ' ' + errors.join('\n'));
        await new Promise(resolve => setTimeout(resolve, 20));
      }
    }
    await until('!document.getElementById("previewControls").hidden');
    await target.evaluate('document.getElementById("previewControls").click()');
    await until('!document.getElementById("previewPanel").hidden && !!document.getElementById("previewPanel").shadowRoot.querySelector("input")');
    await target.evaluate(`const input=document.getElementById('previewPanel').shadowRoot.querySelector('input');input.value='Portable edit';input.dispatchEvent(new Event('change',{bubbles:true}));`);
    await until('document.getElementById("previewFrame").contentDocument.body.textContent.includes("Portable edit")');
    await target.evaluate('document.getElementById("previewPanel").shadowRoot.querySelector("button").click()');
    await until('document.getElementById("previewFrame").contentDocument.body.textContent.includes("Continue")');
    assert.deepEqual(errors, []);
  }
});

test('portable export compiles only selected previews and filters declared variants', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts'),
    'other.workbench.ts': definition('html', './other.ts').replace('components/button', 'components/other'),
    'button.ts': 'export default function(canvas) { canvas.textContent = "Selected"; }',
    'other.ts': 'export default function(canvas) { canvas.textContent = "Unselected"; }',
  });
  fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\n');
  const running = await server.start({ root, isTrusted: true, capture: { close() {} } });
  t.after(() => running.close());
  const base = 'http://127.0.0.1:' + running.port;
  await (await fetch(base + '/_workbench/config')).json();
  const started = await fetch(base + '/_workbench/export', { method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ format: 'browser', scope: 'pages', pages: ['button.workbench.ts'], variants: 'custom', scenarios: ['disabled'], sizes: ['mobile'] }) });
  const job = await started.json();
  assert.equal(started.status, 202, JSON.stringify(job));
  let status = job;
  while (status.status === 'running') {
    await new Promise(resolve => setTimeout(resolve, 100));
    status = await (await fetch(base + '/_workbench/export?job=' + job.id)).json();
  }
  assert.equal(status.status, 'complete', JSON.stringify(status));
  const archive = Buffer.from(await (await fetch(base + '/_workbench/export?job=' + job.id + '&download=1')).arrayBuffer());
  const at = archive.indexOf('{\n  "version": 1');
  assert.ok(at > 0, 'the archive holds the viewer\'s catalog');
  const catalog = JSON.parse(archive.subarray(at, archive.indexOf('\n}', at) + 2).toString('utf8'));
  assert.deepEqual(catalog.previews.map(p => p.file), ['button.workbench.ts']);
  assert.deepEqual(catalog.previews[0].scenarios.map(s => s.id), ['disabled']);
  assert.deepEqual(catalog.previews[0].sizes.map(s => s.key), ['mobile']);
});

test('CLI builds a static viewer without starting a server and refuses to overwrite existing content', t => {
  const cp = require('node:child_process');
  const root = fixture(t, { 'workbench.yaml': 'name: Acme\n', 'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas) { canvas.textContent = "Hello"; }' });
  const output = path.join(root, 'output');
  const result = compilerFixture.build(root, output);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(JSON.parse(fs.readFileSync(path.join(output, 'workbench.json'))).name, 'Acme');
  assert.ok(fs.existsSync(path.join(output, 'index.html')));
  fs.writeFileSync(path.join(output, 'preserve.txt'), 'User content');
  const repeat = compilerFixture.build(root, output);
  assert.notEqual(repeat.status, 0);
  assert.match(repeat.stderr, /must be empty/);
  assert.equal(fs.readFileSync(path.join(output, 'preserve.txt'), 'utf8'), 'User content');
});
