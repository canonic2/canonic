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
with the collection. Stage 2 adopted bulk compaction for teardown; the state
shape for collections is open.

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

## Threats to validity

- The rows reuse an element with three inputs; heavier rows dilute the
  bindings' share.
- The capability's state shape, items by id replaced immutably, is one common
  design; another shape changes the update cost.
- Headless Chromium on one shared machine under varying background load.

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
