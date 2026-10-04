import { test } from 'node:test';
import assert from 'node:assert/strict';
import { chooseLens, createDocsService, type DocsBundle, type DocsLens, type DocsPageEntry, type ListedExample } from './docs-service.ts';

const files: Record<string, string> = {
  'docs/card.md': ['# Card', '', '## Basic', '```example basic', 'caption: children', '```', '## Nested', '```example nested', '```'].join('\n'),
  'docs/card.examples.ts': ['import { Card } from "./card.tsx";', '', 'export const basic = () => <Card>Basic</Card>;', '', 'export const nested = () => (', '  <Card>Nested</Card>', ');'].join('\n'),
  'docs/native/basic.tsx': 'export default () => null;\n',
};

const web: DocsLens = { key: 'web', label: 'Web', adapter: 'react', styles: [], examples: 'docs/card.examples.ts' };
const native: DocsLens = { key: 'native', label: 'Native', adapter: 'react', styles: ['docs/native.css'], examples: 'docs/native/' };
const card: DocsPageEntry = { src: 'docs/card.md', label: 'Card', lens: 'web', lenses: [web, native] };

const listings: Record<string, ListedExample[]> = {
  web: [{ id: 'basic', file: 'docs/card.examples.ts', export: 'basic' }, { id: 'nested', file: 'docs/card.examples.ts', export: 'nested' }, { id: 'large', file: 'docs/card.examples.ts', export: 'large' }],
  native: [{ id: 'basic', file: 'docs/native/basic.tsx', export: 'default' }],
};

function service(overrides: { trusted?: boolean; failing?: string } = {}) {
  const built: string[] = [];
  const docs = createDocsService({
    blocked: () => overrides.trusted === false ? 'Examples run project code, so they appear in a trusted workspace only.' : null,
    readFile: async file => files[file] ?? null,
    listExamples: async lens => {
      if (lens.key === overrides.failing) throw new Error('Build failed: missing ./card.tsx');
      return { examples: listings[lens.key]!, problems: lens.key === 'native' ? ['Shadow.tsx: example file names must be kebab-case.'] : [] };
    },
    bundle: async (lens): Promise<DocsBundle> => {
      built.push(lens.key);
      if (lens.key === overrides.failing) throw new Error('Build failed: missing ./card.tsx');
      return { module: '/_workbench/previews/docs/' + lens.key + '/examples.js', stylesheet: lens.key === 'native' ? '/_workbench/previews/docs/native/examples.css' : null,
        revision: 'r-' + lens.key, examples: listings[lens.key]!, problems: [] };
    },
  });
  return { docs, built };
}

const optionsOf = (html: string) => JSON.parse(/window\.__workbenchDocs=(.*?)<\/script>/.exec(html)![1]!);

test('a page opens in its own lens and mounts the examples that lens has', async () => {
  const { docs, built } = service();
  const html = await docs.page(card, {}, new Set());
  assert.deepEqual(built, ['web']);
  assert.match(html, /<title>Card<\/title>/);
  assert.match(html, /data-wb-example="basic" data-status="ready"/);
  assert.match(html, /data-wb-example="nested" data-status="ready"/);
  const options = optionsOf(html);
  assert.equal(options.lens, 'web');
  assert.deepEqual(options.bundle, { module: '/_workbench/previews/docs/web/examples.js' });
  assert.equal(options.sourceUrl, '/_workbench/docs/source?page=docs%2Fcard.md&lens=web&example=');
});

test('another lens keeps every panel; what it lacks says so', async () => {
  const { docs } = service();
  const html = await docs.page(card, { lens: 'native', state: 'loading' }, new Set());
  assert.match(html, /data-wb-example="basic" data-status="ready"/);
  assert.match(html, /data-wb-example="nested" data-status="missing"><p class="wb-docs-example-note">Not available in Native<\/p>/);
  assert.match(html, /<link rel="stylesheet" href="\/_workbench\/previews\/docs\/native\/examples.css">/);
  assert.equal(optionsOf(html).state, 'loading');
  assert.equal(chooseLens(card, 'sepia')!.key, 'web', 'an unknown lens falls back to the page’s own');
});

test('without trust, or when a lens fails to build, the Markdown still renders', async () => {
  const untrusted = service({ trusted: false });
  const html = await untrusted.docs.page(card, {}, new Set());
  assert.deepEqual(untrusted.built, []);
  assert.match(html, /data-status="unavailable"><p class="wb-docs-example-note">Examples run project code, so they appear in a trusted workspace only.<\/p>/);
  assert.equal(optionsOf(html).bundle, null);
  assert.match(html, /<h2 id="basic">Basic<\/h2>/);

  const failing = service({ failing: 'web' });
  assert.match(await failing.docs.page(card, {}, new Set()), /data-status="unavailable"><p class="wb-docs-example-note">Web: Build failed: missing .\/card.tsx<\/p>/);
  await assert.rejects(failing.docs.page({ ...card, src: 'docs/gone.md' }, {}, new Set()), { status: 404 });
});

test('Show code gets a named export’s statement, or a folder example’s whole file', async () => {
  const { docs } = service();
  const nested = await docs.exampleSource(card, 'web', 'nested');
  assert.equal(nested.text, ['export const nested = () => (', '  <Card>Nested</Card>', ');'].join('\n'));
  assert.match(nested.html, /hljs-keyword/);
  assert.equal((await docs.exampleSource(card, 'native', 'basic')).text, files['docs/native/basic.tsx']);
  await assert.rejects(docs.exampleSource(card, 'web', 'large'), { status: 404, message: /Couldn’t find the export large/ });
  await assert.rejects(docs.exampleSource(card, 'native', 'nested'), { status: 404, message: 'Native has no example “nested”.' });
  await assert.rejects(service({ trusted: false }).docs.exampleSource(card, 'web', 'basic'), { status: 403 });
});

test('problems name the page, the line, and the lens', async () => {
  files['docs/broken.md'] = ['```example basic', 'title: x', '```'].join('\n');
  const { docs } = service({ failing: 'native' });
  assert.deepEqual(await docs.problems([card, { src: 'docs/broken.md', label: 'Broken', lens: null, lenses: [] }, { src: 'docs/gone.md', label: 'Gone', lens: null, lenses: [] }]), [
    'Card (docs/card.md): example “large” in Web is not placed in docs/card.md.',
    'Card (docs/card.md): Native: Build failed: missing ./card.tsx',
    'Broken (docs/broken.md), line 2: example “basic”: unknown key “title”',
    'Gone (docs/gone.md): the Markdown file doesn’t exist.',
  ]);
  assert.deepEqual(await service({ trusted: false }).docs.problems([card]), ['Docs page examples: Examples run project code, so they appear in a trusted workspace only.']);
});

test('the revision follows the Markdown and the lens’s examples', async () => {
  const { docs } = service();
  const first = await docs.revision(card, 'web');
  assert.equal(await docs.revision(card, 'web'), first);
  assert.notEqual(await docs.revision(card, 'native'), first);
  files['docs/card.md'] += '\nMore.';
  assert.notEqual(await docs.revision(card, 'web'), first);
});
