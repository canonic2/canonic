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
