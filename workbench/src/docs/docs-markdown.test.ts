import { test } from 'node:test';
import assert from 'node:assert/strict';
import { headingAnchor, readDocsMarkdown } from './docs-markdown.ts';

test('example blocks become placements labelled by their heading', () => {
  const doc = readDocsMarkdown([
    '# Card',
    '',
    'A plain surface.',
    '',
    '## Basic',
    '',
    '```example basic',
    'caption: children',
    '```',
    '',
    'The surface alone.',
    '',
    '## With custom style',
    '',
    '```example with-custom-style',
    '```',
  ].join('\n'));

  assert.deepEqual(doc.problems, []);
  assert.deepEqual(
    doc.placements.map(p => ({ id: p.id, line: p.line, caption: p.caption, label: p.label, duplicate: p.duplicate })),
    [
      { id: 'basic', line: 7, caption: 'children', label: 'Basic', duplicate: false },
      { id: 'with-custom-style', line: 15, caption: null, label: 'With custom style', duplicate: false },
    ],
  );
});

test('examples sharing a heading, or with none, are labelled by ID', () => {
  const doc = readDocsMarkdown(['```example lone', '```', '## Sizes', '```example small', '```', '```example large', '```'].join('\n'));
  assert.deepEqual(doc.placements.map(p => p.label), ['lone', 'small', 'large']);
});

test('front matter is stripped and lines still refer to the file', () => {
  const doc = readDocsMarkdown(['---', 'title: Card', '---', '# Card', '```example basic', '```'].join('\n'));
  assert.equal(doc.body.startsWith('# Card'), true);
  assert.equal(doc.bodyLineOffset, 3);
  assert.equal(doc.headings[0]!.line, 4);
  assert.equal(doc.placements[0]!.line, 5);
});

test('code fences that are not examples are skipped, including ones that contain example-like text', () => {
  const doc = readDocsMarkdown([
    '````md',
    '```example basic',
    '```',
    '````',
    '~~~ts',
    '# not a heading',
    '~~~',
  ].join('\n'));
  assert.deepEqual(doc.placements, []);
  assert.deepEqual(doc.headings, []);
});

test('placement problems name their line', () => {
  const doc = readDocsMarkdown([
    '```example',
    '```',
    '```example Basic',
    '```',
    '```example basic extra',
    'title: Nope',
    'just text',
    '```',
    '```example basic',
    '```',
    '```example open',
  ].join('\n'));
  assert.deepEqual(doc.problems, [
    { line: 1, message: 'example block names no example; write ```example <id>' },
    { line: 3, message: 'example id “Basic” must be kebab-case' },
    { line: 5, message: 'example “basic”: unexpected “extra” after the ID' },
    { line: 6, message: 'example “basic”: unknown key “title”' },
    { line: 7, message: 'example “basic”: expected “key: value”, found “just text”' },
    { line: 9, message: 'example “basic” is placed more than once' },
    { line: 11, message: 'example “open” has no closing fence' },
  ]);
  assert.deepEqual(doc.placements.map(p => [p.id, p.duplicate]), [
    ['', true],
    ['Basic', true],
    ['basic', false],
    ['basic', true],
    ['open', false],
  ]);
});

test('headings get unique GitHub-style anchors, including setext headings', () => {
  const doc = readDocsMarkdown(['# Card `div`', 'Usage', '-----', '## Usage', '### [Props](#props) **table**'].join('\n'));
  assert.deepEqual(doc.headings.map(h => [h.level, h.text, h.anchor]), [
    [1, 'Card div', 'card-div'],
    [2, 'Usage', 'usage'],
    [2, 'Usage', 'usage-1'],
    [3, 'Props table', 'props-table'],
  ]);
  assert.equal(headingAnchor('Color & Variant'), 'color--variant');
});
