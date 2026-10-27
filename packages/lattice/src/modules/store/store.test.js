/** @file Verifies the store core against the state management contract. */
import { afterEach, describe, expect, test, vi } from 'vitest';
import { createStore } from './store.js';

/** @typedef {{ count: number, label: string, items: string[] }} State */

/** @returns {State} A fresh initial state. */
const initial = () => ({ count: 0, label: 'Acme', items: ['a'] });

/**
 * Creates a store whose delivery errors are collected instead of reported.
 * @returns {{ store: import('./store.js').Store<State>, errors: unknown[] }} The store and its reported errors.
 */
function storeWithReport() {
  /** @type {unknown[]} */
  const errors = [];
  const store = createStore(initial(), { report: (error) => errors.push(error) });
  return { store, errors };
}

/**
 * Wraps a selection's `read` and a `run` with call counters.
 * @template V
 * @param {import('./store.js').Selection<State, V>} selection The selection to count.
 * @returns {{ selection: import('./store.js').Selection<State, V>, reads: () => number, runs: V[], run: (value: V) => void }} The counted selection and records.
 */
function counted(selection) {
  let reads = 0;
  /** @type {V[]} */
  const runs = [];
  return {
    selection: {
      ...selection,
      read: (state) => {
        reads++;
        return selection.read(state);
      },
    },
    reads: () => reads,
    runs,
    run: (value) => runs.push(value),
  };
}

/** @type {import('./store.js').Selection<State, number>} */
const count = { fields: ['count'], read: (state) => state.count };
/** @type {import('./store.js').Selection<State, string>} */
const label = { fields: ['label'], read: (state) => state.label };

/**
 * Collects weak targets after the job that created them ends.
 * @returns {Promise<void>} Settles after forced garbage collection.
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

describe('once-listener cleanup', () => {
  test('normal abort performs no redundant explicit listener removal', () => {
    const store = createStore(initial()),
      controller = new AbortController();
    const detach = vi.spyOn(controller.signal, 'removeEventListener'),
      run = vi.fn();
    store.watch({ fields: ['count'], read: (state) => state.count }, run, {
      signal: controller.signal,
    });
    controller.abort();
    store.update({ count: 1 });
    expect(detach).not.toHaveBeenCalled();
    expect(run).toHaveBeenCalledTimes(1);
  });

  test.each([false, true])(
    'initial abort/throw preserves failure and ownership with aborted=%s',
    (abort) => {
      const store = createStore(initial()),
        controller = new AbortController(),
        failure = new Error('initial');
      const detach = vi.spyOn(controller.signal, 'removeEventListener');
      const run = vi.fn(() => {
        if (abort) controller.abort();
        throw failure;
      });
      expect(() =>
        store.watch({ fields: ['count'], read: (state) => state.count }, run, {
          signal: controller.signal,
        }),
      ).toThrow(failure);
      expect(detach).toHaveBeenCalledTimes(abort ? 0 : 1);
      store.update({ count: 1 });
      expect(run).toHaveBeenCalledTimes(1);
      expect(controller.signal.aborted).toBe(abort);
      controller.abort();
    },
  );
});

describe('snapshots', () => {
  test('copies the initial state into a frozen snapshot returned by identity until it changes', () => {
    const source = initial();
    const store = createStore(source);
    const first = store.get();
    expect(first).not.toBe(source);
    expect(Object.isFrozen(first)).toBe(true);
    expect(store.get()).toBe(first);
    store.update({ count: 1 });
    expect(store.get()).not.toBe(first);
    expect(Object.isFrozen(store.get())).toBe(true);
    expect(first.count).toBe(0);
    /** @type {State[]} */
    const inBatch = [];
    store.batch(() => {
      store.update({ count: 2 });
      inBatch.push(store.get());
    });
    expect(inBatch.every((snapshot) => Object.isFrozen(snapshot))).toBe(true);
    expect(Object.isFrozen(store.get())).toBe(true);
  });

  test('keeps the references of fields a commit did not change', () => {
    const store = createStore(initial());
    const items = store.get().items;
    store.update({ count: 1 });
    expect(store.get().items).toBe(items);
  });
});

describe('writes', () => {
  test('rejects an unknown key with a TypeError and changes nothing', () => {
    const { store } = storeWithReport();
    const { selection, run, runs } = counted(count);
    store.watch(selection, run);
    const before = store.get();
    expect(() => store.update(/** @type {any} */ ({ count: 5, missing: 1 }))).toThrow(TypeError);
    expect(store.get()).toBe(before);
    expect(runs).toEqual([0]);
  });

  test('ignores a write whose values are all unchanged: no snapshot, no read', () => {
    const { store } = storeWithReport();
    const { selection, run, reads } = counted(count);
    store.watch(selection, run);
    const before = store.get();
    store.update({ count: 0, label: 'Acme' });
    expect(store.get()).toBe(before);
    expect(reads()).toBe(1);
  });

  test('accepts a function patch that receives the current snapshot', () => {
    const store = createStore(initial());
    store.update((state) => ({ count: state.count + 2 }));
    expect(store.get().count).toBe(2);
  });

  test('reads each patched value once and records the values it read', () => {
    const store = createStore(initial());
    const runs = /** @type {number[]} */ ([]);
    store.watch(count, (value) => runs.push(value));
    let reads = 0;
    const patch = {
      get count() {
        reads++;
        return reads;
      },
    };
    store.update(patch);
    expect(reads).toBe(1);
    expect(store.get().count).toBe(1);
    expect(runs).toEqual([0, 1]);
  });

  test('allocates exactly one snapshot per changing write', () => {
    const store = createStore(initial());
    const seen = new Set([store.get()]);
    store.update({ count: 1 });
    seen.add(store.get());
    store.update({ count: 1 });
    seen.add(store.get());
    store.update({ count: 2, label: 'Example' });
    seen.add(store.get());
    expect(seen.size).toBe(3);
  });
});

describe('subscriptions', () => {
  test('runs immediately with the current value and an undefined previous value', () => {
    const store = createStore(initial());
    const run = vi.fn();
    store.watch(count, run);
    expect(run).toHaveBeenCalledExactlyOnceWith(0, undefined);
  });

  test('skips subscriptions none of whose fields changed, without calling read', () => {
    const store = createStore(initial());
    const watched = counted(count);
    store.watch(watched.selection, watched.run);
    store.update({ label: 'Example' });
    expect(watched.reads()).toBe(1);
    expect(watched.runs).toEqual([0]);
  });

  test('reads once per commit and runs only when the result changes under equals', () => {
    const store = createStore(initial());
    const parity = counted({ fields: ['count'], read: (state) => state.count % 2 });
    store.watch(parity.selection, parity.run);
    store.update({ count: 2 });
    store.update({ count: 3 });
    expect(parity.reads()).toBe(3);
    expect(parity.runs).toEqual([0, 1]);
  });

  test('passes the previous delivered value and uses a custom equals', () => {
    const store = createStore(initial());
    /** @type {[string, string | undefined][]} */
    const calls = [];
    store.watch(
      {
        fields: ['label'],
        read: (state) => state.label,
        equals: (a, b) => a.toLowerCase() === b.toLowerCase(),
      },
      (value, previous) => calls.push([value, previous]),
    );
    store.update({ label: 'ACME' });
    store.update({ label: 'Example' });
    expect(calls).toEqual([
      ['Acme', undefined],
      ['Example', 'Acme'],
    ]);
  });

  test('notifies a multi-field patch to each considered subscription once', () => {
    const store = createStore(initial());
    const both = counted({
      fields: ['count', 'label'],
      read: (state) => `${state.label}:${state.count}`,
    });
    store.watch(both.selection, both.run);
    store.update({ count: 1, label: 'Example' });
    expect(both.reads()).toBe(2);
    expect(both.runs).toEqual(['Acme:0', 'Example:1']);
  });

  test('rejects an unknown or empty field list with a TypeError and subscribes nothing', () => {
    const store = createStore(initial());
    const run = vi.fn();
    expect(() =>
      store.watch({ fields: /** @type {any} */ (['missing']), read: () => 1 }, run),
    ).toThrow(TypeError);
    expect(() => store.watch({ fields: [], read: () => 1 }, run)).toThrow(TypeError);
    store.update({ count: 1 });
    expect(run).not.toHaveBeenCalled();
  });

  test('subscribes nothing and calls nothing for an already aborted signal', () => {
    const store = createStore(initial());
    const watched = counted(count);
    store.watch(watched.selection, watched.run, { signal: AbortSignal.abort() });
    store.update({ count: 1 });
    expect(watched.reads()).toBe(0);
    expect(watched.runs).toEqual([]);
  });

  test('stops delivering after the signal aborts', () => {
    const store = createStore(initial());
    const controller = new AbortController();
    const watched = counted(count);
    store.watch(watched.selection, watched.run, { signal: controller.signal });
    controller.abort();
    store.update({ count: 1 });
    expect(watched.runs).toEqual([0]);
  });

  test('leaves no subscription when the initial read or run throws', () => {
    const store = createStore(initial());
    let reads = 0;
    /**
     * Subscribes a selection whose initial read throws.
     * @returns {void}
     */
    const failingRead = () =>
      store.watch(
        {
          fields: ['count'],
          read: () => {
            reads++;
            throw new Error('read');
          },
        },
        () => {},
      );
    expect(failingRead).toThrow('read');
    const run = vi.fn(() => {
      throw new Error('run');
    });
    expect(() => store.watch(count, run)).toThrow('run');
    store.update({ count: 1 });
    expect(reads).toBe(1);
    expect(run).toHaveBeenCalledTimes(1);
  });

  test('leaves nothing active when the initial run aborts its own signal', () => {
    const store = createStore(initial());
    const controller = new AbortController();
    const run = vi.fn(() => controller.abort());
    store.watch(count, run, { signal: controller.signal });
    store.update({ count: 1 });
    expect(run).toHaveBeenCalledTimes(1);
  });

  test('delivers a commit made by the initial run to the new subscription', () => {
    const store = createStore(initial());
    /** @type {number[]} */
    const runs = [];
    store.watch(count, (value) => {
      runs.push(value);
      if (value === 0) store.update({ count: 1 });
    });
    expect(runs).toEqual([0, 1]);
  });
});

describe('delivery', () => {
  test('notifies in subscription order', () => {
    const store = createStore(initial());
    /** @type {string[]} */
    const order = [];
    store.watch(count, () => order.push('first'));
    store.watch(count, () => order.push('second'));
    order.length = 0;
    store.update({ count: 1 });
    expect(order).toEqual(['first', 'second']);
  });

  test('returns a nested update before its subscribers run, and runs them before the outer update returns', () => {
    const store = createStore(initial());
    /** @type {string[]} */
    const order = [];
    store.watch(count, (value) => {
      if (value !== 1) return;
      store.update({ label: 'Nested' });
      order.push('nested update returned');
    });
    store.watch(label, (value) => order.push(`label ${value}`));
    order.length = 0;
    store.update({ count: 1 });
    order.push('outer update returned');
    expect(order).toEqual(['nested update returned', 'label Nested', 'outer update returned']);
  });

  test('delivers queued commits in order and never shows an older snapshot after a newer one', () => {
    const store = createStore(initial());
    /** @type {number[]} */
    const seen = [];
    store.watch(count, (value) => {
      if (value === 1) {
        store.update({ count: 2 });
        store.update({ count: 3 });
      }
    });
    store.watch(count, (value) => seen.push(value));
    store.update({ count: 1 });
    // Both queued commits read the latest snapshot, so the 2 is never seen after the 3.
    expect(seen).toEqual([0, 3]);
  });

  test('does not notify a subscription added during delivery for that commit', () => {
    const store = createStore(initial());
    const late = vi.fn();
    store.watch(count, (value) => {
      if (value === 1) store.watch(count, late);
    });
    store.update({ count: 1 });
    expect(late).toHaveBeenCalledExactlyOnceWith(1, undefined);
    store.update({ count: 2 });
    expect(late).toHaveBeenLastCalledWith(2, 1);
  });

  test('does not call a subscription aborted earlier in the same commit or in queued commits', () => {
    const store = createStore(initial());
    const controller = new AbortController();
    const removed = vi.fn();
    store.watch(count, (value) => {
      if (value !== 1) return;
      controller.abort();
      store.update({ count: 2 });
    });
    store.watch(count, removed, { signal: controller.signal });
    store.update({ count: 1 });
    expect(removed).toHaveBeenCalledExactlyOnceWith(0, undefined);
  });
});

describe('delivery errors', () => {
  test('reports a throwing read, equals or run once each, keeps delivering and returns from update', () => {
    const { store, errors } = storeWithReport();
    const after = vi.fn();
    const failures = ['read', 'equals', 'run'].map((name) => new Error(name));
    store.watch(
      {
        fields: ['count'],
        read: (state) => {
          if (state.count === 1) throw failures[0];
          return state.count;
        },
      },
      () => {},
    );
    store.watch(
      {
        fields: ['count'],
        read: (state) => state.count,
        equals: (a) => {
          if (a === 1) throw failures[1];
          return false;
        },
      },
      () => {},
    );
    store.watch(count, (value) => {
      if (value === 1) throw failures[2];
    });
    store.watch(count, after);
    expect(() => store.update({ count: 1 })).not.toThrow();
    expect(errors).toEqual(failures);
    expect(after).toHaveBeenLastCalledWith(1, 0);
    expect(store.get().count).toBe(1);
  });

  test('reports errors from queued commits without throwing from the outer update', () => {
    const { store, errors } = storeWithReport();
    const failure = new Error('queued');
    store.watch(count, (value) => {
      if (value === 1) store.update({ label: 'Queued' });
    });
    store.watch(label, (value) => {
      if (value === 'Queued') throw failure;
    });
    expect(() => store.update({ count: 1 })).not.toThrow();
    expect(errors).toEqual([failure]);
  });

  test('compares a failed subscription against its last successful value', () => {
    const { store } = storeWithReport();
    /** @type {[number, number | undefined][]} */
    const calls = [];
    store.watch(count, (value, previous) => {
      calls.push([value, previous]);
      if (value === 1) throw new Error('run');
    });
    store.update({ count: 1 });
    store.update({ count: 2 });
    expect(calls).toEqual([
      [0, undefined],
      [1, 0],
      [2, 0],
    ]);
  });

  test('sends a throwing report to the platform reportError and keeps delivering', () => {
    const reported = vi.fn();
    vi.stubGlobal('reportError', reported);
    const reportFailure = new Error('report');
    const store = createStore(initial(), {
      report: () => {
        throw reportFailure;
      },
    });
    const after = vi.fn();
    store.watch(count, (value) => {
      if (value === 1) throw new Error('run');
    });
    store.watch(count, after);
    store.update({ count: 1 });
    expect(reported).toHaveBeenCalledExactlyOnceWith(reportFailure);
    expect(after).toHaveBeenLastCalledWith(1, 0);
  });

  test('rethrows from a microtask when the platform has no reportError, and later writes are delivered', () => {
    vi.stubGlobal('reportError', undefined);
    /** @type {(() => void)[]} */
    const microtasks = [];
    vi.stubGlobal('queueMicrotask', (/** @type {() => void} */ callback) =>
      microtasks.push(callback),
    );
    const reportFailure = new Error('report');
    const store = createStore(initial(), {
      report: () => {
        throw reportFailure;
      },
    });
    const after = vi.fn();
    store.watch(count, (value) => {
      if (value === 1) throw new Error('run');
    });
    store.watch(count, after);
    store.update({ count: 1 });
    expect(microtasks).toHaveLength(1);
    expect(microtasks[0]).toThrow(reportFailure);
    store.update({ count: 2 });
    expect(after).toHaveBeenLastCalledWith(2, 1);
  });

  test('uses the fallback as the default report', () => {
    const reported = vi.fn();
    vi.stubGlobal('reportError', reported);
    const store = createStore(initial());
    const failure = new Error('run');
    store.watch(count, (value) => {
      if (value === 1) throw failure;
    });
    store.update({ count: 1 });
    expect(reported).toHaveBeenCalledExactlyOnceWith(failure);
  });

  test('performs an action’s side effects after its write even when a subscriber throws', () => {
    const { store } = storeWithReport();
    const persist = vi.fn();
    store.watch(count, (value) => {
      if (value === 1) throw new Error('run');
    });
    /** An action that writes its state, then performs a side effect. */
    const action = () => {
      store.update({ count: 1 });
      persist(store.get().count);
    };
    action();
    expect(persist).toHaveBeenCalledExactlyOnceWith(1);
  });
});

describe('deactivation', () => {
  test('releases callbacks at once, while the record may still wait to be removed', async () => {
    const store = createStore(initial());
    store.watch(count, () => {});
    store.watch(count, () => {});
    const controller = new AbortController();
    /** @type {WeakRef<object>} */
    let reference = new WeakRef({});
    (() => {
      const captured = { payload: new Array(1000).fill('acme') };
      reference = new WeakRef(captured);
      store.watch(count, () => captured.payload.length, { signal: controller.signal });
    })();
    // One inactive record among three is below the compaction threshold.
    controller.abort();
    await collectGarbage();
    expect(reference.deref()).toBeUndefined();
  });

  test('keeps delivering to the remaining subscriptions after many are removed', () => {
    const store = createStore(initial());
    const controllers = Array.from({ length: 50 }, () => new AbortController());
    for (const controller of controllers)
      store.watch(count, () => {}, { signal: controller.signal });
    const kept = vi.fn();
    store.watch(count, kept);
    for (const controller of controllers) controller.abort();
    store.update({ count: 1 });
    expect(kept).toHaveBeenLastCalledWith(1, 0);
  });

  test('releases a selected value when its callback aborts below the compaction threshold', async () => {
    const store = createStore(initial());
    store.watch(count, () => {});
    store.watch(count, () => {});
    const controller = new AbortController();
    let reference = new WeakRef({});
    store.watch(
      { fields: ['count'], read: (state) => ({ count: state.count }) },
      (value) => {
        if (value.count !== 1) return;
        reference = new WeakRef(value);
        controller.abort();
      },
      { signal: controller.signal },
    );
    store.update({ count: 1 });
    await collectGarbage();
    expect(reference.deref()).toBeUndefined();
    expect(store.get().count).toBe(1);
  });
});

describe('review regressions', () => {
  test('rolls back and delivers nothing when reading the result’s then throws', () => {
    const store = createStore(initial());
    const watched = counted(count);
    store.watch(watched.selection, watched.run);
    const before = store.get();
    const failure = new Error('then');
    const result = {
      get then() {
        throw failure;
      },
    };
    expect(() =>
      store.batch(
        /** @type {() => void} */ (
          () => {
            store.update({ count: 1 });
            return result;
          }
        ),
      ),
    ).toThrow(failure);
    expect(store.get()).toBe(before);
    expect(watched.reads()).toBe(1);
  });

  test('rejects a selection whose initial read writes to the store', () => {
    const store = createStore(initial());
    const run = vi.fn();
    expect(() =>
      store.watch(
        {
          fields: ['count'],
          read: (state) => {
            store.update({ count: state.count + 1 });
            return state.count;
          },
        },
        run,
      ),
    ).toThrow(TypeError);
    expect(run).not.toHaveBeenCalled();
  });

  test('does not run a subscription that aborts itself during read or equals', () => {
    const store = createStore(initial());
    for (const stage of ['read', 'equals']) {
      const controller = new AbortController();
      const run = vi.fn();
      store.watch(
        {
          fields: ['count'],
          read: (state) => {
            if (stage === 'read' && state.count > 0) controller.abort();
            return state.count;
          },
          equals: (a, b) => {
            if (stage === 'equals') controller.abort();
            return Object.is(a, b);
          },
        },
        run,
        { signal: controller.signal },
      );
      store.update((state) => ({ count: state.count + 1 }));
      expect(run).toHaveBeenCalledTimes(1);
    }
  });

  test('keeps delivering and returns from update when the platform reportError throws', () => {
    const reportFailure = new Error('platform');
    vi.stubGlobal('reportError', () => {
      throw reportFailure;
    });
    /** @type {(() => void)[]} */
    const microtasks = [];
    vi.stubGlobal('queueMicrotask', (/** @type {() => void} */ callback) =>
      microtasks.push(callback),
    );
    const store = createStore(initial());
    const queued = vi.fn();
    store.watch(count, (value) => {
      if (value !== 1) return;
      store.update({ label: 'Queued' });
      throw new Error('run');
    });
    store.watch(label, queued);
    expect(() => store.update({ count: 1 })).not.toThrow();
    expect(queued).toHaveBeenLastCalledWith('Queued', 'Acme');
    expect(microtasks).toHaveLength(1);
  });

  test('copies fields, so mutating the caller’s array later changes nothing', () => {
    const store = createStore(initial());
    const fields = /** @type {('count' | 'label')[]} */ (['count']);
    const run = vi.fn();
    store.watch({ fields, read: (state) => state.count }, run);
    fields[0] = 'label';
    store.update({ count: 1 });
    expect(run).toHaveBeenLastCalledWith(1, 0);
  });

  test('rejects a update made while a patch is being read, changing nothing', () => {
    const store = createStore(initial());
    const before = store.get();
    expect(() =>
      store.update({
        label: 'Acme',
        get count() {
          store.update({ label: 'Nested' });
          return 1;
        },
      }),
    ).toThrow(TypeError);
    expect(() =>
      store.update(() => {
        store.update({ label: 'Nested' });
        return { count: 1 };
      }),
    ).toThrow(TypeError);
    expect(store.get()).toBe(before);
  });

  test('changes nothing when a patch getter throws partway through', () => {
    const store = createStore(initial());
    const before = store.get();
    expect(() =>
      store.update({
        label: 'Example',
        /** @returns {number} Never returns; the getter fails. */
        get count() {
          throw new Error('getter');
        },
      }),
    ).toThrow('getter');
    expect(store.get()).toBe(before);
  });

  test('rejects a patch that is not an object, and an initial state that is not one', () => {
    const store = createStore(initial());
    for (const patch of [null, undefined, 'ab', 1]) {
      expect(() => store.update(/** @type {any} */ (patch))).toThrow(TypeError);
    }
    expect(() => store.update(/** @type {any} */ (() => undefined))).toThrow(TypeError);
    expect(() => createStore(/** @type {any} */ (null))).toThrow(TypeError);
    expect(() => createStore({ [Symbol('field')]: 1 })).toThrow(TypeError);
  });

  test('rejects fields that are not an array', () => {
    const store = createStore(initial());
    expect(() =>
      store.watch({ fields: /** @type {any} */ ('count'), read: () => 1 }, () => {}),
    ).toThrow(TypeError);
  });

  test('subscribes nothing when the signal aborts during the initial read', () => {
    const store = createStore(initial());
    const controller = new AbortController();
    const run = vi.fn();
    store.watch(
      {
        fields: ['count'],
        read: (state) => {
          controller.abort();
          return state.count;
        },
      },
      run,
      { signal: controller.signal },
    );
    store.update({ count: 1 });
    expect(run).not.toHaveBeenCalled();
  });
});

describe('batches', () => {
  test('delivers once for several writes, with one snapshot per changing write and earlier snapshots unchanged', () => {
    const store = createStore(initial());
    const both = counted({
      fields: ['count', 'label'],
      read: (state) => `${state.label}:${state.count}`,
    });
    store.watch(both.selection, both.run);
    const before = store.get();
    /** @type {State[]} */
    const snapshots = [];
    store.batch(() => {
      store.update({ count: 1 });
      snapshots.push(store.get());
      store.update({ count: 1 });
      snapshots.push(store.get());
      store.update({ label: 'Example' });
      snapshots.push(store.get());
    });
    expect(new Set([before, ...snapshots]).size).toBe(3);
    expect(snapshots[1]).toBe(snapshots[0]);
    expect(snapshots[0]?.label).toBe('Acme');
    expect(store.get()).toBe(snapshots[2]);
    expect(both.runs).toEqual(['Acme:0', 'Example:1']);
    expect(both.reads()).toBe(2);
  });

  test('delivers nothing when the net change is empty', () => {
    const store = createStore(initial());
    const watched = counted(count);
    store.watch(watched.selection, watched.run);
    store.batch(() => {
      store.update({ count: 1 });
      store.update({ count: 0 });
    });
    expect(watched.reads()).toBe(1);
  });

  test('restores the prior snapshot and delivers nothing when the callback throws', () => {
    const store = createStore(initial());
    const watched = counted(count);
    store.watch(watched.selection, watched.run);
    const before = store.get();
    expect(() =>
      store.batch(() => {
        store.update({ count: 1 });
        throw new Error('fail');
      }),
    ).toThrow('fail');
    expect(store.get()).toBe(before);
    expect(watched.reads()).toBe(1);
  });

  test('rolls back only a nested batch that throws and is caught, then commits the outer batch', () => {
    const store = createStore(initial());
    const both = counted({
      fields: ['count', 'label'],
      read: (state) => `${state.label}:${state.count}`,
    });
    store.watch(both.selection, both.run);
    store.batch(() => {
      store.update({ count: 1 });
      try {
        store.batch(() => {
          store.update({ label: 'Rolled back' });
          throw new Error('inner');
        });
      } catch {
        // The outer batch continues with its earlier writes.
      }
    });
    expect(store.get().count).toBe(1);
    expect(store.get().label).toBe('Acme');
    expect(both.runs).toEqual(['Acme:0', 'Acme:1']);
  });

  test('rolls back, delivers nothing and throws a TypeError when a callback returns a promise or thenable', () => {
    const store = createStore(initial());
    const watched = counted(count);
    store.watch(watched.selection, watched.run);
    const before = store.get();
    for (const result of [Promise.resolve(), { then: () => {} }]) {
      expect(() =>
        store.batch(
          /** @type {() => void} */ (
            () => {
              store.update({ count: 1 });
              return result;
            }
          ),
        ),
      ).toThrow(TypeError);
      expect(store.get()).toBe(before);
    }
    store.batch(() => {
      store.update({ label: 'Kept' });
      expect(() =>
        store.batch(
          /** @type {() => void} */ (
            () => {
              store.update({ count: 1 });
              return Promise.resolve();
            }
          ),
        ),
      ).toThrow(TypeError);
    });
    expect(store.get().count).toBe(0);
    expect(store.get().label).toBe('Kept');
    expect(watched.reads()).toBe(1);
  });

  test('throws a TypeError for watch inside a batch and leaves no subscription after a rollback', () => {
    const store = createStore(initial());
    const run = vi.fn();
    expect(() =>
      store.batch(() => {
        store.update({ count: 1 });
        store.watch(count, run);
      }),
    ).toThrow(TypeError);
    expect(store.get().count).toBe(0);
    store.update({ count: 2 });
    expect(run).not.toHaveBeenCalled();
  });

  test('queues a batch made during delivery like a update', () => {
    const store = createStore(initial());
    /** @type {string[]} */
    const order = [];
    store.watch(count, (value) => {
      if (value !== 1) return;
      store.batch(() => store.update({ label: 'Batched' }));
      order.push('nested batch returned');
    });
    store.watch(label, (value) => order.push(`label ${value}`));
    order.length = 0;
    store.update({ count: 1 });
    expect(order).toEqual(['nested batch returned', 'label Batched']);
  });

  test('releases the pre-batch snapshot after commit, empty commit and throw', async () => {
    /** @type {{ store: import('./store.js').Store<{ count: number }>, reference: WeakRef<object> }[]} */
    const cases = [];
    /**
     * Runs one batch case on its own store, keeping only a weak reference to
     * the snapshot it had before the batch, then moves the store past it.
     * @param {(store: import('./store.js').Store<{ count: number }>) => void} body The batch case.
     * @returns {void}
     */
    const run = (body) => {
      const store = createStore({ count: 0 });
      store.update({ count: 1 });
      cases.push({ store, reference: new WeakRef(store.get()) });
      try {
        store.batch(() => body(store));
      } catch {
        // A throwing batch is one of the cases.
      }
      store.update({ count: 9 });
    };
    run((store) => store.update({ count: 2 }));
    run(() => {});
    run((store) => {
      store.update({ count: 2 });
      throw new Error('fail');
    });
    await collectGarbage();
    expect(cases.map(({ reference }) => reference.deref())).toEqual([
      undefined,
      undefined,
      undefined,
    ]);
    expect(cases.map(({ store }) => store.get().count)).toEqual([9, 9, 9]);
  });
});
