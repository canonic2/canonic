/** @file Exercises aggregate dependency checking through real collection views. */
import { expect, test } from 'vitest';
import { createCollection } from '../modules/collection/collection.js';
import { checkCollectionSelection, checkSelection } from './index.js';

/** @typedef {{ label: string, done: boolean }} Task */
/** @typedef {import('../modules/collection/collection.js').CollectionView<Task>} View */
/** @typedef {import('../modules/collection/collection.js').CollectionSelection<Task, unknown>} Aggregate */

/**
 * Creates a deterministic collection fixture.
 * @param {boolean} done Whether the first task is complete.
 * @returns {import('../modules/collection/collection.js').Collection<Task>} Fixture owner.
 */
function tasks(done = false) {
  return createCollection([['a', { label: 'Acme', done }]]);
}

test('checks aggregate fields across cases and reports in declaration order', () => {
  /** @type {Aggregate} */
  const summary = {
    fields: ['done', 'label'],
    read: (view) => (view.get('a')?.done ? 'Complete' : view.get('a')?.label),
  };
  expect(
    checkCollectionSelection('summary', summary, { complete: tasks(true), open: tasks() }),
  ).toEqual({
    name: 'summary',
    declared: ['done', 'label'],
    cases: [
      { name: 'complete', accessed: ['done'] },
      { name: 'open', accessed: ['done', 'label'] },
    ],
    accessed: ['done', 'label'],
    unobserved: [],
  });
});

test('accepts membership-only reads, missing keys and stable traversal identities', () => {
  /** @type {Aggregate} */
  const membership = {
    fields: [],
    read(view) {
      expect(view.keys()).toBe(view.keys());
      expect(view.get('a')).toBe(view.get('a'));
      expect(view.get('missing')).toBeUndefined();
      expect(view.has('missing')).toBe(false);
      expect(view.size).toBe(1);
      return [...view.keys()];
    },
  };
  expect(checkCollectionSelection('membership', membership, { one: tasks() }).accessed).toEqual([]);
});

test('reports unobserved fields and rejects undeclared and unknown fields including in tests', () => {
  /** @type {Aggregate} */
  const summary = { fields: ['done', 'label'], read: (view) => view.get('a')?.done };
  expect(checkCollectionSelection('summary', summary, { one: tasks() }).unobserved).toEqual([
    'label',
  ]);
  expect(() =>
    checkCollectionSelection('summary', { ...summary, fields: [] }, { one: tasks() }),
  ).toThrow('undeclared field: done');
  expect(() =>
    checkCollectionSelection(
      'unknown',
      {
        fields: ['done'],
        read: (view) => Reflect.get(/** @type {Task} */ (view.get('a')), 'missing'),
      },
      { one: tasks() },
    ),
  ).toThrow('undeclared field: missing');
  expect(() =>
    checkCollectionSelection(
      'presence',
      {
        fields: [],
        read: (view) => 'done' in /** @type {Task} */ (view.get('a')),
      },
      { one: tasks() },
    ),
  ).toThrow('undeclared field: done');
});

test.each([
  /**
   * @param {View} view Fixture.
   * @returns {unknown} Enumeration result.
   */
  (view) => Object.keys(/** @type {Task} */ (view.get('a'))),
  /**
   * @param {View} view Fixture.
   * @returns {unknown} Descriptor result.
   */
  (view) => Object.getOwnPropertyDescriptor(/** @type {Task} */ (view.get('a')), 'done'),
  /**
   * @param {View} view Fixture.
   * @returns {unknown} Mutation result.
   */
  (view) => Reflect.set(/** @type {Task} */ (view.get('a')), 'done', true),
  /**
   * @param {View} view Fixture.
   * @returns {unknown} Mutation result.
   */
  (view) => Reflect.set(view.keys(), '0', 'changed'),
  /**
   * @param {View} view Fixture.
   * @returns {unknown} Mutation result.
   */
  (view) => Reflect.deleteProperty(view, 'get'),
  /**
   * @param {View} view Fixture.
   * @returns {unknown} Mutation result.
   */
  (view) => Object.setPrototypeOf(/** @type {Task} */ (view.get('a')), null),
])('rejects unsupported access even when read catches the refusal (%#)', (operation) => {
  const fixture = tasks();
  expect(() =>
    checkCollectionSelection(
      'caught',
      {
        fields: ['done', 'label'],
        read(view) {
          try {
            operation(view);
          } catch {
            /* A caught refusal must still fail the check. */
          }
          return 0;
        },
      },
      { one: fixture },
    ),
  ).toThrow('which the check cannot follow');
  expect(fixture.keys()).toEqual(['a']);
  expect(fixture.get('a')).toEqual({ label: 'Acme', done: false });
});

test('exposes traversal only and preserves read failures with selection and case context', () => {
  const failure = new Error('derive');
  /** @type {Aggregate} */
  const broken = {
    fields: [],
    read(view) {
      expect(Object.keys(view).sort()).toEqual(['get', 'has', 'keys', 'size']);
      expect(Reflect.get(view, 'update')).toBeUndefined();
      expect(Reflect.get(view, 'watch')).toBeUndefined();
      throw failure;
    },
  };
  let caught;
  try {
    checkCollectionSelection('broken', broken, { one: tasks() });
  } catch (error) {
    caught = error;
  }
  expect(caught).toMatchObject({
    message: 'Selection "broken", case "one" (declares ): read threw.',
    cause: failure,
  });
  expect(() => checkCollectionSelection('empty', broken, {})).toThrow(
    'needs at least one snapshot case',
  );
});

test('uses the record checker for item selections with the same report', () => {
  /** @type {import('../modules/collection/collection.js').ItemSelection<Task, boolean>} */
  const done = { fields: ['done'], read: (item) => item.done };
  expect(checkSelection('done', done, { open: { label: 'Acme', done: false } }).accessed).toEqual([
    'done',
  ]);
});
