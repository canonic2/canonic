/** @file Behavioral acceptance candidates for the opt-in bulk-membership checkpoint. */
import { afterEach, describe, expect, test, vi } from 'vitest';
import { createCollection } from '@canonic2/lattice/experimental/collection';
import { createCollection as shipping } from '@canonic2/lattice/collection';

/** @typedef {{ label: string, done: boolean }} Task */
/** @typedef {import('./experimental.js').MembershipChanges<Task>} Changes */
/** @typedef {import('./experimental.js').ExperimentalCollection<Task>} Tasks */
/** @type {import('./collection.js').ItemSelection<Task, string>} */
const label = { fields: ['label'], read: (task) => task.label };

/**
 * Three canonical tasks with distinct identities and order.
 * @param {import('./collection.js').CollectionOptions} [options] Delivery error policy.
 * @returns {Tasks} The opt-in collection.
 */
function tasks(options = {}) {
  return createCollection([
    ['a', { label: 'A', done: false }],
    ['b', { label: 'B', done: false }],
    ['c', { label: 'C', done: true }],
  ], options);
}

/**
 * Exercise runtime validation without weakening the public parameter type.
 * @param {Tasks} collection The checkpoint under test.
 * @param {unknown} value Deliberately malformed input.
 * @returns {void}
 */
function invalid(collection, value) {
  collection.updateMembership(/** @type {Changes} */ (value));
}

/**
 * Release the current job's weak-reference protection before collection.
 * @returns {Promise<void>} Settles after one forced GC observation.
 */
async function collectGarbage() {
  const collect = /** @type {unknown} */ (Reflect.get(globalThis, 'gc'));
  if (typeof collect !== 'function') throw new Error('Unit tests require --expose-gc.');
  await new Promise((resolve) => setTimeout(resolve, 0));
  collect();
}

afterEach(() => vi.restoreAllMocks());

describe('experimental membership checkpoint', () => {
  test('keeps shipping collections unchanged even after using the experimental entry', () => {
    tasks().updateMembership({ insert: [['x', { label: 'X', done: false }]] });
    const regular = shipping([['a', { label: 'A', done: false }]]);
    expect('updateMembership' in regular).toBe(false);
    const before = regular.keys();
    regular.batch(() => {
      regular.insert('b', { label: 'B', done: false });
      expect(regular.keys()).toEqual(['a', 'b']);
      expect(regular.keys()).not.toBe(before);
      regular.remove('a');
      expect(regular.keys()).toEqual(['b']);
    });
  });

  test('preserves survivor identities and immutable arrays with stable anchor groups and append', () => {
    const c = tasks(), keys = c.keys(), a = c.get('a'), middle = { label: 'Middle', done: false };
    c.updateMembership({ remove: ['b'], insert: [
      ['x', middle, 'c'], ['first', { label: 'First', done: false }, 'a'],
      ['y', { label: 'Y', done: false }, 'c'], ['tail', { label: 'Tail', done: true }],
    ] });
    middle.label = 'Caller mutation';
    expect(c.keys()).toEqual(['first', 'a', 'x', 'y', 'c', 'tail']);
    expect(c.keys()).toBe(c.keys());
    expect(c.keys()).not.toBe(keys);
    expect(keys).toEqual(['a', 'b', 'c']);
    expect(Object.isFrozen(keys)).toBe(true);
    expect(Object.isFrozen(c.keys())).toBe(true);
    expect(Object.isFrozen(c.get('x'))).toBe(true);
    expect(c.get('x')?.label).toBe('Middle');
    expect(c.get('a')).toBe(a);
    expect(c.has('b')).toBe(false);
    expect(c.size).toBe(6);
  });

  test('empty changes preserve keys, item identities and silence every observer', () => {
    const c = tasks(), keys = c.keys(), a = c.get('a');
    const membership = vi.fn(), item = vi.fn(), wide = vi.fn();
    c.watchKeys(membership); c.watchItem('a', label, item);
    c.watch({ fields: [], read: (view) => view.size }, wide);
    c.updateMembership({}); c.updateMembership({ remove: [], insert: [] });
    expect(c.keys()).toBe(keys); expect(c.get('a')).toBe(a);
    for (const run of [membership, item, wide]) expect(run).toHaveBeenCalledTimes(1);
  });

  test('same-key equal-value replacement changes incarnation and keys identity, ending old watchers', () => {
    const c = tasks(), keys = c.keys(), old = c.get('b'), run = vi.fn(), membership = vi.fn();
    const owner = new AbortController(), detach = vi.spyOn(owner.signal, 'removeEventListener');
    c.watchItem('b', label, run, { signal: owner.signal }); c.watchKeys(membership);
    c.updateMembership({ remove: ['b'], insert: [['b', { label: 'B', done: false }, 'c']] });
    expect(c.keys()).toEqual(keys); expect(c.keys()).not.toBe(keys);
    expect(c.get('b')).not.toBe(old);
    expect(membership).toHaveBeenLastCalledWith(c.keys(), keys, { inserted: ['b'], removed: ['b'] });
    expect(detach).toHaveBeenCalledTimes(1); expect(owner.signal.aborted).toBe(false);
    c.update('b', { label: 'New incarnation' }); expect(run).toHaveBeenCalledTimes(1);
    const replacement = vi.fn(); c.watchItem('b', label, replacement, { signal: owner.signal });
    expect(replacement).toHaveBeenCalledTimes(1);
    expect(replacement).toHaveBeenLastCalledWith('New incarnation', undefined);
    owner.abort(); c.update('b', { label: 'After abort' });
    expect(replacement).toHaveBeenCalledTimes(1);
  });

  test('rejects malformed plans atomically before ending subscriptions or changing outer writes', () => {
    const cases = [null, undefined, 1, { remove: null }, { insert: {} }, { remove: ['missing'] },
      { remove: ['a', 'a'] }, { remove: [1] }, { insert: [[]] }, { insert: [['x']] },
      { insert: [['x', {}, undefined, 'extra']] }, { insert: [['a', {}]] },
      { insert: [['x', {}], ['x', {}]] }, { insert: [['x', {}, 'x']] },
      { remove: ['b'], insert: [['x', {}, 'b']] }, { insert: [['x', null]] },
      { insert: [['x', { [Symbol('bad')]: true }]] }];
    for (const value of cases) {
      const c = tasks(), initial = c.keys(), b = c.get('b'), run = vi.fn(); c.watchItem('b', label, run);
      c.batch(() => {
        c.update('a', { label: 'Earlier write' }); const pending = c.get('a');
        expect(() => invalid(c, value)).toThrow(TypeError);
        expect(c.keys()).toBe(initial); expect(c.get('b')).toBe(b); expect(c.get('a')).toBe(pending);
      });
      c.update('b', { label: 'Still watched' }); expect(run).toHaveBeenCalledTimes(2);
    }
  });

  test('input getters read once, see pre-call state and cannot reenter writes', () => {
    const c = tasks(), keys = c.keys(); let removeReads = 0, insertReads = 0, itemReads = 0;
    c.updateMembership({
      get remove() { removeReads++; return ['b']; },
      get insert() { insertReads++; return [['x', {
        get label() { itemReads++; expect(c.keys()).toBe(keys); expect(c.has('b')).toBe(true);
          expect(() => c.remove('a')).toThrow(TypeError);
          expect(() => c.updateMembership({})).toThrow(TypeError); return 'X'; }, done: false,
      }, 'c']]; },
    });
    expect([removeReads, insertReads, itemReads]).toEqual([1, 1, 1]);
    expect(c.keys()).toEqual(['a', 'x', 'c']); expect(c.get('x')?.label).toBe('X');
  });

  test('a throwing getter keeps the original error, records, order and active subscription', () => {
    const c = tasks(), keys = c.keys(), b = c.get('b'), failure = new Error('getter'), run = vi.fn();
    c.watchItem('b', label, run);
    expect(() => c.updateMembership({ remove: ['b'], insert: [['x', {
      get label() { throw failure; }, done: false,
    }]] })).toThrow(failure);
    expect(c.keys()).toBe(keys); expect(c.get('b')).toBe(b);
    c.update('b', { label: 'Alive' }); expect(run).toHaveBeenCalledTimes(2);
  });

  test('ordinary writes remain observable between calls and bulk reads show the complete result', () => {
    const c = tasks();
    c.batch(() => {
      c.insert('d', { label: 'D', done: false }, 'b'); const first = c.keys();
      expect(first).toEqual(['a', 'd', 'b', 'c']);
      c.remove('c'); const second = c.keys();
      expect(second).toEqual(['a', 'd', 'b']); expect(second).not.toBe(first);
      c.updateMembership({ remove: ['b'], insert: [['x', { label: 'X', done: true }, 'd']] });
      expect(c.keys()).toEqual(['a', 'x', 'd']); expect(c.has('b')).toBe(false);
      expect(c.get('x')?.done).toBe(true); expect(c.size).toBe(3);
      expect(first).toEqual(['a', 'd', 'b', 'c']);
    });
  });

  test('nested rollback restores exact savepoint keys, records and still-active watchers', () => {
    const c = tasks(), initial = c.keys(), b = c.get('b'), run = vi.fn(); c.watchItem('b', label, run);
    c.batch(() => {
      c.insert('d', { label: 'D', done: false }, 'b'); const savepoint = c.keys();
      expect(() => c.batch(() => {
        c.updateMembership({ remove: ['b'], insert: [['b', { label: 'Replacement', done: true }]] });
        c.update('a', { label: 'Temporary' }); c.reorder([...c.keys()].reverse()); throw new Error('inner');
      })).toThrow('inner');
      expect(c.keys()).toBe(savepoint); expect(c.get('b')).toBe(b); expect(c.get('a')?.label).toBe('A');
    });
    c.update('b', { label: 'After rollback' }); expect(run).toHaveBeenCalledTimes(2);
    expect(initial).toEqual(['a', 'b', 'c']);
  });

  test('outer rollback undoes committed inner bulk calls, and an independently aborted watcher stays ended', () => {
    const c = tasks(), keys = c.keys(), b = c.get('b'), owner = new AbortController(), run = vi.fn();
    c.watchItem('b', label, run, { signal: owner.signal });
    expect(() => c.batch(() => {
      c.batch(() => c.updateMembership({ remove: ['b'], insert: [['b', { label: 'New', done: true }]] }));
      owner.abort(); throw new Error('outer');
    })).toThrow('outer');
    expect(c.keys()).toBe(keys); expect(c.get('b')).toBe(b);
    c.update('b', { label: 'Unobserved' }); expect(run).toHaveBeenCalledTimes(1);
  });

  test('filters transient incarnations and restores keys identity for a net no-op batch', () => {
    const c = tasks(), keys = c.keys(), run = vi.fn(); c.watchKeys(run);
    c.batch(() => {
      c.updateMembership({ insert: [['temp', { label: 'Temporary', done: false }]] });
      c.updateMembership({ remove: ['temp'] });
    });
    expect(c.keys()).toBe(keys); expect(run).toHaveBeenCalledTimes(1);
    c.batch(() => {
      c.updateMembership({ remove: ['b'], insert: [['b', { label: 'New', done: false }, 'c']] });
      c.updateMembership({ insert: [['temp', { label: 'Temporary', done: false }]] }); c.remove('temp');
    });
    expect(run).toHaveBeenCalledTimes(2);
    expect(run.mock.lastCall?.[2]).toEqual({ inserted: ['b'], removed: ['b'] });
  });

  test('publishes membership, affected items and aggregate once per commit in that order', () => {
    const c = tasks(), calls = /** @type {string[]} */ ([]);
    c.watch({ fields: ['done'], read: (view) => view.size }, () => calls.push('wide'));
    c.watchItem('a', label, () => calls.push('item'));
    c.watchKeys(() => calls.push('keys')); calls.length = 0;
    c.batch(() => {
      c.update('a', { label: 'A2' }); c.update('a', { label: 'A3' });
      c.updateMembership({ remove: ['b'], insert: [['x', { label: 'X', done: true }], ['y', { label: 'Y', done: false }]] });
      expect(calls).toEqual([]);
    });
    expect(calls).toEqual(['keys', 'item', 'wide']);
  });

  test('queues reentrant commits, isolates errors and permits late replacement registration', () => {
    const reports = /** @type {unknown[]} */ ([]), calls = /** @type {string[]} */ ([]);
    const c = tasks({ report: (error) => reports.push(error) }); let first = true;
    c.watchKeys((keys, previous, change) => {
      if (!change) return;
      calls.push(`keys:${change.inserted.join()}`);
      if (first) {
        first = false; c.watchItem('x', label, (value) => calls.push(`late:${value}`));
        c.update('x', { label: 'X2' });
        c.updateMembership({ insert: [['y', { label: 'Y', done: false }]] });
      }
    });
    c.watchItem('a', label, (value) => { if (value === 'A2') throw new Error('delivery'); });
    c.watchItem('a', label, (value) => { if (value === 'A2') calls.push('item:A2'); });
    c.batch(() => { c.update('a', { label: 'A2' }); c.updateMembership({ insert: [['x', { label: 'X', done: false }]] }); });
    expect(calls).toEqual(['keys:x', 'late:X', 'item:A2', 'late:X2', 'keys:y']);
    expect(reports).toHaveLength(1);
  });

  test('abort during delivery suppresses later callbacks and failed initial delivery releases ownership', () => {
    const c = tasks(), owner = new AbortController(), seen = vi.fn(), failure = new Error('initial');
    c.watchKeys((keys, previous, change) => { if (change) owner.abort(); });
    c.watchItem('a', label, seen, { signal: owner.signal });
    c.batch(() => { c.update('a', { label: 'A2' }); c.updateMembership({ remove: ['b'] }); });
    expect(seen).toHaveBeenCalledTimes(1);
    const initial = vi.fn(() => { throw failure; });
    expect(() => c.watchItem('a', label, initial)).toThrow(failure);
    c.update('a', { label: 'A3' }); expect(initial).toHaveBeenCalledTimes(1);
    expect(() => c.batch(() => c.watchItem('a', label, () => {}))).toThrow(TypeError);
    expect(() => c.batch(() => Promise.resolve())).toThrow(TypeError);
  });

  test('committed removal releases captures, selected values, records and old keys with a live owner', async () => {
    const c = tasks(), owner = new AbortController();
    const references = /** @type {WeakRef<object>[]} */ ([]);
    (() => {
      const capture = { marker: 'capture' }, selected = { marker: 'selected' };
      references.push(new WeakRef(capture), new WeakRef(selected), new WeakRef(/** @type {object} */ (c.get('b'))), new WeakRef(c.keys()));
      c.watchItem('b', { fields: ['label'], read: () => selected }, () => { expect(capture.marker).toBe('capture'); }, { signal: owner.signal });
    })();
    c.updateMembership({ remove: ['b'] });
    await collectGarbage(); expect(references.map((reference) => reference.deref())).toEqual([undefined, undefined, undefined, undefined]);
    expect(owner.signal.aborted).toBe(false); owner.abort(); expect(c.has('b')).toBe(false);
  });

  test('abort releases watchers without searching for once listeners already removed by dispatch', () => {
    const c = tasks(), owner = new AbortController(), detach = vi.spyOn(owner.signal, 'removeEventListener');
    const run = vi.fn();
    for (const key of c.keys()) c.watchItem(key, label, run, { signal: owner.signal });
    c.watchKeys(() => {}, { signal: owner.signal });
    c.watch({ fields: [], read: (view) => view.size }, () => {}, { signal: owner.signal });
    owner.abort();
    // The platform removed each {once:true} listener before entering its callback.
    expect(detach).not.toHaveBeenCalled();
    for (const key of c.keys()) c.update(key, { label: 'After abort' });
    expect(run).toHaveBeenCalledTimes(3);
  });

  test('rollback releases discarded replacement records and intermediate key arrays', async () => {
    const c = tasks(), original = c.keys(), b = c.get('b');
    const references = /** @type {WeakRef<object>[]} */ ([]);
    expect(() => c.batch(() => {
      c.updateMembership({ remove: ['b'], insert: [['b', { label: 'Temporary', done: true }]] });
      references.push(new WeakRef(c.keys()), new WeakRef(/** @type {object} */ (c.get('b'))));
      throw new Error('rollback');
    })).toThrow('rollback');
    await collectGarbage(); expect(references.map((reference) => reference.deref())).toEqual([undefined, undefined]);
    expect(c.keys()).toBe(original); expect(c.get('b')).toBe(b);
  });
});
