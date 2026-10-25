# Keyed collections

**Status:** proposal under evaluation, not agreed and not implemented. It
answers the open [collections](../contracts/state-management.md#collections-open)
question in the state management contract. On 2026-10-25 the owner accepted a
separate collection capability as the candidate, settled the open questions as
[decisions](#decisions) records, and agreed the [evaluation](#evaluation) and
its thresholds. The prototype evaluation met every threshold, as the
collection record's stage 4 reports; nothing ships until the owner accepts the
result and the proposal moves into the contracts.

## Problem

The [collection record](../../research/collection.md) measured the two shapes
the current API allows, at 10, 100 and 1,000 items:

- **Shared map.** One store field holds the items by id. Changing one item
  copies the map and considers every row's subscriptions, because selections
  skip subscriptions per field, not per item: 339 µs, 2,001 `read`s and 4 KB
  per label update at 1,000 items.
- **One store per item.** Item updates become independent of the collection's
  size, 5.2 µs and 85 bytes, but each item costs a whole store, about 520
  bytes, and teardown is 30% slower. A bulk update is one commit per item, so
  a collection-wide value is rederived and its DOM written once per item, with
  every intermediate state observable: updating every item's `done` at 1,000
  items took 21 ms against 4.2 ms.

An index of subscriptions by key alone does not fix the shared map: the update
still copies the map to keep the store's whole-state snapshot immutable.
Storage and update semantics have to change with the delivery.

## Requirements

1. **Item updates are local.** Updating one item copies only that item's
   record. It does not copy the keys, the map of items or any other item, and
   it evaluates no subscription of another item.
2. **Bulk actions publish once.** One action that changes many items publishes
   its completed state in one commit. Each affected subscription is evaluated
   at most once per commit, and no subscriber observes a partly applied action.
   A collection-wide value is evaluated at most once per commit, and not at all
   for a commit that cannot change it.
3. **Membership is owned by the collection.** Insertion, removal and reorder
   are collection operations; removing an item ends that item's subscriptions
   when the removal commits.
4. **Costs are measured.** Item subscription overhead, memory per item and
   every workload are measured against the shared-map baseline before the
   proposal is accepted.

## Proposed API

A new pure capability, `createCollection`, beside the store core. It is not a
store field: the store's snapshot contract stays unchanged.

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
    let count = 0;
    for (const key of collection.keys()) if (!collection.get(key).done) count++;
    return count;
  },
};

tasks.keys(); // frozen array of keys in order
tasks.get('t1'); // frozen item record, or undefined
tasks.update('t1', { label: 'Write the proposal' }); // one item, one commit
tasks.update('t1', (task) => ({ done: !task.done }));
tasks.insert('t3', { label: 'Review', done: false }, 't1'); // before t1; at the end without it
tasks.remove('t2');
tasks.reorder(['t3', 't1']);
tasks.batch(() => {
  for (const key of tasks.keys()) tasks.update(key, { done: true });
}); // one commit
tasks.watchKeys((keys, change) => reconcile(keys, change), { signal });
tasks.watchItem('t1', label, (text) => (row.label = text), { signal });
tasks.watch(open, (count) => (summary.text = `${count} open`), { signal });
```

| Function | Behavior |
| --- | --- |
| `createCollection(entries, options)` | Copies each `[key, item]` into a frozen record. Keys are strings and unique; items are objects whose own string keys define that item's fields. Duplicate keys or a non-object item throw a `TypeError`. `options.report` is as in the store |
| `keys()` | The current keys in order, as a frozen array. The same array is returned until membership or order changes |
| `get(key)` | The item's current frozen record, or `undefined` for a key not in the collection. The same record is returned until that item changes |
| `has(key)`, `size` | Membership tests without copying |
| `update(key, patch)` | Patches one item's fields, exactly as the store's `set`: an object or a function of the item's record, each value read once, unknown fields rejected, unchanged values silent, and any write made while a patch is being resolved rejected with a `TypeError`. A key not in the collection throws a `TypeError` and changes nothing |
| `insert(key, item, before)` | Adds an item before an existing key, or at the end without `before`, in one commit. An existing key, or a `before` not in the collection, throws a `TypeError` |
| `remove(key)` | Removes an item; its item subscriptions end when the removal commits. A missing key throws a `TypeError` |
| `reorder(keys)` | Replaces the order. Anything but a permutation of the current keys throws a `TypeError`; the current order is silent |
| `batch(fn)` | As the store's `batch`: synchronous, savepoints, rollback on throw, thenables rejected, one commit |
| `watchKeys(run, options)` | Subscribes to membership and order. `run(keys, change)` receives the keys and, after a commit, which keys were `inserted` and `removed` by it; the initial call receives no change |
| `watchItem(key, selection, run, options)` | Subscribes to one item through a selection over its fields. Selections are plain objects shared by every item. A key not in the collection throws a `TypeError` and subscribes nothing |
| `watch(selection, run, options)` | Subscribes to a collection-wide value. `selection.items` lists the item fields `read` depends on; `read` receives the collection's read-only view (`keys`, `get`, `has`, `size`) |

Every write rejected with a `TypeError` changes nothing. `watchKeys`,
`watchItem` and `watch` inside a batch throw a `TypeError`, as the store's
`watch` does.

A capability keeps the collection private and hands out a read-only view
(`keys`, `get`, `has`, `size`, `watchKeys`, `watchItem`, `watch`), as it does
with a store's `state`. Writes stay behind named actions. Scalar state, such as
a filter, stays in an ordinary store beside the collection; a store and a
collection commit independently, and no commit spans both.

## Storage and update semantics

The collection does not keep a whole-collection snapshot. Its state is:

- **a map from key to item**, held privately. Each item is one incarnation of
  its key: a frozen record of its fields and its item subscriptions. A
  changing update builds one new record for that item; the map's other entries
  and every other record are untouched, so an update copies one item, not the
  collection;
- **a frozen array of keys** in order, replaced only when membership or order
  changes. Insertion, removal and reorder copy it, which costs time
  proportional to the collection, as the shared map's order array does and as
  reconciling the rows already requires.

Snapshot guarantees move from the whole state to its parts. A caller can hold a
keys array or an item record and compare it by identity: neither changes after
it is returned, and each is replaced only when what it describes changes.
There is no single object for "the collection at commit n"; code that needs one,
such as persistence, builds it explicitly from `keys()` and `get`, at a cost
proportional to the collection.

### Item identity

Inserting a key creates a new incarnation, even when the key was removed
earlier, in the same batch or in an earlier commit. Item subscriptions belong
to an incarnation, not to the key: removing an item ends its incarnation's
subscriptions, and a later incarnation of the key starts with none. A
notification queued for an ended incarnation is dropped; it is never
delivered to the replacement's subscriptions. A commit that removes and
reinserts a key reports it in both `removed` and `inserted`, even when the keys
array is unchanged, so a membership binding replaces the key's row.

### Batches and rollback

**Reads during a batch** see the batch's pending writes, as in the store.
**Rollback** restores what the batch changed, not the whole collection: every
write inside a batch records what it replaced in an undo log, and a throw
restores those entries in reverse back to the batch's start. A nested batch is
a savepoint at a position in that log. Rollback costs time proportional to the
changes, and a batch that changes nothing logs nothing.

**Removal is transactional.** Inside a batch, `remove` takes the item out of
the map and the keys, but its incarnation and subscriptions stay intact until
the outermost batch commits. Rolling the removal back, by the batch or by a
savepoint, restores the same incarnation with its subscriptions. When the
outermost batch commits, the removed incarnations' subscriptions end and their
callbacks are released, before any delivery of that commit. A subscription
whose own signal aborted during the batch stays ended after a rollback. Outside
a batch, a removal is its own commit and ends the subscriptions at once.

## Commits and delivery

A commit records whether membership changed (keys inserted or removed, with
which), whether the order changed, and, for each item incarnation that
changed, which of its fields changed, comparing final values with values at
the batch's start as the store does. A key inserted in a batch reports no field
changes; a key removed reports none either.

Subscriptions are held in three groups:

- **Membership subscriptions**, considered when membership or order changed.
- **Item subscriptions**, indexed by incarnation. For a commit, only the
  subscriptions of changed incarnations are considered, and among those only
  the ones whose declared fields changed. Other items' subscriptions are never
  visited.
- **Collection-wide subscriptions**, considered when membership or order
  changed, since `read` may depend on `keys()` order, or when a changed item
  changed a field in `items`. Each is evaluated at most once per commit, and
  `equals` suppresses an unchanged result.

Delivery order within a commit: membership subscriptions, then item
subscriptions by changed incarnation in order of first change and in
subscription order within an incarnation, then collection-wide subscriptions,
each group in subscription order. Membership bindings therefore create and
remove rows before item updates, and collection-wide values see every row
already updated. This differs from the store, which notifies in global
subscription order. The order makes the DOM writes of one commit run in a
fixed sequence within one synchronous call; it does not by itself make the DOM
update atomic or decide which frame presents it.

A subscription created during a delivery, such as a row's item subscriptions
created by a membership binding, receives its initial value once and skips the
commit in progress and every commit made before it. The store's other
guarantees carry over: synchronous delivery, at most one evaluation per
subscription per commit, queued commits for writes made during delivery
delivered in order, the newest state at notification, removed subscriptions
never called again, and the same error policy.

## Costs

| Operation | Shared map | Store per item | Keyed collection, expected |
| --- | --- | --- | --- |
| Update one item | Copies the map; 2N `read`s; one aggregate derivation | One item store; 1 `read` | One record; the incarnation's subscriptions only; an aggregate only if a declared field changed |
| Bulk update of k items | One map copy; 2N `read`s; one derivation | k commits; k derivations of N items each | k records; up to 2k `read`s; at most one derivation |
| Insert or remove | Copies the map and order | Membership store plus one item store | One incarnation and one keys array |
| Memory per item | One record and map entry; two subscription records | A store, about 520 bytes, plus subscriptions | One record, one map entry and one incarnation, plus subscriptions |

The expected figures are proposals until measured. A collection-wide value is
still derived over every item when it is evaluated; the proposal makes that
happen at most once per commit, and not at all for commits that change none of
its fields or the keys. Maintaining aggregates incrementally is out of scope.

## Alternatives considered

| Alternative | Why not proposed |
| --- | --- |
| Index subscriptions by key over the shared map field | Keeps whole-map copying on every update, failing requirement 1 |
| Keyed fields inside `createStore` | One commit could span scalar fields and items, but the store's snapshot would need a collection value with its own identity rules, changing the agreed store contract for every user |
| A persistent map, sharing structure between whole snapshots | Keeps whole immutable snapshots with logarithmic copying per update, but every read and update pays the tree's depth and allocations, and it still needs a keyed index for requirement 1 |
| One store per item with an application-level batch | Needs a cross-store transaction, which is ruled out, and keeps a store's memory per item |
| Incremental aggregates | Removes repeated recounting with stores per item, but not per-store memory or intermediate notifications |

## Decisions

Recorded 2026-10-25, by the owner, on the proposal's open questions:

- **Delivery order:** membership, then items, then collection-wide values.
  Membership bindings create or remove rows before item updates, and summaries
  follow. New subscriptions receive their initial value once and skip the
  commit in progress.
- **Function patches:** `update` takes them, matching `set`: each value read
  once, and writes rejected while a patch is being resolved.
- **Insertion position:** `insert` keeps an optional `before`. Appending and
  then reordering would copy the keys twice and could take two commits.
- **Store and collection transactions:** out of scope for the first version;
  a store and a collection commit independently.
- **Corrections to the first draft:** removal is transactional, as
  [batches and rollback](#batches-and-rollback) describes; a reorder invalidates
  collection-wide selections; reusing a key creates a new incarnation, as
  [item identity](#item-identity) describes; and the evaluation requires at
  most one, not exactly one, collection-wide derivation and summary write per
  action.

## Contract changes if accepted

- [State management](../contracts/state-management.md) gains the collection's
  storage, identity, commit, delivery and rollback rules, and closes the
  collections question with the evaluation's evidence.
- [Delivery](../contracts/delivery.md) gains a pure subpath,
  `@canonic2/lattice/collection`, that loads no DOM code.
- [Bindings](../contracts/bindings.md) gains the rule that removal ends item
  subscriptions when it commits, before the binding's own abort.

## Evaluation

A prototype in the collection lab, written to this proposal, is compared with
the shared map, with store per item as a reference; the collection record's
stage 4 holds the plan and results. Agreed thresholds:

1. A label update at 1,000 items is meaningfully faster than the shared map
   and within 2× of store per item.
2. A bulk update of every item, of `label` or `done`, is no slower than the
   shared map by more than 25% at any size, with at most one collection-wide
   derivation and at most one summary write per action.
3. Mount, insertion, removal, reorder and teardown are no more than 25%
   slower than the shared map.
4. Heap per item is no more than 25% higher than the shared map at 100 and
   1,000 items.
5. Every correctness check passes, including rollback, reentrancy, key reuse
   and order-dependent collection-wide values.

**Result.** Every threshold held. At 1,000 items a label update took 4.9 µs
against 339 µs for the shared map; bulk updates of every item stayed within
8% of the shared map with one collection-wide derivation and one summary
write; membership changes were faster; heap per item was 10–11% higher. Bulk
updates allocated nearly four times the shared map's bytes, and a
collection-wide value stays linear in the collection when it is evaluated.

**Finding for the API.** Iterating the frozen keys array with `for…of`
allocated an iterator result per element in the measured Chromium, where an
indexed loop allocated nothing. Whether `keys()` returns a frozen array is
worth reviewing before the contract is written.
