# Collection cost of bindings

## Question

In a keyed collection, what do Lattice's per-item bindings cost over
hand-written wiring, when items are mounted, updated one at a time, reordered
and torn down? The answer decides whether a cheaper binding setup is worth
comparing, as the [full-flow record](full-flow.md) deferred until a
collection workload shows a meaningful cost.

## Status

Reported, 2026-10-25. The owner asked for the lab to run after the framework
guides; this plan was written before any run but not reviewed separately.
Binding setup costs little per item; single-item updates and teardown grow
with the collection. Stage 2 adopted bulk compaction for teardown. Stage 3
compared the shared map with one store per item and is reported with a mixed
result: per-item stores make item updates independent of the collection's
size but cost more memory and teardown than the thresholds allow. The state
shape for collections stays open until the owner decides. Stage 4 evaluated
a keyed collection prototype against agreed thresholds and is reported as
supported: every threshold holds. Decided 2026-10-25: the keyed collection is
accepted for implementation; stage 5 measures the shipping implementation.

## Hypothesis

- Each Lattice row binding allocates an internal controller, a combined signal,
  `present`, a subscription and a listener, about 380 bytes per field binding in
  the full-flow lab. Mounting and teardown are expected to cost a few
  microseconds and a few hundred bytes more per item than hand-written wiring.
- A single-item update in Lattice replaces the item and the map of items, then
  every row subscription is considered and reads its own item. Its cost is
  expected to grow with the number of items; hand-written wiring updates one
  row directly.
- Reordering runs the same keyed reconciler in both variants, with one
  selection read in Lattice; it is expected to cost about the same.

## Method

### Variants

Both variants mount the example's `<acme-filter-field>` as rows, one per item,
showing the item's label and disabled when the item is done, and run the same
lab-written keyed reconciler. The reconciler validates keys first, removes rows
whose keys are gone, creates rows for new keys and moves rows into order with
`moveBefore`, as [DOM updates](../specs/contracts/dom-updates.md#keyed-collections)
requires. Each row has its own lifetime: an `AbortController` aborted when the
row is removed or the collection is torn down.

| Variant | State | Wiring |
| --- | --- | --- |
| 0. Hand-written | A map of items and an array of ids | Actions update the map and assign the row's inputs directly; one listener per row with the row's signal |
| 1. Lattice | A tasks capability on `createStore`: `order`, an array of ids, and `byId`, items by id, replaced immutably | A collection binding watches `order` and reconciles; a row binding per item, in the bindings contract's shape, watches its item's selection and calls the label action from the row's event |

No new Lattice API is introduced; the capability, bindings and reconciler are
lab code on the store's public API.

### Workloads

| Workload | Sequence |
| --- | --- |
| Mount | Create the state, the rows and their wiring for N items, connected |
| Single-item update | Change one item's label, alternating between two values, on items chosen with seed 1 |
| Reorder | Reverse the order of all items |
| Teardown | Abort the collection's lifetime: every row lifetime ends and every row is removed |

Sizes: 10, 100 and 1,000 items.

### Measures

| Measure | Evidence |
| --- | --- |
| Time per operation | Observed: `performance.now()` around mount, teardown and reorder per item, and around 1,000 single-item updates per update, in cross-origin isolated pages |
| Allocations per update | Profiler: steady-state bytes per single-item update after 20,000 warmup updates, sampled at 32 bytes |
| Memory per item | Profiler: JavaScript heap after garbage collection, mounted minus empty, divided by N |
| Work counts | Observed: selection reads, row input assignments, control and text writes, moves, and rows created and removed per operation |
| Cleanup | Observed: observers disconnected, and DOM listener and node counts back to their baseline after teardown |

### Runs and reading

Each sample is a fresh browser context loading one variant, in an order
shuffled with seed 1. Timing: 15 pages per variant and size. Allocations and
memory: 10. Counted: 3. Each timed operation has 5 warmup repetitions and 10
measured ones.

Results are read as in the full-flow lab: a meaningful difference exceeds 5%
of the control and both interquartile ranges. A cheaper binding setup is worth
a comparison when mount, teardown or memory per item is meaningfully higher in
Lattice by an amount that matters at the measured sizes; the record states the
absolute figures so that judgment can be made.

### Correctness checks

Every counted page checks, in both variants: rows match the items after mount;
an update changes exactly one row; a reorder moves rows without creating or
removing any; after teardown no row remains, observers are disconnected and
listener and node counts return to their baseline.

### Stage 2: bulk compaction (2026-10-25, before any stage 2 run)

**Question.** Does deferring the removal of deactivated subscription records
remove the quadratic teardown without costing retained memory or delivery?

**Variants.** Variant 1 is the Lattice collection with the current store.
Variant 2 serves a lab copy of the store in its place, changed in one place:

- Deactivation stays immediate: the record is marked inactive and its `read`,
  `equals`, `run` and last value are released at once, during a delivery too.
- Only removing inactive records from the array is deferred: outside a
  delivery, one compaction pass runs when inactive records exceed half the
  array, and at the end of a delivery under the same condition. No timer or
  scheduling is introduced.

**Workloads.** Teardown of 10, 100 and 1,000 items; heavy churn, mounting
1,000 items and removing 900 one at a time, then timing single-item updates on
the remaining 100 and measuring the heap; and 20 cycles of mounting and
removing 100 items, timed and followed by a heap measurement.

**Reading.** Variant 2 is adopted when teardown at 1,000 items is meaningfully
faster, no other measure is meaningfully worse, retained memory after churn
and cycles is no larger by more than the inactive records allowed to wait,
and every correctness check passes, including that no callback of a removed
row runs. Adoption means implementing the change in the store with its unit
tests passing and a test that deactivation releases callbacks immediately.

### Stage 3: collection state shape (2026-10-25, before any stage 3 run)

**Question.** Can the existing API meet the collection requirements in the
[state management contract](../specs/contracts/state-management.md#collections)
with one store per item, and at what cost compared with the shared map?

**Variants.** Both use the store as committed, with bulk compaction, and the
same rows, reconciler and per-row lifetimes as stages 1 and 2. Keyed
subscriptions and windowing are out of scope.

| Variant | Shape |
| --- | --- |
| 1. Shared map | One store holding `order` and `byId`, replaced immutably. A collection-wide selection, the number of open items, declares `order` and `byId` |
| 3. Store per item | The capability keeps `order` in a membership store and creates one store per item holding `label` and `done`. `item(id)` returns a stable read-only view of that item's store, `{ get, watch }`, or `undefined` for an id not in the collection; label and done selections are shared by every item. The open count is computed over the items by a summary binding that watches the membership and each item's `done` |

Both variants define the same rules:

- Writes go through named actions only: `setLabel`, `setDone`, `setLabels` for
  several items, `add`, `remove` and `reverse`.
- An action naming an id not in the collection throws a `TypeError` and
  changes nothing.
- Removing an item ends its row's bindings, by aborting the row's lifetime,
  before the item's store is released.
- A bulk update of several items is one commit with the shared map and one
  commit per item with stores per item. With stores per item, subscribers can
  observe a partially applied bulk update between those commits; this is
  documented, not prevented, and no cross-store transaction is added.

**Workloads**, at 10, 100 and 1,000 items: mount; single-item label update;
single-item done toggle, which changes the open count; bulk label update of
10 items and of every item; insertion of one item at the end; removal of one
item; reorder by reversal; teardown.

**Measures.** CPU time per operation, as `performance.now()` around the
synchronous operation in isolated pages, which includes the DOM writes and
excludes rendering, identical by construction in both variants; steady-state
bytes allocated per single-item update, done toggle and bulk update of every
item; JavaScript heap per mounted item; and counted DOM effects, selection
reads and open-count derivations per operation. Pages and repetitions as in
stage 1.

**Correctness checks**, on every counted page, in both variants: identical DOM
effects per operation; keyed identity, every remaining row keeping its element
across insertion, removal and reorder; the open count correct after every
operation; teardown leaving no row, observer, listener or node; an action on a
removed id throwing a `TypeError`; and no callback of a removed row running.

**Decision thresholds**, set before running. Store per item is supported as
the collection shape when every condition holds:

1. A single-item label update at 1,000 items is meaningfully faster than the
   shared map, and at 10 items no more than 25% slower.
2. A bulk update of every item is no more than twice the shared map's time at
   any size.
3. Mount, insertion, removal, reorder and teardown are no more than 25% slower
   at any size.
4. Memory per item is no more than 50% higher at 100 and 1,000 items.
5. A done toggle, which changes the collection-wide open count, is no more than
   25% slower at any size.
6. Every correctness check passes.

If conditions 1 and 6 hold and others fail, the result is mixed: the record
states which costs store per item adds, and the contract stays open with that
evidence. If condition 1 or 6 fails, store per item is not supported. In every
case the contract stays open until the owner decides.

### Stage 3 follow-up: memory attribution and bulk done updates (2026-10-25, before any follow-up run)

The owner asked, after stage 3, to attribute store per item's extra memory
and to measure bulk updates of `done` before accepting those costs or
considering keyed subscriptions.

**Bulk done updates.** Both capabilities gain `setDoneMany(ids, done)`: one
commit with the shared map, one commit per item with stores per item, under
the same rules as `setLabels`. Workloads at 10, 100 and 1,000 items: a bulk
done update of 10 items and of every item. Before each measured set, an
untimed call sets the chosen items to not done; measured calls then alternate
done and not done, so every call changes every chosen item. Measures: CPU time
per call (15 pages, 5 warmup and 10 measured rounds), steady-state bytes per
bulk update of every item (10 pages), and counted DOM effects, open-count
derivations and summary text writes per call, with the rows and open count
checked against the bench's model after each (3 pages). Expected: stores per
item derive the open count once per changed item, so a bulk update of every
item costs work proportional to N², and the summary text is written once per
commit, showing the partly applied update. Reading: the result is reported
against stage 3's condition 2, at most twice the shared map's time at any
size, and the record states whether that condition still holds when the bulk
update changes the field the collection-wide value reads.

**Memory attribution.** Diagnostic variants, never adopted: 1n and 3n are
variants 1 and 3 without the summary binding. A bare-store cell creates N
stores holding `label` and `done`, with no bindings, and keeps them. JavaScript
heap per item after garbage collection, mounted minus empty, for 1, 1n, 3, 3n
and bare stores at 10, 100 and 1,000 items, 10 pages each. Variant 3 minus 3n,
less the shared map's single summary subscription (1 minus 1n), gives the
summary binding's per-item cost; 3n minus 1n gives the cost of the item
stores in the collection; the bare-store cell gives a store's own size.

### Stage 4: keyed collection prototype (2026-10-25, before any stage 4 run)

**Question.** Does the keyed collection proposal, since accepted as the
[collections contract](../specs/contracts/collections.md), as amended by the
owner's decisions of 2026-10-25, meet the collection
requirements within the agreed thresholds?

**Authorization.** The owner accepted the separate collection capability as
the candidate, decided the proposal's open questions, required transactional
removal, order-dependent invalidation of collection-wide values and
incarnation semantics for reused keys, agreed the thresholds below, and
authorized running this comparison without further approval. Nothing ships
from it.

**Variants.** Variant 4 is a lab prototype of `createCollection`, written to
the proposal, with a tasks capability on it: actions `setLabel`, `setDone`,
`setLabels`, `setDoneMany` (the bulk actions in one `batch`), `add`, `remove`
and `reverse`; a membership binding that replaces the rows of removed and
reinserted keys and reconciles with the shared reconciler; row bindings with
one `watchItem` per input; and the summary bound with `watch` on the open
count, `items: ['done']`. Variant 1, the shared map, is the baseline; variant
3, store per item, is a reference. All use the committed store and the same
rows, reconciler and per-row lifetimes.

**Prototype tests**, run before any measurement, in Node with the package's
test runner, covering the proposal: snapshots and identity of keys and
records; function patches and rejected reentrant writes; insertion with and
without `before`; reorder validation; the three-phase delivery order; new
subscriptions skipping the commit in progress; collection-wide values
re-evaluated on reorder and suppressed by `equals`, and not evaluated for a
commit that changes none of their fields; a bulk action in one batch notifying
each subscription at most once; transactional removal, including rollback by
the outer batch and by nested savepoints, restored subscriptions, and a
subscription aborted during a rolled-back batch staying ended; key reuse
inside a batch, across commits and during delivery, with queued
notifications for the old incarnation dropped; and the delivery error policy.

**Workloads**, at 10, 100 and 1,000 items: mount; label update; done toggle;
bulk label and bulk done updates of 10 items and of every item; insertion;
removal; reorder; teardown.

**Measures.** CPU time per operation as in stage 3; steady-state bytes per
label update, done toggle and bulk label and bulk done update of every item;
JavaScript heap per mounted item; for item subscription overhead, heap per
item of a bare collection against a bare shared-map store with the same items,
each with no subscriptions, so that the mounted heap minus the bare heap gives
what the bindings add per row; and counted DOM effects, `read`s, open-count
derivations and summary text writes per operation. Pages and repetitions as in
stage 3.

**Correctness**, on every counted page: row DOM effects identical to the
shared map; at most one open-count derivation and summary write per action,
none for a label update, and no summary write when the count is unchanged;
rows and open count matching the bench's model after each operation; keyed
identity across updates, insertion, removal and reorder; a removed key
reinserted in one action getting a new row while the other rows keep theirs;
actions on a missing key throwing a `TypeError` with no effect; and teardown
leaving no row, observer, listener or node.

**Thresholds**, agreed by the owner before the run. The proposal is supported
when every condition holds:

1. A label update at 1,000 items is meaningfully faster than the shared map
   and within 2× of store per item.
2. A bulk update of every item, of `label` or `done`, is no slower than the
   shared map by more than 25% at any size, with at most one collection-wide
   derivation and at most one summary write per action.
3. Mount, insertion, removal, reorder and teardown are no more than 25%
   slower than the shared map at any size.
4. Heap per item is no more than 25% higher than the shared map at 100 and
   1,000 items.
5. Every correctness check passes, including the prototype tests.

Otherwise the record states which conditions fail, and the collections
question stays open.

**Amendment (2026-10-25, after the smoke run, before any stage 4 run set).**
The first smoke run passed every check but showed the prototype allocating far
more than its design requires: about 2.5 KB per done toggle and 470 bytes per
item in a bulk update at 100 items. A sampling profile found three causes in
the prototype, not in the proposal:

- iterating the frozen keys array with `for…of` in the summary's `read`
  allocated an iterator result per element in this Chromium, while the other
  variants iterate arrays that are not frozen;
- every commit allocated a change object, wrapper arrays, a list of changed
  fields and a queue record;
- batches logged each write as a tuple array, tracked changed items in a
  `Map` and compared every field with `for…in`.

Before any measured run, the prototype's internals were rewritten without
changing behavior: commits are queued in parallel arrays as in the store, the
undo log is flat, an item's base record is kept on its incarnation, and
delivery compares declared fields between the records before and after each
commit. Variant 4's summary iterates the keys with an indexed loop, with a
comment saying why. The prototype tests, extended with a savepoint case, pass.
The `for…of` cost is a finding for the proposal: freezing the keys array
makes the natural loop over it allocate in this engine.

### Stage 5: shipping path (2026-10-25, before any stage 5 run)

**Question.** Does the shipping keyed collection, implemented from the
[collections contract](../specs/contracts/collections.md), meet the stage 4
thresholds on the shipping path, and what does it allocate and collect?

**Authorization.** The owner accepted the proposal for implementation on
2026-10-25 and asked for the comparison to be repeated through the shipping
path, including a confirmation run, without further approval.

**Variants.** Variant 5 is the tasks capability and bindings of variant 4,
unchanged except that it imports `createCollection` from
`@canonic2/lattice/collection`, served unchanged from the package's source
through the page's import map. Variant 1, the shared map, is the baseline;
variant 3, store per item, is needed for condition 1; variant 4, the lab
prototype, is a reference.

**Workloads, measures and correctness** as stage 4, for variants 1, 3, 4 and 5
in timing, 1, 4 and 5 in counted and memory sets. The counted checks of stage 4
apply to variant 5, including the summary limits and key reuse.

**Garbage collection.** For variants 1 and 5 at each size, 10 pages each: after
a warmup, trace Chromium's `MinorGC` and `MajorGC` events while running 20,000
label updates, 20,000 done toggles and a bulk done update of every item
repeated 20,000 ÷ N times (at least 20), each separately, and report the
number of collections and their total duration per 1,000 operations next to
the run's own CPU time and allocated bytes.

**Confirmation.** After the first timing set, a second timing set of variants
1 and 5 alone, 15 pages per size, under comparable conditions.

**Reading.** The shipping path confirms the stage 4 result when the five
thresholds hold for variant 5 in the first timing set and in the
confirmation, and in the memory and counted sets. Allocation and collection
are reported against the prototype and the shared map with no threshold: the
owner accepted bulk allocation as a cost of this design. If a threshold fails,
the prototype figures do not carry over to the product, the record says which
measure failed, and the owner decides.

### Host and tools

The same machine and tools as the full-flow lab. Background load is recorded
with each set.

## Conditions

| Item | Value |
| --- | --- |
| Dates | 2026-10-25: timing 00:34–00:37 UTC, counted 00:37, memory 00:37–00:40 |
| Host | macOS 15, x86-64, Intel Xeon W-2140B with 16 threads, 64 GB memory; Node 24.12.0; Chromium 156.0.8078.4 bundled with Playwright 1.64.1, headless, cross-origin isolated pages |
| Background load | 1-minute load average 27 falling to 11 during the timing set, 11 to 12 for the others: the quietest runs of these labs |
| Samples | Timing: 15 pages per variant and size; memory and allocations: 10; counted: 3. No page errored |
| Measured source | The store, element helpers and example element as committed with the store's write change, plus the lab's capability, bindings, reconciler and runner, hashed in each report |

### Stage 3 (2026-10-25)

| Item | Value |
| --- | --- |
| Dates | Timing 01:16–01:24 UTC, counted 01:24–01:25, memory 01:25–01:33 |
| Host and tools | As stage 1 |
| Background load | 1-minute load average 7 to 9 during timing and counted; memory began at 9 and ended at 45, which affects heap and allocation figures less than timings |
| Samples | Timing: 15 pages per variant and size; memory and allocations: 10; counted: 3. No page errored |
| Measured source | Lattice as committed with bulk compaction, unchanged in both variants, plus the lab's two capabilities, bindings, summary bindings, reconciler and runner, hashed in each report |

### Stage 3 follow-up (2026-10-25)

| Item | Value |
| --- | --- |
| Dates | Timing 01:41–01:43 UTC, counted 01:43, memory 01:43–01:45 |
| Host and tools | As stage 1 |
| Background load | 1-minute load average 49 falling to 28 during timing, the busiest of these runs; 19 to 25 for counted and memory |
| Samples | Timing: 15 pages per variant and size; memory: 10 per variant and size, for variants 1, 1n, 3 and 3n; counted: 3. No page errored |
| Measured source | As stage 3, with `setDoneMany` in both capabilities and the summary binding switchable for the diagnostic variants |

### Stage 4 (2026-10-25)

| Item | Value |
| --- | --- |
| Dates | Timing 02:08–02:21 UTC, counted 02:21, memory 02:22–02:32 |
| Host and tools | As stage 1; the prototype tests ran with the package's Vitest in Node 24.12.0 |
| Background load | 1-minute load average 9 to 21 during timing and counted; memory began at 15 and ended at 26 |
| Samples | Timing: 15 pages per variant and size; memory and allocations: 10; counted: 3. No page errored |
| Measured source | Lattice as committed, unchanged; the lab's collection prototype after the pre-run amendment, its tasks capability and bindings, the stage 3 variants, reconciler and runner, hashed in each report |

## Results

Medians across pages of each page's median, with the full-flow lab's
meaningful-difference rule.

| Measure | Items | Hand-written | Lattice | Difference | Meaningful |
| --- | --- | --- | --- | --- | --- |
| Mount, µs per item | 10, 100, 1,000 | 17.0, 16.7, 20.4 | 19.2, 18.9, 23.1 | +2.2 to +2.7, +13% | Yes, except at 100 |
| Teardown, µs per item | 10, 100, 1,000 | 10.3, 10.1, 11.2 | 11.4, 12.0, 20.2 | +1.1, +1.9, +9.0 | Yes |
| Single-item update, µs | 10, 100, 1,000 | 1.75, 1.94, 1.78 | 3.00, 16.5, 245 | +1.2, +14.6, +243 | Yes |
| Reorder, µs per item | 10, 100, 1,000 | 2.28, 3.28, 12.0 | 2.53, 3.43, 12.1 | +11%, +4%, +1% | Only at 10 |
| JavaScript heap per item, bytes | 100, 1,000 | 575, 417 | 991, 926 | +417, +509 | Yes |
| Bytes allocated per update, steady state | 10, 100, 1,000 | 77, 77, 77 | 276, 636, 4,249 | grows with items | Yes |

Counts per operation were identical in both variants for every DOM effect: an
update made two label setter calls, one assignment and its reflection echo,
one text write and one attribute write; a reorder of N items made N − 1 moves
and created or removed no row; teardown disconnected N observers. Lattice made
2N selection reads per update, two per row, and one per reorder; the control
made none.

### Causes

- **Updates.** Each row binding subscribes to its item's label and done
  selections, both declared on the shared `byId` field. Changing one item
  replaces `byId`, so the commit considers all 2N subscriptions and calls 2N
  `read`s to update one row. The update also copies the map of items, which
  accounts for the allocation growing with N. The cost is linear in the
  number of items; a field index would not help, since every row subscription
  declares the same field.
- **Teardown.** Aborting a row's lifetime deactivates its two subscriptions.
  Outside a delivery, each deactivation compacts the whole subscription array
  at once, so tearing down N rows one by one does work proportional to N².
  The cost per item is small at 10 and 100 items and grows to +9 µs per item
  at 1,000.
- **Mount and memory.** The binding setup the full-flow lab measured costs
  +2 to +3 µs and about 420 to 510 bytes per item here, including the two
  subscription records each row holds.

### Stage 2: bulk compaction (2026-10-25)

Variant 1, the current store, against variant 2, compaction in bulk, both
running the Lattice collection. Timing 00:58–01:03 UTC with load rising from 7
to 24; churn, cycles and counted 01:03–01:04 with load 15 to 22. The lab copy
passed all 48 of the store's unit tests before the runs.

| Measure | Items | Variant 1 | Variant 2 | Difference | Meaningful |
| --- | --- | --- | --- | --- | --- |
| Teardown, µs per item | 10, 100, 1,000 | 11.5, 12.6, 20.7 | 11.7, 12.1, 13.5 | +2%, −5%, −35% | At 1,000 only |
| Mount and teardown of 100 items, ms per cycle | 100 | 4.11 (IQR 0.18) | 3.87 (IQR 0.16) | −6% | Yes |
| Retained after 20 cycles, bytes | 100 | 16,300 | 16,296 | 0% | No |
| Removing 900 of 1,000 items one at a time, µs per removal | 1,000 | 155 | 152 | −2% | No |
| Single-item update after that churn, µs | 100 remaining | 20.3 | 20.6 | +1% | No |
| Heap after that churn, bytes | 100 remaining | 1,774,772 | 1,781,136 | +0.4% | No |
| Mount, update and reorder | 10, 100, 1,000 | | | −2% to +10% | No |

Removing items one at a time stayed near 150 µs per removal in both: each
removal also filters the order and runs the reconciler over the remaining
rows, which compaction does not change. The heap after churn differs by the
inactive records allowed to wait. Both variants passed every counted check,
including that a removed row's callbacks never run when its item changes.

**Outcome.** Under the stage's rule, variant 2 is adopted: teardown of 1,000
items is 35% faster, cycles 6% faster, and nothing is meaningfully worse. It
is implemented in `src/modules/store/store.js` with a unit test that a
deactivated subscription's callbacks are released at once, while its record
still waits; the store's unit tests, the example's tests and the browser tests
pass.

### Stage 3: collection state shape (2026-10-25)

Variant 1, the shared map, against variant 3, one store per item. Medians
across pages of each page's median; "meaningful" uses the stage 1 rule.
Store-per-item p95 and maximum across pages stayed within 10% of its median
except mount at 100 items (35.2 against 21.0 µs) and teardown at 100 items
(23.4 against 15.4 µs), single pages in both cases.

| Measure | Items | Shared map | Store per item | Relative | Meaningful |
| --- | --- | --- | --- | --- | --- |
| Label update, µs | 10, 100, 1,000 | 3.81, 21.2, 339 | 2.68, 2.99, 5.20 | −30%, −86%, −98% | Yes |
| Done toggle, µs | 10, 100, 1,000 | 3.63, 22.0, 342 | 2.84, 4.73, 31.8 | −22%, −79%, −91% | Yes |
| Bulk label update of 10 items, µs | 10, 100, 1,000 | 19.4, 39.5, 361 | 19.8, 19.9, 21.6 | +2%, −50%, −94% | At 100 and 1,000 |
| Bulk label update of every item, µs | 10, 100, 1,000 | 19.2, 220, 3,665 | 19.4, 194, 3,820 | +1%, −12%, +4% | At 100 only |
| Insertion at the end, µs | 10, 100, 1,000 | 34.3, 92.6, 849 | 30.3, 54.5, 355 | −12%, −41%, −58% | At 100 and 1,000 |
| Removal, µs | 10, 100, 1,000 | 31.6, 125, 2,469 | 35.0, 64.3, 400 | +11%, −49%, −84% | At 100 and 1,000 |
| Reorder, µs per item | 10, 100, 1,000 | 2.64, 3.79, 12.6 | 2.44, 3.72, 12.7 | −8% to +1% | No |
| Mount, µs per item | 10, 100, 1,000 | 20.0, 20.8, 23.2 | 22.3, 21.0, 25.7 | +12%, +1%, +11% | At 10 and 1,000 |
| Teardown, µs per item | 10, 100, 1,000 | 11.4, 11.9, 13.0 | 15.1, 15.4, 17.0 | +33%, +30%, +31% | Yes |
| JavaScript heap per item, bytes | 10, 100, 1,000 | 2,618, 920, 807 | 3,206, 1,692, 1,486 | +22%, +84%, +84% | Yes |
| Bytes allocated per label update | 10, 100, 1,000 | 268, 652, 4,253 | 85, 85, 85 | −68% to −98% | Yes |
| Bytes allocated per done toggle | 10, 100, 1,000 | 247, 632, 4,231 | 114, 138, 138 | −54% to −97% | Yes |
| Bytes allocated per bulk update of every item | 10, 100, 1,000 | 700, 4,753, 44,993 | 986, 9,039, 89,424 | +41%, +90%, +99% | Yes |

Selection reads and open-count derivations per operation, from the counted
pages (N items):

| Operation | Shared map reads | Shared map derivations | Store per item reads | Store per item derivations |
| --- | --- | --- | --- | --- |
| Mount | 2N + 2 | 1 | 3N + 2 | 1 |
| Label update | 2N + 1 | 1 | 1 | 0 |
| Done toggle | 2N + 1 | 1 | 2 | 1 |
| Bulk label update of k items | 2N + 1 | 1 | k | 0 |
| Insertion | 2N + 4 | 1 | 5 | 1 |
| Removal | 2N + 2 | 1 | 2 | 1 |
| Reorder | 2 | 1 | 2 | 1 |

Every DOM effect count was identical between the variants at every step and
size: label and disabled assignments, text and attribute writes, moves, rows
created and removed, observers attached and disconnected.

**Reading the costs.**

- **Item updates** cost the same at every size with stores per item: one
  store, one or two subscriptions, about 85 bytes. The shared map pays 2N
  reads, a derivation and a copy of the map on every update.
- **The collection-wide derivation stays linear.** A done toggle at 1,000
  items costs 31.8 µs against 5.2 µs for a label update, because the open
  count is recomputed over every item's store. The shared map derives on every
  change to the map, including label changes that cannot alter the count;
  stores per item derive only when membership or a `done` changes. A bulk
  update of `done` on k items would derive k times with stores per item, one
  per commit; that workload was not in the plan and was not measured.
- **Bulk updates of every item** take about the same time in both, dominated
  by the row writes, and stores per item allocate about twice as much: one
  snapshot and delivery per item instead of one map copy. Subscribers of
  other items can observe the update partly applied between those commits.
- **Memory and teardown** are the variant's costs: about 680 to 770 bytes and
  4 µs more per item at 100 and 1,000 items. Each item adds its store and the
  summary binding's subscription and controller for its `done`; this lab did
  not attribute the bytes between them.

### Stage 3 follow-up: memory attribution and bulk done updates (2026-10-25)

**Bulk done updates**, medians across pages:

| Measure | Items | Shared map | Store per item | Ratio | Meaningful |
| --- | --- | --- | --- | --- | --- |
| Bulk done update of 10 items, µs | 10, 100, 1,000 | 21.4, 40.6, 356 | 29.0, 43.1, 189 | 1.36×, 1.06×, 0.53× | Yes |
| Bulk done update of every item, µs | 10, 100, 1,000 | 20.6, 240, 4,245 | 28.0, 443, 21,065 | 1.35×, 1.84×, 4.96× | Yes |
| Bytes allocated per bulk done update of every item | 10, 100, 1,000 | 928, 7,874, 79,808 | 1,448, 16,285, 162,512 | 1.56×, 2.07×, 2.04× | Yes |

Per call, a bulk done update of k items made the same row DOM effects in both
variants. The shared map read 2N selections, derived the open count once and
wrote the summary text once. Stores per item read 2k selections, derived the
open count k times, each over every item, and wrote the summary text k times:
every intermediate count was written to the DOM during the synchronous call.
That does not establish that the browser painted any of them, but each write is
unnecessary work and an intermediate state that subscribers and DOM observers
can see. At 1,000 items, updating every
item's `done` with stores per item did about 1,000 derivations over 1,000
items, 21 ms against 4.2 ms.

**Memory attribution**, JavaScript heap per item, medians across 10 pages:

| Items | 1. Shared map | 1n. Without summary | 3. Store per item | 3n. Without summary | Bare store |
| --- | --- | --- | --- | --- | --- |
| 100 | 922 | 920 | 1,693 | 1,434 | 527 |
| 1,000 | 811 | 806 | 1,486 | 1,335 | 521 |

At 10 items every variant varies by about 550 bytes between pages, too coarse
to attribute. At 100 and 1,000 items, of the 675 to 770 bytes per item store
per item adds:

- **the item store**, 3n minus 1n: 514 and 529 bytes, matching a bare store
  holding the same two fields, 527 and 521 bytes; a store's own size is nearly
  all of it, and the row bindings cost the same whichever store they watch;
- **the summary binding's per-item subscription and controller**, 3 minus 3n
  less the shared map's single summary subscription: 257 and 146 bytes.

Teardown time was not attributed; the extra subscription and controller per
item, and releasing a store per item, are the only per-item differences in
teardown.

### Stage 4: keyed collection prototype (2026-10-25)

Variant 4, the keyed collection, against variant 1, the shared map, with
variant 3, store per item, for reference. Medians across pages of each page's
median; "meaningful" uses the stage 1 rule against the shared map.

| Measure | Items | Shared map | Store per item | Keyed collection | Keyed vs shared map | Meaningful |
| --- | --- | --- | --- | --- | --- | --- |
| Label update, µs | 10, 100, 1,000 | 3.85, 21.4, 339 | 2.80, 3.08, 5.22 | 2.81, 3.17, 4.92 | −27%, −85%, −99% | Yes |
| Done toggle, µs | 10, 100, 1,000 | 3.70, 22.1, 341 | 2.88, 4.79, 31.5 | 3.14, 6.09, 45.9 | −15%, −72%, −87% | Yes |
| Bulk label update of 10 items, µs | 10, 100, 1,000 | 20.1, 39.7, 358 | 20.3, 20.2, 21.8 | 21.3, 22.8, 22.8 | +6%, −42%, −94% | At 100 and 1,000 |
| Bulk label update of every item, µs | 10, 100, 1,000 | 19.4, 222, 3,680 | 19.8, 198, 3,800 | 20.7, 220, 3,450 | +7%, −1%, −6% | At 10 only |
| Bulk done update of 10 items, µs | 10, 100, 1,000 | 20.3, 40.6, 356 | 27.5, 42.8, 257 | 22.2, 25.8, 64.3 | +9%, −36%, −82% | Yes |
| Bulk done update of every item, µs | 10, 100, 1,000 | 20.4, 237, 3,945 | 27.5, 442, 23,275 | 22.0, 233, 3,635 | +8%, −2%, −8% | At 10 and 1,000 |
| Insertion, µs | 10, 100, 1,000 | 33.0, 87.4, 849 | 31.1, 53.0, 327 | 25.3, 49.4, 252 | −23%, −43%, −70% | Yes |
| Removal, µs | 10, 100, 1,000 | 31.8, 124, 2,469 | 36.3, 64.5, 371 | 26.1, 58.9, 347 | −18%, −53%, −86% | Yes |
| Reorder, µs per item | 10, 100, 1,000 | 2.63, 3.77, 12.6 | 2.59, 3.77, 12.5 | 2.46, 3.57, 12.4 | −6% to −1% | No |
| Mount, µs per item | 10, 100, 1,000 | 21.0, 22.0, 23.4 | 22.2, 20.9, 25.9 | 21.4, 18.7, 23.5 | +2%, −15%, +1% | No |
| Teardown, µs per item | 10, 100, 1,000 | 11.9, 13.4, 12.9 | 15.3, 15.6, 16.8 | 11.5, 11.4, 12.8 | −4%, −15%, −2% | No |
| JavaScript heap per item, bytes | 10, 100, 1,000 | 2,636, 922, 809 | 3,221, 1,693, 1,487 | 3,761, 1,012, 897 | +43%, +10%, +11% | Yes |
| Bare container per item, bytes | 10, 100, 1,000 | 263, 69, 55 | | 596, 150, 119 | +126%, +117%, +119% | Yes |
| Bytes allocated per label update | 10, 100, 1,000 | 268, 652, 4,253 | 86, 86, 85 | 57, 57, 57 | −79% to −99% | Yes |
| Bytes allocated per done toggle | 10, 100, 1,000 | 247, 631, 4,232 | 114, 138, 138 | 86, 110, 110 | −65% to −97% | Yes |
| Bytes allocated per bulk label update of every item | 10, 100, 1,000 | 701, 4,757, 44,983 | 983, 9,044, 89,559 | 1,716, 16,535, 171,439 | +145% to +281% | Yes |
| Bytes allocated per bulk done update of every item | 10, 100, 1,000 | 678, 4,709, 45,202 | 1,446, 16,291, 162,641 | 1,507, 16,557, 171,589 | +122% to +280% | Yes |

Selection reads, open-count derivations and summary writes per operation,
from the counted pages (N items, k items changed):

| Operation | Shared map | Store per item | Keyed collection |
| --- | --- | --- | --- |
| Label update | 2N + 1 reads, 1 derivation, 0 writes | 1, 0, 0 | 1, 0, 0 |
| Done toggle | 2N + 1, 1, 1 | 2, 1, 1 | 2, 1, 1 |
| Bulk label update of k items | 2N + 1, 1, 0 | k, 0, 0 | k, 0, 0 |
| Bulk done update of k items | 2N + 1, 1, 1 | 2k, k, k | k + 1, 1, 1 |
| Insertion | 2N + 4, 1, 1 | 5, 1, 1 | 3, 1, 1 |
| Removal | 2N + 2, 1, 1 | 2, 1, 1 | 1, 1, 1 |
| Reorder | 2, 1, 0 | 2, 1, 0 | 1, 1, 0 |
| Remove and reinsert one key in one action | | | 3, 1, 1 |

Row DOM effects were identical between the keyed collection and the shared map
at every step and size.

**Reading the costs.**

- **Item updates** cost what store per item costs, about 3 to 5 µs at every
  size, and allocate 57 bytes, with no derivation for a field the open count
  does not read.
- **Bulk actions** commit once: one derivation and at most one summary write
  for a bulk done update of every item, whose time matches the shared map's,
  against about 1,000 of each and 6× the time with store per item at 1,000
  items. Bulk updates allocate about 170 bytes per changed item, nearly four
  times the shared map's single map copy: one new record, its undo-log entry
  and its before and after records for delivery.
- **The open count stays linear** when it is evaluated: a done toggle at 1,000
  items costs 45.9 µs, against 31.5 µs with store per item, because the count
  reads each item through the collection's map.
- **Memory.** The bare collection holds about 65 bytes more per item than the
  shared map's store (an incarnation per item); mounted rows add the same
  per-row cost in both, about 755 to 780 bytes at 1,000 items and 850 to 860
  at 100, so item subscriptions cost no more than the shared map's.
- **Membership changes** are cheaper than the shared map's, which copies the
  map of items on every insertion and removal.

## Correctness

| Variant | Counted pages | Failed checks |
| --- | --- | --- |
| 0. Hand-written | 9 | None |
| 1. Lattice | 9 | None |

Both passed, at 10, 100 and 1,000 items: rows matched the items after mount;
an update changed exactly one row; a reorder reversed the rows by moving them
without creating or removing any; after teardown no row remained, every
observer was disconnected and listener and node counts returned to their
baseline.

### Stage 3

| Variant | Counted pages | Failed checks |
| --- | --- | --- |
| 1. Shared map | 9 | None |
| 3. Store per item | 9 | None |

Both passed, at 10, 100 and 1,000 items, after mount and after every
operation: rows and the open count matched the bench's own model; every
remaining row kept its element across label, done and bulk updates,
insertion, removal and reorder, and a reorder created or removed no row; each
of `setLabel`, `setDone`, `setLabels` and `remove` on a removed id threw a
`TypeError` and caused no DOM effect, derivation or row callback; after
teardown no row or summary remained, every observer was disconnected and
listener and node counts returned to their baseline.

The first smoke run failed teardown in both variants: the bench's identity
snapshot still held the rows. Clearing it on unmount fixed the runner; no
variant changed.

### Stage 3 follow-up

Both variants passed every check on 9 counted pages each: after each bulk
done update the rows and open count matched the bench's model and every row
kept its element, and after teardown listener and node counts returned to
their baseline.

### Stage 4

| Variant | Counted pages | Failed checks |
| --- | --- | --- |
| 1. Shared map | 9 | None |
| 3. Store per item | 9 | None |
| 4. Keyed collection | 9 | None |

All three passed the stage 3 checks after every operation, including the bulk
done updates. The keyed collection also passed, at every size: at most one
open-count derivation and summary write per action, and none for label
actions; no summary write for a reorder that left the count unchanged; and a
key removed and reinserted in one action getting a new row in its place while
every other row kept its element.

The prototype's own tests, run before the measured sets, cover the proposal's
contract: identity of keys and records; function patches read once and every
write rejected while a patch is resolved; insertion with and without `before`;
reorder validation; the membership, item, collection-wide delivery order;
subscriptions created during delivery skipping the commit in progress;
collection-wide values re-evaluated on reorder, suppressed by `equals`, and
not evaluated for unrelated fields; one notification per subscription for a
bulk batch; transactional removal rolled back by the batch and by nested
savepoints with subscriptions restored, a savepoint's rolled-back update
notifying nothing, and a subscription whose signal aborted inside a
rolled-back batch staying ended; key reuse inside a batch, across commits and
during delivery, with queued notifications for the old incarnation dropped;
and delivery errors passed to `report`. All pass.

## Threats to validity

- The rows reuse an element with three inputs; heavier rows dilute the
  bindings' share.
- The capability's state shape, items by id replaced immutably, is one common
  design; another shape changes the update cost.
- Headless Chromium on one shared machine under varying background load.
- Stage 3 measures one collection-wide value, a count over one boolean field.
  Derivations over more fields, or bulk updates of the field they read, cost
  more with stores per item and were not measured.
- Stage 3 runs workloads in a fixed order within each page; earlier workloads
  change labels and done states the later ones start from, identically in
  both variants.
- Heap per item at 10 items varies by about 550 bytes between pages in both
  variants, the heap's measurement granularity at that size.
- The follow-up's timing set ran under the heaviest background load of these
  labs, a load average of 49 falling to 28; its deciding ratio, about 5× at
  1,000 items, is far outside the interquartile ranges, and it matches the
  counted derivations.
- Stage 4 measures a lab prototype, tuned once before the run as its
  amendment records; a product implementation must be measured again on the
  shipping path. Under a load average of 9 to 21, some shared-map cells had
  wide interquartile ranges, such as mount at 10 items (IQR 10.8 µs); every
  threshold holds by a margin larger than those ranges.
- Bulk updates allocate nearly four times the shared map's bytes in the
  keyed collection. No threshold covers allocations; their garbage-collection
  cost is inside the measured times only as far as collections happened
  during the timed calls.

## Decision and follow-ups

**Outcome.** Binding setup is not the collection's material cost: mounting
costs 2 to 3 µs and about 500 bytes more per item, the price of the bindings
contract's rollback and cleanup guarantees, so a cheaper binding setup is not
worth a comparison on this evidence. Two other costs are material and grow
with the collection:

1. **Single-item updates are linear in the number of items:** 245 µs and
   about 4 KB per update at 1,000 items, against 1.8 µs and 77 bytes for
   hand-written wiring. The cause is the state shape the current API leads to:
   per-item selections on one shared field, and an immutable map copied on
   every update.
2. **Teardown of many subscriptions is quadratic**, because each deactivation
   outside a delivery compacts the subscription array.

**Owner's decision (2026-10-25):**

- **Teardown:** compare bulk compaction, keeping deactivation and the release
  of callbacks and selected values immediate and deferring only the removal of
  empty records, without timers or scheduling. Done in stage 2 and adopted.
- **Updates:** open the collection state-shape question in the state
  management contract, then measure candidates. Field-level selectivity does
  not give item-level selectivity inside one shared field; Lattice is not
  called ready for large collections until that is settled. One store per
  item is a candidate, not yet the rule.

**Stage 3 against its thresholds:**

| Condition | Result |
| --- | --- |
| 1. Label update faster at 1,000 items, at most 25% slower at 10 | Holds: −98% at 1,000, −30% at 10 |
| 2. Bulk update of every item at most twice the shared map | Holds: +1%, −12%, +4% |
| 3. Mount, insertion, removal, reorder and teardown at most 25% slower | Fails for teardown, +30% to +33% at every size; the others hold, at most +12% |
| 4. Memory per item at most 50% higher at 100 and 1,000 | Fails: +84% at both |
| 5. Done toggle at most 25% slower | Holds: faster at every size |
| 6. Every correctness check | Holds |

**Outcome: mixed.** Conditions 1 and 6 hold and 3 and 4 fail, so by the
plan's rule store per item is not established as the collection shape. It
meets the contract's update requirements with the current API, at about
700 bytes and 4 µs of teardown more per item, a collection-wide derivation
that stays linear in the collection, and bulk updates that are observable
part-way and allocate twice as much. The collection contract stays open until
the owner decides.

**Stage 3 follow-up.** Condition 2 does not hold when a bulk update changes
the field a collection-wide value reads: updating every item's `done` takes
1.35×, 1.84× and 4.96× the shared map's time at 10, 100 and 1,000 items, and
the ratio grows with the collection, because each per-item commit rederives
the value over every item. Updates to a small subset stay within the bar and
become cheaper than the shared map at 1,000 items. The extra memory is almost
entirely the item stores themselves, about 520 bytes each, with 150 to
260 bytes more for the summary binding's per-item subscription. Store per item
therefore costs, beyond stage 3's findings, quadratic bulk updates of any
field a collection-wide value derives from, unless that value is maintained
some other way, and its memory cost is a store's own size. The collection
contract stays open until the owner decides.

**Owner's decision (2026-10-25)** on stage 3 and its follow-up: store per item
is not the default shape. A separate keyed collection capability is the
candidate, evaluated in stage 4.

**Stage 4 against its thresholds:**

| Condition | Result |
| --- | --- |
| 1. Label update at 1,000 items meaningfully faster than the shared map, within 2× of store per item | Holds: 4.92 µs against 339; 0.94× store per item |
| 2. Bulk update of every item within +25% of the shared map, at most one derivation and summary write | Holds: label +7%, −1%, −6%; done +8%, −2%, −8%; one derivation and one write |
| 3. Mount, insertion, removal, reorder and teardown within +25% | Holds: at most +2% (mount at 10 items); the others are faster or equal |
| 4. Heap per item within +25% at 100 and 1,000 items | Holds: +10% and +11% |
| 5. Every correctness check, including the prototype tests | Holds |

**Outcome: supported.** Every condition holds. The keyed collection meets the
collection requirements: item updates cost the same at every size, bulk
actions publish once, membership changes are cheaper than the shared map's,
and memory is about 90 bytes more per item. Its costs are bulk-update
allocations, nearly four times the shared map's, and collection-wide values
that remain linear when evaluated. The collection contract stays open until
the owner decides; adopting the proposal means implementing it in the
product with its tests and measuring it again on the shipping path.

**Owner's decision (2026-10-25):** the proposal is accepted for
implementation, within the measured scope. It meets the agreed thresholds and
resolves the trade-off between shared-map updates and per-item stores.

- `keys()` stays a frozen array; measured hot paths use indexed loops.
- Bulk allocation, about 170 bytes per changed item, is an accepted cost of
  this design. It was not a rejection threshold and none is added; allocation
  and garbage-collection effects are measured again through the shipping
  implementation.
- Aggregate costs stay explicit: item-local updates are selective, and
  collection-wide derivations can still scan every item.
- Prototype performance is not established product performance; stage 5
  measures the shipping implementation.
- This settles the primitive, not any application's large-list readiness.
  Selection over 10,000 items and windowing remain separate work.

The [collections contract](../specs/contracts/collections.md) records the
accepted design, and `src/modules/collection/` implements it.
