/** @file Verifies shallow equality. */
import { expect, test } from 'vitest';
import { shallow } from './shallow.js';

test('compares arrays item by item with Object.is', () => {
  const item = { id: 1 };
  expect(shallow([1, item], [1, item])).toBe(true);
  expect(shallow([1, { id: 1 }], [1, { id: 1 }])).toBe(false);
  expect(shallow([1], [1, 2])).toBe(false);
  expect(shallow([Number.NaN], [Number.NaN])).toBe(true);
});

test('compares the own enumerable keys of two objects with Object.is', () => {
  const nested = { id: 1 };
  expect(shallow({ a: 1, nested }, { nested, a: 1 })).toBe(true);
  expect(shallow({ a: 1 }, { a: 1, b: undefined })).toBe(false);
  expect(shallow({ a: 1, b: undefined }, { a: 1, c: undefined })).toBe(false);
  expect(shallow({ nested: { id: 1 } }, { nested: { id: 1 } })).toBe(false);
});

test('treats a non-enumerable key as missing, in both argument orders', () => {
  const a = { x: 1 };
  const b = { y: 2 };
  Object.defineProperty(b, 'x', { value: 1 });
  expect(shallow(/** @type {object} */ (a), /** @type {object} */ (b))).toBe(false);
  expect(shallow(/** @type {object} */ (b), /** @type {object} */ (a))).toBe(false);
});

test('treats objects with different prototypes as unequal', () => {
  expect(shallow(new Map([['a', 1]]), new Map([['a', 1]]))).toBe(true);
  expect(shallow(/** @type {object} */ (new Map()), /** @type {object} */ (new Set()))).toBe(false);
  expect(shallow(/** @type {object} */ ({}), /** @type {object} */ (Object.create(null)))).toBe(false);
});

test('treats identical values as equal and differing kinds as unequal', () => {
  const value = { a: 1 };
  expect(shallow(value, value)).toBe(true);
  expect(shallow(/** @type {unknown} */ ([]), /** @type {unknown} */ ({}))).toBe(false);
  expect(shallow(/** @type {unknown} */ (null), /** @type {unknown} */ ({}))).toBe(false);
  expect(shallow(/** @type {unknown} */ (1), /** @type {unknown} */ ('1'))).toBe(false);
});
