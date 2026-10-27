/** @file Product aggregate commit, work-bound, recovery and lifetime contracts. */
import { expect, test, vi } from 'vitest';
import { createCollection } from '@canonic2/lattice/collection';
import { checkSelection } from '@canonic2/lattice/testing';
import { entries, reduction, recompute } from '../../../tests/aggregate-fixtures.js';

test.each(/** @type {const} */ (['count', 'sum']))(
  '%s visits only net relevant records',
  (kind) => {
    const collection = createCollection(entries(100));
    const owner = new AbortController();
    const descriptor = reduction(kind);
    const project = descriptor.read;
    descriptor.read = vi.fn(project);
    descriptor.add = vi.fn(descriptor.add);
    descriptor.remove = vi.fn(descriptor.remove);
    const run = vi.fn();
    collection.watchAggregate(descriptor, run, { signal: owner.signal });
    expect(descriptor.read).toHaveBeenCalledTimes(100);
    expect(descriptor.add).toHaveBeenCalledTimes(100);
    vi.clearAllMocks();
    collection.update('t0', kind === 'count' ? { done: false } : { cents: 7 });
    expect(descriptor.read).toHaveBeenCalledTimes(2);
    expect(descriptor.add).toHaveBeenCalledTimes(1);
    expect(descriptor.remove).toHaveBeenCalledTimes(1);
    expect(run).toHaveBeenLastCalledWith(recompute(collection, kind), kind === 'count' ? 66 : 4950);
    vi.clearAllMocks();
    collection.update('t0', { label: 'Changed' });
    collection.reorder([...collection.keys()].reverse());
    collection.batch(() => {
      collection.update('t1', { cents: 20 });
      collection.update('t1', { cents: 1 });
      collection.insert('transient', { done: false, cents: 3, label: 'Transient' });
      collection.remove('transient');
    });
    expect(descriptor.read).not.toHaveBeenCalled();
    expect(run).not.toHaveBeenCalled();
    collection.batch(() => {
      for (let index = 0; index < 10; index++) {
        collection.update(
          `t${index}`,
          kind === 'count' ? { done: index % 3 !== 0 } : { cents: index + 20 },
        );
      }
    });
    expect(descriptor.read).toHaveBeenCalledTimes(kind === 'count' ? 18 : 20);
    expect(run).toHaveBeenCalledTimes(1);
    expect(run.mock.calls.at(-1)?.[0]).toBe(recompute(collection, kind));
    owner.abort();
  },
);

test.each(/** @type {const} */ (['count', 'sum']))(
  '%s handles membership, key reuse and rollback',
  (kind) => {
    const collection = createCollection(entries(3));
    const owner = new AbortController();
    const run = vi.fn();
    collection.watchAggregate(reduction(kind), run, { signal: owner.signal });
    const keys = collection.keys();
    const original = collection.get('t0');
    expect(() =>
      collection.batch(() => {
        collection.update('t0', { cents: 100 });
        collection.remove('t1');
        throw new Error('rollback');
      }),
    ).toThrow('rollback');
    expect(collection.keys()).toBe(keys);
    expect(collection.get('t0')).toBe(original);
    expect(run).toHaveBeenCalledTimes(1);
    collection.batch(() => {
      collection.update('t0', { done: false, cents: 50 });
      collection.remove('t0');
      collection.insert('new', { done: true, cents: 1, label: 'New' });
      collection.update('new', { done: false, cents: 20 });
      try {
        collection.batch(() => {
          collection.remove('t1');
          throw new Error('inner');
        });
      } catch {
        /* The outer changes still commit. */
      }
    });
    expect(run.mock.calls.at(-1)?.[0]).toBe(recompute(collection, kind));
    collection.updateMembership({
      remove: ['new'],
      insert: [['new', { done: true, cents: 8, label: 'Replacement' }]],
    });
    expect(run.mock.calls.at(-1)?.[0]).toBe(recompute(collection, kind));
    expect(() =>
      collection.batch(() => {
        collection.update('new', { cents: 99 });
        return Promise.resolve();
      }),
    ).toThrow(TypeError);
    expect(run.mock.calls.at(-1)?.[0]).toBe(recompute(collection, kind));
    owner.abort();
  },
);

test('initialization and dirty recovery follow current key order', () => {
  /** @type {unknown[]} */
  const errors = [];
  const collection = createCollection(entries(3), { report: (error) => errors.push(error) });
  const owner = new AbortController();
  const descriptor = reduction('sum');
  const read = descriptor.read;
  /** @type {number[]} */
  const visited = [];
  let fail = false;
  descriptor.read = (item) => {
    if (fail) throw new Error('dirty');
    visited.push(item.cents);
    return read(item);
  };
  collection.reorder(['t2', 't0', 't1']);
  const run = vi.fn();
  collection.watchAggregate(descriptor, run, { signal: owner.signal });
  expect(visited).toEqual([2, 0, 1]);
  fail = true;
  collection.update('t0', { cents: 10 });
  expect(errors).toHaveLength(1);
  expect(run).toHaveBeenCalledTimes(1);
  visited.length = 0;
  fail = false;
  collection.update('t1', { cents: 20 });
  expect(visited).toEqual([2, 10, 20]);
  expect(run).toHaveBeenLastCalledWith(32, 3);
  owner.abort();
});

test('reentrant writes deliver committed prefixes in wide registration order', () => {
  const collection = createCollection(entries(1));
  const owner = new AbortController();
  /** @type {unknown[][]} */
  const seen = [];
  collection.watchItem(
    't0',
    { fields: ['cents'], read: (item) => item.cents },
    (value) => {
      if (value === 1) collection.update('t0', { cents: 2 });
    },
    { signal: owner.signal },
  );
  collection.watch(
    { fields: ['cents'], read: (view) => view.get('t0')?.cents },
    (value) => seen.push(['scan', value]),
    { signal: owner.signal },
  );
  collection.watchAggregate(
    reduction('sum'),
    (value, previous) => seen.push(['incremental', value, previous]),
    { signal: owner.signal },
  );
  seen.length = 0;
  collection.update('t0', { cents: 1 });
  expect(seen).toEqual([
    ['scan', 2],
    ['incremental', 1, 0],
    ['incremental', 2, 1],
  ]);
  owner.abort();
});

test('registration during delivery skips commits already reflected in initialization', () => {
  const collection = createCollection(entries(1));
  const owner = new AbortController();
  const run = vi.fn();
  let installed = false;
  collection.watch(
    { fields: ['cents'], read: (view) => view.get('t0')?.cents },
    (value) => {
      if (value === 1 && !installed) {
        installed = true;
        collection.update('t0', { cents: 2 });
        collection.watchAggregate(reduction('sum'), run, { signal: owner.signal });
      }
    },
    { signal: owner.signal },
  );
  collection.update('t0', { cents: 1 });
  collection.update('t0', { cents: 3 });
  expect(run.mock.calls.map(([value]) => value)).toEqual([2, 3]);
  owner.abort();
});

test('aborting an aggregate during maintenance does not skip later aggregates', () => {
  const collection = createCollection(entries(2));
  const firstOwner = new AbortController();
  const laterOwner = new AbortController();
  const first = reduction('sum');
  const read = first.read;
  let abort = false;
  first.read = (item) => {
    if (abort) firstOwner.abort();
    return read(item);
  };
  collection.watchAggregate(first, () => {}, { signal: firstOwner.signal });
  const later = vi.fn();
  collection.watchAggregate(reduction('sum'), later, { signal: laterOwner.signal });
  abort = true;
  collection.update('t0', { cents: 7 });
  expect(later).toHaveBeenLastCalledWith(8, 1);
  collection.update('t1', { cents: 9 });
  expect(later).toHaveBeenLastCalledWith(16, 8);
  laterOwner.abort();
});

test('unaffected and aborted aggregates do not misalign committed result delivery', () => {
  const collection = createCollection(entries(2));
  const owner = new AbortController();
  const abortedOwner = new AbortController();
  const count = vi.fn();
  const doomed = vi.fn();
  const sum = vi.fn();
  collection.watchAggregate(reduction('count'), count, { signal: owner.signal });
  collection.watchAggregate(reduction('sum'), doomed, { signal: abortedOwner.signal });
  collection.watchAggregate(reduction('sum'), sum, { signal: owner.signal });
  collection.watchKeys(
    () => {
      if (collection.size === 3) abortedOwner.abort();
    },
    { signal: owner.signal },
  );
  collection.update('t0', { cents: 7 });
  expect(count).toHaveBeenCalledTimes(1);
  expect(sum).toHaveBeenLastCalledWith(8, 1);
  collection.insert('next', { done: true, cents: 3, label: 'Next' });
  expect(doomed).toHaveBeenCalledTimes(2);
  expect(count).toHaveBeenCalledTimes(1);
  expect(sum).toHaveBeenLastCalledWith(11, 8);
  owner.abort();
});

test.each(/** @type {const} */ (['read', 'add', 'remove']))(
  '%s failure discards partial reduction and recovers once',
  (name) => {
    /** @type {unknown[]} */
    const errors = [];
    const collection = createCollection(entries(4), { report: (error) => errors.push(error) });
    const owner = new AbortController();
    const descriptor = reduction('sum');
    const original = descriptor[name];
    let fail = false;
    Reflect.set(
      descriptor,
      name,
      /**
       * @param {unknown[]} args Reducer arguments.
       * @returns {unknown} Result or injected error.
       */ (...args) => {
        if (fail) throw new Error('injected');
        return Reflect.apply(original, undefined, args);
      },
    );
    const run = vi.fn();
    collection.watchAggregate(descriptor, run, { signal: owner.signal });
    fail = true;
    collection.batch(() => {
      collection.update('t0', { cents: 10 });
      collection.update('t1', { cents: 11 });
    });
    expect(errors).toHaveLength(1);
    expect(run).toHaveBeenCalledTimes(1);
    collection.update('t0', { label: 'Unrelated' });
    expect(errors).toHaveLength(1);
    fail = false;
    collection.update('t2', { cents: 20 });
    expect(run.mock.calls.at(-1)?.[0]).toBe(recompute(collection, 'sum'));
    owner.abort();
  },
);

test('delivery errors preserve healthy state and previous successful value', () => {
  /** @type {unknown[]} */
  const errors = [];
  const collection = createCollection(entries(3), { report: (error) => errors.push(error) });
  const owner = new AbortController();
  const descriptor = reduction('sum');
  let failEquals = false;
  let failRun = false;
  descriptor.equals = (a, b) => {
    if (failEquals) throw new Error('equals');
    return a === b;
  };
  const run = vi.fn((/** @type {number} */ _value, /** @type {number | undefined} */ _previous) => {
    if (failRun) throw new Error('run');
  });
  collection.watchAggregate(descriptor, run, { signal: owner.signal });
  failEquals = true;
  collection.update('t0', { cents: 7 });
  failEquals = false;
  failRun = true;
  collection.update('t1', { cents: 9 });
  failRun = false;
  collection.update('t2', { cents: 10 });
  expect(errors).toHaveLength(2);
  expect(run).toHaveBeenLastCalledWith(26, 3);
  owner.abort();
});

test.each(/** @type {const} */ (['initial', 'read', 'add', 'remove', 'equals']))(
  '%s callbacks cannot write or publish after abort',
  (name) => {
    const collection = createCollection(entries(3));
    const owner = new AbortController();
    const descriptor = reduction('sum');
    const original = descriptor[name] ?? Object.is;
    let abort = name === 'initial';
    Reflect.set(
      descriptor,
      name,
      /**
       * @param {unknown[]} args Computational arguments.
       * @returns {unknown} Original result.
       */ (...args) => {
        expect(() => collection.update('t0', { cents: 99 })).toThrow(TypeError);
        if (abort) owner.abort();
        return Reflect.apply(original, undefined, args);
      },
    );
    const run = vi.fn();
    collection.watchAggregate(descriptor, run, { signal: owner.signal });
    if (abort) {
      expect(run).not.toHaveBeenCalled();
      return;
    }
    abort = true;
    collection.update('t0', { cents: 7 });
    expect(run).toHaveBeenCalledTimes(1);
    collection.update('t1', { cents: 9 });
    expect(collection.get('t1')?.cents).toBe(9);
    owner.abort();
  },
);

test('already-aborted and failing initialization leave no subscription', () => {
  const collection = createCollection(entries(3));
  const owner = new AbortController();
  const run = vi.fn();
  const descriptor = reduction('sum');
  descriptor.initial = () => {
    throw new Error('initial');
  };
  expect(() => collection.watchAggregate(descriptor, run)).toThrow('initial');
  owner.abort();
  descriptor.read = vi.fn(descriptor.read);
  collection.watchAggregate(descriptor, run, { signal: owner.signal });
  expect(descriptor.read).not.toHaveBeenCalled();
  expect(run).not.toHaveBeenCalled();
  expect(() => collection.batch(() => collection.watchAggregate(reduction('sum'), run))).toThrow(
    TypeError,
  );
  collection.update('t0', { cents: 5 });
  expect(run).not.toHaveBeenCalled();
});

test.each(/** @type {const} */ (['initial', 'read', 'add']))(
  'abort during initialization %s leaves no owner',
  (name) => {
    const collection = createCollection(entries(3));
    const owner = new AbortController();
    const descriptor = reduction('sum');
    const original = descriptor[name];
    Reflect.set(
      descriptor,
      name,
      /**
       * @param {unknown[]} args Initialization arguments.
       * @returns {unknown} Original result after cancellation.
       */ (...args) => {
        owner.abort();
        return Reflect.apply(original, undefined, args);
      },
    );
    const run = vi.fn();
    collection.watchAggregate(descriptor, run, { signal: owner.signal });
    collection.update('t0', { cents: 5 });
    expect(run).not.toHaveBeenCalled();
  },
);

test('reporter can queue recovery, and throwing reporters reach the platform', () => {
  const owner = new AbortController();
  let fail = false;
  const collection = createCollection(entries(2), {
    report: () => {
      fail = false;
      collection.update('t0', { cents: 8 });
    },
  });
  const descriptor = reduction('sum');
  const read = descriptor.read;
  descriptor.read = (item) => {
    if (fail) throw new Error('injected');
    return read(item);
  };
  const run = vi.fn();
  collection.watchAggregate(descriptor, run, { signal: owner.signal });
  fail = true;
  collection.update('t0', { cents: 7 });
  expect(run.mock.calls.map(([value]) => value)).toEqual([1, 9]);
  owner.abort();
  const platform = vi.fn();
  vi.stubGlobal('reportError', platform);
  try {
    const failure = new Error('reporter');
    const other = createCollection(entries(2), {
      report: () => {
        throw failure;
      },
    });
    const otherOwner = new AbortController();
    const otherDescriptor = reduction('sum');
    const original = otherDescriptor.read;
    let bad = false;
    otherDescriptor.read = (item) => {
      if (bad) throw new Error('read');
      return original(item);
    };
    other.watchAggregate(otherDescriptor, () => {}, { signal: otherOwner.signal });
    bad = true;
    other.update('t0', { cents: 7 });
    expect(platform).toHaveBeenCalledExactlyOnceWith(failure);
    otherOwner.abort();
  } finally {
    vi.unstubAllGlobals();
  }
});

test('aborting during delivery releases a structured accumulator while collection stays live', async () => {
  const collection = createCollection(entries(2));
  const owner = new AbortController();
  /** @type {WeakRef<{ sum: number }> | undefined} */
  let reference;
  collection.watchAggregate(
    {
      fields: ['cents'],
      initial: () => ({ sum: 0 }),
      read: (item) => item.cents,
      add: (total, value) => ({ sum: total.sum + value }),
      remove: (total, value) => ({ sum: total.sum - value }),
    },
    (value) => {
      if (value.sum !== 1) {
        reference = new WeakRef(value);
        owner.abort();
      }
    },
    { signal: owner.signal },
  );
  collection.update('t0', { cents: 7 });
  expect(reference).toBeInstanceOf(WeakRef);
  for (let index = 0; index < 4; index++) {
    await new Promise((resolve) => setImmediate(resolve));
    if (globalThis.gc === undefined)
      throw new Error('Retention case requires the configured --expose-gc');
    globalThis.gc();
  }
  expect(reference?.deref()).toBeUndefined();
  expect(collection.size).toBe(2);
});

test('empty dependencies count membership and use the existing declaration checker', () => {
  const collection = createCollection(entries(3));
  const owner = new AbortController();
  const read = vi.fn(() => 1);
  const descriptor = {
    fields: [],
    initial: () => 0,
    read,
    add: (/** @type {number} */ a, /** @type {number} */ b) => a + b,
    remove: (/** @type {number} */ a, /** @type {number} */ b) => a - b,
  };
  expect(checkSelection('constant', descriptor, { one: { cents: 1 } }).accessed).toEqual([]);
  const run = vi.fn();
  collection.watchAggregate(descriptor, run, { signal: owner.signal });
  read.mockClear();
  collection.update('t0', { cents: 10 });
  collection.reorder([...collection.keys()].reverse());
  expect(read).not.toHaveBeenCalled();
  collection.remove('t0');
  expect(read).toHaveBeenCalledTimes(1);
  expect(run).toHaveBeenLastCalledWith(2, 3);
  owner.abort();
});

test('exact numeric reducers reject unsafe intermediate arithmetic', () => {
  /** @type {unknown[]} */
  const errors = [];
  const collection = createCollection(entries(3), { report: (error) => errors.push(error) });
  collection.update('t0', { cents: Number.MAX_SAFE_INTEGER });
  collection.update('t1', { cents: -2 });
  collection.update('t2', { cents: 2 });
  const owner = new AbortController();
  const run = vi.fn();
  collection.watchAggregate(reduction('sum'), run, { signal: owner.signal });
  collection.update('t1', { cents: -3 });
  expect(errors).toHaveLength(1);
  expect(run).toHaveBeenCalledTimes(1);
  owner.abort();
});

test.each([10, 100])('count and sum match a reference model for 200 commits at size %s', (size) => {
  const collection = createCollection(entries(size));
  const model = new Map(entries(size));
  const owner = new AbortController();
  const seen = { count: 0, sum: 0 };
  for (const kind of /** @type {const} */ (['count', 'sum'])) {
    collection.watchAggregate(
      reduction(kind),
      (value) => {
        seen[kind] = value;
      },
      { signal: owner.signal },
    );
  }
  let seed = 1;
  for (let step = 0; step < 200; step++) {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    const key = `t${seed % size}`;
    const old = model.get(key);
    if (old === undefined) throw new Error('Missing reference item');
    const next = { ...old, done: !old.done, cents: (old.cents + 7) % 1000 };
    model.set(key, next);
    if (step % 5 === 0) {
      collection.batch(() => {
        collection.update(key, { cents: old.cents + 20 });
        collection.remove(key);
        collection.insert(key, next);
      });
    } else {
      collection.batch(() => {
        collection.update(key, next);
        try {
          collection.batch(() => {
            collection.update(key, { cents: 999 });
            throw new Error('inner');
          });
        } catch {
          /* Keep the outer update. */
        }
      });
    }
    let count = 0;
    let sum = 0;
    for (const item of model.values()) {
      count += item.done ? 0 : 1;
      sum += item.cents;
    }
    expect(seen).toEqual({ count, sum });
  }
  owner.abort();
});
