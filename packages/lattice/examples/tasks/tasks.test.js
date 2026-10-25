/** @file Observable task policy, independent of list presentation. */
import { expect, test } from 'vitest';
import { createTasks } from './tasks.js';

test('selection has one owner and repeated selection is silent', () => {
  const tasks = createTasks(3);
  /** @type {(string | null)[]} */
  const selected = [];
  tasks.state.watch(tasks.select.selectedKey, (key) => selected.push(key));
  tasks.choose('t1'); tasks.choose('t1');
  expect(selected).toEqual([null, 't1']);
  expect(() => tasks.choose('missing')).toThrow(TypeError);
  expect(tasks.state.get().selectedKey).toBe('t1');
});

test('bulk completion delivers one final summary and item changes stay local', () => {
  const tasks = createTasks(3);
  /** @type {string[]} */
  const summaries = [], other = [];
  tasks.items.watch(tasks.select.summary, (text) => summaries.push(text));
  tasks.items.watchItem('t2', tasks.select.text, (text) => other.push(text));
  tasks.rename('t1', 'Edited'); tasks.markAll(true);
  expect(other).toEqual(['Task 3']);
  expect(summaries).toEqual(['3 open / 3 tasks', '0 open / 3 tasks']);
});

test('batched key reuse reports a new incarnation and clears old selection', () => {
  const tasks = createTasks(3), old = tasks.items.get('t1');
  /** @type {import('@canonic2/lattice/collection').KeysChange[]} */
  const changes = [];
  tasks.items.watchKeys((_keys, change) => { if (change) changes.push(change); });
  tasks.choose('t1'); tasks.reuse('t1');
  expect(tasks.items.keys()).toEqual(['t0', 't1', 't2']);
  expect(changes).toEqual([{ inserted: ['t1'], removed: ['t1'] }]);
  expect(tasks.items.get('t1')).not.toBe(old);
  expect(tasks.state.get().selectedKey).toBeNull();
});
