/**
 * @file The keyed collection: items by key, each a frozen record replaced on
 * change, a frozen array of keys in order, transactional batches with an undo
 * log, and delivery in three phases (membership, items, collection-wide
 * values). Pure: it uses no DOM or Node API. Importing it has no effects.
 */

import { fallback, isThenable } from '../../report.js';
import { synchronousResult } from './synchronous-result.js';

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
 * @property {readonly (keyof T & string)[]} fields The item fields `read`
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
 * A pure, order-independent reversible reduction over item contributions.
 * Initialize by visiting each current key once; later commits project only
 * changed records. No per-item contribution cache is retained. Every callback
 * must return synchronously and treat inputs and accumulator values as immutable.
 * @template T The item record.
 * @template C The projected contribution.
 * @template V The immutable accumulator and delivered value.
 * @typedef {object} AggregateSelection
 * @property {readonly (keyof T & string)[]} fields Fields read by the projection;
 *   an empty list observes membership alone.
 * @property {(item: Readonly<T>) => C} read Projects one immutable item.
 * @property {() => V} initial Creates the empty accumulator for initialization
 *   and error recovery.
 * @property {(value: V, contribution: C) => V} add Includes one contribution.
 * @property {(value: V, contribution: C) => V} remove Reverses its addition.
 * @property {(value: V, previous: V) => boolean} [equals] Delivery equality,
 *   defaulting to Object.is; it does not change accumulator maintenance.
 */

/**
 * The collection's read-only view.
 * @template T The item.
 * @typedef {object} CollectionTraversal
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
 * Read-only traversal supplied to a collection derivation.
 * @template T The item.
 * @typedef {Readonly<CollectionTraversal<T>>} CollectionView
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
 *   Patches one item's fields, as the store's `update` does: an object or a
 *   function of the item's record, each value read once. A field the item does
 *   not have, a patch that is not an object, a key not in the collection, or a
 *   write made while a patch is resolved throws a `TypeError` and changes
 *   nothing. Unchanged values are silent. Otherwise the item gets a new frozen
 *   record and, outside a batch, the commit is delivered before `update`
 *   returns.
 * @property {(changes: MembershipChanges<T>) => void} updateMembership
 *   Atomically removes and inserts
 *   memberships with one new frozen order. Anchors must survive the call;
 *   insertions at each anchor keep their supplied order. Inputs are validated
 *   and copied before mutation, with writes rejected during getters. Empty
 *   changes are silent. Reads after this call inside a batch see its complete
 *   result immediately; ordinary operations keep their existing batch reads.
 *   Nested rollback restores exact prior identities and item subscriptions.
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
 * @property {(run: (keys: readonly string[], previous: readonly string[] | undefined, change: KeysChange | undefined) => void, options?: CollectionWatchOptions) => void} watchKeys
 *   Calls `run` with the keys now, undefined previous keys and no change,
 *   then after each commit that inserts, removes or reorders keys, with that
 *   commit's change. Membership subscriptions run first in a commit's
 *   delivery. `previous` holds the last
 *   successfully delivered keys; a throwing callback does not advance it.
 *   Replacement of an item delivers even when the key strings are unchanged.
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
 *   that inserts, removes or reorders keys or changes one of `selection.fields`
 *   in any item, when the value changes under `equals`. Collection-wide
 *   subscriptions run last in a commit's delivery and are evaluated at most
 *   once per commit. Throws a `TypeError` inside a batch.
 * @property {<C, V>(selection: AggregateSelection<T, C, V>, run: (value: V, previous: V | undefined) => void, options?: CollectionWatchOptions) => void} watchAggregate
 *   Initializes a reversible reduction, calls run immediately, then maintains
 *   it from committed before/after records. Reorder and unrelated fields skip
 *   maintenance. Results preserve committed prefixes during reentrant writes
 *   and publish at most once per commit in collection-wide subscription order.
 *   Initial errors throw without subscribing. Maintenance errors report, omit
 *   partial results and recover by a full scan on the next relevant commit.
 *   Equality/delivery errors report without discarding healthy maintenance.
 *   Computational Promise/thenable results are rejected synchronously; actual
 *   Promise rejection is observed. Aborting releases retained values/callbacks.
 *   Throws inside batches or while another pure callback resolves a value.
 */

/**
 * One atomic membership request. Keys removed and reinserted get new
 * incarnations; their old item subscriptions do not transfer.
 * @template T The item record.
 * @typedef {object} MembershipChanges
 * @property {readonly string[]} [remove] Existing, unique keys to remove.
 * @property {readonly (readonly [string, T, (string | undefined)?])[]} [insert]
 *   New incarnations in stable supplied order. The optional anchor must be a
 *   current key not removed by this call; omission or undefined appends.
 */

/**
 * A fully validated, call-owned membership plan. Nothing publishes until this
 * plan exists; only journal/commit operands may outlive its application.
 * @typedef {object} MembershipPlan
 * @property {readonly Incarnation[]} losses Original incarnations to remove.
 * @property {readonly Incarnation[]} gains Fresh copied incarnations to insert.
 * @property {readonly string[]} removedKeys Removal metadata in supplied order.
 * @property {readonly string[]} insertedKeys Insertion metadata in supplied order.
 * @property {readonly string[]} next The frozen next order.
 */

/**
 * Read-and-subscribe access without mutation methods. Forward `size` with a
 * getter when constructing a wrapper; methods may be detached.
 * @template T The item.
 * @typedef {Readonly<Pick<Collection<T>, 'keys' | 'get' | 'has' | 'size' | 'watchKeys' | 'watchItem' | 'watch' | 'watchAggregate'>>} CollectionReader
 */

/**
 * A subscription record.
 * @typedef {object} SubscriptionBase
 * @property {readonly string[]} fields Declared item fields; none for membership.
 * @property {(argument: never) => unknown} read The selection's `read`.
 * @property {(a: unknown, b: unknown) => boolean} equals The selection's equality.
 * @property {(...args: never[]) => void} run The value or membership callback.
 * @property {unknown} last The last value delivered successfully.
 * @property {number} version The commit count when it was created.
 * @property {boolean} active Whether it still receives commits.
 * @property {Subscription[]} owner The array that holds it.
 * @property {AbortSignal | undefined} signal The subscription's owning signal.
 * @property {(() => void) | undefined} onAbort Listener removed when this subscription ends.
 */

/**
 * An ordinary item, membership or collection-wide watcher.
 * @typedef {SubscriptionBase & { aggregate?: false }} ValueSubscription
 */

/**
 * A reversible reduction's lifetime, healthy accumulator and recovery state.
 * @typedef {SubscriptionBase & {
 *   aggregate: true, registered: boolean, total: unknown, dirty: boolean,
 *   initial: () => unknown,
 *   add: (value: unknown, contribution: unknown) => unknown,
 *   remove: (value: unknown, contribution: unknown) => unknown
 * }} AggregateSubscription
 */

/** @typedef {ValueSubscription | AggregateSubscription} Subscription */

/**
 * Flat old/new immutable membership records; undefined denotes absence.
 * @typedef {(ItemRecord | undefined)[]} MembershipOperands
 */

/**
 * Flat triples: aggregate subscription, committed value, nullable error wrapper.
 * Wrapping errors preserves thrown null/undefined values without a sentinel clash.
 * @typedef {unknown[]} AggregateValues
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

/**
 * The shared empty list for immutable key and plan operands.
 * @type {readonly never[]}
 */
const NONE = Object.freeze([]);

/**
 * The change of a commit that inserts and removes nothing.
 * @type {KeysChange}
 */
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
  if (typeof item !== 'object' || item === null)
    throw new TypeError('A collection item must be an object.');
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
 * Validates and copies membership inputs and builds one order. Scratch belongs
 * only to this call; the caller guards getters against writes to this map.
 * @param {Map<string, Incarnation>} items Current incarnations, read only here.
 * @param {readonly string[]} keys Current immutable order.
 * @param {MembershipChanges<ItemRecord>} changes Supplied atomic changes.
 * @returns {MembershipPlan | null} A call-owned plan, or null for empty input.
 * @throws {TypeError} For invalid removals, insertions, anchors or records.
 */
function planMembership(items, keys, changes) {
  if (typeof changes !== 'object' || changes === null)
    throw new TypeError('updateMembership() needs a change object.');
  const suppliedRemove = changes.remove;
  const suppliedInsert = changes.insert;
  const remove = suppliedRemove === undefined ? NONE : suppliedRemove;
  const insert = suppliedInsert === undefined ? NONE : suppliedInsert;
  if (!Array.isArray(remove) || !Array.isArray(insert))
    throw new TypeError('Membership changes must be arrays.');
  const removeLength = remove.length;
  const insertLength = insert.length;
  if (removeLength === 0 && insertLength === 0) return null;
  /** @type {Incarnation[] | null} */
  const losses = removeLength > 0 ? new Array(removeLength) : null;
  /** @type {Incarnation[] | null} */
  const gains = insertLength > 0 ? new Array(insertLength) : null;
  /** @type {string[] | null} */
  const removedKeys = removeLength > 0 ? new Array(removeLength) : null;
  /** @type {string[] | null} */
  const insertedKeys = insertLength > 0 ? new Array(insertLength) : null;
  /** @type {string | undefined} */
  let singleRemoved;
  /** @type {Set<string> | null} */
  let removing = null;
  /** @type {Set<string> | null} */
  let adding = null;
  /** @type {Map<string | undefined, Incarnation[]> | null} */
  let buckets = null;
  /** @type {string | undefined} */
  let singleBefore;
  /** @type {readonly string[] | null} */
  let next = null;
  // Exact-size operands avoid growable backing stores; one removal needs no set.
  if (removeLength > 1) removing = new Set();
  if (insertLength > 1) adding = new Set();
  if (insertLength > 1) buckets = new Map();
  for (let at = 0; at < removeLength; at++) {
    const key = remove[at];
    if (typeof key !== 'string' || removing?.has(key) || !items.has(key))
      throw new TypeError('Removal keys must be existing unique strings.');
    removing?.add(key);
    if (removeLength === 1) singleRemoved = key;
    if (losses !== null) losses[at] = /** @type {Incarnation} */ (items.get(key));
    if (removedKeys !== null) removedKeys[at] = key;
  }
  for (let at = 0; at < insertLength; at++) {
    const entry = insert[at];
    if (!Array.isArray(entry))
      throw new TypeError('Insertion entries must be [key, item, before?].');
    const entryLength = entry.length;
    if (entryLength < 2 || entryLength > 3)
      throw new TypeError('Insertion entries must be [key, item, before?].');
    const key = entry[0],
      value = entry[1],
      before = entry[2];
    if (
      typeof key !== 'string' ||
      adding?.has(key) ||
      (items.has(key) && key !== singleRemoved && !removing?.has(key))
    )
      throw new TypeError('Insertion keys must be unique and cannot replace survivors.');
    if (
      before !== undefined &&
      (typeof before !== 'string' ||
        !items.has(before) ||
        before === singleRemoved ||
        removing?.has(before))
    )
      throw new TypeError('An insertion anchor must be a surviving key.');
    adding?.add(key);
    const item = incarnate(key, toItemRecord(value));
    if (gains !== null) gains[at] = item;
    if (insertedKeys !== null) insertedKeys[at] = key;
    if (insertLength === 1) singleBefore = before;
    else {
      let bucket = buckets?.get(before);
      if (bucket === undefined) {
        bucket = [];
        buckets?.set(before, bucket);
      }
      bucket.push(item);
    }
  }
  // A validated single insertion needs the same order copy as ordinary insert,
  // without per-key removal/anchor/bucket checks or empty removal buffers.
  const singleKey =
    insertLength === 1
      ? /** @type {Incarnation} */ (/** @type {Incarnation[]} */ (gains)[0]).key
      : undefined;
  if (removeLength === 0 && insertLength === 1) {
    const order = [...keys];
    const key = /** @type {string} */ (singleKey);
    if (singleBefore === undefined) order.push(key);
    else order.splice(order.indexOf(singleBefore), 0, key);
    return {
      losses: NONE,
      gains: gains ?? NONE,
      removedKeys: NONE,
      insertedKeys: insertedKeys ?? NONE,
      next: Object.freeze(order),
    };
  }
  const order = [];
  for (let at = 0; at < keys.length; at++) {
    const key = /** @type {string} */ (keys[at]);
    if (key === singleRemoved || removing?.has(key)) continue;
    if (insertLength === 1 && singleBefore === key) order.push(/** @type {string} */ (singleKey));
    const bucket = buckets?.get(key);
    if (bucket !== undefined)
      for (let index = 0; index < bucket.length; index++)
        order.push(/** @type {Incarnation} */ (bucket[index]).key);
    order.push(key);
  }
  const tail = buckets?.get(undefined);
  if (insertLength === 1 && singleBefore === undefined)
    order.push(/** @type {string} */ (singleKey));
  if (tail !== undefined)
    for (let index = 0; index < tail.length; index++)
      order.push(/** @type {Incarnation} */ (tail[index]).key);
  next = Object.freeze(order);
  return {
    losses: losses ?? NONE,
    gains: gains ?? NONE,
    removedKeys: removedKeys ?? NONE,
    insertedKeys: insertedKeys ?? NONE,
    next,
  };
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
  let aggregateCount = 0;
  /** @type {(AggregateValues | undefined)[] | null} */
  let queuedAggregate = null;
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
  let baseKeys = NONE;

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
    if (writing)
      throw new TypeError('The collection cannot be written while a patch is being resolved.');
  }

  /**
   * Ends a subscription and releases what it captured.
   * @param {Subscription} subscription The subscription.
   * @returns {void}
   */
  function release(subscription) {
    if (subscription.aggregate) {
      if (subscription.registered) aggregateCount--;
      subscription.registered = false;
      subscription.total = undefined;
      subscription.initial = released;
      subscription.add = released;
      subscription.remove = released;
      subscription.dirty = false;
    }
    subscription.active = false;
    subscription.read = released;
    subscription.equals = releasedEquals;
    subscription.run = released;
    subscription.last = undefined;
    if (subscription.onAbort !== undefined) {
      subscription.signal?.removeEventListener('abort', subscription.onAbort);
    }
    subscription.signal = undefined;
    subscription.onAbort = undefined;
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
      if (
        !subscription.active ||
        subscription.equals(value, subscription.last) ||
        !subscription.active
      )
        return;
      run(value, subscription.last);
      if (subscription.active) subscription.last = value;
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
    if (!Array.isArray(changed))
      return changedIn(
        fields,
        /** @type {ItemRecord} */ (after),
        /** @type {ItemRecord} */ (before),
      );
    const befores = /** @type {ItemRecord[]} */ (before);
    const afters = /** @type {ItemRecord[]} */ (after);
    for (let index = 0; index < changed.length; index++) {
      if (
        changedIn(
          fields,
          /** @type {ItemRecord} */ (afters[index]),
          /** @type {ItemRecord} */ (befores[index]),
        )
      )
        return true;
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
            const run =
              /** @type {(keys: readonly string[], previous: readonly string[] | undefined, change: KeysChange | undefined) => void} */ (
                subscription.run
              );
            try {
              const value = keys;
              run(value, /** @type {readonly string[]} */ (subscription.last), change);
              if (subscription.active) subscription.last = value;
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
            deliverItem(
              changed,
              /** @type {ItemRecord} */ (queuedBefore[at]),
              /** @type {ItemRecord} */ (queuedAfter[at]),
              commitVersion,
            );
          }
        }
        const aggregateValues = queuedAggregate?.[at];
        let aggregateAt = 0;
        for (let index = 0; index < wide.length; index++) {
          const subscription = /** @type {Subscription} */ (wide[index]);
          const resultAt = aggregateValues?.[aggregateAt] === subscription ? aggregateAt : -1;
          if (resultAt !== -1) aggregateAt += 3;
          if (!subscription.active || subscription.version >= commitVersion) continue;
          if (subscription.aggregate) {
            if (aggregateValues && resultAt !== -1) {
              if (aggregateValues[resultAt + 2] !== null)
                deliverError(
                  /** @type {{ caught: unknown }} */ (aggregateValues[resultAt + 2]).caught,
                );
              else {
                const value = aggregateValues[resultAt + 1];
                try {
                  const equal = calculate(
                    subscription.equals,
                    value,
                    subscription.last,
                    2,
                    'equals',
                  );
                  if (!subscription.active || equal) continue;
                  const run = /** @type {(value: unknown, previous: unknown) => void} */ (
                    subscription.run
                  );
                  run(value, subscription.last);
                  if (subscription.active) subscription.last = value;
                } catch (error) {
                  deliverError(error);
                }
              }
            }
            continue;
          }
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
      queuedAggregate = null;
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
   * @param {MembershipOperands | null} [membershipOperands] Committed old/new membership pairs.
   * @returns {void}
   */
  function commit(keysChanged, change, changed, before, after, membershipOperands = null) {
    version++;
    queuedVersions[queued] = version;
    queuedKeys[queued] = keysChanged;
    queuedChanges[queued] = change;
    queuedItems[queued] = changed;
    queuedBefore[queued] = before;
    queuedAfter[queued] = after;
    queued++;
    if (aggregateCount > 0) {
      // Abort inside a projection must not splice the wide array while another
      // aggregate is still awaiting maintenance. Drain owns the deferred unlink.
      const wasDelivering = delivering;
      delivering = true;
      let values;
      try {
        values = maintainAggregates(changed, before, after, membershipOperands);
      } finally {
        delivering = wasDelivering;
      }
      if (values !== null) {
        if (queuedAggregate === null && queued === 1) queuedAggregate = [values];
        else {
          queuedAggregate ??= [];
          queuedAggregate[queued - 1] = values;
        }
      }
    }
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
      if (kind === RECORD)
        /** @type {Incarnation} */ (first).record = /** @type {ItemRecord} */ (second);
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

  /**
   * Applies one operation-local plan through the existing undo/delivery path.
   * @param {MembershipChanges<ItemRecord>} changes Atomic membership changes.
   * @returns {void}
   */
  function updateMembership(changes) {
    guard();
    /** @type {MembershipPlan | null} */
    let plan;
    writing = true;
    try {
      plan = planMembership(items, keys, changes);
    } finally {
      writing = false;
    }
    if (plan === null) return;
    const { losses, gains, removedKeys, insertedKeys, next } = plan;
    const previous = keys;
    keys = /** @type {readonly string[]} */ (next);
    if (depth > 0) log.push(KEYS, previous, undefined);
    for (let at = 0; at < losses.length; at++) {
      const item = /** @type {Incarnation} */ (losses[at]);
      items.delete(item.key);
      if (depth > 0) {
        log.push(ENTRY, item.key, item, REMOVED, undefined, undefined);
        removed.push(item);
      } else end(item);
    }
    for (let at = 0; at < gains.length; at++) {
      const item = /** @type {Incarnation} */ (gains[at]);
      items.set(item.key, item);
      if (depth > 0) {
        log.push(ENTRY, item.key, undefined, INSERTED, undefined, undefined);
        inserted.push(item);
      }
    }
    if (depth === 0)
      commit(
        true,
        { inserted: insertedKeys, removed: removedKeys },
        null,
        null,
        null,
        aggregateCount > 0
          ? [
              ...losses.flatMap((item) => [item.record, undefined]),
              ...gains.flatMap((item) => [undefined, item.record]),
            ]
          : null,
      );
  }

  /** @type {Collection<ItemRecord>['insert']} */
  function insert(key, value, before) {
    guard();
    if (typeof key !== 'string') throw new TypeError('Collection keys must be strings.');
    if (items.has(key)) throw new TypeError(`Item "${key}" is already in the collection.`);
    if (before !== undefined && !items.has(before))
      throw new TypeError(`No item "${before}" to insert before.`);
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
    commit(
      true,
      { inserted: [key], removed: NONE },
      null,
      null,
      null,
      aggregateCount > 0 ? [undefined, item.record] : null,
    );
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
    commit(
      true,
      { inserted: NONE, removed: [key] },
      null,
      null,
      null,
      aggregateCount > 0 ? [item.record, undefined] : null,
    );
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
      if (!items.has(key) || seen.has(key))
        throw new TypeError('reorder() needs a permutation of the current keys.');
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
    baseKeys = NONE;
  }

  /**
   * Builds and queues the outermost batch's commit: ends the removed items,
   * then reports membership, order and the items whose final records differ
   * from their records before the batch.
   * @returns {void}
   */
  function commitBatch() {
    const base = baseKeys;
    /** @type {MembershipOperands | null} */
    const aggregateMembership =
      aggregateCount > 0 && (inserted.length > 0 || removed.length > 0) ? [] : null;
    const fresh = inserted.length > 0 ? new Set(inserted) : null;
    /** @type {string[]} */
    const removedKeys = [];
    for (const item of removed) {
      if (fresh?.has(item)) continue;
      aggregateMembership?.push(item.base ?? item.record, undefined);
      end(item);
      removedKeys.push(item.key);
    }
    /** @type {string[]} */
    const insertedKeys = [];
    for (const item of inserted)
      if (items.get(item.key) === item) {
        insertedKeys.push(item.key);
        aggregateMembership?.push(undefined, item.record);
      }
    /** @type {Incarnation[]} */
    const changed = [];
    /** @type {ItemRecord[]} */
    const before = [];
    /** @type {ItemRecord[]} */
    const after = [];
    for (let index = 0; index < touched.length; index++) {
      const item = /** @type {Incarnation} */ (touched[index]);
      const base = /** @type {ItemRecord} */ (item.base);
      if (
        fresh?.has(item) ||
        item.ended ||
        items.get(item.key) !== item ||
        !differs(item.record, base)
      )
        continue;
      changed.push(item);
      before.push(base);
      after.push(item.record);
    }
    resetBatch();
    const membershipChanged = insertedKeys.length > 0 || removedKeys.length > 0;
    if (!membershipChanged && sameKeys(base, keys)) keys = base;
    const keysChanged = membershipChanged || keys !== base;
    if (!keysChanged && changed.length === 0) return;
    const change = membershipChanged ? { inserted: insertedKeys, removed: removedKeys } : NO_CHANGE;
    if (changed.length === 0) commit(keysChanged, change, null, null, null, aggregateMembership);
    else if (changed.length === 1) {
      commit(
        keysChanged,
        change,
        /** @type {Incarnation} */ (changed[0]),
        /** @type {ItemRecord} */ (before[0]),
        /** @type {ItemRecord} */ (after[0]),
        aggregateMembership,
      );
    } else commit(keysChanged, change, changed, before, after, aggregateMembership);
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
          throw new TypeError(
            'batch() needs a synchronous callback; it returned a promise or thenable.',
          );
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
   * @param {(...args: never[]) => void} run Its value or membership callback.
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
      throw new TypeError(
        "A selection's read wrote to the collection during watch; read must be pure.",
      );
    }
    if (signal?.aborted) return;
    /** @type {Subscription} */
    const subscription = {
      fields,
      read,
      equals,
      run,
      last: value,
      version,
      active: true,
      owner,
      signal,
      onAbort: undefined,
    };
    owner.push(subscription);
    if (signal !== undefined) {
      /**
       * Ends the subscription with its owner.
       * @returns {void}
       */
      const onAbort = () => {
        // Dispatch already removed this once-listener. Other release paths
        // must still detach listeners whose owning signal remains live.
        subscription.signal = undefined;
        subscription.onAbort = undefined;
        deactivate(subscription);
      };
      subscription.onAbort = onAbort;
      signal.addEventListener('abort', onAbort, { once: true });
    }
    try {
      first(value);
    } catch (error) {
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
      throw new TypeError(
        'A collection cannot be watched inside batch(); its first value could be rolled back.',
      );
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
      (value) => run(/** @type {readonly string[]} */ (value), undefined, undefined),
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
    const { fields: declared, read, equals = Object.is } = selection;
    if (!Array.isArray(declared) || declared.some((field) => typeof field !== 'string')) {
      throw new TypeError('A collection-wide selection needs a fields array of field names.');
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

  /**
   * Invokes a pure callback and validates its result under the existing write guard.
   * @param {(...args: never[]) => unknown} fn Computational callback.
   * @param {unknown} a Its optional first argument.
   * @param {unknown} b Its optional second argument.
   * @param {0 | 1 | 2} arity Number of supplied arguments.
   * @param {string} name Callback name for diagnostics.
   * @returns {unknown} The synchronous result.
   */
  function calculate(fn, a, b, arity, name) {
    const previous = writing;
    writing = true;
    const callback = /** @type {(a?: unknown, b?: unknown) => unknown} */ (fn);
    try {
      return synchronousResult(
        arity === 0 ? callback() : arity === 1 ? callback(a) : callback(a, b),
        name,
      );
    } finally {
      writing = previous;
    }
  }

  /**
   * Checks registration lifetime and in-progress initialization cancellation.
   * @param {AggregateSubscription} subscription Aggregate owner.
   * @returns {boolean} Whether user work may continue.
   */
  function aggregateAlive(subscription) {
    return subscription.active && !subscription.signal?.aborted;
  }

  /**
   * Initializes or exceptionally rebuilds in current key order, without a cache.
   * @param {AggregateSubscription} subscription Aggregate owner.
   * @returns {unknown} The completed accumulator, or undefined after abort.
   */
  function rebuildAggregate(subscription) {
    let total = calculate(subscription.initial, undefined, undefined, 0, 'initial');
    for (let index = 0; index < keys.length; index++) {
      if (!aggregateAlive(subscription)) return undefined;
      const item = /** @type {Incarnation} */ (items.get(/** @type {string} */ (keys[index])));
      const contribution = calculate(subscription.read, item.record, undefined, 1, 'read');
      if (!aggregateAlive(subscription)) return undefined;
      total = calculate(subscription.add, total, contribution, 2, 'add');
    }
    return total;
  }

  /**
   * Applies one immutable old/new record pair to a staged accumulator.
   * @param {AggregateSubscription} subscription Aggregate owner.
   * @param {unknown} total Healthy or partially staged accumulator.
   * @param {ItemRecord | undefined} before Removed/previous record.
   * @param {ItemRecord | undefined} after Inserted/next record.
   * @returns {unknown} The next staged accumulator.
   */
  function aggregatePair(subscription, total, before, after) {
    const oldValue =
      before === undefined ? undefined : calculate(subscription.read, before, undefined, 1, 'read');
    if (!aggregateAlive(subscription)) return total;
    const newValue =
      after === undefined ? undefined : calculate(subscription.read, after, undefined, 1, 'read');
    if (!aggregateAlive(subscription)) return total;
    if (before !== undefined && after !== undefined && Object.is(oldValue, newValue)) return total;
    if (before !== undefined) total = calculate(subscription.remove, total, oldValue, 2, 'remove');
    if (!aggregateAlive(subscription)) return total;
    if (after !== undefined) total = calculate(subscription.add, total, newValue, 2, 'add');
    return total;
  }

  /**
   * Maintains affected aggregates before user delivery can queue a later commit.
   * @param {Incarnation | Incarnation[] | null} changed Surviving changed items.
   * @param {ItemRecord | ItemRecord[] | null} before Their pre-commit records.
   * @param {ItemRecord | ItemRecord[] | null} after Their committed records.
   * @param {MembershipOperands | null} membershipOperands Removed/inserted records.
   * @returns {AggregateValues | null} Results and errors, or no affected watcher.
   */
  function maintainAggregates(changed, before, after, membershipOperands) {
    /** @type {AggregateValues | null} */
    let values = null;
    for (const subscription of wide) {
      if (!subscription.aggregate || !subscription.active || subscription.version >= version)
        continue;
      const many = Array.isArray(changed);
      let relevant = membershipOperands !== null && membershipOperands.length > 0;
      if (!relevant && changed !== null && subscription.fields.length > 0) {
        if (!many)
          relevant = changedIn(
            subscription.fields,
            /** @type {ItemRecord} */ (after),
            /** @type {ItemRecord} */ (before),
          );
        else
          for (let at = 0; at < changed.length && !relevant; at++)
            relevant = changedIn(
              subscription.fields,
              /** @type {ItemRecord} */ (/** @type {ItemRecord[]} */ (after)[at]),
              /** @type {ItemRecord} */ (/** @type {ItemRecord[]} */ (before)[at]),
            );
      }
      if (!relevant) continue;
      let total = subscription.total,
        error = null;
      try {
        if (subscription.dirty) total = rebuildAggregate(subscription);
        else {
          if (membershipOperands)
            for (
              let at = 0;
              at < membershipOperands.length && aggregateAlive(subscription);
              at += 2
            )
              total = aggregatePair(
                subscription,
                total,
                membershipOperands[at],
                membershipOperands[at + 1],
              );
          if (changed !== null && aggregateAlive(subscription)) {
            if (!many) {
              const previousRecord = /** @type {ItemRecord} */ (before);
              const nextRecord = /** @type {ItemRecord} */ (after);
              if (changedIn(subscription.fields, nextRecord, previousRecord))
                total = aggregatePair(subscription, total, previousRecord, nextRecord);
            } else
              for (let at = 0; at < changed.length && aggregateAlive(subscription); at++) {
                const previousRecord = /** @type {ItemRecord} */ (
                  /** @type {ItemRecord[]} */ (before)[at]
                );
                const nextRecord = /** @type {ItemRecord} */ (
                  /** @type {ItemRecord[]} */ (after)[at]
                );
                if (changedIn(subscription.fields, nextRecord, previousRecord))
                  total = aggregatePair(subscription, total, previousRecord, nextRecord);
              }
          }
        }
        if (!aggregateAlive(subscription)) continue;
        subscription.total = total;
        subscription.dirty = false;
      } catch (caught) {
        subscription.dirty = true;
        error = { caught };
      }
      if (values === null) values = [subscription, total, error];
      else values.push(subscription, total, error);
    }
    return values;
  }

  /** @type {Collection<ItemRecord>['watchAggregate']} */
  function watchAggregate(reduction, run, options = {}) {
    notInBatch();
    guard();
    /** @type {AggregateSubscription | undefined} */
    let subscription;
    writing = true;
    try {
      if (typeof reduction !== 'object' || reduction === null)
        throw new TypeError('An aggregate needs a reduction object.');
      const { fields: declared, read, initial, add, remove, equals = Object.is } = reduction;
      if (!Array.isArray(declared)) throw new TypeError('An aggregate needs fields.');
      const fields = [...declared];
      if (
        fields.some((field) => typeof field !== 'string') ||
        [read, initial, add, remove, equals, run].some((fn) => typeof fn !== 'function')
      )
        throw new TypeError('Invalid aggregate fields or callbacks.');
      subscription = {
        aggregate: true,
        registered: false,
        fields,
        read: /** @type {SubscriptionBase['read']} */ (read),
        initial,
        add: /** @type {AggregateSubscription['add']} */ (add),
        remove: /** @type {AggregateSubscription['remove']} */ (remove),
        equals: /** @type {SubscriptionBase['equals']} */ (equals),
        run: /** @type {SubscriptionBase['run']} */ (run),
        active: true,
        dirty: false,
        total: undefined,
        last: undefined,
        version,
        owner: wide,
        signal: options.signal,
        onAbort: undefined,
      };
      if (subscription.signal?.aborted) return;
      const total = rebuildAggregate(subscription);
      if (!aggregateAlive(subscription)) {
        release(subscription);
        return;
      }
      subscription.total = total;
      subscription.last = total;
    } catch (error) {
      if (subscription) release(subscription);
      throw error;
    } finally {
      writing = false;
    }
    wide.push(subscription);
    subscription.registered = true;
    aggregateCount++;
    if (subscription.signal) {
      /** @returns {void} Release the aggregate after its once-listener dispatch. */
      const onAbort = () => {
        subscription.signal = undefined;
        subscription.onAbort = undefined;
        deactivate(subscription);
      };
      subscription.onAbort = onAbort;
      subscription.signal.addEventListener('abort', onAbort, { once: true });
    }
    try {
      const first = /** @type {(value: unknown, previous: undefined) => void} */ (subscription.run);
      first(subscription.total, undefined);
    } catch (error) {
      deactivate(subscription);
      throw error;
    }
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
    updateMembership,
    insert,
    remove,
    reorder,
    batch,
    watchKeys,
    watchItem,
    watch,
    watchAggregate,
  };
  // Items are typed by T at the boundary; inside, records are frozen field maps.
  return /** @type {Collection<T>} */ (/** @type {unknown} */ (collection));
}
