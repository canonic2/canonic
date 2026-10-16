import { test } from 'node:test';
import assert from 'node:assert/strict';
import { discoveryOrder, readAnswer } from './answer.ts';

test('discovery ordering compares each directory before descending', () => {
  const paths = ['A/a.ts', 'a_.ts', 'a-.ts', 'é.ts', '2.ts', '10.ts', 'a/b.ts', 'a.ts'];
  const expected = paths.reduce((tree, file) => { const [head, ...tail] = file.split('/'); tree.set(head, tail.length ? file : file); return tree; }, new Map<string, string>());
  assert.deepEqual([...paths].sort(discoveryOrder), [...expected.keys()].sort((a, b) => a.localeCompare(b)).map(name => expected.get(name)));
});

test('invalid answers fail and duplicate IDs share one namespace', () => {
  for (const raw of [null, {}, { records: [{}], timings: {} }]) assert.throws(() => readAnswer(raw));
  const preview = { id: 'page', file: 'a.ts', adapter: 'html', source: 'page.html', states: [] };
  const docs = { id: 'page', file: 'b.ts', src: 'readme.md', lenses: [], states: [] };
  const { answer } = readAnswer({ records: [
    { file: docs.file, docs: true, entry: { Ok: JSON.stringify(docs) } },
    { file: preview.file, docs: false, entry: { Ok: JSON.stringify(preview) } },
  ], timings: {} });
  assert.equal(answer.previews.length, 1);
  assert.deepEqual(answer.errors, ['b.ts: Duplicate preview id: page']);
});
