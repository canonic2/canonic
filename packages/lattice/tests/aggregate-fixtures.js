/** @file Shared pure fixtures for product aggregate Node and Chromium tests. */

/**
 * One task used by aggregate lifecycle and exact-arithmetic cases.
 * @typedef {{ done: boolean, cents: number, label: string }} Task
 */

/**
 * Builds deterministic task entries.
 * @param {number} count Number of records.
 * @returns {[string, Task][]} Entries with exact integer amounts.
 */
export function entries(count) {
  return Array.from({ length: count }, (_, index) => [
    `t${index}`,
    { done: index % 3 === 0, cents: index, label: 'Task' },
  ]);
}

/**
 * Rejects unsafe arithmetic in the example reducer, not in framework internals.
 * @param {number} value Intermediate numeric result.
 * @returns {number} The safe integer.
 */
export function safe(value) {
  if (!Number.isSafeInteger(value)) throw new RangeError('Unsafe integer intermediate');
  return value;
}

/**
 * Creates an exact count or sum descriptor using the public JSDoc contract.
 * @param {'count' | 'sum'} kind Desired reduction.
 * @returns {import('../src/modules/collection/collection.js').AggregateSelection<Task, number, number>} Descriptor.
 */
export function reduction(kind) {
  return {
    fields: [kind === 'count' ? 'done' : 'cents'],
    initial: () => 0,
    read: (item) => (kind === 'count' ? (item.done ? 0 : 1) : safe(item.cents)),
    add: (total, contribution) => safe(total + contribution),
    remove: (total, contribution) => safe(total - contribution),
  };
}

/**
 * Independently folds current records for a result oracle.
 * @param {import('../src/modules/collection/collection.js').CollectionView<Task>} collection Read-only traversal.
 * @param {'count' | 'sum'} kind Desired result.
 * @returns {number} Exact recomputation.
 */
export function recompute(collection, kind) {
  let total = 0;
  const keys = collection.keys();
  for (let index = 0; index < keys.length; index++) {
    const item = collection.get(/** @type {string} */ (keys[index]));
    if (item === undefined) throw new Error('Missing oracle item');
    total = safe(total + (kind === 'count' ? (item.done ? 0 : 1) : item.cents));
  }
  return total;
}
