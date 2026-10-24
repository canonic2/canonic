const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { mockFetches } = require('./preview/compiler.cjs');
const server = require('./server');
const compilerFixture = require('./compiler-fixture.cjs');

// Portable builds: the Rspack production compiler the export and the CLI use.
// A compiler for the fixture, started on the first build it is asked for.
function builder(t, root) {
  let opened = null;
  const compiler = () => opened || (opened = compilerFixture.open(root).then(started => { t.after(() => started.close()); return started; }));
  return {
    compile: async file => (await compiler()).compile(file),
    compileDocs: async request => (await compiler()).compileDocs(request),
  };
}
async function build(t, root, file) {
  return builder(t, root).compile(file);
}

/* The portable build of `root`, by its browser/ names. */
function portableFiles(root) {
  const output = path.join(root, '.workbench-test-output');
  const result = compilerFixture.build(root, output);
  const out = new Map();
  const walk = folder => {
    for (const entry of fs.readdirSync(folder, { withFileTypes: true })) {
      const file = path.join(folder, entry.name);
      if (entry.isDirectory()) walk(file);
      else out.set('browser/' + path.relative(output, file).split(path.sep).join('/'), fs.readFileSync(file));
    }
  };
  walk(output);
  return { result, files: out };
}

function fixture(t, files = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-astro-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.symlinkSync(process.env.CANONIC_ASTRO_TEST_MODULES || path.join(__dirname, "../../node_modules"), path.join(root, 'node_modules'), 'junction');
  const defaults = {
    'workbench.yaml': 'name: Acme\n',
    'button.workbench.ts': `import { definePreview } from '@canonic2/workbench';
      export default definePreview({ id: 'components/button', adapter: 'astro', source: { entry: './Button.astro' },
        inputs: { label: 'Continue', disabled: false }, controls: { label: { type: 'text' } },
        scenarios: { default: {}, disabled: { inputs: { disabled: true } }, alternate: { source: { entry: './Alternate.astro' } } } });`,
    'Button.astro': `---
      import Frame from './nested/Frame.astro';
      import './button.css';
      import { secret } from './server.ts';
      const { label, disabled } = Astro.props;
      ---
      <Frame><button disabled={disabled} data-secret-length={secret.length}>{label}</button></Frame>
      <style>button { color: purple; }</style>
      <script>import { decorate } from './client.ts'; decorate();</script>`,
    'Alternate.astro': '---\nconst { label } = Astro.props;\n---\n<strong>{label}</strong>',
    'nested/Frame.astro': '<article><slot /></article><img src="/dot.svg" /><style>article { border: 1px solid blue; }</style>',
    'button.css': 'button { background: url(./background.svg); }',
    'public/dot.svg': '<svg xmlns="http://www.w3.org/2000/svg"/>',
    'background.svg': '<svg xmlns="http://www.w3.org/2000/svg"/>',
    'server.ts': `import fs from 'node:fs'; export const secret = 'SERVER_ONLY_SENTINEL'; if (!fs.existsSync(new URL('./server.ts', import.meta.url))) throw new Error('Lost source URL');`,
    'client.ts': 'export function decorate() { document.querySelector("button").dataset.decorated = "yes"; }',
  };
  for (const [name, body] of Object.entries({ ...defaults, ...files })) {
    fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true });
    fs.writeFileSync(path.join(root, name), body);
  }
  return root;
}

test('Astro renders props, scenarios, slots and alternate sources, packages client assets and keeps frontmatter in Node', async t => {
  const root = fixture(t);
  const built = await build(t, root, 'button.workbench.ts');
  assert.match(built.rendered.default.html, /<article[^>]*><button/);
  assert.match(built.rendered.default.html, />Continue<\/button>/);
  assert.doesNotMatch(built.rendered.default.html, / disabled/);
  assert.match(built.rendered.disabled.html, / disabled/);
  assert.match(built.rendered.alternate.html, /<strong>Continue<\/strong>/);
  assert.ok(Array.from(built.outputs.keys()).some(name => name.endsWith('.svg')));
  // The client script is bundled; it runs when the page renders.
  assert.ok(Array.from(built.outputs).some(([name, body]) => name.endsWith('.js') && body.includes(Buffer.from('decorated'))));
  const css = Array.from(built.outputs.values()).map(body => body.toString()).join('\n');
  assert.match(css, /purple/);
  assert.match(css, /astro-/);
  assert.doesNotMatch(css, /SERVER_ONLY_SENTINEL|node:fs/);
  assert.ok(built.localFiles.includes(path.join(root, 'server.ts')));
  assert.ok(built.localFiles.includes(path.join(root, 'nested/Frame.astro')));
  assert.ok(built.localFiles.includes(path.join(root, 'client.ts')));
  assert.ok(built.packages.some(pkg => pkg.name === 'astro'));
  fs.writeFileSync(path.join(root, 'nested/Frame.astro'), '<section><slot /></section>');
  const changed = await build(t, root, 'button.workbench.ts');
  assert.notEqual(changed.revision, built.revision);
  assert.match(changed.rendered.default.html, /<section>/);
});

test('Astro frontmatter fetches are answered by the scenario request mocks', async t => {
  const root = fixture(t, {
    'plans.workbench.ts': `import { definePreview } from '@canonic2/workbench';
      export default definePreview({ id: 'pages/plans', adapter: 'astro', source: { entry: './Plans.astro' },
        requests: { 'GET /api/plans': { body: [{ name: 'Team' }, { name: 'Business' }] } },
        scenarios: { default: {}, empty: { requests: { 'GET /api/plans': { body: [] } } },
          failed: { requests: { 'GET /api/plans': { status: 500 } } } } });`,
    'Plans.astro': `---
      const response = await fetch('/api/plans');
      const plans = response.ok ? await response.json() : null;
      ---
      {plans ? <ul>{plans.map(plan => <li>{plan.name}</li>)}</ul> : <p>Plans could not load</p>}
      {plans && !plans.length && <p>No plans</p>}`,
  });
  const original = globalThis.fetch;
  const built = await build(t, root, 'plans.workbench.ts');
  assert.equal(globalThis.fetch, original);
  assert.match(built.rendered.default.html, /<li>Team<\/li><li>Business<\/li>/);
  assert.match(built.rendered.empty.html, /No plans/);
  assert.match(built.rendered.failed.html, /Plans could not load/);
});

test('concurrent Astro renders isolate scenario request mocks and restore fetch after failure', async t => {
  const original = globalThis.fetch;
  t.after(() => { globalThis.fetch = original; });
  const definition = { id: 'pages/guide', scenarios: {
    overview: { requests: { 'GET /api/message': { body: { text: 'Overview' } } } },
    details: { requests: { 'GET /api/message': { body: { text: 'Details' } } } },
    plain: {},
  } };
  let entered;
  const started = new Promise(resolve => { entered = resolve; });
  let releaseFirst, releaseSecond;
  const firstGate = new Promise(resolve => { releaseFirst = resolve; });
  const secondGate = new Promise(resolve => { releaseSecond = resolve; });
  const message = async () => (await (await fetch('/api/message')).json()).text;
  const first = mockFetches(definition, 'overview', {}, async () => { entered(); await firstGate; return message(); });
  await started;
  const second = mockFetches(definition, 'details', {}, async () => { await secondGate; return message(); });
  const plain = mockFetches(definition, 'plain', {}, async () => {
    assert.equal(globalThis.fetch, original, 'a render without mocks cannot inherit another render’s fetch');
    return 'Plain';
  });
  await new Promise(resolve => setImmediate(resolve));
  releaseFirst(); releaseSecond();
  assert.deepEqual(await Promise.all([first, second, plain]), ['Overview', 'Details', 'Plain']);
  await assert.rejects(mockFetches(definition, 'overview', {}, async () => { throw new Error('render failed'); }), /render failed/);
  assert.equal(await mockFetches(definition, 'details', {}, message), 'Details');
  assert.equal(globalThis.fetch, original);
});

test('live Astro input renders have their own served assets and validate scenario and input requests', async t => {
  const root = fixture(t);
  const running = await server.start({ root, capture: { close: async () => {} } });
  t.after(() => running.close());
  const base = 'http://127.0.0.1:' + running.port;
  const page = await fetch(base + '/button.workbench.ts?scenario=disabled');
  assert.equal(page.status, 200);
  const render = async (scenario, inputs) => fetch(base + '/_workbench/previews/render?file=button.workbench.ts&scenario=' + scenario + '&inputs=' + encodeURIComponent(JSON.stringify(inputs)));
  const response = await render('disabled', { label: '<Edited>', disabled: false });
  assert.equal(response.status, 200);
  const answer = await response.json();
  assert.match(answer.html, /&lt;Edited&gt;/);
  assert.doesNotMatch(answer.html, / disabled/);
  const css = /href="(\.\/assets\/[^"\s]+\.css)"/.exec(answer.html)[1];
  assert.equal((await fetch(new URL(css, base + answer.base))).status, 200);
  assert.equal((await render('missing', {})).status, 500);
  assert.equal((await render('default', [])).status, 500);
  const config = await (await fetch(base + '/_workbench/config')).json();
  assert.deepEqual(config.problems, []);
});

test('Astro previews without a default scenario render their base and named request mocks', async t => {
  const root = fixture(t, {
    'button.workbench.ts': `export default { id: 'pages/guide', adapter: 'astro', source: { entry: './Guide.astro' },
      inputs: { title: 'Base' }, requests: { 'GET /api/message': { body: { text: 'Base response' } } },
      scenarios: { index: { inputs: { title: 'Overview' }, requests: { 'GET /api/message': { body: { text: 'Overview response' } } } },
        details: { inputs: { title: 'Details' } } } };`,
    'Guide.astro': `---
      const { title } = Astro.props;
      const { text } = await (await fetch('/api/message')).json();
      ---
      <h1>{title}</h1><p>{text}</p>`,
  });
  const built = await build(t, root, 'button.workbench.ts');
  assert.match(built.rendered.default.html, /<h1>Base<\/h1><p>Base response<\/p>/);
  assert.match(built.rendered.index.html, /<h1>Overview<\/h1><p>Overview response<\/p>/);
  assert.match(built.rendered.details.html, /<h1>Details<\/h1><p>Base response<\/p>/);
  const running = await server.start({ root, capture: { close: async () => {} } });
  t.after(() => running.close());
  const response = await fetch('http://127.0.0.1:' + running.port + '/_workbench/previews/render?file=button.workbench.ts&scenario=index&inputs=%7B%7D');
  assert.equal(response.status, 200);
  assert.match((await response.json()).html, /<h1>Overview<\/h1><p>Overview response<\/p>/);
});

test('portable Astro exports retain all reference scenarios without requiring a rendering server', async t => {
  const root = fixture(t);
  const { result, files } = portableFiles(root);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  const catalog = JSON.parse(files.get('browser/workbench.json'));
  assert.deepEqual(catalog.warnings, []);
  assert.equal(catalog.previews.length, 1);
  assert.deepEqual(catalog.previews[0].controls, {});
  const js = [...files].filter(([name]) => name.endsWith('.js')).map(([, body]) => body.toString()).join('\n');
  assert.doesNotMatch(js, /SERVER_ONLY_SENTINEL|node:fs/);
  assert.match(js, /renderUrl:\s*null/);
});

test('Astro compilation reports missing framework dependencies and unsupported application integrations', async t => {
  const root = fixture(t, { 'Button.astro': '---\nimport { getCollection } from "astro:content";\nconst posts = await getCollection("posts");\n---\n{posts.length}' });
  await assert.rejects(build(t, root, 'button.workbench.ts'), /astro:content needs the Astro application pipeline; use a URL lens, or stand in for it with an alias/);
  fs.writeFileSync(path.join(root, 'Button.astro'), '<style lang="scss">$color:red; button{color:$color}</style>');
  await assert.rejects(build(t, root, 'button.workbench.ts'), /style preprocessors are not supported; use plain CSS/);
  fs.writeFileSync(path.join(root, 'Button.astro'), '---\nimport Card from "./card.tsx";\n---\n<Card client:load />');
  fs.writeFileSync(path.join(root, 'card.tsx'), 'export default function Card() { return null; }');
  await assert.rejects(build(t, root, 'button.workbench.ts'), /client\/server islands need application integrations/);
  fs.unlinkSync(path.join(root, 'node_modules'));
  await assert.rejects(build(t, root, 'button.workbench.ts'), /needs astro installed/);
});

test('raw and URL imports preserve assets from a nested package public directory', async t => {
  const root = fixture(t, {
    'button.workbench.ts': `export default { id: 'acme/card', adapter: 'astro', source: { entry: './packages/acme/Card.astro' } };`,
    'packages/acme/package.json': '{"name":"acme"}',
    'packages/acme/Card.astro': `---\nimport svg from './icon.svg?raw'; import url from './icon.svg?url';\n---\n<div set:html={svg} /><img src={url} /><img src="/public.svg" />`,
    'packages/acme/icon.svg': '<svg xmlns="http://www.w3.org/2000/svg"/>',
    'packages/acme/public/public.svg': '<svg xmlns="http://www.w3.org/2000/svg"/>',
  });
  const built = await build(t, root, 'button.workbench.ts');
  assert.match(built.rendered.default.html, /<svg/);
  assert.ok(Array.from(built.outputs.keys()).filter(name => /^assets\/.+\.svg$/.test(name)).length >= 2, 'the URL import and the public file are packaged');
  assert.ok(built.localFiles.includes(path.join(root, 'packages/acme/icon.svg')));
  assert.ok(built.localFiles.includes(path.join(root, 'packages/acme/public/public.svg')));
});

test('an alias stands in for an Astro application module', async t => {
  const root = fixture(t, {
    'Button.astro': '---\nimport { getCollection } from "astro:content";\nconst posts = await getCollection("blog");\n---\n<p>{posts[0].data.title}</p>',
    'preview/content.ts': 'export async function getCollection(name: string) { return [{ id: "launch", data: { title: "Acme launch" } }]; }',
    'workbench.config.ts': "export default { aliases: { 'astro:content': './preview/content.ts' } };",
  });
  const built = await build(t, root, 'button.workbench.ts');
  assert.match(built.rendered.default.html, /<p>Acme launch<\/p>/);
});

test('Astro frontmatter honors Workbench aliases and environment defines', async t => {
  const root = fixture(t, {
    'Button.astro': `---\nimport { label } from '#acme';\n---\n<p>{label} {import.meta.env.SITE}</p>`,
    'fixtures/message.ts': 'export const label = "Acme";',
    'workbench.config.ts': `export default { aliases: { '#acme': './fixtures/message.ts' }, define: { 'import.meta.env.SITE': JSON.stringify('https://example.com/acme/') } };`,
  });
  const built = await build(t, root, 'button.workbench.ts');
  assert.match(built.rendered.default.html, /<p>Acme https:\/\/example.com\/acme\/<\/p>/);
  assert.ok(built.localFiles.includes(path.join(root, 'fixtures/message.ts')));
});

test('docs examples render Astro components from a folder, with their assets made absolute', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\nimplementations:\n  astro:\n    kind: docs\n    adapter: astro\ncollections:\n  - name: Docs\n    items:\n      - label: Guide\n        src: guide.md\n        implementations:\n          astro: examples/\n',
    'guide.md': '# Guide\n\n```example primary\n```\n',
    'examples/primary.astro': "---\nimport Frame from '../nested/Frame.astro';\nimport { secret } from '../server.ts';\n---\n<Frame><button data-secret-length={secret.length}>Continue</button></Frame>\n<style>button { color: purple; }</style>",
    'examples/plain.astro': '<p>Plain</p>',
    'one.astro': '<p>One</p>',
  });
  const compiler = builder(t, root);
  const compiled = await compiler.compileDocs({ adapter: 'astro', examples: 'examples/' });
  assert.deepEqual(compiled.examples.map(example => example.id), ['plain', 'primary']);
  const bundle = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-astro-docs-')), 'examples.mjs');
  t.after(() => fs.rmSync(path.dirname(bundle), { recursive: true, force: true }));
  fs.writeFileSync(bundle, compiled.outputs.get('examples.js'));
  const loaded = await import(require('node:url').pathToFileURL(bundle).href);
  const primary = loaded.examples.primary;
  assert.deepEqual(primary.inputs, {});
  assert.match(primary.html, /<article[^>]*><button data-secret-length="20"[^>]*>Continue<\/button><\/article>/);
  assert.doesNotMatch(primary.html, /SERVER_ONLY_SENTINEL/, 'frontmatter stays in Node');
  const assetRef = /(?:src|href)="([^"]*assets\/[^"]+)"/.exec(primary.html);
  assert.ok(assetRef, 'the rendered page references a packaged asset');
  assert.ok(assetRef[1].startsWith(require('node:url').pathToFileURL(fs.realpathSync(path.dirname(bundle))).href + '/assets/'), assetRef[1]);
  assert.ok(Array.from(compiled.outputs.keys()).some(name => name.startsWith('assets/') && name.endsWith('.svg')));
  await assert.rejects(compiler.compileDocs({ adapter: 'astro', examples: 'one.astro' }), /point the lens at a folder of \.astro files/);
  const running = await server.start({ root, capture: { close: async () => {} } });
  t.after(() => running.close());
  const base = 'http://127.0.0.1:' + running.port;
  const html = await (await fetch(base + '/guide.md')).text();
  const options = JSON.parse(/window\.__workbenchDocs=(.*?)<\/script>/.exec(html)[1]);
  const info = await (await fetch(base + options.bundle.info)).json();
  assert.equal(info.error, null);
  assert.deepEqual(info.examples, ['plain', 'primary']);
  assert.equal((await fetch(base + info.module)).status, 200, 'live Astro docs handle the renderer dependency set');
});

test('a restarted server rebuilds an Astro render edited while it was stopped', async t => {
  const root = fixture(t, {
    'button.workbench.ts': `export default { id: 'components/note', adapter: 'astro', source: { entry: './Note.astro' } };`,
    'Note.astro': '<p>ACME_FIRST_RENDER</p>',
  });
  // Every script the live preview page loads, as one text.
  const served = async () => {
    const running = await server.start({ root, capture: { close: async () => {} } });
    try {
      const base = 'http://127.0.0.1:' + running.port;
      const html = await (await fetch(base + '/button.workbench.ts?scenario=default')).text();
      const modulePath = /import \{ mount \} from "([^"]+)"/.exec(html)[1];
      const scripts = JSON.parse(/const scripts = (\[[^\]]*\]);/.exec(await (await fetch(base + modulePath)).text())[1]);
      return (await Promise.all(scripts.map(async script => (await fetch(base + script)).text()))).join('\n');
    } finally {
      await running.close();
    }
  };
  assert.ok((await served()).includes('ACME_FIRST_RENDER'));
  fs.writeFileSync(path.join(root, 'Note.astro'), '<p>ACME_SECOND_RENDER</p>');
  const restarted = await served();
  assert.ok(restarted.includes('ACME_SECOND_RENDER'), 'the restarted server serves the new render');
  assert.ok(!restarted.includes('ACME_FIRST_RENDER'), 'nothing of the old render is restored from the cache');
});

test('a live Astro docs lens builds its examples and watches what they read', async t => {
  const root = fixture(t, {
    'workbench.yaml': [
      'name: Acme',
      'implementations:',
      '  site:',
      '    kind: docs',
      '    adapter: astro',
      'collections:',
      '  - name: Components',
      '    items:',
      '      - label: Card',
      '        src: docs/card.md',
      '        implementations:',
      '          site: examples/',
      '',
    ].join('\n'),
    'docs/card.md': '# Card\n\n```example plain\n```\n',
    'examples/plain.astro': "---\nimport Frame from '../nested/Frame.astro';\n---\n<Frame><p>Plain</p></Frame>",
  });
  const running = await server.start({ root, capture: { close: async () => {} } });
  t.after(() => running.close());
  const response = await fetch('http://127.0.0.1:' + running.port + '/_workbench/docs/bundle?page=docs%2Fcard.md&lens=site');
  assert.equal(response.status, 200);
  const info = await response.json();
  assert.equal(info.error, null);
  assert.deepEqual(info.examples, ['plain']);
  assert.ok(info.module);
});
