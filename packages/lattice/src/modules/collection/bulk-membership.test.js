/** @file Exercises the final bulk candidate through the regular package entry. */
import { afterEach, describe, expect, test, vi } from 'vitest';
import { createCollection } from '@canonic2/lattice/collection';

/** @typedef {{label: string, done: boolean}} Task */
/** @typedef {import('./collection.js').Collection<Task>} Tasks */
/** @typedef {readonly [string, Task, (string | undefined)?]} Insertion */
/** @type {import('./collection.js').ItemSelection<Task, string>} */
const label = { fields: ['label'], read: (item) => item.label };

/**
 * A fresh fixture whose original incarnations are observable by identity.
 * @param {import('./collection.js').CollectionOptions} [options] Error owner.
 * @returns {Tasks} Three independent original tasks.
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
 * Ends the weak-reference creation job before forcing collection.
 * @returns {Promise<void>} Settles after one forced collection.
 */
async function collectGarbage() {
  const collect = /** @type {unknown} */ (Reflect.get(globalThis, 'gc'));
  if (typeof collect !== 'function') throw new Error('Unit tests require --expose-gc.');
  await new Promise((resolve) => setTimeout(resolve, 0));
  collect();
}

afterEach(() => vi.restoreAllMocks());

describe('bulk membership candidate', () => {
  test('detached calls preserve anchor groups, supplied order and immutable identities', () => {
    const c = tasks(),
      before = c.keys(),
      oldA = c.get('a'),
      survivor = c.get('b');
    const changes = vi.fn();
    c.watchKeys(changes);
    changes.mockClear();
    const item = { label: 'Replacement', done: true },
      apply = c.updateMembership;
    apply({
      remove: ['a'],
      insert: [
        ['x', { label: 'X', done: false }, 'c'],
        ['y', { label: 'Y', done: false }, 'b'],
        ['z', { label: 'Z', done: false }, 'c'],
        ['a', item, 'b'],
        ['tail', { label: 'Tail', done: false }],
      ],
    });
    expect(c.keys()).toEqual(['y', 'a', 'b', 'x', 'z', 'c', 'tail']);
    expect(c.keys()).not.toBe(before);
    expect(c.keys()).toBe(c.keys());
    expect(before).toEqual(['a', 'b', 'c']);
    expect(c.get('b')).toBe(survivor);
    expect(c.get('a')).not.toBe(oldA);
    expect(Object.isFrozen(c.keys())).toBe(true);
    expect(Object.isFrozen(c.get('a'))).toBe(true);
    item.label = 'Caller mutation';
    expect(c.get('a')?.label).toBe('Replacement');
    expect(changes).toHaveBeenCalledTimes(1);
    expect(changes.mock.calls[0]?.[2]).toEqual({
      removed: ['a'],
      inserted: ['x', 'y', 'z', 'a', 'tail'],
    });
  });

  test('ordinary and bulk reads inside a batch remain immediate; delivery commits once', () => {
    const c = tasks(),
      run = vi.fn();
    c.watchKeys(run);
    run.mockClear();
    c.batch(() => {
      c.insert('d', { label: 'D', done: false }, 'b');
      expect(c.keys()).toEqual(['a', 'd', 'b', 'c']);
      const pending = c.keys();
      c.updateMembership({ remove: ['b'], insert: [['b', { label: 'New B', done: false }, 'c']] });
      expect(c.keys()).not.toBe(pending);
      expect(c.keys()).toEqual(['a', 'd', 'b', 'c']);
      expect(pending).toEqual(['a', 'd', 'b', 'c']);
      expect(c.get('b')?.label).toBe('New B');
      c.remove('d');
      expect(c.keys()).toEqual(['a', 'b', 'c']);
      expect(run).not.toHaveBeenCalled();
    });
    expect(run).toHaveBeenCalledTimes(1);
    expect(run.mock.calls[0]?.[2]).toEqual({ removed: ['b'], inserted: ['b'] });
  });

  test('empty requests preserve keys and stay silent', () => {
    const c = tasks(),
      before = c.keys(),
      run = vi.fn();
    c.watchKeys(run);
    run.mockClear();
    c.updateMembership({});
    c.updateMembership({ remove: [], insert: [] });
    expect(c.keys()).toBe(before);
    expect(run).not.toHaveBeenCalled();
  });

  test('validation reads getters once against pre-call state and rejects reentrant writes', () => {
    const c = tasks(),
      before = c.keys(),
      reads = { remove: 0, insert: 0, label: 0 };
    c.updateMembership({
      /** @returns {string[]} Existing key to replace. */
      get remove() {
        reads.remove++;
        expect(c.keys()).toBe(before);
        return ['b'];
      },
      /** @returns {readonly Insertion[]} Copies whose getters must stay guarded. */
      get insert() {
        reads.insert++;
        for (const write of [
          () => c.update('a', { done: true }),
          () => c.remove('a'),
          () => c.insert('x', { label: 'X', done: false }),
          () => c.reorder(before),
          () => c.batch(() => {}),
          () => c.updateMembership({}),
        ])
          expect(write).toThrow(TypeError);
        return [
          [
            'b',
            {
              /** @returns {string} Copied once against the original incarnation. */
              get label() {
                reads.label++;
                expect(c.get('b')?.label).toBe('B');
                return 'New B';
              },
              done: false,
            },
            'c',
          ],
        ];
      },
    });
    expect(reads).toEqual({ remove: 1, insert: 1, label: 1 });
    expect(c.get('b')?.label).toBe('New B');
  });

  test('insertion tuple accessors are read only once', () => {
    const c = tasks();
    /** @type {[string, Task, string]} */ const tuple = ['x', { label: 'X', done: false }, 'c'];
    /** @type {Map<string, number>} */ const reads = new Map();
    const entry = new Proxy(tuple, {
      get(target, key, receiver) {
        if (typeof key === 'string' && ['length', '0', '1', '2'].includes(key)) {
          const count = (reads.get(key) ?? 0) + 1;
          reads.set(key, count);
          if (count > 1) throw new Error(`Repeated input read: ${key}`);
        }
        return Reflect.get(target, key, receiver);
      },
    });
    c.updateMembership({ insert: [entry] });
    expect(c.keys()).toEqual(['a', 'b', 'x', 'c']);
    expect(Object.fromEntries(reads)).toEqual({ length: 1, 0: 1, 1: 1, 2: 1 });
  });

  test.each([
    undefined,
    null,
    { remove: 1 },
    { remove: 'a' },
    { insert: 'a' },
    { remove: ['absent'] },
    { remove: ['a', 'a'] },
    { insert: [['a', {}]] },
    {
      insert: [
        ['x', {}],
        ['x', {}],
      ],
    },
    { insert: [['x']] },
    { insert: [['x', {}, 'a', 'extra']] },
    { insert: [['x', {}, 'absent']] },
    { remove: ['a'], insert: [['x', {}, 'a']] },
    {
      insert: [
        ['x', {}, 'y'],
        ['y', {}],
      ],
    },
    { insert: [['x', null]] },
    { insert: [['x', { [Symbol('field')]: true }]] },
  ])('invalid request leaves the complete original state intact: %j', (request) => {
    const c = tasks(),
      before = c.keys(),
      records = before.map((key) => c.get(key)),
      run = vi.fn();
    c.watchKeys(run);
    run.mockClear();
    expect(() => Reflect.apply(c.updateMembership, undefined, [request])).toThrow(TypeError);
    expect(c.keys()).toBe(before);
    expect(before.map((key) => c.get(key))).toEqual(records);
    for (let at = 0; at < before.length; at++) expect(c.get(before[at] ?? '')).toBe(records[at]);
    expect(run).not.toHaveBeenCalled();
  });

  test('throwing input after a copied gain preserves earlier outer writes and restores the guard', () => {
    const c = tasks(),
      failure = new Error('record getter'),
      before = c.keys();
    c.batch(() => {
      c.update('a', { done: true });
      const pendingA = c.get('a');
      expect(() =>
        c.updateMembership({
          remove: ['b'],
          insert: [
            ['x', { label: 'Copied first', done: false }],
            [
              'y',
              {
                /** @returns {never} Aborts validation after a prior gain was copied. */
                get label() {
                  throw failure;
                },
                done: false,
              },
            ],
          ],
        }),
      ).toThrow(failure);
      expect(c.keys()).toBe(before);
      expect(c.get('a')).toBe(pendingA);
      expect(c.has('b')).toBe(true);
      expect(c.has('x')).toBe(false);
      c.update('c', { done: true });
    });
    expect(c.get('a')?.done).toBe(true);
    expect(c.get('c')?.done).toBe(true);
  });

  test('the write guard is collection-local and also covers ordinary function patches', () => {
    const c = tasks(),
      other = tasks(),
      before = c.get('a');
    c.update('a', () => {
      expect(() => c.updateMembership({ remove: ['b'] })).toThrow(TypeError);
      return {};
    });
    expect(c.get('a')).toBe(before);
    expect(c.has('b')).toBe(true);
    c.updateMembership({
      /** @returns {readonly Insertion[]} Another collection has an independent guard. */
      get insert() {
        other.updateMembership({ remove: ['b'] });
        expect(c.has('b')).toBe(true);
        expect(other.has('b')).toBe(false);
        return [['x', { label: 'X', done: false }]];
      },
    });
    expect(c.keys()).toEqual(['a', 'b', 'c', 'x']);
    expect(other.keys()).toEqual(['a', 'c']);
  });

  test('nested replacement rollback restores original records, order and live subscriptions', () => {
    const c = tasks(),
      controller = new AbortController(),
      run = vi.fn();
    const detach = vi.spyOn(controller.signal, 'removeEventListener');
    c.watchItem('b', label, run, { signal: controller.signal });
    const before = c.keys(),
      original = c.get('b'),
      failure = new Error('savepoint');
    c.batch(() => {
      expect(() =>
        c.batch(() => {
          c.updateMembership({
            remove: ['b'],
            insert: [['b', { label: 'Transient', done: false }, 'c']],
          });
          expect(c.get('b')).not.toBe(original);
          expect(detach).not.toHaveBeenCalled();
          throw failure;
        }),
      ).toThrow(failure);
      expect(c.keys()).toBe(before);
      expect(c.get('b')).toBe(original);
      c.update('b', { label: 'Restored watcher' });
    });
    expect(run.mock.calls.map(([value]) => value)).toEqual(['B', 'Restored watcher']);
    expect(detach).not.toHaveBeenCalled();
    controller.abort();
    expect(detach).not.toHaveBeenCalled();
  });

  test('aborting during a rolled-back replacement does not revive the subscription', () => {
    const c = tasks(),
      controller = new AbortController(),
      run = vi.fn(),
      original = c.get('b');
    c.watchItem('b', label, run, { signal: controller.signal });
    expect(() =>
      c.batch(() => {
        c.updateMembership({ remove: ['b'], insert: [['b', { label: 'Transient', done: false }]] });
        controller.abort();
        throw new Error('rollback');
      }),
    ).toThrow('rollback');
    expect(c.get('b')).toBe(original);
    c.update('b', { label: 'After rollback' });
    expect(run).toHaveBeenCalledTimes(1);
  });

  test('abort during a failing planner stays ended without publishing the plan', () => {
    const c = tasks(),
      owner = new AbortController(),
      run = vi.fn(),
      before = c.keys(),
      original = c.get('b');
    c.watchItem('b', label, run, { signal: owner.signal });
    expect(() =>
      c.updateMembership({
        remove: ['b'],
        insert: [
          [
            'x',
            {
              /** @returns {never} Ends subscription ownership before failing validation. */
              get label() {
                owner.abort();
                throw new Error('input');
              },
              done: false,
            },
          ],
        ],
      }),
    ).toThrow('input');
    expect(c.keys()).toBe(before);
    expect(c.get('b')).toBe(original);
    c.update('b', { label: 'After abort' });
    expect(run).toHaveBeenCalledTimes(1);
  });

  test('committed replacement detaches old ownership without transferring item subscriptions', () => {
    const c = tasks(),
      owner = new AbortController(),
      old = vi.fn(),
      fresh = vi.fn();
    const detach = vi.spyOn(owner.signal, 'removeEventListener');
    c.watchItem('b', label, old, { signal: owner.signal });
    c.updateMembership({ remove: ['b'], insert: [['b', { label: 'New B', done: false }]] });
    expect(detach).toHaveBeenCalledTimes(1);
    expect(owner.signal.aborted).toBe(false);
    c.watchItem('b', label, fresh, { signal: owner.signal });
    c.update('b', { label: 'Next B' });
    expect(old.mock.calls.map(([value]) => value)).toEqual(['B']);
    expect(fresh.mock.calls.map(([value]) => value)).toEqual(['New B', 'Next B']);
    owner.abort();
    expect(detach).toHaveBeenCalledTimes(1);
  });

  test('reentrant membership commits preserve previous keys and late-subscription versions', () => {
    const c = tasks(),
      owner = new AbortController();
    /** @type {string[][]} */ const history = [];
    /** @type {(readonly string[] | undefined)[]} */ const previous = [];
    const late = vi.fn();
    c.watchKeys(
      (keys, before, change) => {
        history.push([...keys]);
        previous.push(before);
        if (change?.removed.includes('b')) {
          c.updateMembership({ insert: [['x', { label: 'X', done: false }]] });
          c.watchKeys(late, { signal: owner.signal });
        }
      },
      { signal: owner.signal },
    );
    c.updateMembership({ remove: ['b'] });
    expect(history).toEqual([
      ['a', 'b', 'c'],
      ['a', 'c'],
      ['a', 'c', 'x'],
    ]);
    expect(previous).toEqual([undefined, ['a', 'b', 'c'], ['a', 'c']]);
    expect(late).toHaveBeenCalledTimes(1);
    c.updateMembership({ remove: ['x'] });
    expect(late).toHaveBeenCalledTimes(2);
    owner.abort();
  });

  test('delivery errors are reported after a complete plan and cannot block later commits', () => {
    const report = vi.fn(),
      c = tasks({ report }),
      failure = new Error('delivery'),
      later = vi.fn();
    c.watchKeys((keys, before, change) => {
      if (change) throw failure;
    });
    c.watchKeys(later);
    c.updateMembership({ remove: ['b'], insert: [['x', { label: 'X', done: false }]] });
    expect(c.keys()).toEqual(['a', 'c', 'x']);
    expect(report).toHaveBeenCalledWith(failure);
    expect(later).toHaveBeenCalledTimes(2);
    c.updateMembership({ remove: ['x'] });
    expect(c.keys()).toEqual(['a', 'c']);
    expect(later).toHaveBeenCalledTimes(3);
  });

  test('transient membership is silent and thenable batches restore exact identities', () => {
    const c = tasks(),
      before = c.keys(),
      record = c.get('b'),
      run = vi.fn();
    c.watchKeys(run);
    run.mockClear();
    c.batch(() => {
      c.updateMembership({ insert: [['x', { label: 'X', done: false }]] });
      c.updateMembership({ remove: ['x'] });
    });
    expect(c.keys()).toBe(before);
    expect(run).not.toHaveBeenCalled();
    expect(() =>
      c.batch(() => {
        c.updateMembership({ remove: ['b'], insert: [['b', { label: 'Transient', done: false }]] });
        return {
          /** @returns {void} Must never be invoked; the batch rejects thenables. */
          then() {},
        };
      }),
    ).toThrow(TypeError);
    expect(c.keys()).toBe(before);
    expect(c.get('b')).toBe(record);
    expect(run).not.toHaveBeenCalled();
  });

  test.each(['direct', 'commit', 'rollback'])(
    'releases obsolete snapshots and records after %s',
    async (mode) => {
      const c = tasks();
      /** @type {WeakRef<object>[]} */ const references = [];
      (() => {
        if (mode !== 'rollback') {
          references.push(new WeakRef(c.keys()), new WeakRef(/** @type {Task} */ (c.get('b'))));
        }
        /** @returns {void} Publishes a replacement or throws back to a savepoint. */
        const replace = () => {
          c.updateMembership({
            remove: ['b'],
            insert: [['b', { label: 'Replacement', done: false }]],
          });
          if (mode === 'rollback') {
            references.push(new WeakRef(c.keys()), new WeakRef(/** @type {Task} */ (c.get('b'))));
            throw new Error('rollback');
          }
        };
        if (mode === 'direct') replace();
        else if (mode === 'commit') c.batch(replace);
        else expect(() => c.batch(replace)).toThrow('rollback');
      })();
      await collectGarbage();
      expect(references.map((reference) => reference.deref())).toEqual([undefined, undefined]);
      expect(c.size).toBe(3);
      expect(c.get('b')?.label).toBe(mode === 'rollback' ? 'B' : 'Replacement');
    },
  );

  test('teardown releases the collection after replacement while the caller still holds its signal', async () => {
    const owner = new AbortController();
    let reference = new WeakRef({});
    (() => {
      const c = tasks();
      reference = new WeakRef(c);
      for (const key of c.keys()) c.watchItem(key, label, () => {}, { signal: owner.signal });
      c.watchKeys(() => {}, { signal: owner.signal });
      c.watch({ fields: [], read: (view) => view.size }, () => {}, { signal: owner.signal });
      c.updateMembership({
        remove: [...c.keys()],
        insert: [['new', { label: 'New', done: false }]],
      });
      owner.abort();
    })();
    await collectGarbage();
    expect(reference.deref()).toBeUndefined();
    expect(owner.signal.aborted).toBe(true);
  });
});
