/** @file Observable task policy, independent of list presentation. */
import { expect, test } from 'vitest';
import { checkCollectionSelection, checkSelection } from '@canonic2/lattice/testing';
import { createTasks } from './tasks.js';

test('task selections declare their dependencies and readers forward live membership', () => {
  const tasks = createTasks(3);
  /** @type {Record<string, import('./tasks.js').Task>} */
  const items = {
    open: { text: 'Write', done: false },
    complete: { text: 'Review', done: true },
  };
  expect(checkSelection('text', tasks.select.text, items).unobserved).toEqual([]);
  expect(checkSelection('done', tasks.select.done, items).unobserved).toEqual([]);
  expect(
    checkSelection('selectedKey', tasks.select.selectedKey, {
      clear: { selectedKey: null },
      selected: { selectedKey: 't0' },
    }).unobserved,
  ).toEqual([]);
  expect(
    checkCollectionSelection('summary', tasks.select.summary, {
      empty: createTasks(0).items,
      populated: tasks.items,
    }).unobserved,
  ).toEqual([]);
  expect(tasks.items.size).toBe(3);
  const added = tasks.insert();
  expect(tasks.items.size).toBe(4);
  tasks.remove(added);
  expect(tasks.items.size).toBe(3);
});

test('selection has one owner and repeated selection is silent', () => {
  const tasks = createTasks(3);
  /** @type {(string | null)[]} */
  const selected = [];
  tasks.state.watch(tasks.select.selectedKey, (key) => selected.push(key));
  tasks.choose('t1');
  tasks.choose('t1');
  expect(selected).toEqual([null, 't1']);
  expect(() => tasks.choose('missing')).toThrow(TypeError);
  expect(tasks.state.get().selectedKey).toBe('t1');
});

test('bulk completion delivers one final summary and item changes stay local', () => {
  const tasks = createTasks(3);
  /** @type {string[]} */
  const summaries = [];
  /** @type {string[]} */
  const other = [];
  tasks.items.watch(tasks.select.summary, (text) => summaries.push(text));
  tasks.items.watchItem('t2', tasks.select.text, (text) => other.push(text));
  tasks.rename('t1', 'Edited');
  tasks.markAll(true);
  expect(other).toEqual(['Task 3']);
  expect(summaries).toEqual(['3 open / 3 tasks', '0 open / 3 tasks']);
});

test('batched key reuse reports a new incarnation and clears old selection', () => {
  const tasks = createTasks(3),
    old = tasks.items.get('t1');
  /** @type {import('@canonic2/lattice/collection').KeysChange[]} */
  const changes = [];
  tasks.items.watchKeys((_keys, _previous, change) => {
    if (change) changes.push(change);
  });
  tasks.choose('t1');
  tasks.reuse('t1');
  expect(tasks.items.keys()).toEqual(['t0', 't1', 't2']);
  expect(changes).toEqual([{ inserted: ['t1'], removed: ['t1'] }]);
  expect(tasks.items.get('t1')).not.toBe(old);
  expect(tasks.state.get().selectedKey).toBeNull();
});
