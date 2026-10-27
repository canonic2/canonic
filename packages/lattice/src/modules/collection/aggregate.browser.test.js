/** @file Chromium aggregate rejection handling, native abort and committed prefixes. */
import { expect, test } from 'vitest';
import { createCollection } from '@canonic2/lattice/collection';
import { entries, reduction, recompute } from '../../../tests/aggregate-fixtures.js';

test('callback Promise violations are reported without browser rejection suppression', async () => {
  /** @type {unknown[]} */
  const unhandled = [];
  /** @param {PromiseRejectionEvent} event Native rejection event. @returns {void} Record without suppressing it. */
  const observe = (event) => {
    unhandled.push(event.reason);
  };
  window.addEventListener('unhandledrejection', observe);
  try {
    for (const name of ['initial', 'read', 'add']) {
      const collection = createCollection(entries(2));
      const descriptor = reduction('sum');
      Reflect.set(descriptor, name, () => Promise.reject(new Error('invalid initialization')));
      expect(() => collection.watchAggregate(descriptor, () => {})).toThrow(TypeError);
    }
    for (const name of /** @type {const} */ (['read', 'add', 'remove', 'equals'])) {
      const owner = new AbortController();
      /** @type {unknown[]} */
      const errors = [];
      const collection = createCollection(entries(2), { report: (error) => errors.push(error) });
      const descriptor = reduction('sum');
      const original = descriptor[name] ?? Object.is;
      let bad = false;
      Reflect.set(
        descriptor,
        name,
        /**
         * @param {unknown[]} args Reducer arguments.
         * @returns {unknown} Synchronous value or deliberately invalid Promise.
         */ (...args) =>
          bad
            ? new Promise((_, reject) => queueMicrotask(() => reject(new Error('later rejection'))))
            : Reflect.apply(original, undefined, args),
      );
      let value;
      collection.watchAggregate(
        descriptor,
        (next) => {
          value = next;
        },
        { signal: owner.signal },
      );
      bad = true;
      collection.update('t0', { cents: 7 });
      expect(errors).toHaveLength(1);
      bad = false;
      collection.update('t1', { cents: 9 });
      expect(value).toBe(recompute(collection, 'sum'));
      owner.abort();
    }
    await new Promise((resolve) => setTimeout(resolve, 0));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(unhandled).toEqual([]);
  } finally {
    window.removeEventListener('unhandledrejection', observe);
  }
});

test('native abort and reentrant writes preserve each committed prefix', () => {
  const owner = new AbortController();
  const collection = createCollection(entries(1));
  /** @type {number[]} */
  const seen = [];
  collection.watchItem(
    't0',
    { fields: ['cents'], read: (item) => item.cents },
    (value) => {
      if (value === 1) collection.update('t0', { cents: 2 });
    },
    { signal: owner.signal },
  );
  collection.watchAggregate(reduction('sum'), (value) => seen.push(value), {
    signal: owner.signal,
  });
  expect(() =>
    collection.batch(() => {
      collection.update('t0', { cents: 99 });
      throw new Error('rollback');
    }),
  ).toThrow('rollback');
  collection.update('t0', { cents: 1 });
  expect(seen).toEqual([0, 1, 2]);
  owner.abort();
  collection.update('t0', { cents: 3 });
  expect(seen).toEqual([0, 1, 2]);
});
