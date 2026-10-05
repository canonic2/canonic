import { test } from 'node:test';
import assert from 'node:assert/strict';
import { addressLens, canvasMode, chooseLens, docsLinkLens, hasDesignLens, hasLens, pageLenses, type Lens, type LensPage } from './lenses.ts';

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

test('a page with a design has its design, its implementations, and its docs lenses', () => {
  assert.equal(hasDesignLens(button), true);
  assert.deepEqual(keys(pageLenses(button, impls)), ['dev', 'web', 'native']);
  assert.equal(chooseLens(button, impls, null), null, 'it opens on its design');
  assert.equal(chooseLens(button, impls, 'web')!.key, 'web');
  assert.equal(canvasMode(chooseLens(button, impls, 'web')), 'docs');
  assert.equal(canvasMode(chooseLens(button, impls, 'dev')), 'default');
});

test('a Markdown page has no design and opens on its own docs lens', () => {
  assert.equal(hasDesignLens(card), false);
  assert.deepEqual(keys(pageLenses(card, impls)), ['web', 'native', 'dev'], 'its docs lenses come first');
  assert.equal(chooseLens(card, impls, null)!.key, 'native');
  assert.equal(chooseLens(card, impls, 'dev')!.key, 'dev', 'it can map other lenses too');
  assert.equal(addressLens(card, impls, impls.native), null, 'its own lens is left out of the address');
  assert.equal(addressLens(card, impls, impls.web), 'web');
  assert.equal(addressLens(button, impls, impls.web), 'web');
  assert.equal(addressLens(button, impls, null), null);
});

test('the lens carries across pages, and a page without it shows its default', () => {
  assert.equal(chooseLens(card, impls, 'web')!.key, 'web');
  assert.equal(chooseLens(signIn, impls, 'web'), null, 'no docs: the design');
  assert.equal(chooseLens(signIn, impls, 'dev')!.key, 'dev');
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
