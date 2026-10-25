/** @file Verifies the keyed collection against the collections contract. */
import { afterEach, describe, expect, test, vi } from 'vitest';
import { createCollection } from './collection.js';

/** @typedef {{ label: string, done: boolean }} Task */
/** @typedef {import('./collection.js').Collection<Task>} Tasks */
/** @typedef {import('./collection.js').CollectionView<Task>} TasksView */

/** @type {import('./collection.js').ItemSelection<Task, string>} */
const label = { fields: ['label'], read: (task) => task.label };
/** @type {import('./collection.js').ItemSelection<Task, boolean>} */
const done = { fields: ['done'], read: (task) => task.done };

/**
 * Counts the items that are not done.
 * @param {TasksView} view The collection's view.
 * @returns {number} The open count.
 */
function countOpen(view) {
  const keys = view.keys();
  let count = 0;
  for (let index = 0; index < keys.length; index++) if (!view.get(/** @type {string} */ (keys[index]))?.done) count++;
  return count;
}

/** @type {import('./collection.js').CollectionSelection<Task, number>} */
const open = { items: ['done'], read: countOpen };

/**
 * Three tasks: a open, b done, c open.
 * @param {import('./collection.js').CollectionOptions} [options] Collection options.
 * @returns {Tasks} The collection.
 */
function tasks(options = {}) {
  return createCollection(
    [
      ['a', { label: 'A', done: false }],
      ['b', { label: 'B', done: true }],
      ['c', { label: 'C', done: false }],
    ],
    options,
  );
}

/**
 * Forces garbage collection after the current job, so new weak references can die.
 * @returns {Promise<void>} Settles after collection.
 */
async function collectGarbage() {
  const collect = /** @type {unknown} */ (Reflect.get(globalThis, 'gc'));
  if (typeof collect !== 'function') throw new Error('The unit project must run with --expose-gc.');
  await new Promise((resolve) => setTimeout(resolve, 0));
  collect();
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('storage', () => {
  test('copies items into frozen records and returns keys and records by identity until they change', () => {
    const source = { label: 'A', done: false };
    const collection = createCollection([['a', source], ['b', { label: 'B', done: true }]]);
    const keys = collection.keys();
    const b = collection.get('b');
    expect(collection.get('a')).not.toBe(source);
    expect(Object.isFrozen(keys)).toBe(true);
    expect(Object.isFrozen(collection.get('a'))).toBe(true);
    collection.update('a', { label: 'A2' });
    expect(collection.keys()).toBe(keys);
    expect(collection.get('b')).toBe(b);
    expect(collection.get('a')).toEqual({ label: 'A2', done: false });
    expect(Object.isFrozen(collection.get('a'))).toBe(true);
  });

  test('answers membership without copying', () => {
    const collection = tasks();
    expect(collection.size).toBe(3);
    expect(collection.has('b')).toBe(true);
    expect(collection.has('z')).toBe(false);
    expect(collection.get('z')).toBeUndefined();
    collection.remove('b');
    expect(collection.size).toBe(2);
  });

  test('rejects keys that are not strings, duplicates, and items that are not objects or have symbol keys', () => {
    expect(() => createCollection([[/** @type {string} */ (/** @type {unknown} */ (1)), {}]])).toThrow(TypeError);
    expect(() => createCollection([['a', {}], ['a', {}]])).toThrow(TypeError);
    expect(() => createCollection([['a', /** @type {object} */ (/** @type {unknown} */ (null))]])).toThrow(TypeError);
    expect(() => createCollection([['a', { [Symbol('x')]: 1 }]])).toThrow(TypeError);
  });

  test('inserts at the end or before a key, and rejects an existing key or an unknown position', () => {
    const collection = tasks();
    collection.insert('d', { label: 'D', done: false });
    collection.insert('e', { label: 'E', done: false }, 'b');
    expect(collection.keys()).toEqual(['a', 'e', 'b', 'c', 'd']);
    const keys = collection.keys();
    expect(() => collection.insert('a', { label: '', done: false })).toThrow(TypeError);
    expect(() => collection.insert('f', { label: '', done: false }, 'z')).toThrow(TypeError);
    expect(collection.keys()).toBe(keys);
    expect(collection.has('f')).toBe(false);
  });

  test('accepts only permutations in reorder and is silent for the current order', () => {
    const collection = tasks();
    const keys = collection.keys();
    const run = vi.fn();
    collection.watchKeys(run);
    collection.reorder(['a', 'b', 'c']);
    expect(collection.keys()).toBe(keys);
    expect(run).toHaveBeenCalledTimes(1);
    for (const order of [['a', 'b'], ['a', 'a', 'b'], ['a', 'b', 'z']]) {
      expect(() => collection.reorder(order)).toThrow(TypeError);
    }
    expect(collection.keys()).toBe(keys);
  });

  test('rejects writes to a missing key or an unknown field, changing nothing', () => {
    const collection = tasks();
    const record = collection.get('a');
    const run = vi.fn();
    collection.watchItem('a', label, run);
    expect(() => collection.update('z', { label: 'Z' })).toThrow(TypeError);
    expect(() => collection.remove('z')).toThrow(TypeError);
    expect(() => collection.update('a', /** @type {Partial<Task>} */ ({ colour: 'red' }))).toThrow(TypeError);
    expect(() => collection.update('a', /** @type {Partial<Task>} */ (/** @type {unknown} */ (null)))).toThrow(TypeError);
    expect(collection.get('a')).toBe(record);
    expect(run).toHaveBeenCalledTimes(1);
  });
});

describe('function patches', () => {
  test('receive the item record and read each value once', () => {
    const collection = tasks();
    let reads = 0;
    collection.update('a', (task) => ({
      get done() {
        reads++;
        return !task.done;
      },
    }));
    expect(reads).toBe(1);
    expect(collection.get('a')?.done).toBe(true);
  });

  test('reject every write made while a patch is being resolved', () => {
    const collection = tasks();
    /** @type {(() => void)[]} */
    const writes = [
      () => collection.update('b', { label: 'x' }),
      () => collection.insert('x', { label: 'x', done: false }),
      () => collection.remove('b'),
      () => collection.reorder(['c', 'b', 'a']),
      () => collection.batch(() => {}),
    ];
    for (const write of writes) {
      expect(() =>
        collection.update('a', () => {
          write();
          return { label: 'never' };
        }),
      ).toThrow(TypeError);
    }
    expect(collection.get('a')?.label).toBe('A');
    expect(collection.keys()).toEqual(['a', 'b', 'c']);
    collection.update('a', { label: 'after' });
    expect(collection.get('a')?.label).toBe('after');
  });

  test('are silent when nothing changes', () => {
    const collection = tasks();
    const record = collection.get('a');
    const run = vi.fn();
    collection.watchItem('a', label, run);
    collection.update('a', (task) => ({ label: task.label }));
    expect(collection.get('a')).toBe(record);
    expect(run).toHaveBeenCalledTimes(1);
  });
});

describe('delivery', () => {
  test('notifies membership, then items, then collection-wide values', () => {
    const collection = tasks();
    /** @type {string[]} */
    const calls = [];
    collection.watch({ items: ['done'], read: (view) => (calls.push('open'), countOpen(view)) }, () => {});
    collection.watchItem('a', done, () => calls.push('a'));
    collection.watchKeys(() => calls.push('keys'));
    calls.length = 0;
    collection.batch(() => {
      collection.update('a', { done: true });
      collection.insert('d', { label: 'D', done: false });
    });
    expect(calls).toEqual(['keys', 'a', 'open']);
  });

  test('reports inserted and removed keys to membership subscriptions', () => {
    const collection = tasks();
    /** @type {unknown[]} */
    const changes = [];
    collection.watchKeys((keys, change) => changes.push([keys, change]));
    collection.insert('d', { label: 'D', done: false });
    collection.remove('a');
    collection.reorder(['d', 'c', 'b']);
    expect(changes).toEqual([
      [['a', 'b', 'c'], undefined],
      [['a', 'b', 'c', 'd'], { inserted: ['d'], removed: [] }],
      [['b', 'c', 'd'], { inserted: [], removed: ['a'] }],
      [['d', 'c', 'b'], { inserted: [], removed: [] }],
    ]);
  });

  test('visits only the changed item and skips collection-wide values whose fields did not change', () => {
    const collection = tasks();
    const reads = vi.fn((/** @type {Task} */ task) => task.label);
    for (const key of ['a', 'b', 'c']) collection.watchItem(key, { fields: ['label'], read: reads }, () => {});
    const derive = vi.fn(countOpen);
    collection.watch({ items: ['done'], read: derive }, () => {});
    reads.mockClear();
    derive.mockClear();
    collection.update('b', { label: 'B2' });
    expect(reads).toHaveBeenCalledTimes(1);
    expect(derive).not.toHaveBeenCalled();
  });

  test('re-evaluates collection-wide values on reorder, and equals suppresses an unchanged result', () => {
    const collection = tasks();
    const first = vi.fn();
    collection.watch({ items: [], read: (view) => view.keys()[0] }, first);
    const count = vi.fn();
    const derive = vi.fn(countOpen);
    collection.watch({ items: ['done'], read: derive }, count);
    derive.mockClear();
    collection.reorder(['c', 'b', 'a']);
    expect(first).toHaveBeenLastCalledWith('c', 'a');
    expect(derive).toHaveBeenCalledTimes(1);
    expect(count).toHaveBeenCalledTimes(1);
  });

  test('evaluates each subscription at most once for a bulk action in one batch', () => {
    const collection = tasks();
    const counts = vi.fn();
    const derive = vi.fn(countOpen);
    collection.watch({ items: ['done'], read: derive }, counts);
    const rows = vi.fn();
    for (const key of ['a', 'b', 'c']) collection.watchItem(key, done, rows);
    derive.mockClear();
    rows.mockClear();
    counts.mockClear();
    collection.batch(() => {
      for (const key of collection.keys()) collection.update(key, { done: true });
      for (const key of collection.keys()) collection.update(key, { done: true });
    });
    expect(derive).toHaveBeenCalledTimes(1);
    expect(counts).toHaveBeenCalledTimes(1);
    expect(counts).toHaveBeenLastCalledWith(0, 2);
    expect(rows).toHaveBeenCalledTimes(2);
  });

  test('publishes no intermediate state of a batch', () => {
    const collection = tasks();
    /** @type {number[]} */
    const seen = [];
    collection.watch(open, (count) => seen.push(count));
    collection.batch(() => {
      for (const key of collection.keys()) collection.update(key, { done: true });
      collection.insert('d', { label: 'D', done: false });
    });
    expect(seen).toEqual([2, 1]);
  });

  test('commits nothing for a batch that ends where it began', () => {
    const collection = tasks();
    const keys = collection.keys();
    const run = vi.fn();
    collection.watchKeys(run);
    collection.watchItem('a', label, run);
    collection.batch(() => {
      collection.update('a', { label: 'x' });
      collection.update('a', { label: 'A' });
      collection.reorder(['c', 'b', 'a']);
      collection.reorder(['a', 'b', 'c']);
    });
    expect(run).toHaveBeenCalledTimes(2);
    expect(collection.keys()).toBe(keys);
  });

  test('skips the commit in progress for a subscription created during delivery', () => {
    const collection = tasks();
    const late = vi.fn();
    collection.watchKeys((_, change) => {
      if (change?.inserted.includes('d')) collection.watchItem('d', label, late);
    });
    collection.batch(() => {
      collection.insert('d', { label: 'D', done: false });
    });
    expect(late).toHaveBeenCalledTimes(1);
    expect(late).toHaveBeenCalledWith('D', undefined);
  });

  test('queues writes made during delivery and delivers them in order', () => {
    const collection = tasks();
    /** @type {string[]} */
    const seen = [];
    collection.watchItem('a', label, (value) => {
      seen.push(`a:${value}`);
      if (value === 'A2') collection.update('b', { label: 'B2' });
    });
    collection.watchItem('b', label, (value) => seen.push(`b:${value}`));
    seen.length = 0;
    collection.update('a', { label: 'A2' });
    expect(seen).toEqual(['a:A2', 'b:B2']);
  });

  test('does not call a subscription aborted earlier in the same delivery', () => {
    const collection = tasks();
    const controller = new AbortController();
    const later = vi.fn();
    collection.watchItem('a', label, (value) => {
      if (value === 'A2') controller.abort();
    });
    collection.watchItem('a', label, later, { signal: controller.signal });
    collection.update('a', { label: 'A2' });
    collection.update('a', { label: 'A3' });
    expect(later).toHaveBeenCalledTimes(1);
  });
});

describe('subscriptions', () => {
  test('reject watching inside a batch, missing keys and unknown fields', () => {
    const collection = tasks();
    expect(() => collection.batch(() => collection.watchItem('a', label, () => {}))).toThrow(TypeError);
    expect(() => collection.batch(() => collection.watchKeys(() => {}))).toThrow(TypeError);
    expect(() => collection.batch(() => collection.watch(open, () => {}))).toThrow(TypeError);
    expect(() => collection.watchItem('z', label, () => {})).toThrow(TypeError);
    expect(() =>
      collection.watchItem('a', /** @type {import('./collection.js').ItemSelection<Task, unknown>} */ ({ fields: [], read: () => 0 }), () => {}),
    ).toThrow(TypeError);
    expect(() =>
      collection.watchItem('a', /** @type {import('./collection.js').ItemSelection<Task, unknown>} */ (/** @type {unknown} */ ({ fields: ['colour'], read: () => 0 })), () => {}),
    ).toThrow(TypeError);
    expect(() =>
      collection.watch(/** @type {import('./collection.js').CollectionSelection<Task, unknown>} */ (/** @type {unknown} */ ({ items: [1], read: () => 0 })), () => {}),
    ).toThrow(TypeError);
  });

  test('copy the declared fields', () => {
    const collection = tasks();
    /** @type {('label' | 'done')[]} */
    const fields = ['label'];
    const run = vi.fn();
    collection.watchItem('a', { fields, read: (task) => task.label }, run);
    fields[0] = 'done';
    collection.update('a', { done: true });
    collection.update('a', { label: 'A2' });
    expect(run).toHaveBeenLastCalledWith('A2', 'A');
    expect(run).toHaveBeenCalledTimes(2);
  });

  test('stop when the signal aborts, and subscribe nothing for an aborted signal', () => {
    const collection = tasks();
    const controller = new AbortController();
    const run = vi.fn();
    collection.watchItem('a', label, run, { signal: controller.signal });
    controller.abort();
    const read = vi.fn(countOpen);
    collection.watch({ items: ['done'], read }, run, { signal: controller.signal });
    collection.update('a', { label: 'x', done: true });
    expect(run).toHaveBeenCalledTimes(1);
    expect(read).not.toHaveBeenCalled();
  });

  test('leave nothing when the initial read or run throws, or the initial read writes', () => {
    const collection = tasks();
    const read = vi.fn(() => {
      throw new Error('read');
    });
    expect(() => collection.watchItem('a', { fields: ['label'], read }, () => {})).toThrow('read');
    const run = vi.fn(() => {
      throw new Error('run');
    });
    expect(() => collection.watchItem('a', label, run)).toThrow('run');
    expect(() =>
      collection.watch(
        {
          items: [],
          read: () => {
            collection.update('b', { label: 'written' });
            return 0;
          },
        },
        () => {},
      ),
    ).toThrow(TypeError);
    read.mockClear();
    run.mockClear();
    collection.update('a', { label: 'A2' });
    expect(read).not.toHaveBeenCalled();
    expect(run).not.toHaveBeenCalled();
  });
});

describe('delivery errors', () => {
  test('go to report while delivery continues and the write returns', () => {
    const report = vi.fn();
    const collection = tasks({ report });
    const after = vi.fn();
    collection.watchItem('a', label, (value) => {
      if (value === 'boom') throw new Error('row');
    });
    collection.watchItem('a', label, after);
    collection.watchKeys((_, change) => {
      if (change !== undefined) throw new Error('keys');
    });
    expect(() => collection.update('a', { label: 'boom' })).not.toThrow();
    expect(() => collection.insert('d', { label: 'D', done: false })).not.toThrow();
    expect(report).toHaveBeenCalledTimes(2);
    expect(after).toHaveBeenLastCalledWith('boom', 'A');
  });

  test('reach the platform reportError when report throws', () => {
    const reported = vi.fn();
    vi.stubGlobal('reportError', reported);
    const collection = tasks({
      report: () => {
        throw new Error('report');
      },
    });
    collection.watchItem('a', label, (value) => {
      if (value === 'boom') throw new Error('row');
    });
    collection.update('a', { label: 'boom' });
    expect(reported).toHaveBeenCalledTimes(1);
  });
});

describe('batches', () => {
  test('roll back on throw and on a thenable, delivering nothing', () => {
    const collection = tasks();
    const keys = collection.keys();
    const record = collection.get('a');
    const run = vi.fn();
    collection.watchKeys(run);
    collection.watchItem('a', label, run);
    expect(() =>
      collection.batch(() => {
        collection.update('a', { label: 'x' });
        collection.insert('d', { label: 'D', done: false }, 'a');
        collection.reorder(['c', 'b', 'a', 'd']);
        throw new Error('stop');
      }),
    ).toThrow('stop');
    expect(() =>
      collection.batch(() => {
        collection.update('a', { label: 'y' });
        return /** @type {void} */ (/** @type {unknown} */ (Promise.resolve()));
      }),
    ).toThrow(TypeError);
    expect(collection.keys()).toBe(keys);
    expect(collection.get('a')).toBe(record);
    expect(collection.has('d')).toBe(false);
    expect(run).toHaveBeenCalledTimes(2);
  });

  test('roll back a nested savepoint only, and commit the outer writes once', () => {
    const collection = tasks();
    const runs = vi.fn();
    collection.watchItem('a', label, runs);
    collection.watchItem('c', label, runs);
    runs.mockClear();
    collection.batch(() => {
      collection.update('a', { label: 'A2' });
      try {
        collection.batch(() => {
          collection.update('c', { label: 'C2' });
          throw new Error('inner');
        });
      } catch {
        // The savepoint rolled back; the outer batch continues.
      }
    });
    expect(collection.get('c')?.label).toBe('C');
    expect(runs).toHaveBeenCalledTimes(1);
    expect(runs).toHaveBeenLastCalledWith('A2', 'A');
  });

  test('release the batch bookkeeping after it commits or throws', async () => {
    const collection = tasks();
    /** @type {WeakRef<object>[]} */
    const references = [];
    (() => {
      references.push(new WeakRef(/** @type {object} */ (collection.get('a'))));
      collection.batch(() => collection.update('a', { label: 'A2' }));
      references.push(new WeakRef(/** @type {object} */ (collection.get('b'))));
      try {
        collection.batch(() => {
          collection.update('b', { label: 'B2' });
          collection.update('b', { label: 'B3' });
          throw new Error('stop');
        });
      } catch {
        // Rolled back.
      }
    })();
    collection.update('b', { label: 'B4' });
    await collectGarbage();
    expect(references.map((reference) => reference.deref())).toEqual([undefined, undefined]);
  });
});

describe('transactional removal', () => {
  test('ends item subscriptions when the outermost batch commits', () => {
    const collection = tasks();
    const row = vi.fn();
    collection.watchItem('b', label, row);
    collection.batch(() => {
      collection.remove('b');
      expect(collection.has('b')).toBe(false);
    });
    collection.insert('b', { label: 'B again', done: false });
    collection.update('b', { label: 'changed' });
    expect(row).toHaveBeenCalledTimes(1);
  });

  test('restores the same item and its subscriptions when the batch throws', () => {
    const collection = tasks();
    const record = collection.get('b');
    const keys = collection.keys();
    const row = vi.fn();
    const membership = vi.fn();
    collection.watchItem('b', label, row);
    collection.watchKeys(membership);
    expect(() =>
      collection.batch(() => {
        collection.remove('b');
        throw new Error('stop');
      }),
    ).toThrow('stop');
    expect(collection.get('b')).toBe(record);
    expect(collection.keys()).toBe(keys);
    expect(membership).toHaveBeenCalledTimes(1);
    collection.update('b', { label: 'still bound' });
    expect(row).toHaveBeenLastCalledWith('still bound', 'B');
  });

  test('restores a removal rolled back by a nested savepoint, and commits the outer removal', () => {
    const collection = tasks();
    const rowA = vi.fn();
    const rowB = vi.fn();
    collection.watchItem('a', label, rowA);
    collection.watchItem('b', label, rowB);
    collection.batch(() => {
      collection.remove('a');
      try {
        collection.batch(() => {
          collection.remove('b');
          collection.update('c', { label: 'C2' });
          throw new Error('inner');
        });
      } catch {
        // The savepoint rolled back; the outer batch continues.
      }
    });
    expect(collection.keys()).toEqual(['b', 'c']);
    expect(collection.get('c')?.label).toBe('C');
    collection.update('b', { label: 'B2' });
    expect(rowB).toHaveBeenLastCalledWith('B2', 'B');
    collection.insert('a', { label: 'A again', done: false });
    collection.update('a', { label: 'x' });
    expect(rowA).toHaveBeenCalledTimes(1);
  });

  test('keeps a subscription ended when its own signal aborted during a rolled-back batch', () => {
    const collection = tasks();
    const controller = new AbortController();
    const row = vi.fn();
    collection.watchItem('b', label, row, { signal: controller.signal });
    expect(() =>
      collection.batch(() => {
        collection.remove('b');
        controller.abort();
        throw new Error('stop');
      }),
    ).toThrow('stop');
    collection.update('b', { label: 'B2' });
    expect(row).toHaveBeenCalledTimes(1);
  });

  test('releases a removed item and its callbacks once the removal commits', async () => {
    const collection = tasks();
    /** @type {WeakRef<object>[]} */
    const references = [];
    (() => {
      const captured = { payload: new Array(1000).fill('acme') };
      references.push(new WeakRef(captured), new WeakRef(/** @type {object} */ (collection.get('b'))));
      collection.watchItem('b', label, () => captured.payload.length);
    })();
    collection.remove('b');
    await collectGarbage();
    expect(references.map((reference) => reference.deref())).toEqual([undefined, undefined]);
  });
});

describe('key reuse', () => {
  test('creates a new item inside one batch, reported as removed and inserted', () => {
    const collection = tasks();
    const old = vi.fn();
    collection.watchItem('b', label, old);
    /** @type {(import('./collection.js').KeysChange | undefined)[]} */
    const changes = [];
    collection.watchKeys((_, change) => changes.push(change));
    collection.batch(() => {
      collection.remove('b');
      collection.insert('b', { label: 'B new', done: false }, 'c');
    });
    expect(collection.keys()).toEqual(['a', 'b', 'c']);
    expect(changes.at(-1)).toEqual({ inserted: ['b'], removed: ['b'] });
    collection.update('b', { label: 'B newer' });
    expect(old).toHaveBeenCalledTimes(1);
  });

  test('drops queued notifications of a removed item during delivery', () => {
    const collection = tasks();
    const old = vi.fn();
    const replacement = vi.fn();
    collection.watchItem('b', label, old);
    collection.watchItem('a', label, (value) => {
      if (value !== 'A2') return;
      collection.update('b', { label: 'B2' });
      collection.remove('b');
      collection.insert('b', { label: 'B3', done: false });
      collection.watchItem('b', label, replacement);
    });
    collection.update('a', { label: 'A2' });
    expect(old).toHaveBeenCalledTimes(1);
    expect(replacement).toHaveBeenCalledTimes(1);
    expect(replacement).toHaveBeenCalledWith('B3', undefined);
  });

  test('starts a key reinserted in a later commit with no subscriptions', () => {
    const collection = tasks();
    const old = vi.fn();
    collection.watchItem('c', label, old);
    collection.remove('c');
    collection.insert('c', { label: 'C2', done: false });
    collection.update('c', { label: 'C3' });
    expect(old).toHaveBeenCalledTimes(1);
  });
});
