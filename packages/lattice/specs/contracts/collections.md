# Collections

## Purpose and status

A keyed collection holds a capability's items by key, so that changing one item
costs work independent of the collection's size and one action that changes
many items publishes its result once. It is a separate pure capability beside
the store core, with its own entry point; the store's snapshot contract does
not change.

**Status:** agreed contract, implemented in `src/modules/collection/` and
exported from `@canonic2/lattice/collection`. It was accepted on 2026-10-25 for
implementation within the scope the [collection record](../../research/collection.md)
measured: a lab prototype met every agreed threshold against the shared map and
against one store per item, at 10, 100 and 1,000 items. Those figures are
prototype evidence, not established product performance; the shipping
implementation is measured again as the record's next stage.

This contract settles the primitive. It does not establish that any
application's large lists are ready: selection over much larger collections,
such as 10,000 items, and mounting only visible rows are separate work.

## Requirements

1. **Item updates are local.** Updating one item copies only that item's
   record. It does not copy the keys, the map of items or any other item, and
   evaluates no subscription of another item.
2. **Bulk actions publish once.** One action that changes many items, written
   as one `batch`, publishes its completed state in one commit. Each affected
   subscription is evaluated at most once per commit, and no subscriber
   observes a partly applied action. A collection-wide value is evaluated at
   most once per commit, and not at all for a commit that cannot change it.
3. **Membership is owned by the collection.** Insertion, removal and reorder
   are collection operations. Removing an item ends that item's subscriptions
   when the removal commits.
4. **Aggregate costs stay explicit.** Item-local updates are selective. A
   collection-wide derivation may read every item each time it is evaluated;
   the collection bounds how often it runs, not what it costs.

## API

```js
import { createCollection } from '@canonic2/lattice/collection';

const tasks = createCollection([
  ['t1', { label: 'Write the plan', done: false }],
  ['t2', { label: 'Run the lab', done: true }],
]);

/** An item selection: the item fields `read` depends on. */
const label = { fields: ['label'], read: (task) => task.label };
/** A collection-wide selection: the item fields `read` depends on. */
const open = {
  items: ['done'],
  read: (collection) => {
    const keys = collection.keys();
    let count = 0;
    for (let index = 0; index < keys.length; index++) if (!collection.get(keys[index]).done) count++;
    return count;
  },
};

tasks.update('t1', { label: 'Write the proposal' });
tasks.update('t1', (task) => ({ done: !task.done }));
tasks.insert('t3', { label: 'Review', done: false }, 't1');
tasks.remove('t2');
tasks.reorder(['t3', 't1']);
tasks.batch(() => {
  for (const key of tasks.keys()) tasks.update(key, { done: true });
});
tasks.watchKeys((keys, change) => reconcile(keys, change), { signal });
tasks.watchItem('t1', label, (text) => (row.label = text), { signal });
tasks.watch(open, (count) => (summary.text = `${count} open`), { signal });
```

| Function | Behavior |
| --- | --- |
| `createCollection(entries, options)` | Copies each `[key, item]` into a frozen record. Keys are strings and unique; each item's own enumerable string keys define that item's fields. A key that is not a string, a duplicate key, or an item that is not an object or has symbol keys throws a `TypeError`. `options.report` is as in the store |
| `keys()` | The current keys in order, as a frozen array. The same array is returned until membership or order changes |
| `get(key)` | The item's current frozen record, or `undefined` for a key not in the collection. The same record is returned until that item changes |
| `has(key)`, `size` | Membership without copying |
| `update(key, patch)` | Patches one item's fields as the store's `set` does: an object or a function of the item's record, each patched value read once, unchanged values silent. A field the item does not have, a patch that is not an object, or a key not in the collection throws a `TypeError` |
| `insert(key, item, before)` | Adds an item before the key `before`, or at the end without it, in one commit. A key already in the collection, or a `before` not in it, throws a `TypeError` |
| `remove(key)` | Removes an item; its item subscriptions end when the removal commits. A key not in the collection throws a `TypeError` |
| `reorder(keys)` | Replaces the order with a permutation of the current keys. Anything else throws a `TypeError`; the current order is silent |
| `batch(fn)` | As the store's `batch`: synchronous, savepoints, rollback on throw, thenables rejected with a `TypeError`, one commit |
| `watchKeys(run, options)` | Subscribes to membership and order. `run(keys, change)` receives the keys and, after a commit, the keys it `inserted` and `removed`; the initial call receives no change |
| `watchItem(key, selection, run, options)` | Subscribes to one item through a selection over its fields. Selections are plain objects shared by every item. A key not in the collection or a field the item does not have throws a `TypeError` and subscribes nothing |
| `watch(selection, run, options)` | Subscribes to a collection-wide value. `selection.items` lists the item fields `read` depends on and may be empty; `read` receives the collection's read-only view: `keys`, `get`, `has` and `size` |

- Every write rejected with a `TypeError` changes nothing.
- Any write, including `batch`, made while `update` resolves a patch, from a
  patch function or a getter, throws a `TypeError`, as in the store.
- `watchKeys`, `watchItem` and `watch` inside a batch throw a `TypeError`. An
  aborted signal subscribes nothing; an error from the initial `read` or `run`
  propagates and leaves no subscription; an initial `read` that writes to the
  collection throws a `TypeError`.
- The functions do not use `this`. A capability keeps the collection private
  and hands out a read-only view (`keys`, `get`, `has`, `size`, `watchKeys`,
  `watchItem`, `watch`), as it does with a store's `state`. Writes stay behind
  the capability's named actions.
- Scalar state, such as a filter, stays in an ordinary store beside the
  collection. A store and a collection commit independently; no commit spans
  both.

The entry point exports `createCollection` and the `Collection`,
`CollectionView`, `ItemSelection`, `CollectionSelection`, `KeysChange`,
`CollectionOptions` and `CollectionWatchOptions` types.

## Storage

The collection keeps no whole-collection snapshot. Its state is:

- **a map from key to item.** Each item is one incarnation of its key: a frozen
  record of its fields and the item's subscriptions. A changing update builds
  one new record for that item; the map's other entries and every other record
  are untouched;
- **a frozen array of keys** in order, replaced only when membership or order
  changes. Insertion, removal and reorder copy it, at a cost proportional to
  the collection.

A caller can hold a keys array or an item record and compare it by identity:
neither changes after it is returned, and each is replaced only when what it
describes changes. There is no single object for the whole collection at one
commit; code that needs one, such as persistence, builds it from `keys()` and
`get`, at a cost proportional to the collection.

`keys()` returns a frozen array, for stable identity and immutability. In the
measured Chromium, iterating a frozen array with `for…of` allocated per
element where an indexed loop allocated nothing; measured hot paths, such as a
collection-wide `read`, use indexed loops. That engine cost does not change the
API.

### Item identity

Inserting a key creates a new incarnation, even when the key was removed
earlier, in the same batch or in an earlier commit. Item subscriptions belong
to an incarnation, not to the key: removing an item ends its incarnation's
subscriptions, and a later incarnation of the key starts with none. A
notification queued for an ended incarnation is dropped; it is never delivered
to the replacement's subscriptions. A commit that removes and reinserts a key
reports it in both `removed` and `inserted`, even when the keys array is
unchanged, so a membership binding replaces the key's row.

### Batches and rollback

- Reads during a batch see its pending writes.
- Rollback restores what the batch changed, not the whole collection: every
  write inside a batch records what it replaced, and a throw restores those
  records newest first back to the batch's start. A nested batch is a
  savepoint. Rollback costs time proportional to the changes.
- **Removal is transactional.** Inside a batch, `remove` takes the item out of
  the map and the keys, but its incarnation and subscriptions stay intact until
  the outermost batch commits. Rolling the removal back, by the batch or by a
  savepoint, restores the same incarnation with its subscriptions. When the
  outermost batch commits, the removed incarnations' subscriptions end and
  their callbacks are released, before any delivery of that commit. A
  subscription whose own signal aborted during the batch stays ended after a
  rollback. Outside a batch, a removal is its own commit and ends the
  subscriptions at once.
- A batch whose final state equals its starting state, in keys and in every
  changed item's fields, commits nothing.

## Commits and delivery

A commit records whether membership changed and which keys were inserted and
removed, whether the order changed, and, for each item that changed, its record
before and after, comparing final values with values at the batch's start. A
key inserted in a batch reports no field changes, and neither does a removed
one.

Subscriptions are held in three groups:

- **Membership subscriptions**, considered when membership or order changed.
- **Item subscriptions**, held by their incarnation. For a commit, only the
  subscriptions of changed items are considered, and among those only the ones
  with a declared field that differs between the item's records before and
  after the commit. Other items' subscriptions are never visited.
- **Collection-wide subscriptions**, considered when membership or order
  changed, since `read` may depend on `keys()` order, or when a changed item
  changed a field in `items`. Each is evaluated at most once per commit, and
  `equals` suppresses an unchanged result.

**Order.** Within a commit: membership subscriptions, then item subscriptions
by changed item in the order of each item's first change and in subscription
order within an item, then collection-wide subscriptions, each group in
subscription order. Membership bindings therefore create and remove rows
before item updates, and collection-wide values see every row already updated.
This differs from the store, which notifies in global subscription order. The
fixed order makes one commit's DOM writes run in a known sequence within one
synchronous call; it does not make the DOM update atomic or decide which frame
presents it.

A subscription created during a delivery, such as a row's item subscriptions
created by a membership binding, receives its initial value once and skips the
commit in progress and every earlier commit. The store's other delivery
guarantees apply unchanged: synchronous delivery before the writing call
returns, queued commits for writes made during delivery delivered in order,
the newest state at notification, removed subscriptions never called again, and
the same [delivery error](state-management.md#errors-during-delivery) policy.

## Costs

| Operation | Work |
| --- | --- |
| Update one item | One new record; the item's subscriptions only; collection-wide values only if a declared field changed |
| Bulk update of k items in one batch | k new records and their undo entries; up to one evaluation per affected item subscription; at most one evaluation per collection-wide value |
| Insert or remove | One incarnation and one keys array; membership and collection-wide subscriptions |
| Reorder | One keys array; membership and collection-wide subscriptions |
| Collection-wide value | Its `read` may scan every item each time it is evaluated |

**Accepted costs** (2026-10-25), from the prototype:

- **Bulk allocation.** A bulk update allocated about 170 bytes per changed item,
  nearly four times the shared map's single map copy: the new record, its undo
  entry and its records before and after for delivery. No threshold covers
  allocation; it is accepted for this design and measured again, with its
  garbage-collection effects, through the shipping implementation.
- **Linear aggregates.** A collection-wide value stays linear in the
  collection when evaluated: a done toggle at 1,000 items cost 45.9 µs in the
  prototype against 4.9 µs for a label update, which evaluates none.

Incremental aggregates and windowing are not part of this contract.

## Acceptance

| Requirement | Evidence |
| --- | --- |
| Storage identity and frozen records | `collection.test.js`, storage |
| Function patches, read once, reentrant writes rejected | `collection.test.js`, function patches |
| Delivery order, item-local evaluation, order-dependent aggregates, at most one evaluation per batch, no intermediate state | `collection.test.js`, delivery |
| Subscription validation, copied fields, abort, initial failures | `collection.test.js`, subscriptions |
| Delivery errors and the reporter fallback | `collection.test.js`, delivery errors |
| Rollback, savepoints, released bookkeeping | `collection.test.js`, batches |
| Transactional removal, restored subscriptions, released removed items | `collection.test.js`, transactional removal |
| Key reuse in a batch, during delivery and across commits | `collection.test.js`, key reuse |
| Pure entry point in Node | `tests/import.test.js` |
| Packed entry point and consumer types | `tests/package.test.js` with `tests/types/valid-collection.ts` and `invalid-collection-field.ts` |
| Performance within the agreed thresholds on the shipping path | The [collection record](../../research/collection.md); prototype evidence until the shipping-path stage is reported |

## Decisions

- **A separate capability, not store fields** (2026-10-25). Keyed fields inside
  `createStore` would need a collection value with its own identity rules in
  the store's snapshot, changing the store contract for every user.
- **No whole snapshot.** A persistent map would keep whole immutable snapshots
  with logarithmic copying, but every read and update would pay the tree's
  depth and it would still need a keyed index for item-local delivery.
- **Delivery order: membership, items, collection-wide values** (2026-10-25,
  owner). Membership bindings create or remove rows before item updates, and
  summaries follow.
- **Function patches, matching `set`** (2026-10-25, owner), with each value read
  once and writes rejected while a patch is resolved.
- **Optional `before` on `insert`** (2026-10-25, owner). Appending and then
  reordering would copy the keys twice and could take two commits.
- **No store-and-collection transactions** (2026-10-25, owner). They commit
  independently.
- **Transactional removal, order-dependent invalidation and incarnations**
  (2026-10-25, owner), as [storage](#storage) and
  [commits and delivery](#commits-and-delivery) describe.
- **Frozen `keys()`** (2026-10-25, owner). Stable identity and immutability are
  kept; an engine's iteration cost does not weaken the API.
- **Rejected shapes.** One store per item met the item-update requirement but
  cost about 84% more memory per item and 30% more teardown, and its bulk
  updates rederived collection-wide values once per item; keyed subscriptions
  over the shared map would keep copying the map on every update. Both are in
  the collection record.
