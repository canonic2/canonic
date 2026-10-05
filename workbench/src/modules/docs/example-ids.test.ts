import { test } from 'node:test';
import assert from 'node:assert/strict';
import { examplesInFolder, exportNameToId, isExampleId } from './example-ids.ts';

test('export names become kebab-case IDs', () => {
  assert.equal(exportNameToId('basic'), 'basic');
  assert.equal(exportNameToId('withCustomStyle'), 'with-custom-style');
  assert.equal(exportNameToId('Basic'), 'basic');
  assert.equal(exportNameToId('HTMLButton'), 'html-button');
  assert.equal(exportNameToId('size2x'), 'size2x');
  assert.equal(exportNameToId('with_nested_content'), 'with-nested-content');
  assert.equal(isExampleId('with-custom-style'), true);
  assert.equal(isExampleId('WithCustomStyle'), false);
});

test('a folder holds one example per compilable file, in name order', () => {
  const entries = [
    { name: 'with-custom-style.tsx', isFile: true },
    { name: 'basic.tsx', isFile: true },
    { name: 'basic.test.tsx', isFile: true },
    { name: 'types.d.ts', isFile: true },
    { name: 'notes.md', isFile: true },
    { name: '.hidden.tsx', isFile: true },
    { name: 'nested', isFile: false },
    { name: 'WithIcon.tsx', isFile: true },
    { name: 'basic.ts', isFile: true },
  ];
  assert.deepEqual(examplesInFolder(entries, 'react'), {
    examples: [
      { id: 'basic', file: 'basic.ts' },
      { id: 'with-custom-style', file: 'with-custom-style.tsx' },
    ],
    rejected: [
      { file: 'basic.tsx', reason: 'duplicate-id' },
      { file: 'WithIcon.tsx', reason: 'not-kebab-case' },
    ],
  });
});

test('the adapter decides which extensions are examples', () => {
  const entries = [
    { name: 'basic.vue', isFile: true },
    { name: 'basic.astro', isFile: true },
  ];
  assert.deepEqual(examplesInFolder(entries, 'vue').examples, [{ id: 'basic', file: 'basic.vue' }]);
  assert.deepEqual(examplesInFolder(entries, 'astro').examples, [{ id: 'basic', file: 'basic.astro' }]);
  assert.deepEqual(examplesInFolder(entries, 'html').examples, []);
});
