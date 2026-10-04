const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { Compiler } = require('./preview/compiler.cjs');
const server = require('./server');
const portable = require('./preview/portable.cjs');

function fixture(t, files = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-astro-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.symlinkSync(process.env.CANONIC_ASTRO_TEST_MODULES || path.join(__dirname, 'node_modules'), path.join(root, 'node_modules'), 'junction');
  const defaults = {
    'workbench.yaml': 'name: Acme\n',
    'button.workbench.ts': `import { definePreview } from '@canonic/workbench';
      export default definePreview({ id: 'components/button', adapter: 'astro', source: { entry: './Button.astro' },
        inputs: { label: 'Continue', disabled: false }, controls: { label: { type: 'text' } },
        states: { default: {}, disabled: { inputs: { disabled: true } }, alternate: { source: { entry: './Alternate.astro' } } } });`,
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

test('Astro renders props, states, slots and alternate sources, packages client assets and keeps frontmatter in Node', async t => {
  const root = fixture(t);
  const compiler = new Compiler(root);
  const built = await compiler.compile('button.workbench.ts', false);
  assert.match(built.rendered.default.html, /<article[^>]*><button/);
  assert.match(built.rendered.default.html, />Continue<\/button>/);
  assert.doesNotMatch(built.rendered.default.html, / disabled/);
  assert.match(built.rendered.disabled.html, / disabled/);
  assert.match(built.rendered.alternate.html, /<strong>Continue<\/strong>/);
  assert.ok(Array.from(built.outputs.keys()).some(name => name.endsWith('.svg')));
  assert.match(built.rendered.default.html, /<script type="module" src="\.\/assets\//);
  const css = Array.from(built.outputs.values()).map(body => body.toString()).join('\n');
  assert.match(css, /purple/);
  assert.match(css, /astro-/);
  assert.doesNotMatch(css, /SERVER_ONLY_SENTINEL|node:fs/);
  assert.ok(built.localFiles.includes(path.join(root, 'server.ts')));
  assert.ok(built.localFiles.includes(path.join(root, 'nested/Frame.astro')));
  assert.ok(built.localFiles.includes(path.join(root, 'client.ts')));
  assert.ok(built.packages.some(pkg => pkg.name === 'astro'));
  fs.writeFileSync(path.join(root, 'nested/Frame.astro'), '<section><slot /></section>');
  const changed = await compiler.compile('button.workbench.ts', false);
  assert.notEqual(changed.revision, built.revision);
  assert.match(changed.rendered.default.html, /<section>/);
});

test('Astro frontmatter fetches are answered by the state request mocks', async t => {
  const root = fixture(t, {
    'plans.workbench.ts': `import { definePreview } from '@canonic/workbench';
      export default definePreview({ id: 'pages/plans', adapter: 'astro', source: { entry: './Plans.astro' },
        requests: { 'GET /api/plans': { body: [{ name: 'Team' }, { name: 'Business' }] } },
        states: { default: {}, empty: { requests: { 'GET /api/plans': { body: [] } } },
          failed: { requests: { 'GET /api/plans': { status: 500 } } } } });`,
    'Plans.astro': `---
      const response = await fetch('/api/plans');
      const plans = response.ok ? await response.json() : null;
      ---
      {plans ? <ul>{plans.map(plan => <li>{plan.name}</li>)}</ul> : <p>Plans could not load</p>}
      {plans && !plans.length && <p>No plans</p>}`,
  });
  const original = globalThis.fetch;
  const built = await new Compiler(root).compile('plans.workbench.ts', false);
  assert.equal(globalThis.fetch, original);
  assert.match(built.rendered.default.html, /<li>Team<\/li><li>Business<\/li>/);
  assert.match(built.rendered.empty.html, /No plans/);
  assert.match(built.rendered.failed.html, /Plans could not load/);
});

test('live Astro input renders have their own served assets and validate state and input requests', async t => {
  const root = fixture(t);
  const running = await server.start({ root, capture: { close: async () => {} } });
  t.after(() => running.close());
  const base = 'http://127.0.0.1:' + running.port;
  const page = await fetch(base + '/button.workbench.ts?state=disabled');
  assert.equal(page.status, 200);
  const render = async (state, inputs) => fetch(base + '/_workbench/previews/render?file=button.workbench.ts&state=' + state + '&inputs=' + encodeURIComponent(JSON.stringify(inputs)));
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

test('portable Astro exports retain all reference states without requiring a rendering server', async t => {
  const root = fixture(t);
  const result = await portable.create(new Compiler(root), { name: 'Acme' });
  assert.deepEqual(result.warnings, []);
  assert.equal(result.previews.length, 1);
  assert.deepEqual(result.previews[0].controls, {});
  const js = result.files.filter(file => file.path.endsWith('.js')).map(file => Buffer.from(file.data, 'base64').toString()).join('\n');
  assert.doesNotMatch(js, /SERVER_ONLY_SENTINEL|node:fs/);
  assert.match(js, /renderUrl: null/);
});

test('Astro compilation reports missing framework dependencies and unsupported application integrations', async t => {
  const root = fixture(t, { 'Button.astro': '---\nimport { getCollection } from "astro:content";\nconst posts = await getCollection("posts");\n---\n{posts.length}' });
  await assert.rejects(new Compiler(root).compile('button.workbench.ts'), /astro:content needs the Astro application pipeline/);
  fs.writeFileSync(path.join(root, 'Button.astro'), '<style lang="scss">$color:red; button{color:$color}</style>');
  await assert.rejects(new Compiler(root).compile('button.workbench.ts'), /style preprocessors need a compiler plugin/);
  fs.writeFileSync(path.join(root, 'Button.astro'), '---\nimport Card from "./card.tsx";\n---\n<Card client:load />');
  fs.writeFileSync(path.join(root, 'card.tsx'), 'export default function Card() { return null; }');
  await assert.rejects(new Compiler(root).compile('button.workbench.ts'), /client\/server islands need application integrations/);
  fs.unlinkSync(path.join(root, 'node_modules'));
  await assert.rejects(new Compiler(root).compile('button.workbench.ts'), /needs astro installed/);
});

test('raw and URL imports preserve assets from a nested package public directory', async t => {
  const root = fixture(t, {
    'button.workbench.ts': `export default { id: 'acme/card', adapter: 'astro', source: { entry: './packages/acme/Card.astro' } };`,
    'packages/acme/package.json': '{"name":"acme"}',
    'packages/acme/Card.astro': `---\nimport svg from './icon.svg?raw'; import url from './icon.svg?url';\n---\n<div set:html={svg} /><img src={url} /><img src="/public.svg" />`,
    'packages/acme/icon.svg': '<svg xmlns="http://www.w3.org/2000/svg"/>',
    'packages/acme/public/public.svg': '<svg xmlns="http://www.w3.org/2000/svg"/>',
  });
  const built = await new Compiler(root).compile('button.workbench.ts', false);
  assert.match(built.rendered.default.html, /<svg/);
  assert.match(built.rendered.default.html, /<img src="\.\/assets\/[^"\s]+\.svg"/);
  assert.ok(built.localFiles.includes(path.join(root, 'packages/acme/icon.svg')));
  assert.ok(built.localFiles.includes(path.join(root, 'packages/acme/public/public.svg')));
});

test('Astro frontmatter honors Workbench aliases and environment defines', async t => {
  const root = fixture(t, {
    'Button.astro': `---\nimport { label } from '#acme';\n---\n<p>{label} {import.meta.env.SITE}</p>`,
    'fixtures/message.ts': 'export const label = "Acme";',
    'workbench.config.ts': `export default { aliases: { '#acme': './fixtures/message.ts' }, define: { 'import.meta.env.SITE': JSON.stringify('https://example.com/acme/') } };`,
  });
  const built = await new Compiler(root).compile('button.workbench.ts', false);
  assert.match(built.rendered.default.html, /<p>Acme https:\/\/example.com\/acme\/<\/p>/);
  assert.ok(built.localFiles.includes(path.join(root, 'fixtures/message.ts')));
});
