# Collections

## Purpose and status

A keyed collection holds a capability's items by key, so that changing one item
costs work independent of the collection's size and one action that changes
many items publishes its result once. It is a separate pure capability beside
the store core, with its own entry point; the store's snapshot contract does
not change.

**Status:** agreed contract, implemented in `src/modules/collection/` and
exported from `@canonic2/lattice/collection`. The
[collection research](../../../research/collection.md) owns the adoption decision,
comparisons against shared-map and per-item-store alternatives, and their
measured scope. Its measurements describe the recorded implementation and
workloads; later API changes do not establish a fresh performance result.

This contract settles the primitive. It does not establish that any
application's large lists are ready: selection over much larger collections,
such as 10,000 items, and mounting only visible rows are separate work.

The [completed-reconciliation guard](../framework/collections.md) is an accepted
application presentation pattern. It preserves membership incarnations and
interaction while avoiding duplicate work for an unchanged retained range.
Large-list readiness remains open; this does not add collection or windowing APIs.

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
  fields: ['done'],
  read: (collection) => {
    const keys = collection.keys();
    let count = 0;
    for (let index = 0; index < keys.length; index++)
      if (!collection.get(keys[index]).done) count++;
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
tasks.watchKeys((keys, previous, change) => reconcile(keys, previous, change), { signal });
tasks.watchItem('t1', label, (text) => (row.label = text), { signal });
tasks.watch(open, (count) => (summary.text = `${count} open`), { signal });
```

| Function                                  | Behavior                                                                                                                                                                                                                                                                                                  |
| ----------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `createCollection(entries, options)`      | Copies each `[key, item]` into a frozen record. Keys are strings and unique; each item's own enumerable string keys define that item's fields. A key that is not a string, a duplicate key, or an item that is not an object or has symbol keys throws a `TypeError`. `options.report` is as in the store |
| `keys()`                                  | The current keys in order, as a frozen array. The same array is returned until membership or order changes                                                                                                                                                                                                |
| `get(key)`                                | The item's current frozen record, or `undefined` for a key not in the collection. The same record is returned until that item changes                                                                                                                                                                     |
| `has(key)`, `size`                        | Membership without copying                                                                                                                                                                                                                                                                                |
| `update(key, patch)`                      | Patches one item's fields as the store's `update` does: an object or a function of the item's record, each patched value read once, unchanged values silent. A field the item does not have, a patch that is not an object, or a key not in the collection throws a `TypeError`                           |
| `insert(key, item, before)`               | Adds an item before the key `before`, or at the end without it, in one commit. A key already in the collection, or a `before` not in it, throws a `TypeError`                                                                                                                                             |
| `remove(key)`                             | Removes an item; its item subscriptions end when the removal commits. A key not in the collection throws a `TypeError`                                                                                                                                                                                    |
| `reorder(keys)`                           | Replaces the order with a permutation of the current keys. Anything else throws a `TypeError`; the current order is silent                                                                                                                                                                                |
| `batch(fn)`                               | As the store's `batch`: synchronous, savepoints, rollback on throw, thenables rejected with a `TypeError`, one commit                                                                                                                                                                                     |
| `watchKeys(run, options)`                 | Subscribes to membership and order. `run(keys, previous, change)` receives the keys and, after a commit, the keys it `inserted` and `removed`; the initial call receives no change                                                                                                                        |
| `watchItem(key, selection, run, options)` | Subscribes to one item through a selection over its fields. Selections are plain objects shared by every item. A key not in the collection or a field the item does not have throws a `TypeError` and subscribes nothing                                                                                  |
| `watch(selection, run, options)`          | Subscribes to a collection-wide value. `selection.fields` lists the item fields `read` depends on and may be empty; `read` receives the collection's read-only view: `keys`, `get`, `has` and `size`                                                                                                      |

- Every write rejected with a `TypeError` changes nothing.
- Record selections require non-empty `fields` and validate against the
  watched record. Collection-wide `fields` may be empty and are checked for
  string entries, not against a shared runtime schema: each item establishes
  its own shape. Consumer types and declaration checks catch invalid
  dependencies in their respective scopes.
- Any write, including `batch`, made while `update` resolves a patch, from a
  patch function or a getter, throws a `TypeError`, as in the store.
- `watchKeys`, `watchItem` and `watch` inside a batch throw a `TypeError`. An
  aborted signal subscribes nothing; an error from the initial `read` or `run`
  propagates and leaves no subscription; an initial `read` that writes to the
  collection throws a `TypeError`. Initial delivery does not call `equals`.
  Batch/key/selection validation precedes the aborted-signal check and may
  still throw for invalid arguments.
- The functions do not use `this`. A capability keeps the collection private
  and hands out a read-only view (`keys`, `get`, `has`, `size`, `watchKeys`,
  `watchItem`, `watch`), as it does with a store's `state`. Writes stay behind
  the capability's named actions.
- Scalar state, such as a filter, stays in an ordinary store beside the
  collection. A store and a collection commit independently; no commit spans
  both.

`CollectionReader<T>` contains `keys`, `get`, `has`, readonly `size`,
`watchKeys`, `watchItem` and `watch`. It is the read-and-subscribe surface a
capability exposes. `CollectionView<T>` contains only traversal (`keys`, `get`,
`has` and readonly `size`) and is passed to derivations. Neither type adds a
runtime wrapper; forward `size` through a getter when constructing a reader.

Every watcher receives the current value first and its last successfully
delivered value second, or `undefined` on its initial call. `watchKeys` receives
membership metadata third: `run(keys, previous, change)`, with `change`
undefined initially. A failed callback does not advance `previous`. Key
replacement still delivers metadata when key strings and order are unchanged;
membership delivery does not use value equality to suppress that notification.
The current keys can include writes queued by earlier callbacks; `change`
describes its queued commit and is not a complete diff from `previous`.

The entry point exports `createCollection` and the `Collection`,
`CollectionReader`, `CollectionView`, `ItemSelection`, `CollectionSelection`, `KeysChange`,
`CollectionOptions` and `CollectionWatchOptions` types.

## Declaration check

`checkCollectionSelection(name, selection, cases)` is exported from
`@canonic2/lattice/testing`. Cases map names to `CollectionView<T>` fixtures,
including a collection's traversal surface. It returns the same `SelectionCheck`
report as `checkSelection`: declared and accessed fields, per-case access and
unobserved declarations, all in declaration order. No cases throws.

The check calls `read` once per case with an instrumented traversal view.
`get(key)` returns an instrumented item record, or `undefined` for a missing key;
property reads and `in` tests on records must use declared, known item fields.
`keys()`, `has(key)` and `size` do not count as item-field access: every
collection selection already depends on membership and order.

Writes to the supplied view, its returned keys or item records, record key
enumeration and descriptor access fail even if the reader catches the refusal.
The check exposes only the four traversal members, never collection mutation
or subscription methods. Other read failures retain their cause and identify
the selection and case. Nested data remains subject to the shallow immutability
contract; the check does not prove purity or cover branches absent from fixtures.

Acceptance covers successful aggregates, membership-only selections, missing
keys, branch coverage across cases, unknown and undeclared fields, caught
unsupported operations, read errors and empty cases. Consumer type fixtures
cover collection checker inference and read-only reader surfaces.

The implementation and behavioral cases live in `src/testing/index.js` and
`src/testing/collection.test.js`; `examples/tasks/tasks.test.js` exercises
application-owned selectors. `src/modules/collection/collection.test.js`
covers membership callback history, failed delivery, nested membership writes
and key replacement. `tests/package.test.js` and `tests/types/` exercise the
public exports and consumer types.

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
reports it in both `removed` and `inserted`, even when the final key strings
and order are unchanged, so a membership binding replaces the key's row.

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
- A batch with no net membership, order, incarnation or item-field change
  commits nothing. Removing and reinserting an existing key changes its
  incarnation even if its final position and record values match.

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
  changed a field in `fields`. Each is evaluated at most once per commit, and
  `equals` suppresses an unchanged result.

**Order.** Within a commit: membership subscriptions, then item subscriptions
by changed item in the order of each item's first change and in subscription
order within an item, then collection-wide subscriptions, each group in
subscription order. Membership bindings therefore create and remove rows
before item updates, and collection-wide delivery follows item delivery.
Errors are isolated as below; the ordering does not guarantee that every
binding assignment succeeded.
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

| Operation                           | Work                                                                                                                                        |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| Update one item                     | One new record; the item's subscriptions only; collection-wide values only if a declared field changed                                      |
| Bulk update of k items in one batch | k new records and their undo entries; up to one evaluation per affected item subscription; at most one evaluation per collection-wide value |
| Insert                              | One new incarnation and item record, plus a keys array; membership and collection-wide subscriptions                                        |
| Remove                              | A new keys array and release of the removed incarnation's subscriptions; membership and collection-wide subscriptions                       |
| Reorder                             | One keys array; membership and collection-wide subscriptions                                                                                |
| Collection-wide value               | Its `read` may scan every item each time it is evaluated                                                                                    |

The accepted design allocates item records, undo entries and before/after
delivery bookkeeping for bulk changes. Collection-wide derivations can remain
linear even though item-local subscriptions are selective. The
[collection research](../../../research/collection.md) owns allocation, garbage
collection and aggregate timing results, including the alternatives' costs.

Windowing is application-owned. The implemented
[incremental aggregate contract](incremental-aggregates.md) defines the
reversible-reduction API; ordinary scanning selections remain supported and unchanged.

## Atomic bulk membership

**Status: supported and implemented.** The owner adopts the explicit
`collection.updateMembership(changes)` operation together with product aggregate
integration. It uses the regular collection entry point; no experimental API,
registry or duplicate runtime exists. Independent listener cleanup remains part
of normal collection/store lifecycle behavior.

Use `insert`, `remove`, `reorder` and `batch` with their existing identities,
ordering, incarnation, rollback and subscription guarantees. A batch combines
delivery, not order copies: each ordinary insert/remove still creates its own
frozen keys array, visible immediately to later reads in that batch.

`updateMembership` takes `{ remove?, insert? }`. `remove` is an array of existing
unique string keys. Each insertion is `[key, record, before?]`; keys must be
unique within the request and cannot already survive in the collection. A
removed key may be reinserted as a new incarnation. An anchor must be an
existing key not removed by this request; it cannot name another insertion.
Omitting the anchor or supplying undefined appends. Insertions at an anchor,
including append, retain supplied order; surviving keys retain relative order.

The operation validates and copies inputs once under the existing write guard
before publishing anything. Getters can read pre-call state; writes to that
collection throw. Getters on another collection retain that collection's own
write boundary. Invalid input or a throwing getter publishes no partial plan,
including when caught inside a batch after earlier writes. Empty changes are
silent and keep keys identity. Nonempty calls expose one frozen next order;
existing records preserve identity, inserted records are frozen copies.

Inside a batch, reads after the bulk call immediately see its complete result.
Ordinary insert/remove reads are unchanged. The existing undo log restores exact
order/record/incarnation identities at nested savepoints. Removal subscriptions
end only when removal commits; aborts remain ended across rollback. Reused keys
never inherit old subscriptions. Outside a batch the operation commits once;
inside an outer batch it joins that commit. Reentrant delivery and error/report
behavior use the existing queue, with no new scheduler or lifetime registry.

The [research record](../../../research/collection.md) preserves the full
comparison history, allocation/ownership findings and shared-host diagnostic.
Adoption does not assert that every original quiet-host or tail-latency gate was
measured or passed. No additional benchmark campaign is required for this API
integration; future performance work starts from a concrete reported workload.

## Acceptance

| Requirement                                                                                                                | Evidence                                                                                         |
| -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| Storage identity and frozen records                                                                                        | `collection.test.js`, storage                                                                    |
| Function patches, read once, reentrant writes rejected                                                                     | `collection.test.js`, function patches                                                           |
| Delivery order, item-local evaluation, order-dependent aggregates, at most one evaluation per batch, no intermediate state | `collection.test.js`, delivery                                                                   |
| Subscription validation, copied fields, abort, initial failures                                                            | `collection.test.js`, subscriptions                                                              |
| Current/previous keys, failed callbacks, queued membership writes and replacement metadata                                 | `collection.test.js`, membership callback history                                                |
| Delivery errors and the reporter fallback                                                                                  | `collection.test.js`, delivery errors                                                            |
| Rollback, savepoints, released bookkeeping                                                                                 | `collection.test.js`, batches                                                                    |
| Transactional removal, restored subscriptions, released removed items                                                      | `collection.test.js`, transactional removal                                                      |
| Key reuse in a batch, during delivery and across commits                                                                   | `collection.test.js`, key reuse                                                                  |
| Pure entry point in Node                                                                                                   | `tests/import.test.js`                                                                           |
| Packed entry point and consumer types                                                                                      | `tests/package.test.js` with `tests/types/valid-collection.ts` and `invalid-collection-field.ts` |
| Record and aggregate dependency checks                                                                                     | `src/testing/index.test.js`, `src/testing/collection.test.js` and `examples/tasks/tasks.test.js` |
| Performance against the agreed thresholds for a named implementation and workload                                          | The [collection research](../../../research/collection.md), with its conditions and limits       |

Final-candidate behavioral coverage, distinct from performance acceptance, maps:

| Candidate requirement                                                                                       | Coverage                                                                                                                           |
| ----------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| Atomic membership validation, stable anchors, copied records and read-once inputs                           | `bulk-membership.test.js`                                                                                                          |
| Single insertion into empty/nonempty collections, detached append/anchors, snapshot identities and rollback | `bulk-append.test.js`                                                                                                              |
| Immediate batch reads, nested/thenable rollback, incarnation reuse and transient no-ops                     | `bulk-membership.test.js`                                                                                                          |
| Abort during planning/rollback, committed listener detach, reentrant delivery and released snapshots        | `bulk-membership.test.js`, `bulk-membership.browser.test.js`                                                                       |
| Regular entry and isolated packed runtime, detached calls and generic consumer rejection                    | `tests/import.test.js`, `tests/package.test.js`, `tests/types/valid-collection.ts`, `tests/types/invalid-collection-membership.ts` |

### Release coverage

The `released ownership` cases in `collection.test.js` cover selected values
released during callbacks, removal detaching a still-live owning signal, and
old key arrays released outside open batches. Every delivery phase advances
its cached value only while the subscription remains active. Committed removal
also detaches the subscription's abort listener without aborting its caller's
signal.

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
- **Function patches, matching `update`** (2026-10-25, owner), with each value read
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
- **Alternative shapes have different costs.** Independent stores add an
  owner per item and do not share bulk transactions; keyed subscriptions over
  a shared immutable map still pay for replacing that map. The collection
  research owns the comparisons. These alternatives remain application choices
  when their ownership and snapshot properties fit the workload.
