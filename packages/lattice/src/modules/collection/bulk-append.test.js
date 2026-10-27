/** @file Observable single-insertion behavior of atomic bulk membership. */
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
])(
  'one insertion preserves snapshots, delivery and rollback before $before',
  ({ before, order }) => {
    const c = tasks(),
      originalKeys = c.keys(),
      original = c.get('b');
    const owner = new AbortController();
    /** @type {(readonly string[])[]} */ const seen = [];
    c.watchKeys((keys) => seen.push(keys), { signal: owner.signal });
    const value = { label: 'X', done: false },
      apply = c.updateMembership;
    apply({ insert: [['x', value, before]] });
    value.label = 'Caller mutation';
    const committedKeys = c.keys();
    expect(committedKeys).toEqual(order);
    expect(Object.isFrozen(committedKeys)).toBe(true);
    expect(originalKeys).toEqual(['a', 'b', 'c']);
    expect(c.get('b')).toBe(original);
    expect(c.get('x')?.label).toBe('X');
    expect(c.get('x')).not.toBe(value);
    expect(seen).toEqual([originalKeys, committedKeys]);
    expect(() =>
      c.batch(() => {
        apply({ insert: [['y', { label: 'Y', done: false }]] });
        expect(c.keys()).toEqual([...order, 'y']);
        throw new Error('rollback append');
      }),
    ).toThrow('rollback append');
    expect(c.keys()).toBe(committedKeys);
    expect(c.has('y')).toBe(false);
    expect(seen).toEqual([originalKeys, committedKeys]);
    owner.abort();
  },
);

test('one insertion into an empty collection commits once with a copied incarnation', () => {
  /** @type {import('./collection.js').Collection<Task>} */ const c = createCollection([]);
  const before = c.keys(),
    owner = new AbortController(),
    run = vi.fn();
  c.watchKeys(run, { signal: owner.signal });
  run.mockClear();
  c.updateMembership({ insert: [['x', { label: 'X', done: false }]] });
  expect(before).toEqual([]);
  expect(c.keys()).toEqual(['x']);
  expect(c.keys()).not.toBe(before);
  expect(Object.isFrozen(c.get('x'))).toBe(true);
  expect(run).toHaveBeenCalledTimes(1);
  owner.abort();
});

test.each(['a', 'b', 'c'])(
  'one removal of %s reads inputs once and preserves rollback and incarnation cleanup',
  (key) => {
    const c = tasks(),
      keys = c.keys(),
      record = c.get(key),
      owner = new AbortController();
    const run = vi.fn(),
      detach = vi.spyOn(owner.signal, 'removeEventListener');
    c.watchItem(key, { fields: ['label'], read: (item) => item.label }, run, {
      signal: owner.signal,
    });
    run.mockClear();
    let reads = 0;
    /** @returns {string[]} A fresh input whose slot must be copied once. */
    function removal() {
      return new Proxy([key], {
        get(target, property, receiver) {
          if (property === '0') {
            reads++;
            expect(c.keys()).toBe(keys);
          }
          return Reflect.get(target, property, receiver);
        },
      });
    }
    expect(() =>
      c.batch(() => {
        c.updateMembership({ remove: removal() });
        expect(c.keys()).toEqual(keys.filter((value) => value !== key));
        expect(c.has(key)).toBe(false);
        expect(detach).not.toHaveBeenCalled();
        throw new Error('restore single removal');
      }),
    ).toThrow('restore single removal');
    expect(reads).toBe(1);
    expect(c.keys()).toBe(keys);
    expect(c.get(key)).toBe(record);
    c.update(key, { label: 'Still subscribed' });
    expect(run).toHaveBeenCalledTimes(1);
    c.updateMembership({ remove: removal() });
    expect(reads).toBe(2);
    expect(detach).toHaveBeenCalledTimes(1);
    expect(keys).toEqual(['a', 'b', 'c']);
    c.updateMembership({ insert: [[key, { label: 'New incarnation', done: false }]] });
    c.update(key, { label: 'New update' });
    expect(run).toHaveBeenCalledTimes(1);
    owner.abort();
    detach.mockRestore();
  },
);
