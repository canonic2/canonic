/**
 * @file The store core: frozen snapshots, synchronous commits, batches with
 * savepoints, and selective delivery to subscriptions. Pure: it uses no DOM or
 * Node API. Importing it has no effects.
 */

import { fallback, isThenable } from '../../report.js';

/**
 * A selection: the state fields `read` depends on, and the derivation itself.
 * Selections are plain objects; annotate them with
 * `@satisfies {Selection<State, Value>}` to type-check `fields` and type `read`.
 * @template S The store's state.
 * @template V The selected value.
 * @typedef {object} Selection
 * @property {readonly (keyof S & string)[]} fields The state keys `read`
 *   depends on: non-empty, each a key of the state. A commit that changes none
 *   of them skips this selection without calling `read`.
 * @property {(state: Readonly<S>) => V} read Derives the value from a
 *   snapshot. Pure: no I/O, no other stores, no mutation.
 * @property {(a: V, b: V) => boolean} [equals] Compares two results; `run` is
 *   called only when it returns `false`. Defaults to `Object.is`.
 */

/**
 * Options for `watch`.
 * @typedef {object} WatchOptions
 * @property {AbortSignal} [signal] Ends the subscription when it aborts. An
 *   already aborted signal subscribes nothing and calls nothing. Without a
 *   signal the subscription lasts as long as the store.
 */

/**
 * Options for `createStore`.
 * @typedef {object} StoreOptions
 * @property {(error: unknown) => void} [report] Receives errors thrown by a
 *   subscription's `read`, `equals` or `run` during delivery. Synchronous and
 *   expected not to throw; if it throws, the error goes to the fallback. The
 *   fallback, also the default, is the platform's `reportError` where one
 *   exists, and otherwise rethrowing the error from a microtask.
 */

/**
 * A store. Its functions do not use `this`, so `get` and `watch` can be handed
 * out detached as a read-only view.
 * @template S The state.
 * @typedef {object} Store
 * @property {() => Readonly<S>} get Returns the current snapshot. The same
 *   frozen snapshot is returned until a changing write replaces it.
 * @property {(patch: Partial<S> | ((state: Readonly<S>) => Partial<S>)) => void} set
 *   Writes the patched fields, or the fields a function of the current
 *   snapshot returns. Each patched value is read once. An unknown key throws a
 *   `TypeError` and changes nothing. When every value is `Object.is`-equal to
 *   the current one, nothing happens. Otherwise one new snapshot becomes
 *   current and, outside a batch, is delivered before `set` returns; errors
 *   from subscribers go to `report`, never out of `set`.
 * @property {(fn: () => void) => void} batch Runs `fn` synchronously and
 *   delivers its writes as one commit when the outermost batch returns. If
 *   `fn` throws, the store returns to its snapshot at this batch's start and
 *   the error is rethrown; a nested batch rolls back only its own writes. If
 *   `fn` returns a promise or other thenable, the batch rolls back and throws a
 *   `TypeError`.
 * @property {<V>(selection: Selection<S, V>, run: (value: V, previous: V | undefined) => void, options?: WatchOptions) => void} watch
 *   Calls `run` with the selected value now, with `previous` undefined, then
 *   after each commit that changes one of the selection's fields and changes
 *   the value under `equals`. Throws a `TypeError`, subscribing nothing, inside
 *   a batch or for a field that is not a key of the state. An error from the
 *   initial `read` or `run` propagates and leaves no subscription.
 */

/**
 * A subscription record.
 * @typedef {object} Subscription
 * @property {readonly string[]} fields Declared fields.
 * @property {(state: Record<string, unknown>) => unknown} read The selection's `read`.
 * @property {(a: unknown, b: unknown) => boolean} equals The selection's equality.
 * @property {(value: unknown, previous: unknown) => void} run The callback.
 * @property {unknown} last The last value delivered successfully.
 * @property {number} version The commit count when it was created.
 * @property {boolean} active Whether it still receives commits.
 */

/**
 * Stand-in for a deactivated subscription's released `read` and `run`.
 * @returns {undefined} Nothing.
 */
const released = () => undefined;

/**
 * Stand-in for a deactivated subscription's released `equals`.
 * @returns {boolean} Always true; an inactive record is never compared anyway.
 */
const releasedEquals = () => true;

/** Placeholder for cleared queue slots, so no changed-key array stays referenced. */
const NO_CHANGES = Object.freeze(/** @type {string[]} */ ([]));

/**
 * Tells whether any declared field is among a commit's changed fields.
 * @param {readonly string[]} fields A subscription's declared fields.
 * @param {readonly string[]} changed The commit's changed fields.
 * @returns {boolean} Whether the subscription must be considered.
 */
function intersects(fields, changed) {
  for (const field of fields) {
    if (changed.includes(field)) return true;
  }
  return false;
}

/**
 * Creates a store holding a frozen copy of `initial`. The keys of `initial`
 * define the state's shape for the store's lifetime.
 * @template {object} S
 * @param {S} initial The initial state; its own enumerable keys are copied.
 * @param {StoreOptions} [options] Delivery error reporting.
 * @returns {Store<S>} The store.
 * @throws {TypeError} When `initial` is not an object or has symbol keys;
 *   state fields are string keys.
 */
export function createStore(initial, options = {}) {
  if (typeof initial !== 'object' || initial === null) {
    throw new TypeError('createStore() needs an object as its initial state.');
  }
  if (Object.getOwnPropertySymbols(initial).length > 0) {
    throw new TypeError('createStore() state fields must be string keys, not symbols.');
  }
  /** @type {Readonly<Record<string, unknown>>} */
  let current = Object.freeze({ ...initial });
  let version = 0;
  /** @type {Subscription[]} */
  const subscriptions = [];
  // Commits waiting for delivery, as parallel arrays that keep their capacity
  // between deliveries, so a commit allocates no record and no backing store.
  /** @type {number[]} */
  const queuedVersions = [];
  /** @type {(readonly string[])[]} */
  const queuedChanges = [];
  let queued = 0;
  let delivering = false;
  let batchDepth = 0;
  /** @type {Readonly<Record<string, unknown>> | undefined} */
  let batchBase;
  /** @type {string[]} */
  const touched = [];
  let inactive = 0;
  /** Whether `set` is reading a patch; a write made then is rejected. */
  let writing = false;
  const report = options.report ?? fallback;

  /**
   * Passes a delivery error to `report`, and anything `report` throws to the
   * fallback, so nothing escapes into the delivery loop.
   * @param {unknown} error The error from `read`, `equals` or `run`.
   * @returns {void}
   */
  function deliverError(error) {
    try {
      report(error);
    } catch (failure) {
      fallback(failure);
    }
  }

  /**
   * Removes deactivated subscriptions in one pass, compacting in place.
   * @returns {void}
   */
  function removeInactive() {
    let kept = 0;
    for (const record of subscriptions) {
      if (record.active) subscriptions[kept++] = record;
    }
    subscriptions.length = kept;
    inactive = 0;
  }

  /**
   * Deactivates a subscription at once: it is skipped from now on, and its
   * callbacks and last value are released. Only its slot in the array waits:
   * outside a delivery, inactive records are removed in one pass once they are
   * more than half the array, so removing many subscriptions stays linear.
   * @param {Subscription} record The subscription.
   * @returns {void}
   */
  function deactivate(record) {
    if (!record.active) return;
    record.active = false;
    record.read = released;
    record.equals = releasedEquals;
    record.run = released;
    record.last = undefined;
    inactive++;
    if (!delivering && inactive * 2 > subscriptions.length) removeInactive();
  }

  /**
   * Delivers queued commits in order until the queue is empty. Commits made by
   * subscribers are appended and delivered by the same loop, never reentrantly.
   * @returns {void}
   */
  function drain() {
    delivering = true;
    try {
      for (let index = 0; index < queued; index++) {
        const commitVersion = /** @type {number} */ (queuedVersions[index]);
        const changed = /** @type {readonly string[]} */ (queuedChanges[index]);
        for (const record of subscriptions) {
          if (!record.active || record.version >= commitVersion) continue;
          if (!intersects(record.fields, changed)) continue;
          const { read, equals, run } = record;
          try {
            const value = read(current);
            // read and equals may abort this subscription; it is not called afterwards.
            if (!record.active || equals(value, record.last) || !record.active) continue;
            run(value, record.last);
            record.last = value;
          } catch (error) {
            deliverError(error);
          }
        }
      }
    } finally {
      for (let index = 0; index < queued; index++) queuedChanges[index] = NO_CHANGES;
      queued = 0;
      delivering = false;
      if (inactive * 2 > subscriptions.length) removeInactive();
    }
  }

  /**
   * Queues a commit of the given fields and delivers it unless a delivery is
   * already running.
   * @param {readonly string[]} changed The fields the commit changed.
   * @returns {void}
   */
  function commit(changed) {
    version++;
    queuedVersions[queued] = version;
    queuedChanges[queued] = changed;
    queued++;
    if (!delivering) drain();
  }

  /** @type {Store<S>['get']} */
  function get() {
    // The snapshot is a frozen copy of S with only S's keys ever written.
    return /** @type {Readonly<S>} */ (/** @type {unknown} */ (current));
  }

  /** @type {Store<S>['set']} */
  function set(patch) {
    if (writing) {
      throw new TypeError('set() cannot be called while another set() is reading its patch.');
    }
    /** @type {string[] | null} */
    let changedKeys = null;
    // Built at the first change, during the walk; published only after it.
    /** @type {Record<string, unknown> | null} */
    let next = null;
    writing = true;
    try {
      const values = typeof patch === 'function' ? patch(get()) : patch;
      if (typeof values !== 'object' || values === null) {
        throw new TypeError('set() needs an object of fields, or a function returning one.');
      }
      for (const key in values) {
        if (!Object.hasOwn(values, key)) continue;
        if (!Object.hasOwn(current, key)) {
          throw new TypeError(`Cannot write "${key}": it is not a field of this store's state.`);
        }
        const value = Reflect.get(values, key);
        if (Object.is(value, current[key])) continue;
        if (next === null || changedKeys === null) {
          next = { ...current };
          changedKeys = [key];
        } else {
          changedKeys.push(key);
        }
        next[key] = value;
      }
    } finally {
      writing = false;
    }
    if (changedKeys === null || next === null) return;
    current = Object.freeze(next);
    if (batchDepth > 0) {
      for (const key of changedKeys) {
        if (!touched.includes(key)) touched.push(key);
      }
      return;
    }
    commit(changedKeys);
  }

  /** @type {Store<S>['batch']} */
  function batch(fn) {
    const start = current;
    const outermost = batchDepth === 0;
    if (outermost) batchBase = current;
    batchDepth++;
    /** @type {string[] | null} */
    let changed = null;
    try {
      try {
        // Checking for a thenable reads `then`, which may throw; both roll back.
        if (isThenable(fn())) {
          throw new TypeError('batch() needs a synchronous callback; it returned a promise or thenable.');
        }
      } catch (error) {
        current = start;
        throw error;
      } finally {
        batchDepth--;
      }
      if (outermost) {
        const base = /** @type {Readonly<Record<string, unknown>>} */ (batchBase);
        changed = touched.filter((key) => !Object.is(current[key], base[key]));
      }
    } finally {
      if (outermost) {
        batchBase = undefined;
        touched.length = 0;
      }
    }
    if (changed !== null && changed.length > 0) commit(changed);
  }

  /** @type {Store<S>['watch']} */
  function watch(selection, run, options = {}) {
    if (batchDepth > 0) {
      throw new TypeError('watch() cannot be called inside batch(); its first value could be rolled back.');
    }
    const { fields: declared, read, equals = Object.is } = selection;
    if (!Array.isArray(declared) || declared.length === 0) {
      throw new TypeError('A selection needs a non-empty fields array.');
    }
    // A copy, so changing the caller's array later cannot bypass this validation.
    const fields = [...declared];
    for (const field of fields) {
      if (typeof field !== 'string' || !Object.hasOwn(current, field)) {
        throw new TypeError(`Cannot watch "${String(field)}": it is not a field of this store's state.`);
      }
    }
    const { signal } = options;
    if (signal?.aborted) return;
    const before = version;
    const value = read(get());
    if (version !== before) {
      throw new TypeError("A selection's read wrote to the store during watch(); read must be pure.");
    }
    if (signal?.aborted) return;
    /** @type {Subscription} */
    const record = {
      fields,
      read: /** @type {Subscription['read']} */ (read),
      equals: /** @type {Subscription['equals']} */ (equals),
      run: /** @type {Subscription['run']} */ (run),
      last: value,
      version,
      active: true,
    };
    subscriptions.push(record);
    /**
     * Ends the subscription when its signal aborts.
     * @returns {void}
     */
    const onAbort = () => deactivate(record);
    signal?.addEventListener('abort', onAbort, { once: true });
    try {
      run(value, undefined);
    } catch (error) {
      signal?.removeEventListener('abort', onAbort);
      deactivate(record);
      throw error;
    }
  }

  return { get, set, batch, watch };
}
