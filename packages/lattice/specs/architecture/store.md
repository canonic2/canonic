# Store

**Status:** implemented as described. This page explains how the store core
meets [state management](../contracts/state-management.md). It adds no
behavior; where the contract and this page differ, the contract wins and this
page is corrected.

## Files

| File | Contents |
| --- | --- |
| `src/modules/store/store.js` | `createStore` and the `Store`, `Selection` and related typedefs |
| `src/modules/store/shallow.js` | `shallow` |
| `src/modules/store/store.test.js`, `shallow.test.js` | Node unit tests, grouped by contract area as [tests](#tests) lists |
| `src/index.js` | Re-exports `createStore`, `shallow` and the `Selection`, `Store`, `StoreOptions` and `WatchOptions` types |

## Store state

`createStore` returns an object of four functions that close over private
variables; none uses `this`, so `get` and `watch` can be handed out detached.

| Variable | Holds |
| --- | --- |
| `current` | The current frozen snapshot |
| `version` | A counter incremented by every commit |
| `subscriptions` | An array of subscription records in subscription order |
| `queuedVersions`, `queuedChanges`, `queued` | Commits waiting for delivery: parallel arrays of versions and changed fields, and how many are queued. The arrays keep their capacity between deliveries |
| `delivering` | Whether a delivery is running |
| `batchDepth`, `batchBase`, `touched` | The open batch, if any: its nesting depth, the snapshot before the outermost batch, and the distinct keys written inside it |
| `inactive` | How many records were deactivated and not yet removed |
| `writing` | Whether `set` is reading a patch |
| `report` | The delivery error policy: the injected `report` or, by default, `fallback` |

`createStore` first rejects an `initial` that is not an object or has symbol
keys, with a `TypeError`, then freezes a spread copy of it.

`fallback(error)` is the single last-resort path: it calls the platform's
`reportError` when one exists, and when there is none, or it throws, queues a
microtask that rethrows the original error. It never throws. Every report goes
through `deliverError(error)`, which calls `report` inside `try`/`catch` and
sends anything `report` throws to `fallback`. Neither can throw into the
delivery loop.

A subscription record holds `fields`, `read`, `equals`, `run`, the last value
delivered successfully, the version at which it was created, and whether it is
still active.

## Writes

`set(patch)`:

1. Throws `TypeError` when `writing` is set: a getter or patch function of
   another `set` is calling it. Otherwise sets `writing` until step 2 ends, in
   a `finally` block.
2. Resolves a function patch by calling it with `current`, and throws
   `TypeError` when the patch is not an object. Then walks the patch's own
   enumerable string keys once with `for…in` and `Object.hasOwn`, which
   allocates no key array, reading each value exactly once into a local
   variable. An unknown key, checked with `Object.hasOwn(current, key)`,
   throws `TypeError`, so a rejected write changes nothing; so does an error a
   getter throws. At the first key whose value is not `Object.is`-equal to the
   current one, the walk builds the next snapshot by spreading `current` and
   starts an array of changed keys with that key; it writes each changed value
   into the next snapshot as it reads it. A write rejected later in the walk
   discards the unpublished snapshot.
3. Returns if none changed: no snapshot, no delivery, and nothing allocated.
4. Freezes the next snapshot and makes it current. The patch is never read
   again, so a getter on a patch cannot make the snapshot disagree with the
   changed keys. Unchanged fields keep their references because the spread
   copies references.
5. Inside a batch, adds each changed key to `touched` if it is not already
   there, and stops. Outside one, increments `version`, stores the version and
   the changed keys in the next queue slots, and drains the queue unless a
   delivery is already running. Queuing allocates no commit object.

The unknown-key check needs no separate key set: the snapshot's own keys are the
state's shape, which never changes. `touched` can hold at most one entry per
state field, so checking it before adding keeps it free of duplicates at a cost
bounded by the state's size.

## Batches and savepoints

`batch(fn)` uses the JavaScript call stack as its savepoint stack:

1. Remembers `current` in a local variable as this batch's start. If
   `batchDepth` is zero, also stores it as `batchBase` and clears `touched`.
2. Increments `batchDepth` and calls `fn`.
3. If `fn` throws, or returns a value with a `then` method, restores `current`
   to this batch's start, decrements `batchDepth` and rethrows, or throws
   `TypeError` for the thenable. The thenable check runs inside the same
   rollback, because reading `then` can itself throw. Nothing is delivered for
   the rolled-back writes.
4. Otherwise decrements `batchDepth`. A nested batch stops here; its writes stay
   in `current` and `touched` for the outer batch.
5. When the outermost batch returns, builds a new array of the keys in `touched`
   whose value in `current` differs from `batchBase`. If any remain, increments
   `version`, queues one commit with that array and drains as `set` does.
6. On every exit from the outermost batch, whether it commits, has nothing to
   commit, throws or is rejected as a thenable, sets `batchBase` to `undefined`
   and empties `touched`, in a `finally` block. The commit's changed keys were
   already copied into their own array, so clearing loses nothing, and no
   obsolete snapshot stays referenced until the next batch.

Snapshots are immutable, so restoring a savepoint is one assignment. Changed-field
rollback needs no bookkeeping either: keys a rolled-back nested batch left in
`touched` are filtered out at the outer commit because their values equal
`batchBase` again, and a key changed and changed back is filtered the same way.

## Subscriptions

`watch(selection, run, { signal })`:

1. Throws `TypeError` if a batch is open, as the contract requires.
2. Validates `fields`: a non-empty array whose entries are all own keys of
   `current`. Otherwise throws `TypeError`. Validates and keeps a copy, so the
   caller cannot change the record's fields afterwards.
3. Returns if the signal is already aborted.
4. Calls `read(current)`. If it throws, the error propagates and nothing was
   registered. If `version` changed during `read`, the read wrote to the store
   and `watch` throws `TypeError` without registering. If the signal aborted
   during `read`, returns without registering.
5. Appends a record created at the current `version`, with `last` set to the
   initial value, and adds an `abort` listener with `{ once: true }` that
   deactivates the record. Both happen before any further user code runs.
6. Calls `run(value, undefined)`. The record is registered first so that a
   commit made by `run` itself is delivered to the new subscription and
   compared against the initial value. If `run` aborts the signal, the listener
   has already deactivated the record, so nothing stays active. If `run`
   throws, the record is deactivated, the abort listener removed and the error
   propagates.

The initial call does not use `equals`, since there is no previous value.

## Delivery

Draining the queue:

1. Sets `delivering` and takes commits from the queue in order, by index up to
   `queued`, which grows if subscribers commit during the loop. At the end it
   resets `queued` and replaces each used `queuedChanges` slot with a shared
   empty array, so no changed-key array stays referenced while the arrays keep
   their capacity.
2. For each commit, walks `subscriptions` from the first record to the last. It
   skips a record that is inactive or was created at or after the commit's
   version: such a subscription's initial `run` already saw that state.
3. For each remaining record, checks whether any of its `fields` is among the
   commit's changed keys, with two small array scans. Records with no changed
   field are skipped without calling `read`.
4. For a matching record, calls `read(current)`, reading the latest snapshot
   rather than the commit's, then `equals(value, last)`. If the value changed,
   calls `run(value, last)` and only then stores the value as `last`, so a
   throwing `run` leaves the previous value in place. It checks `active` again
   after `read` and after `equals`, so a subscription that aborts itself there
   is not called afterwards.
5. Catches any error from `read`, `equals` or `run`, passes it to
   `deliverError`, and continues with the next record.
6. When the queue is empty, clears `delivering` in a `finally` block, so the
   store can never stay marked as delivering, and removes inactive records if
   they are more than half the array.

Commits made by subscribers during step 4 are appended to the queue and drained
by the same loop, so they never reenter. The outermost `set` or `batch` returns
only after the loop ends.

### Removal and additions during delivery

Deactivation is immediate, during a delivery or not: the record is skipped from
that moment on, including later in the same commit, and its `read`, `equals`,
`run` and last value are replaced by shared stand-ins, so nothing the
subscription captured stays reachable. Deactivating increments `inactive`.
Only the empty record's slot in the array waits: one compaction pass removes
inactive records when they are more than half the array, outside a delivery
or at its end. Removing N subscriptions one by one therefore costs work
proportional to N, not N², and at most as many inactive records as active ones
wait in the array. Additions are appended and excluded from commits made before
them by the version check. No delivery copies the subscription array, and no
timer or scheduling is involved.

## Errors

| Error | Handling |
| --- | --- |
| `initial` not an object, or with symbol keys | `TypeError` from `createStore` |
| Unknown key in `set`, a patch that is not an object, `set` while another `set` reads its patch | `TypeError` before any change |
| A patch getter or patch function throws | Propagates before any change |
| `watch` while a batch is open, invalid `fields`, an initial `read` that writes | `TypeError` before any registration |
| Initial `read` or `run` in `watch` | Propagates; no subscription remains |
| `read`, `equals` or `run` during delivery | Caught per record and passed to `deliverError`; delivery continues |
| `report` throws | Caught by `deliverError` and passed to `fallback`; delivery continues |
| `fallback` | The platform's `reportError` when it exists, otherwise, or when that throws, a microtask that rethrows; the same path for the default `report` and for a throwing injected one |
| `fn` in `batch` throws, returns a thenable, or returns a value whose `then` getter throws | Rollback to the batch's start, then rethrow or `TypeError` |

## Typing

The typedefs are written in JSDoc in `store.js`:

- `createStore` is generic over the state type `S`, inferred from `initial`.
- `set` accepts `Partial<S>` or a function from `Readonly<S>` to `Partial<S>`,
  so an object literal with an unknown key is a type error.
- `Selection<S, V>` has `fields` as an array of keys of `S`, `read` from
  `Readonly<S>` to `V`, and an optional `equals` over `V`.
- `watch` is generic over `V`, taking a `Selection<S, V>` and a `run` that
  receives `V` and the previous `V` or `undefined`.

This declaration of `Selection` is settled by the
[consumer type fixture](package.md#consumer-type-fixture): from a packed
archive, it shows valid inference, rejected misspelled fields and unknown
patch keys, `read` receiving `Readonly<S>` and `run` receiving `V` under a
`satisfies` annotation, with no export typed `any`.

## Tests

| Guarantee | Mechanism | Test |
| --- | --- | --- |
| Rejected writes change nothing | Validation before building | Unknown key leaves `get()` identical and delivers nothing |
| Unchanged writes are silent | Change detection in the key walk | Same values: no new snapshot identity, no `read` |
| One snapshot per changing write | One spread per changing `set` | Identity changes once per changing write, also inside a batch |
| Unchanged fields keep references | Spread copies references | Nested object field identical across a write to another field |
| Skipping unrelated subscriptions | Field check before `read` | Counter on `read` stays zero for unrelated commits |
| At most one evaluation per commit | One walk per commit | Counter on `read` is one per matching commit |
| Nested writes do not reenter | Queue plus `delivering` flag | A subscriber's `set` returns before its subscribers run; order of `run` calls recorded |
| Latest snapshot, never older | `read(current)` at notification | Subscriber reading during queued commits sees non-decreasing versions |
| Removal during delivery | Deactivation | A record aborted by an earlier subscriber is not called in the same or queued commits |
| Callbacks released at deactivation | Stand-ins replace the record's functions | A callback's captured object is collected after its signal aborts, while the record waits below the compaction threshold |
| Delivery after many removals | Compaction above half | After 50 of 51 subscriptions abort, a commit still reaches the remaining one |
| Addition during delivery | Version check | A subscription added by a subscriber is not notified for the current commit |
| Delivery errors isolated | Per-record catch | Throwing `read`, `equals` and `run` each reach `report`; later records still run; `set` returns |
| Batch rollback | Savepoint in a local variable | Throwing callback restores the start snapshot and delivers nothing |
| Nested savepoint | Same, per call | Caught inner throw keeps outer writes; outer commits once |
| Thenable callback | Result check | Returning a promise or thenable rolls back and throws `TypeError` |
| Initial `watch` failures | Registration order | Throwing `read` or `run` leaves no subscription |
| Abort during initialization | Abort listener installed before `run` | A `run` that aborts its own signal leaves nothing active; later commits do not call it |
| Already aborted signal | Early return | Nothing registered, `read` and `run` not called |
| Patch read once | Values captured in the key walk | A patch getter returning different values on each read yields a snapshot whose changed fields match, and its subscribers are notified |
| No `watch` in a batch | Batch check first | `watch` inside a batch throws `TypeError`; after a rollback no subscription shows the discarded value |
| Reporter fallback | `deliverError` and `fallback` | With no platform `reportError` and a throwing injected `report`, the error is rethrown from a microtask, the rest of the delivery runs and later writes are delivered |
| Batch bookkeeping released | `finally` on the outermost batch | After a batch commits, has nothing to commit or throws, each case on its own store, the pre-batch snapshot is collected; the unit project runs with `--expose-gc` |
| Throwing `then` | Thenable check inside the rollback | A result whose `then` getter throws rolls back and delivers nothing |
| Reentrant patch | `writing` flag | A `set` from a patch getter or patch function throws `TypeError` and nothing changes |
| Rejected patches | Type check after resolving the patch | A patch that is not an object, and a function returning none, throw `TypeError` |
| Writing initial `read` | `version` compared around the initial `read` | `watch` throws `TypeError` and `run` is not called |
| Abort in `read` or `equals` | `active` checked after each | A subscription aborting itself there is not run |
| Throwing platform reporter | `fallback` catches it | Delivery continues, queued commits are delivered, `set` returns, and the error surfaces from a microtask |
| Copied fields | Copy at `watch` | Changing the caller's `fields` array afterwards changes nothing |
| Frozen snapshots | `Object.freeze` per snapshot | Snapshots from `createStore`, `set` and inside a batch are frozen |

Work counts use wrappers around `read`, `equals` and `run`, as
[performance and evidence](../contracts/performance.md#work-counts) describes.

## Measured choices

- **Snapshot built during the key walk; queue without commit objects**
  (2026-10-24). The first versions collected changed keys and values in two
  growable arrays and queued a commit object in an array emptied with
  `length = 0`, which also released its backing store. In steady state those
  were about 250 of 352 bytes per unrelated write. Building the snapshot at the
  first change and queuing into parallel arrays that keep their capacity cut
  that to about 98 bytes, confirmed on the shipping path, with no time measure
  slower. An unrelated write was also about 21% faster at 10 fields; at 100
  fields the gain was inconclusive under heavy machine load. See the
  [full-flow record](../../research/full-flow.md), stage 2. The remaining
  bytes are the snapshot itself and the changed-key array.
- **Compaction in bulk** (2026-10-25). Removing inactive records on every
  deactivation outside a delivery made tearing down N subscriptions
  quadratic. Compacting once inactive records exceed half the array cut
  teardown of a 1,000-item collection from 20.7 to 13.5 µs per item and
  repeated mounting of 100 items by 6%, with no other measure worse and
  retained memory unchanged within 0.4%. See the
  [collection record](../../research/collection.md), stage 2.

## Open questions

- **Field check representation.** Two array scans per record are the simplest
  check and fit selections with one or two fields. A batch that changes many
  fields may favor a `Set` of changed keys built once per commit. The choice is
  measured on the wide fan-out and burst workloads before any change.
- **Savepoint cost.** The mechanism above suggests savepoints add no structure
  beyond `batchBase` and `touched`. If implementation shows otherwise, the
  contract simplification is proposed explicitly before coding continues.
