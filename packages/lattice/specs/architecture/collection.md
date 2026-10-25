# Collection

**Status:** implemented as described. This page explains how the keyed
collection meets [collections](../contracts/collections.md). It adds no
behavior; where the contract and this page differ, the contract wins and this
page is corrected.

## Files

| File | Contents |
| --- | --- |
| `src/modules/collection/collection.js` | `createCollection` and the `Collection`, `CollectionView`, `ItemSelection`, `CollectionSelection`, `KeysChange`, `CollectionOptions` and `CollectionWatchOptions` typedefs |
| `src/modules/collection/collection.test.js` | Node unit tests, grouped by contract area |
| `src/report.js` | `fallback` and `isThenable`, shared with the store |

The `@canonic2/lattice/collection` entry in `exports` points at
`collection.js` directly, so the entry point loads that module and
`src/report.js`, not the store core.

## State

`createCollection` returns an object of functions closing over private
variables; none uses `this`.

| Variable | Holds |
| --- | --- |
| `items` | A `Map` from key to incarnation: `{ key, record, subscriptions, ended, base, batch }` |
| `keys` | The current frozen keys array |
| `membership`, `wide` | Membership and collection-wide subscription records, in subscription order |
| `version` | A counter incremented by every commit |
| `queuedVersions`, `queuedKeys`, `queuedChanges`, `queuedItems`, `queuedBefore`, `queuedAfter`, `queued` | Commits waiting for delivery, as parallel arrays that keep their capacity. A commit that changed one item stores the incarnation and its two records directly; one that changed several stores three arrays |
| `delivering`, `deferred` | Whether a delivery runs, and subscriptions aborted during it, unlinked when it ends |
| `writing` | Whether `update` is resolving a patch |
| `depth`, `batchId`, `log`, `touched`, `inserted`, `removed`, `baseKeys` | The open batch: nesting depth, the outermost batch's number, the undo log, the items it changed, inserted and removed, and the keys before it |

A subscription record holds `fields`, `read`, `equals`, `run`, the last value
delivered, the version at which it was created, whether it is active, and the
array that owns it: an incarnation's `subscriptions`, `membership` or `wide`.

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
- A removal inside a batch only pushes the incarnation to `removed`; its
  subscriptions stay active, so a rollback that restores the map entry
  restores a working item.
- When the outermost batch returns, `commitBatch` ends each removed
  incarnation that was not inserted in the same batch, collects inserted keys
  whose incarnation is still in the map, collects every touched incarnation
  still in the map whose record differs from `base` field by field, clears the
  bookkeeping, restores `baseKeys` when the keys are unchanged in content, and
  commits once if anything changed.
- On every exit from a failing outermost batch, `resetBatch` clears `base` on
  touched incarnations and empties the log and lists, so nothing they
  referenced stays reachable.

## Delivery

For each queued commit, in order:

1. If membership or order changed, membership subscriptions run with the
   current keys and the commit's change.
2. For each changed incarnation that has not ended, its subscriptions run
   when one of their fields differs between the commit's before and after
   records. `read` receives the incarnation's current record.
3. Collection-wide subscriptions run when membership or order changed, or when
   a changed incarnation's records differ in one of their `items`; `read`
   receives the frozen read-only view.

Each step skips inactive subscriptions and those created at or after the
commit's version. `read`, `equals` and `run` errors go to `deliverError`,
which passes anything `report` throws to `fallback`. A subscription aborted
during delivery is released at once and unlinked from its array when the
delivery ends, so arrays are never spliced while a loop walks them. Aborts
outside a delivery unlink immediately; an item's arrays hold a few records, so
the scan is short.

## Tests

| Guarantee | Mechanism | Test |
| --- | --- | --- |
| Records and keys keep identity | One record per changing update, one array per membership change | Storage: identity across unrelated writes |
| Only the changed item is visited | Subscriptions held per incarnation | Delivery: a counter on other items' `read` stays at zero |
| Collection-wide values only for their fields or keys | `touches` and the keys flag | Delivery: unrelated field does not derive; reorder does |
| One commit per batch, no intermediate state | Undo log and `commitBatch` | Delivery: one derivation and one run for a bulk batch; batch ends where it began commits nothing |
| Transactional removal | Removal deferred to `commitBatch` | Transactional removal: rollback by batch and savepoint restores subscriptions; aborted signal stays ended |
| Incarnations | New incarnation per insert; queued item deliveries skip ended ones | Key reuse: in a batch, during delivery, across commits |
| Released references | `resetBatch`, `end` and released stand-ins | Batches and transactional removal: weak references to replaced records and captured callbacks are collected |

## Open questions

- **Bulk allocation.** A bulk update allocates about 170 bytes per changed item,
  in the prototype and the shipping implementation alike: the new record, its
  undo entry and the before and after arrays. Whether the shipping implementation can drop any of them, such as
  the after array, which equals the current record when no later commit is
  queued, is measured before any change.
