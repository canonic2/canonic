import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readDocsMarkdown } from './docs-markdown.ts';
import { matchLens } from './placement.ts';

const doc = readDocsMarkdown([
  '## Basic',
  '```example basic',
  '```',
  '## Nested',
  '```example nested',
  '```',
  '```example basic',
  '```',
].join('\n'));

test('every placement keeps a panel; a lens without the example shows it missing', () => {
  const match = matchLens(doc.placements, ['basic']);
  assert.deepEqual(match.panels.map(p => [p.id, p.status]), [
    ['basic', 'ready'],
    ['nested', 'missing'],
    ['basic', 'invalid'],
  ]);
  assert.deepEqual(match.unplaced, []);
});

test('exported examples the Markdown never places are reported, not shown', () => {
  const match = matchLens(doc.placements, ['basic', 'nested', 'with-icon', 'large']);
  assert.equal(match.panels.length, 3);
  assert.deepEqual(match.unplaced, ['with-icon', 'large']);
});
