import { test } from 'node:test';
import assert from 'node:assert/strict';
import { addressLens, canvasMode, chooseLens, docsLinkLens, hasDesignLens, hasLens, pageLenses, type Lens, type LensPage } from './lenses.js';

const impls: Record<string, Lens> = {
  dev: { key: 'dev', label: 'Dev', kind: 'url' },
  web: { key: 'web', label: 'Web', kind: 'docs' },
  native: { key: 'native', label: 'React Native Web', kind: 'docs' },
};

const button: LensPage = { src: 'pages/button.html', markdown: 'docs/button.md', implementations: { dev: {}, web: {}, native: {} } };
const card: LensPage = { src: 'docs/card.md', markdown: 'docs/card.md', lens: 'native', implementations: { web: {}, native: {}, dev: {} } };
const signIn: LensPage = { src: 'pages/sign-in.html', implementations: { dev: {} } };
const guide: LensPage = { src: 'pages/guide.html', markdown: 'docs/guide.md' };
const colors: LensPage = { src: 'docs/colors.md', markdown: 'docs/colors.md', docsLenses: [{ key: 'docs', label: 'Docs' }], implementations: { docs: {} } };

const keys = (lenses: Lens[]) => lenses.map(lens => lens.key);

test('a page with a design and docs lists its docs first and opens on them', () => {
  assert.equal(hasDesignLens(button), true);
  assert.deepEqual(keys(pageLenses(button, impls)), ['web', 'native', 'dev']);
  assert.equal(chooseLens(button, impls, null)!.key, 'web', 'it opens on its first docs lens');
  assert.equal(chooseLens(button, impls, 'design'), null, 'design asks for its design');
  assert.equal(chooseLens(button, impls, 'dev')!.key, 'dev');
  assert.equal(canvasMode(chooseLens(button, impls, null)), 'docs');
  assert.equal(canvasMode(chooseLens(button, impls, 'design')), 'default');
  assert.equal(hasLens(button, impls, 'design'), true);
});

test('the address leaves out a page’s default lens and names its design when docs are the default', () => {
  assert.equal(addressLens(button, impls, impls.web), null, 'its first docs lens is its default');
  assert.equal(addressLens(button, impls, impls.native), 'native');
  assert.equal(addressLens(button, impls, null), 'design');
  assert.equal(addressLens(signIn, impls, null), null, 'without docs, the design is the default');
  assert.equal(addressLens(signIn, impls, impls.dev), 'dev');
});

test('a Markdown page has no design and opens on its own docs lens', () => {
  assert.equal(hasDesignLens(card), false);
  assert.deepEqual(keys(pageLenses(card, impls)), ['web', 'native', 'dev'], 'its docs lenses come first');
  assert.equal(chooseLens(card, impls, null)!.key, 'native');
  assert.equal(chooseLens(card, impls, 'dev')!.key, 'dev', 'it can map other lenses too');
  assert.equal(addressLens(card, impls, impls.native), null, 'its own lens is left out of the address');
  assert.equal(addressLens(card, impls, impls.web), 'web');
  assert.equal(hasLens(card, impls, 'design'), false);
  assert.equal(chooseLens(card, impls, 'design')!.key, 'native', 'asked for a design it lacks, it shows its default');
});

test('the lens carries across pages, and a page without it shows its default', () => {
  assert.equal(chooseLens(card, impls, 'web')!.key, 'web');
  assert.equal(chooseLens(signIn, impls, 'web'), null, 'no docs: the design');
  assert.equal(chooseLens(signIn, impls, 'dev')!.key, 'dev');
  assert.equal(chooseLens(signIn, impls, 'design'), null);
  assert.equal(chooseLens(button, impls, 'design'), null, 'the design carries to a page with docs');
  assert.equal(chooseLens(colors, impls, 'web')!.key, 'docs', 'a Markdown page without it: its own docs lens');
});

test('Markdown without a docs implementation gets the built-in Docs lens', () => {
  assert.deepEqual(pageLenses(guide, impls), [{ key: 'docs', label: 'Docs', kind: 'docs' }]);
  assert.equal(hasLens(guide, impls, 'docs'), true);
  assert.equal(chooseLens(guide, impls, 'docs')!.kind, 'docs');
  assert.deepEqual(keys(pageLenses(colors, impls)), ['docs'], 'a definition’s own docs lens is not doubled');
});

test('a link between docs opens the target in a docs lens', () => {
  assert.equal(docsLinkLens(button, impls, 'native')!.key, 'native');
  assert.equal(docsLinkLens(card, impls, 'docs')!.key, 'native', 'without the lens, its default docs lens');
  assert.equal(docsLinkLens(guide, impls, 'web')!.key, 'docs');
  assert.equal(docsLinkLens(signIn, impls, 'web'), null);
});

test('an imported page shows only its implementation', () => {
  const story: LensPage = { src: 'storybook:button', implementationOnly: 'dev', implementations: { dev: {} } };
  assert.equal(hasDesignLens(story), false);
  assert.equal(chooseLens(story, impls, 'web')!.key, 'dev');
  assert.equal(addressLens(story, impls, impls.dev), 'dev', 'its address names the implementation');
});

test('every selectable lens round-trips through its page address', () => {
  const story: LensPage = { src: 'storybook:button', implementationOnly: 'dev', implementations: { dev: {} } };
  for (const page of [button, card, signIn, guide, colors, story]) {
    const lenses: (Lens | null)[] = pageLenses(page, impls);
    if (hasDesignLens(page)) lenses.unshift(null);
    for (const lens of lenses) {
      const key = addressLens(page, impls, lens);
      assert.equal(chooseLens(page, impls, key)?.key ?? null, lens?.key ?? null,
        page.src + ' through ' + (lens?.key ?? 'design'));
    }
  }
});
