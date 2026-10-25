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
shape for collections stays open until the owner decides.

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
[state management contract](../specs/contracts/state-management.md#collections-open)
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
