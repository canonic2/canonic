/** @file Observable single-insertion behavior of the integrated, unaccepted bulk candidate. */
import { expect, test, vi } from 'vitest';
import { createCollection } from '@canonic2/lattice/collection';

/** @typedef {{label: string, done: boolean}} Task */
/**
 * Creates independent original incarnations for snapshot and rollback assertions.
 * @returns {import('./collection.js').Collection<Task>} Three initial items.
 */
function tasks() {
  return createCollection([
    ['a', { label: 'A', done: false }],
    ['b', { label: 'B', done: true }],
    ['c', { label: 'C', done: false }],
  ]);
}

test.each([
  { before: undefined, order: ['a', 'b', 'c', 'x'] },
  { before: 'a', order: ['x', 'a', 'b', 'c'] },
  { before: 'b', order: ['a', 'x', 'b', 'c'] },
  { before: 'c', order: ['a', 'b', 'x', 'c'] },
])('one insertion preserves snapshots, delivery and rollback before $before', ({ before, order }) => {
  const c = tasks(), originalKeys = c.keys(), original = c.get('b');
  const owner = new AbortController();
  /** @type {(readonly string[])[]} */ const seen = [];
  c.watchKeys((keys) => seen.push(keys), { signal: owner.signal });
  const value = { label: 'X', done: false }, apply = c.updateMembership;
  apply({ insert: [['x', value, before]] });
  value.label = 'Caller mutation';
  const committedKeys = c.keys();
  expect(committedKeys).toEqual(order); expect(Object.isFrozen(committedKeys)).toBe(true);
  expect(originalKeys).toEqual(['a', 'b', 'c']); expect(c.get('b')).toBe(original);
  expect(c.get('x')?.label).toBe('X'); expect(c.get('x')).not.toBe(value);
  expect(seen).toEqual([originalKeys, committedKeys]);
  expect(() => c.batch(() => {
    apply({ insert: [['y', { label: 'Y', done: false }]] });
    expect(c.keys()).toEqual([...order, 'y']); throw new Error('rollback append');
  })).toThrow('rollback append');
  expect(c.keys()).toBe(committedKeys); expect(c.has('y')).toBe(false);
  expect(seen).toEqual([originalKeys, committedKeys]); owner.abort();
});

test('one insertion into an empty collection commits once with a copied incarnation', () => {
  /** @type {import('./collection.js').Collection<Task>} */ const c = createCollection([]);
  const before = c.keys(), owner = new AbortController(), run = vi.fn();
  c.watchKeys(run, { signal: owner.signal }); run.mockClear();
  c.updateMembership({ insert: [['x', { label: 'X', done: false }]] });
  expect(before).toEqual([]); expect(c.keys()).toEqual(['x']);
  expect(c.keys()).not.toBe(before); expect(Object.isFrozen(c.get('x'))).toBe(true);
  expect(run).toHaveBeenCalledTimes(1); owner.abort();
});
