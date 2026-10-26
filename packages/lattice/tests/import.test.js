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
  store.set({ count: 1 });
  expect(seen).toEqual([0, 1]);
  expect(lattice.shallow({ a: 1 }, { a: 1 })).toBe(true);
});

test('imports the collection entry point in Node without browser globals and uses it', async () => {
  expect(Reflect.has(globalThis, 'document')).toBe(false);
  const entry = await import('@canonic2/lattice/collection');
  expect(Object.keys(entry).sort()).toEqual(['createCollection']);

  const collection = entry.createCollection([['a', { count: 0 }]]);
  /** @type {number[]} */
  const seen = [];
  collection.watchItem('a', { fields: ['count'], read: (item) => item.count }, (value) => seen.push(value));
  collection.update('a', { count: 1 });
  expect(seen).toEqual([0, 1]);
});

test('experimental collection is an explicit pure opt-in without changing the regular entry', async () => {
  const experimental = await import('@canonic2/lattice/experimental/collection');
  const regular = await import('@canonic2/lattice/collection');
  expect(Reflect.has(globalThis, 'document')).toBe(false);
  expect(Object.keys(experimental)).toEqual(['createCollection']);
  const checkpoint = experimental.createCollection([['a', { count: 1 }]]);
  checkpoint.updateMembership({ remove: ['a'], insert: [['b', { count: 2 }]] });
  expect(checkpoint.keys()).toEqual(['b']);
  expect(checkpoint.get('b')?.count).toBe(2);
  expect('updateMembership' in regular.createCollection([])).toBe(false);
});
