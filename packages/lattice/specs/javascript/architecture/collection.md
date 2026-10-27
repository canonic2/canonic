# Collection

**Status:** current implementation.
This page maps the keyed collection to [collections](../contracts/collections.md).
It adds no
behavior; where the contract and this page differ, the contract wins and this
page is corrected.

## Files

| File                                                                                                  | Contents                                                                                                                                                                                       |
| ----------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/modules/collection/collection.js`                                                                | `createCollection` and the `Collection`, `CollectionReader`, `CollectionView`, `ItemSelection`, `CollectionSelection`, `KeysChange`, `CollectionOptions` and `CollectionWatchOptions` typedefs |
| `src/modules/collection/collection.test.js`                                                           | Node unit tests, grouped by contract area                                                                                                                                                      |
| `src/modules/collection/synchronous-result.js`                                                        | Internal synchronous-result validation and invalid-Promise observation                                                                                                                         |
| `src/modules/collection/aggregate.test.js`, `aggregate.browser.test.js`, `synchronous-result.test.js` | Aggregate work, ordering, failure, cancellation and Promise cases                                                                                                                              |
| `src/report.js`                                                                                       | `fallback` and `isThenable`, shared with the store                                                                                                                                             |

The `@canonic2/lattice/collection` entry in `exports` points at
`collection.js` directly, so the entry point loads that module and
`synchronous-result.js` and `src/report.js`, not the store core.

## State

`createCollection` returns an object of functions closing over private
variables; none uses `this`.

| Variable                                                                                                | Holds                                                                                                                                                                                                       |
| ------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `items`                                                                                                 | A `Map` from key to incarnation: `{ key, record, subscriptions, ended, base, batch }`                                                                                                                       |
| `keys`                                                                                                  | The current frozen keys array                                                                                                                                                                               |
| `membership`, `wide`                                                                                    | Membership and collection-wide subscription records, in subscription order                                                                                                                                  |
| `version`                                                                                               | A counter incremented by every commit                                                                                                                                                                       |
| `queuedVersions`, `queuedKeys`, `queuedChanges`, `queuedItems`, `queuedBefore`, `queuedAfter`, `queued` | Commits waiting for delivery, as parallel arrays that keep their capacity. A commit that changed one item stores the incarnation and its two records directly; one that changed several stores three arrays |
| `delivering`, `deferred`                                                                                | Whether a delivery runs, and subscriptions aborted during it, unlinked when it ends                                                                                                                         |
| `writing`                                                                                               | Whether `update` is resolving a patch                                                                                                                                                                       |
| `depth`, `batchId`, `log`, `touched`, `inserted`, `removed`, `baseKeys`                                 | The open batch: nesting depth, the outermost batch's number, the undo log, the items it changed, inserted and removed, and the keys before it                                                               |

A subscription record holds `fields`, `read`, `equals`, `run`, the last successfully delivered value, creation version, active state, owning
signal/abort listener and owning array: an incarnation's `subscriptions`,
`membership` or `wide`. Ending a subscription removes that abort listener and
clears its signal reference.

## Writes

- `update` checks `writing`, finds the incarnation, sets `writing` while it
  resolves the patch and walks its own keys with `for…in` and `Object.hasOwn`,
  reading each value once. At the first changed value it spreads the record;
  unknown fields throw before anything is published. The frozen new record
  replaces the incarnation's `record`. Outside a batch it commits the
  incarnation with its records before and after.
- `insert` creates a new incarnation, copies the keys array with the key at
  its position and freezes it. `remove` deletes the map entry and filters the
  keys. `reorder` validates a permutation with a `Set`, returns when the order
  is unchanged, and freezes a copy.
- Outside a batch, `insert` and `remove` commit with their key in `inserted`
  or `removed`; `remove` first ends the incarnation, which releases every
  subscription it holds and empties its array.

## Batches

- Every write inside a batch pushes a triple onto the flat undo log: the
  replaced record, the replaced keys array, the replaced map entry, or a
  marker that an incarnation was inserted or removed. A savepoint is the log's
  length when `batch` starts; a throw, or a thenable result, pops and restores
  entries back to it.
- The first change to an incarnation in an outermost batch stores its record
  as `base`, stamps `batch` with the batch's number and adds it to `touched`.
  Later changes and rolled-back savepoints leave `base` alone.
- A removal inside a batch updates the map and keys with undo-log entries,
  and pushes the incarnation to `removed`. Its subscriptions stay active
  until the outermost commit, so rollback restores a working item.
- When the outermost batch returns, `commitBatch` ends each removed
  incarnation that was not inserted in the same batch, collects inserted keys
  whose incarnation is still in the map, collects every touched incarnation
  still in the map whose record differs from `base` field by field, clears the
  bookkeeping, restores the locally saved base keys when order is unchanged, and
  commits once if anything changed.
- On every exit from a failing outermost batch, `resetBatch` clears `base` on
  touched incarnations and empties the log and lists, so nothing they
  referenced stays reachable. `baseKeys` is cleared too; commit comparison
  uses a local reference and no closed batch retains an obsolete keys array.

## Delivery

For each queued commit, in order:

1. If membership or order changed, membership subscriptions run with the
   current keys, the previous successfully delivered keys and the commit's
   change. The record's `last` advances only after a successful callback,
   using the keys captured before that callback can queue another write.
2. For each changed incarnation that has not ended, its subscriptions run
   when one of their fields differs between the commit's before and after
   records. `read` receives the incarnation's current record.
3. Collection-wide subscriptions run when membership or order changed, or when
   a changed incarnation's records differ in one of their `fields`; `read`
   receives the frozen read-only view.

Each step skips inactive subscriptions and those created at or after the
commit's version. `read`, `equals` and `run` errors go to `deliverError`,
which passes anything `report` throws to `fallback`. A subscription aborted
during delivery is released at once and unlinked from its array when the
delivery ends, so arrays are never spliced while a loop walks them. Aborts
outside a delivery unlink immediately; an item's arrays hold a few records, so
the scan is short. Every delivery phase guards its assignment to `last` with
`active`, so a callback cannot restore a released selected value.

## Tests

| Guarantee                                            | Mechanism                                                          | Test                                                                                                                                          |
| ---------------------------------------------------- | ------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Records and keys keep identity                       | One record per changing update, one array per membership change    | Storage: identity across unrelated writes                                                                                                     |
| Only the changed item is visited                     | Subscriptions held per incarnation                                 | Delivery: a counter on other items' `read` stays at zero                                                                                      |
| Collection-wide values only for their fields or keys | `touches` and the keys flag                                        | Delivery: unrelated field does not derive; reorder does                                                                                       |
| One commit per batch, no intermediate state          | Undo log and `commitBatch`                                         | Delivery: one derivation and one run for a bulk batch; no-op batches are silent, while replacement incarnations still deliver                 |
| Transactional removal                                | Removal deferred to `commitBatch`                                  | Transactional removal: rollback by batch and savepoint restores subscriptions; aborted signal stays ended                                     |
| Incarnations                                         | New incarnation per insert; queued item deliveries skip ended ones | Key reuse: in a batch, during delivery, across commits                                                                                        |
| Released references                                  | `resetBatch`, `end`, detached abort listeners and active guards    | Batches, transactional removal and released ownership: old key arrays, selected values, removed owners and callback captures can be collected |

## Incremental aggregates

The [aggregate contract](../contracts/incremental-aggregates.md) owns observable
behavior. The collection owns `watchAggregate`, aggregate subscription records,
healthy accumulators, dirty recovery and queued committed values. The internal
`synchronous-result.js` helper validates callback results, observes invalid native
Promise settlements and rejects thenables. It has no public subpath or I/O.

Aggregates initialize in current keys order with indexed traversal and direct
record lookup. They project old/new commit records rather than retaining one
contribution per item. When at least one aggregate exists, membership writes
capture immutable operands; batched normalization chooses pre-batch removals,
final insertions and surviving changed records. Rolled-back or transient changes
create no maintenance. Without aggregate subscribers, these operands are not
allocated.

Maintenance runs before user delivery can queue later writes. Each healthy
aggregate stages a new immutable total, then stores its result with that commit.
Delivery uses the collection-wide registration order shared with scanning
watchers. Stored values preserve each committed prefix while legacy readers
retain their latest-live-view behavior. No existing watch reader moves earlier.

Maintenance defers subscription-array unlinking until drain, so aborting one
aggregate cannot shift the array and skip a later owner. Delivery consumes
queued result triples with one ordered cursor, including inactive owners;
it does not search every result again for each subscription.

Computational callbacks and result validation run under the same-collection
write guard. A failed reduction discards partial work and marks the aggregate
dirty; the next relevant commit rebuilds in current order. Equality/delivery
errors retain healthy maintenance and last-successful-delivery history. Abort
clears functions, totals and selected values; queued results clear at drain.
There is no per-item watcher or contribution cache.

`aggregate.test.js` covers work bounds, normalization, rollback, reentrancy,
recovery, abort and structured-value release. `aggregate.browser.test.js`
exercises native browser lifecycle/Promise handling. `synchronous-result.test.js`
uses an owned strict-rejection fixture and cross-realm promises. The packed
consumer exercises the real package path and public types.

## Atomic membership implementation

The [bulk contract](../contracts/collections.md#atomic-bulk-membership) owns
`updateMembership({ remove, insert })`. One module-level planner validates and
copies inputs under the write guard, then returns an operation-local plan.
The method applies it through existing incarnation, journal and commit paths.
It uses no registry, receiver protocol, experimental entry or persistent planner
state. Detached calls retain normal collection semantics.

Operand buffers use captured lengths and shared empty arrays for absent changes.
A single removal needs a scalar rather than a Set. One insertion copies the
order once and appends/splices directly, skipping the general removal/anchor
loop. General changes build one frozen next order; surviving records retain
identity, reused keys get new incarnations. Getters cannot publish partial
changes or reenter writes. Nested rollback restores exact prior identities.

When aggregates are active, bulk membership also captures removed/inserted
records for the same commit. Item subscriptions end on committed removal and
never transfer to a replacement. Abort dispatch avoids redundantly detaching
an already-dispatched once-listener; other release paths detach live listeners.

`bulk-membership.test.js`, `bulk-append.test.js` and browser cases cover atomic
validation, ordering, identities, getters, rollback and cleanup. Aggregate cases
exercise bulk maintenance through the package entry. Research values and host
limitations remain in the [collection record](../../../research/collection.md)
and [aggregate record](../../../research/incremental-aggregates.md), rather than
being implementation guarantees or a pending benchmark task.
