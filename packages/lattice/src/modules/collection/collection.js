/**
 * @file The keyed collection: items by key, each a frozen record replaced on
 * change, a frozen array of keys in order, transactional batches with an undo
 * log, and delivery in three phases (membership, items, collection-wide
 * values). Pure: it uses no DOM or Node API. Importing it has no effects.
 */

import { fallback, isThenable } from '../../report.js';

/**
 * A selection over one item: the item fields `read` depends on, and the
 * derivation itself. Selections are plain objects shared by every item; annotate
 * them with `@satisfies {ItemSelection<Item, Value>}`.
 * @template T The item.
 * @template V The selected value.
 * @typedef {object} ItemSelection
 * @property {readonly (keyof T & string)[]} fields The item fields `read`
 *   depends on: non-empty, each a field of the watched item. A commit that
 *   changes none of them in that item skips this selection without calling
 *   `read`.
 * @property {(item: Readonly<T>) => V} read Derives the value from the item's
 *   record. Pure: no I/O, no other state, no mutation.
 * @property {(a: V, b: V) => boolean} [equals] Compares two results; `run` is
 *   called only when it returns `false`. Defaults to `Object.is`.
 */

/**
 * A collection-wide selection: the item fields `read` depends on, and a
 * derivation over the whole collection.
 * @template T The item.
 * @template V The selected value.
 * @typedef {object} CollectionSelection
 * @property {readonly (keyof T & string)[]} items The item fields `read`
 *   depends on in any item; may be empty. The selection is evaluated after a
 *   commit that inserts, removes or reorders keys, or changes one of these
 *   fields in any item, and at most once per commit.
 * @property {(collection: CollectionView<T>) => V} read Derives the value from
 *   the collection's read-only view. Pure. It may read every item, at a cost
 *   proportional to the collection.
 * @property {(a: V, b: V) => boolean} [equals] Compares two results; `run` is
 *   called only when it returns `false`. Defaults to `Object.is`.
 */

/**
 * The keys a commit inserted and removed. A key removed and inserted in the
 * same commit, which creates a new item for it, appears in both.
 * @typedef {object} KeysChange
 * @property {readonly string[]} inserted Keys added by the commit.
 * @property {readonly string[]} removed Keys removed by the commit.
 */

/**
 * The collection's read-only view.
 * @template T The item.
 * @typedef {object} CollectionView
 * @property {() => readonly string[]} keys Returns the keys in order as a
 *   frozen array. The same array is returned until membership or order
 *   changes.
 * @property {(key: string) => Readonly<T> | undefined} get Returns the item's
 *   frozen record, or `undefined` for a key not in the collection. The same
 *   record is returned until that item changes.
 * @property {(key: string) => boolean} has Tells whether a key is in the
 *   collection.
 * @property {number} size The number of items.
 */

/**
 * Options for subscriptions.
 * @typedef {object} CollectionWatchOptions
 * @property {AbortSignal} [signal] Ends the subscription when it aborts. An
 *   already aborted signal subscribes nothing and calls nothing. Without a
 *   signal the subscription lasts as long as the collection, or for an item
 *   subscription until the item's removal commits.
 */

/**
 * Options for `createCollection`.
 * @typedef {object} CollectionOptions
 * @property {(error: unknown) => void} [report] Receives errors thrown by a
 *   subscription's `read`, `equals` or `run` during delivery, as the store's
 *   option does. Synchronous and expected not to throw; if it throws, the
 *   error goes to the fallback, which is also the default.
 */

/**
 * A keyed collection. Its functions do not use `this`, so a capability can
 * hand out the read-only functions detached.
 * @template T The item.
 * @typedef {object} Collection
 * @property {() => readonly string[]} keys See {@link CollectionView}.
 * @property {(key: string) => Readonly<T> | undefined} get See {@link CollectionView}.
 * @property {(key: string) => boolean} has See {@link CollectionView}.
 * @property {number} size The number of items.
 * @property {(key: string, patch: Partial<T> | ((item: Readonly<T>) => Partial<T>)) => void} update
 *   Patches one item's fields, as the store's `set` does: an object or a
 *   function of the item's record, each value read once. A field the item does
 *   not have, a patch that is not an object, a key not in the collection, or a
 *   write made while a patch is resolved throws a `TypeError` and changes
 *   nothing. Unchanged values are silent. Otherwise the item gets a new frozen
 *   record and, outside a batch, the commit is delivered before `update`
 *   returns.
 * @property {(key: string, item: T, before?: string) => void} insert Adds an
 *   item, as a new frozen record, before the key `before`, or at the end
 *   without it. A key already in the collection, or a `before` not in it,
 *   throws a `TypeError` and changes nothing. A key that was removed earlier
 *   gets a new item with no subscriptions.
 * @property {(key: string) => void} remove Removes an item. Its item
 *   subscriptions end when the removal commits: at once outside a batch, when
 *   the outermost batch commits inside one. A batch that rolls the removal back
 *   restores the item with its subscriptions. A key not in the collection
 *   throws a `TypeError`.
 * @property {(keys: readonly string[]) => void} reorder Replaces the order with
 *   a permutation of the current keys. Anything else throws a `TypeError` and
 *   changes nothing; the current order is silent.
 * @property {(fn: () => void) => void} batch Runs `fn` synchronously and
 *   delivers its writes as one commit when the outermost batch returns, as the
 *   store's `batch` does. If `fn` throws, the collection returns to its state
 *   at this batch's start, including removed items and their subscriptions,
 *   and the error is rethrown; a nested batch rolls back only its own writes.
 *   If `fn` returns a promise or other thenable, the batch rolls back and
 *   throws a `TypeError`.
 * @property {(run: (keys: readonly string[], change: KeysChange | undefined) => void, options?: CollectionWatchOptions) => void} watchKeys
 *   Calls `run` with the keys now and no change, then after each commit that
 *   inserts, removes or reorders keys, with that commit's change. Membership
 *   subscriptions run first in a commit's delivery.
 * @property {<V>(key: string, selection: ItemSelection<T, V>, run: (value: V, previous: V | undefined) => void, options?: CollectionWatchOptions) => void} watchItem
 *   Calls `run` with the item's selected value now, with `previous` undefined,
 *   then after each commit that changes one of the selection's fields in that
 *   item and changes the value under `equals`. Item subscriptions run after
 *   membership subscriptions. The subscription belongs to the item now under
 *   `key`; it ends when that item's removal commits. A key not in the
 *   collection or a field the item does not have throws a `TypeError`, as does
 *   a call inside a batch; an error from the initial `read` or `run`
 *   propagates and leaves no subscription.
 * @property {<V>(selection: CollectionSelection<T, V>, run: (value: V, previous: V | undefined) => void, options?: CollectionWatchOptions) => void} watch
 *   Calls `run` with the collection-wide value now, then after each commit
 *   that inserts, removes or reorders keys or changes one of `selection.items`
 *   in any item, when the value changes under `equals`. Collection-wide
 *   subscriptions run last in a commit's delivery and are evaluated at most
 *   once per commit. Throws a `TypeError` inside a batch.
 */

/**
 * A subscription record.
 * @typedef {object} Subscription
 * @property {readonly string[]} fields Declared item fields; none for membership.
 * @property {(argument: never) => unknown} read The selection's `read`.
 * @property {(a: unknown, b: unknown) => boolean} equals The selection's equality.
 * @property {(value: never, previous: never) => void} run The callback.
 * @property {unknown} last The last value delivered successfully.
 * @property {number} version The commit count when it was created.
 * @property {boolean} active Whether it still receives commits.
 * @property {Subscription[]} owner The array that holds it.
 */

/**
 * One item: an incarnation of its key. Inserting a key always creates a new
 * one, so subscriptions never move to a later item with the same key.
 * @typedef {object} Incarnation
 * @property {string} key Its key.
 * @property {ItemRecord} record Its current frozen record.
 * @property {Subscription[]} subscriptions Its item subscriptions.
 * @property {boolean} ended Whether its removal has committed.
 * @property {ItemRecord | null} base Its record when the current outermost batch
 *   first changed it.
 * @property {number} batch The outermost batch that set `base`.
 */

/** @typedef {Readonly<{ [field: string]: unknown }>} ItemRecord */

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

/** The shared empty key list. @type {readonly string[]} */
const NONE = Object.freeze([]);

/** The change of a commit that inserts and removes nothing. @type {KeysChange} */
const NO_CHANGE = Object.freeze({ inserted: NONE, removed: NONE });

/** Undo-log entry kinds. */
const RECORD = 0;
const KEYS = 1;
const ENTRY = 2;
const INSERTED = 3;
const REMOVED = 4;

/**
 * Tells whether two key arrays hold the same keys in the same order.
 * @param {readonly string[]} a One array.
 * @param {readonly string[]} b The other.
 * @returns {boolean} Whether they match.
 */
function sameKeys(a, b) {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let index = 0; index < a.length; index++) if (a[index] !== b[index]) return false;
  return true;
}

/**
 * Tells whether any of `fields` differs between two records of one item.
 * @param {readonly string[]} fields The fields to compare.
 * @param {ItemRecord} after The later record.
 * @param {ItemRecord} before The earlier record.
 * @returns {boolean} Whether one differs under `Object.is`.
 */
function changedIn(fields, after, before) {
  for (let index = 0; index < fields.length; index++) {
    const field = /** @type {string} */ (fields[index]);
    if (!Object.is(after[field], before[field])) return true;
  }
  return false;
}

/**
 * Tells whether two records of one item differ in any field.
 * @param {ItemRecord} after The later record.
 * @param {ItemRecord} before The earlier record.
 * @returns {boolean} Whether a field differs under `Object.is`.
 */
function differs(after, before) {
  if (after === before) return false;
  for (const field in before) {
    if (Object.hasOwn(before, field) && !Object.is(after[field], before[field])) return true;
  }
  return false;
}

/**
 * Copies an item's own enumerable string-keyed fields into a frozen record.
 * @param {unknown} item The item.
 * @returns {ItemRecord} Its record.
 * @throws {TypeError} When `item` is not an object or has symbol keys.
 */
function toItemRecord(item) {
  if (typeof item !== 'object' || item === null) throw new TypeError('A collection item must be an object.');
  if (Object.getOwnPropertySymbols(item).length > 0) {
    throw new TypeError('Collection item fields must be string keys, not symbols.');
  }
  return Object.freeze({ ...item });
}

/**
 * Creates an incarnation of a key.
 * @param {string} key The key.
 * @param {ItemRecord} record Its first record.
 * @returns {Incarnation} The incarnation.
 */
function incarnate(key, record) {
  return { key, record, subscriptions: [], ended: false, base: null, batch: 0 };
}

/**
 * Creates a keyed collection holding a frozen copy of each item.
 * @template {object} T
 * @param {Iterable<readonly [string, T]>} entries The keys and items, in order.
 *   Keys are strings and unique; each item's own enumerable string keys define
 *   that item's fields.
 * @param {CollectionOptions} [options] Delivery error reporting.
 * @returns {Collection<T>} The collection.
 * @throws {TypeError} For a key that is not a string, a duplicate key, or an
 *   item that is not an object or has symbol keys.
 */
export function createCollection(entries, options = {}) {
  const report = options.report ?? fallback;
  /** @type {Map<string, Incarnation>} */
  const items = new Map();
  /** @type {string[]} */
  const initial = [];
  for (const [key, item] of entries) {
    if (typeof key !== 'string') throw new TypeError('Collection keys must be strings.');
    if (items.has(key)) throw new TypeError(`Duplicate key "${key}".`);
    items.set(key, incarnate(key, toItemRecord(item)));
    initial.push(key);
  }
  /** @type {readonly string[]} */
  let keys = Object.freeze(initial);
  /** @type {Subscription[]} */
  const membership = [];
  /** @type {Subscription[]} */
  const wide = [];
  let version = 0;
  // Commits waiting for delivery, as parallel arrays that keep their capacity.
  // A commit that changed one item stores it and its records directly; one
  // that changed several stores arrays.
  /** @type {number[]} */
  const queuedVersions = [];
  /** @type {boolean[]} */
  const queuedKeys = [];
  /** @type {KeysChange[]} */
  const queuedChanges = [];
  /** @type {(Incarnation | Incarnation[] | null)[]} */
  const queuedItems = [];
  /** @type {(ItemRecord | ItemRecord[] | null)[]} */
  const queuedBefore = [];
  /** @type {(ItemRecord | ItemRecord[] | null)[]} */
  const queuedAfter = [];
  let queued = 0;
  let delivering = false;
  /** @type {Subscription[]} */
  const deferred = [];
  /** Whether `update` is resolving a patch; a write made then is rejected. */
  let writing = false;
  let depth = 0;
  let batchId = 0;
  // The undo log of the open batch, as flat triples of kind and two operands.
  /** @type {unknown[]} */
  const log = [];
  /** @type {Incarnation[]} */
  const touched = [];
  /** @type {Incarnation[]} */
  const inserted = [];
  /** @type {Incarnation[]} */
  const removed = [];
  /** @type {readonly string[]} */
  let baseKeys = keys;

  /** @type {CollectionView<ItemRecord>} */
  const view = Object.freeze({
    /** @returns {readonly string[]} The keys in order. */
    keys: () => keys,
    /**
     * @param {string} key The key.
     * @returns {ItemRecord | undefined} The item's record.
     */
    get: (key) => items.get(key)?.record,
    /**
     * @param {string} key The key.
     * @returns {boolean} Whether the key is in the collection.
     */
    has: (key) => items.has(key),
    /** @returns {number} The number of items. */
    get size() {
      return items.size;
    },
  });

  /**
   * Passes a delivery error to `report`, and anything `report` throws to the
   * fallback, so nothing escapes into the delivery loop.
   * @param {unknown} error The error from a subscription.
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
   * Rejects a write made while a patch is being resolved.
   * @returns {void}
   * @throws {TypeError} When `update` is resolving a patch.
   */
  function guard() {
    if (writing) throw new TypeError('The collection cannot be written while a patch is being resolved.');
  }

  /**
   * Ends a subscription and releases what it captured.
   * @param {Subscription} subscription The subscription.
   * @returns {void}
   */
  function release(subscription) {
    subscription.active = false;
    subscription.read = released;
    subscription.equals = releasedEquals;
    subscription.run = released;
    subscription.last = undefined;
  }

  /**
   * Ends a subscription whose signal aborted, and removes it from its array,
   * after the delivery when one is running.
   * @param {Subscription} subscription The subscription.
   * @returns {void}
   */
  function deactivate(subscription) {
    if (!subscription.active) return;
    release(subscription);
    if (delivering) deferred.push(subscription);
    else unlink(subscription);
  }

  /**
   * Removes a subscription from the array that holds it.
   * @param {Subscription} subscription The subscription.
   * @returns {void}
   */
  function unlink(subscription) {
    const at = subscription.owner.indexOf(subscription);
    if (at !== -1) subscription.owner.splice(at, 1);
  }

  /**
   * Ends a removed item and every subscription it holds.
   * @param {Incarnation} item The item.
   * @returns {void}
   */
  function end(item) {
    item.ended = true;
    for (const subscription of item.subscriptions) release(subscription);
    item.subscriptions.length = 0;
  }

  /**
   * Calls a subscription's `read` and, when the value changed, its `run`.
   * @param {Subscription} subscription The subscription.
   * @param {unknown} argument What `read` receives.
   * @returns {void}
   */
  function evaluate(subscription, argument) {
    const read = /** @type {(argument: unknown) => unknown} */ (subscription.read);
    const run = /** @type {(value: unknown, previous: unknown) => void} */ (subscription.run);
    try {
      const value = read(argument);
      // read and equals may abort this subscription; it is not called afterwards.
      if (!subscription.active || subscription.equals(value, subscription.last) || !subscription.active) return;
      run(value, subscription.last);
      subscription.last = value;
    } catch (error) {
      deliverError(error);
    }
  }

  /**
   * Delivers one changed item of a commit to its own subscriptions.
   * @param {Incarnation} item The item.
   * @param {ItemRecord} before Its record before the commit.
   * @param {ItemRecord} after Its record after the commit.
   * @param {number} commitVersion The commit.
   * @returns {void}
   */
  function deliverItem(item, before, after, commitVersion) {
    if (item.ended) return;
    const subscriptions = item.subscriptions;
    for (let index = 0; index < subscriptions.length; index++) {
      const subscription = /** @type {Subscription} */ (subscriptions[index]);
      if (!subscription.active || subscription.version >= commitVersion) continue;
      if (!changedIn(subscription.fields, after, before)) continue;
      evaluate(subscription, item.record);
    }
  }

  /**
   * Tells whether a queued commit changed any of `fields` in any item.
   * @param {readonly string[]} fields The fields.
   * @param {number} at The commit's queue slot.
   * @returns {boolean} Whether one changed.
   */
  function touches(fields, at) {
    const changed = queuedItems[at];
    if (changed === null || changed === undefined || fields.length === 0) return false;
    const before = queuedBefore[at];
    const after = queuedAfter[at];
    if (!Array.isArray(changed)) return changedIn(fields, /** @type {ItemRecord} */ (after), /** @type {ItemRecord} */ (before));
    const befores = /** @type {ItemRecord[]} */ (before);
    const afters = /** @type {ItemRecord[]} */ (after);
    for (let index = 0; index < changed.length; index++) {
      if (changedIn(fields, /** @type {ItemRecord} */ (afters[index]), /** @type {ItemRecord} */ (befores[index]))) return true;
    }
    return false;
  }

  /**
   * Delivers queued commits in order until the queue is empty: membership,
   * then changed items, then collection-wide subscriptions for each. Commits
   * made by subscribers are appended and delivered by the same loop.
   * @returns {void}
   */
  function drain() {
    delivering = true;
    try {
      for (let at = 0; at < queued; at++) {
        const commitVersion = /** @type {number} */ (queuedVersions[at]);
        const keysChanged = queuedKeys[at] === true;
        if (keysChanged) {
          const change = /** @type {KeysChange} */ (queuedChanges[at]);
          for (let index = 0; index < membership.length; index++) {
            const subscription = /** @type {Subscription} */ (membership[index]);
            if (!subscription.active || subscription.version >= commitVersion) continue;
            const run = /** @type {(keys: readonly string[], change: KeysChange) => void} */ (subscription.run);
            try {
              run(keys, change);
            } catch (error) {
              deliverError(error);
            }
          }
        }
        const changed = queuedItems[at];
        if (changed !== null && changed !== undefined) {
          if (Array.isArray(changed)) {
            const befores = /** @type {ItemRecord[]} */ (queuedBefore[at]);
            const afters = /** @type {ItemRecord[]} */ (queuedAfter[at]);
            for (let index = 0; index < changed.length; index++) {
              deliverItem(
                /** @type {Incarnation} */ (changed[index]),
                /** @type {ItemRecord} */ (befores[index]),
                /** @type {ItemRecord} */ (afters[index]),
                commitVersion,
              );
            }
          } else {
            deliverItem(changed, /** @type {ItemRecord} */ (queuedBefore[at]), /** @type {ItemRecord} */ (queuedAfter[at]), commitVersion);
          }
        }
        for (let index = 0; index < wide.length; index++) {
          const subscription = /** @type {Subscription} */ (wide[index]);
          if (!subscription.active || subscription.version >= commitVersion) continue;
          if (!keysChanged && !touches(subscription.fields, at)) continue;
          evaluate(subscription, view);
        }
      }
    } finally {
      for (let at = 0; at < queued; at++) {
        queuedChanges[at] = NO_CHANGE;
        queuedItems[at] = null;
        queuedBefore[at] = null;
        queuedAfter[at] = null;
      }
      queued = 0;
      delivering = false;
      for (const subscription of deferred) unlink(subscription);
      deferred.length = 0;
    }
  }

  /**
   * Queues a commit and delivers it unless a delivery is already running.
   * @param {boolean} keysChanged Whether membership or order changed.
   * @param {KeysChange} change The inserted and removed keys.
   * @param {Incarnation | Incarnation[] | null} changed The changed items.
   * @param {ItemRecord | ItemRecord[] | null} before Their records before the commit.
   * @param {ItemRecord | ItemRecord[] | null} after Their records after it.
   * @returns {void}
   */
  function commit(keysChanged, change, changed, before, after) {
    version++;
    queuedVersions[queued] = version;
    queuedKeys[queued] = keysChanged;
    queuedChanges[queued] = change;
    queuedItems[queued] = changed;
    queuedBefore[queued] = before;
    queuedAfter[queued] = after;
    queued++;
    if (!delivering) drain();
  }

  /**
   * Undoes the open batch's writes back to a log position, newest first.
   * @param {number} mark The log length at the savepoint.
   * @returns {void}
   */
  function rollback(mark) {
    while (log.length > mark) {
      const second = log.pop();
      const first = log.pop();
      const kind = log.pop();
      if (kind === RECORD) /** @type {Incarnation} */ (first).record = /** @type {ItemRecord} */ (second);
      else if (kind === KEYS) keys = /** @type {readonly string[]} */ (first);
      else if (kind === ENTRY) {
        if (second === undefined) items.delete(/** @type {string} */ (first));
        else items.set(/** @type {string} */ (first), /** @type {Incarnation} */ (second));
      } else if (kind === INSERTED) inserted.pop();
      else if (kind === REMOVED) removed.pop();
    }
  }

  /** @type {Collection<ItemRecord>['update']} */
  function update(key, patch) {
    guard();
    const item = items.get(key);
    if (item === undefined) throw new TypeError(`No item "${key}" in the collection.`);
    /** @type {{ [field: string]: unknown } | null} */
    let next = null;
    writing = true;
    try {
      const values = typeof patch === 'function' ? patch(item.record) : patch;
      if (typeof values !== 'object' || values === null) {
        throw new TypeError('update() needs an object of fields, or a function returning one.');
      }
      for (const field in values) {
        if (!Object.hasOwn(values, field)) continue;
        if (!Object.hasOwn(item.record, field)) {
          throw new TypeError(`Cannot write "${field}": it is not a field of item "${key}".`);
        }
        const value = Reflect.get(values, field);
        if (Object.is(value, item.record[field])) continue;
        next ??= { ...item.record };
        next[field] = value;
      }
    } finally {
      writing = false;
    }
    if (next === null) return;
    const previous = item.record;
    item.record = Object.freeze(next);
    if (depth > 0) {
      log.push(RECORD, item, previous);
      if (item.batch !== batchId) {
        item.batch = batchId;
        item.base = previous;
        touched.push(item);
      }
      return;
    }
    commit(false, NO_CHANGE, item, previous, item.record);
  }

  /** @type {Collection<ItemRecord>['insert']} */
  function insert(key, value, before) {
    guard();
    if (typeof key !== 'string') throw new TypeError('Collection keys must be strings.');
    if (items.has(key)) throw new TypeError(`Item "${key}" is already in the collection.`);
    if (before !== undefined && !items.has(before)) throw new TypeError(`No item "${before}" to insert before.`);
    const item = incarnate(key, toItemRecord(value));
    const next = [...keys];
    if (before === undefined) next.push(key);
    else next.splice(next.indexOf(before), 0, key);
    const previous = keys;
    keys = Object.freeze(next);
    items.set(key, item);
    if (depth > 0) {
      log.push(KEYS, previous, undefined, ENTRY, key, undefined, INSERTED, undefined, undefined);
      inserted.push(item);
      return;
    }
    commit(true, { inserted: [key], removed: NONE }, null, null, null);
  }

  /** @type {Collection<ItemRecord>['remove']} */
  function remove(key) {
    guard();
    const item = items.get(key);
    if (item === undefined) throw new TypeError(`No item "${key}" in the collection.`);
    const previous = keys;
    keys = Object.freeze(keys.filter((candidate) => candidate !== key));
    items.delete(key);
    if (depth > 0) {
      log.push(KEYS, previous, undefined, ENTRY, key, item, REMOVED, undefined, undefined);
      removed.push(item);
      return;
    }
    end(item);
    commit(true, { inserted: NONE, removed: [key] }, null, null, null);
  }

  /** @type {Collection<ItemRecord>['reorder']} */
  function reorder(order) {
    guard();
    if (!Array.isArray(order) || order.length !== keys.length) {
      throw new TypeError('reorder() needs a permutation of the current keys.');
    }
    /** @type {Set<string>} */
    const seen = new Set();
    for (let index = 0; index < order.length; index++) {
      const key = /** @type {string} */ (order[index]);
      if (!items.has(key) || seen.has(key)) throw new TypeError('reorder() needs a permutation of the current keys.');
      seen.add(key);
    }
    if (sameKeys(order, keys)) return;
    const previous = keys;
    keys = Object.freeze([...order]);
    if (depth > 0) {
      log.push(KEYS, previous, undefined);
      return;
    }
    commit(true, NO_CHANGE, null, null, null);
  }

  /**
   * Clears the outermost batch's bookkeeping, so nothing it referenced stays
   * reachable.
   * @returns {void}
   */
  function resetBatch() {
    for (const item of touched) item.base = null;
    touched.length = 0;
    inserted.length = 0;
    removed.length = 0;
    log.length = 0;
  }

  /**
   * Builds and queues the outermost batch's commit: ends the removed items,
   * then reports membership, order and the items whose final records differ
   * from their records before the batch.
   * @returns {void}
   */
  function commitBatch() {
    const fresh = inserted.length > 0 ? new Set(inserted) : null;
    /** @type {string[]} */
    const removedKeys = [];
    for (const item of removed) {
      if (fresh?.has(item)) continue;
      end(item);
      removedKeys.push(item.key);
    }
    /** @type {string[]} */
    const insertedKeys = [];
    for (const item of inserted) if (items.get(item.key) === item) insertedKeys.push(item.key);
    /** @type {Incarnation[]} */
    const changed = [];
    /** @type {ItemRecord[]} */
    const before = [];
    /** @type {ItemRecord[]} */
    const after = [];
    for (let index = 0; index < touched.length; index++) {
      const item = /** @type {Incarnation} */ (touched[index]);
      const base = /** @type {ItemRecord} */ (item.base);
      if (fresh?.has(item) || item.ended || items.get(item.key) !== item || !differs(item.record, base)) continue;
      changed.push(item);
      before.push(base);
      after.push(item.record);
    }
    resetBatch();
    const membershipChanged = insertedKeys.length > 0 || removedKeys.length > 0;
    if (!membershipChanged && sameKeys(baseKeys, keys)) keys = baseKeys;
    const keysChanged = membershipChanged || keys !== baseKeys;
    if (!keysChanged && changed.length === 0) return;
    const change = membershipChanged ? { inserted: insertedKeys, removed: removedKeys } : NO_CHANGE;
    if (changed.length === 0) commit(keysChanged, change, null, null, null);
    else if (changed.length === 1) {
      commit(
        keysChanged,
        change,
        /** @type {Incarnation} */ (changed[0]),
        /** @type {ItemRecord} */ (before[0]),
        /** @type {ItemRecord} */ (after[0]),
      );
    } else commit(keysChanged, change, changed, before, after);
  }

  /** @type {Collection<ItemRecord>['batch']} */
  function batch(fn) {
    guard();
    const mark = log.length;
    const outermost = depth === 0;
    if (outermost) {
      baseKeys = keys;
      batchId++;
    }
    depth++;
    let failed = true;
    try {
      try {
        // Checking for a thenable reads `then`, which may throw; both roll back.
        if (isThenable(fn())) {
          throw new TypeError('batch() needs a synchronous callback; it returned a promise or thenable.');
        }
      } catch (error) {
        rollback(mark);
        throw error;
      } finally {
        depth--;
      }
      failed = false;
    } finally {
      if (outermost && failed) resetBatch();
    }
    if (outermost) commitBatch();
  }

  /**
   * Registers a subscription after its initial value, runs it once, and ties
   * it to the signal.
   * @param {Subscription[]} owner The array to hold it.
   * @param {readonly string[]} fields Its declared item fields.
   * @param {(argument: never) => unknown} read Its `read`.
   * @param {(a: unknown, b: unknown) => boolean} equals Its equality.
   * @param {(value: never, previous: never) => void} run Its callback.
   * @param {AbortSignal | undefined} signal Its lifetime.
   * @param {() => unknown} initial Computes the initial value.
   * @param {(value: unknown) => void} first Calls `run` with the initial value.
   * @returns {void}
   */
  function subscribe(owner, fields, read, equals, run, signal, initial, first) {
    if (signal?.aborted) return;
    const before = version;
    const value = initial();
    if (version !== before) {
      throw new TypeError("A selection's read wrote to the collection during watch; read must be pure.");
    }
    if (signal?.aborted) return;
    /** @type {Subscription} */
    const subscription = { fields, read, equals, run, last: value, version, active: true, owner };
    owner.push(subscription);
    /**
     * Ends the subscription when its signal aborts.
     * @returns {void}
     */
    const onAbort = () => deactivate(subscription);
    signal?.addEventListener('abort', onAbort, { once: true });
    try {
      first(value);
    } catch (error) {
      signal?.removeEventListener('abort', onAbort);
      deactivate(subscription);
      throw error;
    }
  }

  /**
   * Rejects a subscription made while a batch is open.
   * @returns {void}
   * @throws {TypeError} Inside a batch.
   */
  function notInBatch() {
    if (depth > 0) {
      throw new TypeError('A collection cannot be watched inside batch(); its first value could be rolled back.');
    }
  }

  /** @type {Collection<ItemRecord>['watchKeys']} */
  function watchKeys(run, options = {}) {
    notInBatch();
    subscribe(
      membership,
      NONE,
      released,
      releasedEquals,
      run,
      options.signal,
      () => keys,
      () => run(keys, undefined),
    );
  }

  /** @type {Collection<ItemRecord>['watchItem']} */
  function watchItem(key, selection, run, options = {}) {
    notInBatch();
    const item = items.get(key);
    if (item === undefined) throw new TypeError(`No item "${key}" in the collection.`);
    const { fields: declared, read, equals = Object.is } = selection;
    if (!Array.isArray(declared) || declared.length === 0) {
      throw new TypeError('An item selection needs a non-empty fields array.');
    }
    // A copy, so changing the caller's array later cannot bypass this validation.
    const fields = [...declared];
    for (const field of fields) {
      if (typeof field !== 'string' || !Object.hasOwn(item.record, field)) {
        throw new TypeError(`Cannot watch "${String(field)}": it is not a field of item "${key}".`);
      }
    }
    subscribe(
      item.subscriptions,
      fields,
      read,
      /** @type {(a: unknown, b: unknown) => boolean} */ (equals),
      run,
      options.signal,
      () => read(item.record),
      (value) => run(/** @type {never} */ (value), undefined),
    );
  }

  /** @type {Collection<ItemRecord>['watch']} */
  function watch(selection, run, options = {}) {
    notInBatch();
    const { items: declared, read, equals = Object.is } = selection;
    if (!Array.isArray(declared) || declared.some((field) => typeof field !== 'string')) {
      throw new TypeError('A collection-wide selection needs an items array of field names.');
    }
    subscribe(
      wide,
      [...declared],
      read,
      /** @type {(a: unknown, b: unknown) => boolean} */ (equals),
      run,
      options.signal,
      () => read(view),
      (value) => run(/** @type {never} */ (value), undefined),
    );
  }

  const collection = {
    keys: view.keys,
    get: view.get,
    has: view.has,
    /** @returns {number} The number of items. */
    get size() {
      return items.size;
    },
    update,
    insert,
    remove,
    reorder,
    batch,
    watchKeys,
    watchItem,
    watch,
  };
  // Items are typed by T at the boundary; inside, records are frozen field maps.
  return /** @type {Collection<T>} */ (/** @type {unknown} */ (collection));
}
