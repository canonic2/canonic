/** @file Checks that the root entry point loads and works in Node without browser globals. */
import { expect, test } from 'vitest';

test('imports the root entry point in Node without browser globals and uses every export', async () => {
  expect(Reflect.has(globalThis, 'document')).toBe(false);
  expect(Reflect.has(globalThis, 'window')).toBe(false);
  const lattice = await import('@canonic2/lattice');
  expect(Object.keys(lattice).sort()).toEqual(['createStore', 'shallow']);

  const store = lattice.createStore({ count: 0 });
  /** @type {number[]} */
  const seen = [];
  store.watch({ fields: ['count'], read: (state) => state.count }, (value) => seen.push(value));
  store.update({ count: 1 });
  expect(seen).toEqual([0, 1]);
  expect(lattice.shallow({ a: 1 }, { a: 1 })).toBe(true);
});

test('imports the collection entry point in Node without browser globals and uses it', async () => {
  expect(Reflect.has(globalThis, 'document')).toBe(false);
  const entry = await import('@canonic2/lattice/collection');
  expect(Object.keys(entry).sort()).toEqual(['createCollection']);

  const collection = entry.createCollection([['a', { count: 0 }]]);
  expect(typeof collection.updateMembership).toBe('function');
  const totals = /** @type {number[]} */ ([]);
  collection.watchAggregate(
    {
      fields: ['count'],
      read: (item) => item.count,
      initial: () => 0,
      add: (total, value) => total + value,
      remove: (total, value) => total - value,
    },
    (value) => totals.push(value),
  );
  /** @type {number[]} */
  const seen = [];
  collection.watchItem('a', { fields: ['count'], read: (item) => item.count }, (value) =>
    seen.push(value),
  );
  collection.update('a', { count: 1 });
  const membership = collection.updateMembership;
  membership({
    remove: ['a'],
    insert: [
      ['a', { count: 2 }],
      ['b', { count: 3 }],
    ],
  });
  collection.update('a', { count: 4 });
  expect(collection.keys()).toEqual(['a', 'b']);
  expect(collection.get('a')?.count).toBe(4);
  expect(seen).toEqual([0, 1]);
  expect(totals).toEqual([0, 1, 5, 7]);
});
